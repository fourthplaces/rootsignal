use anyhow::{Context, Result};
use sqlx::postgres::PgPoolOptions;
use tracing::info;
use tracing_subscriber::EnvFilter;

use rootsignal_common::{Config, ScoutScope};
use rootsignal_graph::{connect_graph, migrate::migrate, GraphClient};
use rootsignal_scout_supervisor::{
    notify::{backend::NotifyBackend, noop::NoopBackend, router::NotifyRouter},
    supervisor::Supervisor,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("rootsignal=info".parse()?))
        .init();

    info!("Root Signal Scout Supervisor starting...");

    // Load config
    let config = Config::supervisor_from_env();
    config.log_redacted();

    // Connect to Postgres
    let database_url =
        std::env::var("DATABASE_URL").context("DATABASE_URL required for validation issues")?;
    let pg_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .context("Failed to connect to Postgres")?;

    let neo4j_db = std::env::var("NEO4J_DB").unwrap_or_else(|_| "neo4j".into());
    info!(db = neo4j_db.as_str(), "Connecting to Neo4j");

    let client = connect_graph(
        &config.neo4j_uri,
        &config.neo4j_user,
        &config.neo4j_password,
        &neo4j_db,
    )
    .await?;

    // Run migrations (idempotent)
    migrate(&client).await?;

    // Build ScoutScope from config
    let region = ScoutScope {
        center_lat: config.region_lat.unwrap_or(44.9778),
        center_lng: config.region_lng.unwrap_or(-93.2650),
        radius_km: config.region_radius_km.unwrap_or(30.0),
        name: config
            .region_name
            .clone()
            .unwrap_or_else(|| config.region.clone()),
    };

    info!(name = region.name.as_str(), "Loaded region");

    // Build notification backend: Slack if configured, otherwise Noop
    let notifier: Box<dyn NotifyBackend> = match NotifyRouter::from_env() {
        Some(router) => {
            info!("Slack notifications enabled");
            Box::new(router)
        }
        None => {
            info!("No SLACK_WEBHOOK_URL set, notifications disabled");
            Box::new(NoopBackend)
        }
    };

    // Create and run supervisor
    let supervisor = Supervisor::new(
        client,
        pg_pool,
        region,
        config.anthropic_api_key.clone(),
        notifier,
    );
    let (stats, events) = supervisor.run().await?;

    info!(events = events.len(), "Supervisor complete. {stats}");
    Ok(())
}
