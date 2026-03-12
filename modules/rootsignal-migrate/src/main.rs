use anyhow::Result;
use clap::Parser;
use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::EnvFilter;

use std::sync::Arc;

use rootsignal_migrate::{runner, checks, MigrateContext};
use rootsignal_migrate::registry::migrations;

#[derive(Parser)]
#[command(name = "rootsignal-migrate", about = "Database migration runner")]
struct Cli {
    /// Actually execute pending migrations (default: dry run).
    #[arg(long)]
    commit: bool,

    /// Lint SQL migrations for risky patterns.
    #[arg(long)]
    check: bool,

    /// Seed migrations up to this name as already applied.
    #[arg(long)]
    baseline: Option<String>,

    /// Mark specific migrations as completed without running them.
    /// Comma-separated list of migration names.
    #[arg(long, value_delimiter = ',')]
    mark_completed: Vec<String>,

    /// Postgres connection string. Defaults to DATABASE_URL env var.
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let all = migrations();

    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&cli.database_url)
        .await?;

    let mut ctx = MigrateContext::new(pool);

    // Register geocoder for data migrations that need it
    if let Ok(token) = std::env::var("MAPBOX_TOKEN") {
        if !token.is_empty() {
            ctx.insert(Arc::new(rootsignal_graph::geocoder::MapboxGeocoder::new(token)));
        }
    }

    // Register Neo4j client for data migrations that need it
    if let (Ok(uri), Ok(user), Ok(pass)) = (
        std::env::var("NEO4J_URI"),
        std::env::var("NEO4J_USER"),
        std::env::var("NEO4J_PASSWORD"),
    ) {
        let db = std::env::var("NEO4J_DB").unwrap_or_else(|_| "neo4j".into());
        match rootsignal_graph::connect_graph(&uri, &user, &pass, &db).await {
            Ok(client) => {
                tracing::info!(db = db.as_str(), "Connected to Neo4j");
                ctx.insert(Arc::new(client));
            }
            Err(e) => {
                tracing::warn!(error = %e, "Neo4j not available — graph migrations will skip");
            }
        }
    }

    if cli.check {
        let warnings = checks::check(&all);
        if warnings.is_empty() {
            println!("\nNo warnings found.\n");
        } else {
            println!();
            for w in &warnings {
                println!("  \u{26a0} {} (line {}): {}", w.migration, w.line, w.message);
            }
            println!("\n{} warning{}.\n", warnings.len(), if warnings.len() == 1 { "" } else { "s" });
        }
        return Ok(());
    }

    if !cli.mark_completed.is_empty() {
        return runner::mark_completed(&ctx, &all, &cli.mark_completed).await;
    }

    if let Some(ref target) = cli.baseline {
        return runner::baseline(&ctx, &all, target).await;
    }

    if cli.commit {
        runner::commit(&ctx, &all).await
    } else {
        runner::dry_run(&ctx, &all).await
    }
}
