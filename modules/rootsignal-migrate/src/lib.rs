pub mod checks;
pub mod data_migrations;
pub mod registry;
pub mod runner;

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use sqlx::PgPool;

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Type-map context passed to data migration functions.
///
/// Postgres is always available via `.pg()`. Other backends (Neo4j, KurrentDB,
/// S3, Redis, etc.) are registered by the binary's main() and retrieved by
/// migrations that need them.
///
/// ```ignore
/// // In main.rs:
/// let mut ctx = MigrateContext::new(pg_pool);
/// ctx.insert(neo4j_client);
/// ctx.insert(s3_client);
///
/// // In a data migration:
/// let pg = ctx.pg();
/// let neo4j = ctx.get::<GraphClient>()?;
/// ```
pub struct MigrateContext {
    pg: PgPool,
    resources: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl MigrateContext {
    pub fn new(pg: PgPool) -> Self {
        Self {
            pg,
            resources: HashMap::new(),
        }
    }

    pub fn pg(&self) -> &PgPool {
        &self.pg
    }

    /// Register a resource. Overwrites any existing value of the same type.
    pub fn insert<T: Send + Sync + 'static>(&mut self, value: T) {
        self.resources.insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Retrieve a registered resource by type.
    pub fn get<T: Send + Sync + 'static>(&self) -> anyhow::Result<&T> {
        self.resources
            .get(&TypeId::of::<T>())
            .and_then(|v| v.downcast_ref::<T>())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "migration requires {} but it was not registered",
                    std::any::type_name::<T>()
                )
            })
    }

    /// Retrieve a registered resource, returning None if not registered.
    pub fn try_get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.resources
            .get(&TypeId::of::<T>())
            .and_then(|v| v.downcast_ref::<T>())
    }
}

/// A single migration — either raw SQL or a Rust data migration function.
pub struct Migration {
    pub name: &'static str,
    pub body: MigrationBody,
}

pub enum MigrationBody {
    Sql(&'static str),
    DataMigration {
        /// Report what would happen without modifying anything.
        plan: fn(&MigrateContext) -> BoxFuture<anyhow::Result<String>>,
        /// Execute the migration. Must be idempotent.
        run: fn(&MigrateContext) -> BoxFuture<anyhow::Result<()>>,
    },
}

impl Migration {
    pub fn kind(&self) -> &'static str {
        match self.body {
            MigrationBody::Sql(_) => "sql",
            MigrationBody::DataMigration { .. } => "data",
        }
    }

    pub fn sql_text(&self) -> Option<&'static str> {
        match self.body {
            MigrationBody::Sql(sql) => Some(sql),
            MigrationBody::DataMigration { .. } => None,
        }
    }
}

pub fn sql(name: &'static str, text: &'static str) -> Migration {
    Migration {
        name,
        body: MigrationBody::Sql(text),
    }
}

pub fn data(
    name: &'static str,
    plan: fn(&MigrateContext) -> BoxFuture<anyhow::Result<String>>,
    run: fn(&MigrateContext) -> BoxFuture<anyhow::Result<()>>,
) -> Migration {
    Migration {
        name,
        body: MigrationBody::DataMigration { plan, run },
    }
}
