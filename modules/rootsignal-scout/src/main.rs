use anyhow::Result;
use tracing::info;
use tracing_subscriber::EnvFilter;

use rootsignal_common::Config;
use rootsignal_graph::{cause_heat::compute_cause_heat, migrate::{migrate, backfill_source_diversity}, GraphClient};
use rootsignal_scout::{scout::Scout, sources};

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

    // Backfill source diversity for existing signals
    let profile = sources::city_profile(&config.city);
    let entity_mappings: Vec<_> = profile.entity_mappings.iter().map(|m| m.to_owned()).collect();
    backfill_source_diversity(&client, &entity_mappings).await?;

    // Create and run scout
    let scout = Scout::new(
        client.clone(),
        &config.anthropic_api_key,
        &config.voyage_api_key,
        &config.tavily_api_key,
        &config.apify_api_key,
        &config.city,
    )?;

    let stats = scout.run().await?;

    info!("Scout run complete. {stats}");

    // Compute cause heat (cross-story signal boosting via embedding similarity)
    compute_cause_heat(&client, 0.7).await?;

    Ok(())
}
