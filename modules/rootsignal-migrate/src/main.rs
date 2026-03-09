use anyhow::Result;
use clap::Parser;
use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::EnvFilter;

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

    let ctx = MigrateContext::new(pool);

    // Register additional backends here as they're added:
    // if let Ok(url) = std::env::var("NEO4J_URL") {
    //     ctx.insert(GraphClient::new(&url).await?);
    // }

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
