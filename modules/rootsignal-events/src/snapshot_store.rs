//! PostgresSnapshotStore — seesaw snapshot persistence backed by Postgres.

use std::pin::Pin;
use std::future::Future;

use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use seesaw_core::{Snapshot, SnapshotStore};

/// Postgres-backed snapshot store for seesaw aggregates.
#[derive(Clone)]
pub struct PostgresSnapshotStore {
    pool: PgPool,
}

impl PostgresSnapshotStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl SnapshotStore for PostgresSnapshotStore {
    fn load_snapshot(
        &self,
        aggregate_type: &str,
        aggregate_id: Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Snapshot>>> + Send + '_>> {
        let agg_type = aggregate_type.to_string();
        Box::pin(async move {
            let row = sqlx::query_as::<_, SnapshotRow>(
                "SELECT aggregate_type, aggregate_id, version, state, created_at
                 FROM aggregate_snapshots
                 WHERE aggregate_type = $1 AND aggregate_id = $2",
            )
            .bind(&agg_type)
            .bind(aggregate_id)
            .fetch_optional(&self.pool)
            .await?;

            Ok(row.map(|r| Snapshot {
                aggregate_type: r.aggregate_type,
                aggregate_id: r.aggregate_id,
                version: r.version as u64,
                state: r.state,
                created_at: r.created_at,
            }))
        })
    }

    fn save_snapshot(
        &self,
        snapshot: Snapshot,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            sqlx::query(
                "INSERT INTO aggregate_snapshots (aggregate_type, aggregate_id, version, state, created_at)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (aggregate_type, aggregate_id)
                 DO UPDATE SET version = EXCLUDED.version, state = EXCLUDED.state, created_at = EXCLUDED.created_at",
            )
            .bind(&snapshot.aggregate_type)
            .bind(snapshot.aggregate_id)
            .bind(snapshot.version as i64)
            .bind(&snapshot.state)
            .bind(snapshot.created_at)
            .execute(&self.pool)
            .await?;

            Ok(())
        })
    }
}

#[derive(sqlx::FromRow)]
struct SnapshotRow {
    aggregate_type: String,
    aggregate_id: Uuid,
    version: i64,
    state: serde_json::Value,
    created_at: chrono::DateTime<chrono::Utc>,
}
