use anyhow::Result;
use tracing::info;
use tracing_subscriber::EnvFilter;

use rootsignal_common::Config;
use rootsignal_graph::{migrate::migrate, GraphClient};
use rootsignal_scout::scout::Scout;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("rootsignal=info".parse()?))
        .init();

    info!("Root Signal Scout starting...");

    // Load config
    let config = Config::scout_from_env();
    config.log_redacted();

    // Connect to Neo4j
    let client =
        GraphClient::connect(&config.neo4j_uri, &config.neo4j_user, &config.neo4j_password)
            .await?;

    // Run migrations
    migrate(&client).await?;

    // Create and run scout
    let scout = Scout::new(
        client,
        &config.anthropic_api_key,
        &config.voyage_api_key,
        &config.firecrawl_api_key,
        &config.tavily_api_key,
    )?;

    let stats = scout.run().await?;

    info!("Scout run complete. {stats}");

    Ok(())
}
