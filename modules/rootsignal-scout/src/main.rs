use anyhow::Result;
use tracing::info;
use tracing_subscriber::EnvFilter;

use rootsignal_common::{CityNode, Config};
use rootsignal_graph::{cause_heat::compute_cause_heat, migrate::{migrate, backfill_source_diversity, backfill_source_canonical_keys}, GraphClient, GraphWriter};
use rootsignal_scout::{bootstrap, scout::Scout, scraper::TavilySearcher};

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

    // Connect to Memgraph
    let client =
        GraphClient::connect(&config.memgraph_uri, &config.memgraph_user, &config.memgraph_password)
            .await?;

    // Run migrations
    migrate(&client).await?;

    // Load or seed CityNode from graph
    let writer = GraphWriter::new(client.clone());
    let city_node = match writer.get_city(&config.city).await? {
        Some(node) => {
            info!(slug = node.slug.as_str(), name = node.name.as_str(), "Loaded city from graph");
            node
        }
        None => {
            // Cold start — create from env vars + bootstrap
            let city_name = config.city_name.as_deref()
                .unwrap_or(&config.city);
            let center_lat = config.city_lat
                .expect("CITY_LAT required for cold start (no city in graph)");
            let center_lng = config.city_lng
                .expect("CITY_LNG required for cold start (no city in graph)");
            let radius_km = config.city_radius_km.unwrap_or(30.0);

            info!(
                slug = config.city.as_str(),
                name = city_name,
                lat = center_lat,
                lng = center_lng,
                radius_km,
                "Cold start: creating CityNode from env vars"
            );

            let node = CityNode {
                id: uuid::Uuid::new_v4(),
                name: city_name.to_string(),
                slug: config.city.clone(),
                center_lat,
                center_lng,
                radius_km,
                geo_terms: vec![city_name.to_string()],
                active: true,
                created_at: chrono::Utc::now(),
            };
            writer.upsert_city(&node).await?;

            // Run cold start bootstrapper to generate seed sources
            let searcher = TavilySearcher::new(&config.tavily_api_key);
            let bootstrapper = bootstrap::ColdStartBootstrapper::new(
                &writer, &searcher, &config.anthropic_api_key, node.clone(),
            );
            let sources_created = bootstrapper.run().await?;
            info!(sources_created, "Cold start bootstrap complete");

            node
        }
    };

    // Backfill canonical keys on existing Source nodes (idempotent migration)
    backfill_source_canonical_keys(&client).await?;

    // Backfill source diversity for existing signals (no entity mappings — domain fallback handles it)
    backfill_source_diversity(&client, &[]).await?;

    // Create and run scout
    let scout = Scout::new(
        client.clone(),
        &config.anthropic_api_key,
        &config.voyage_api_key,
        &config.tavily_api_key,
        &config.apify_api_key,
        city_node,
        config.daily_budget_cents,
    )?;

    let stats = scout.run().await?;
    info!("Scout run complete. {stats}");

    // Compute cause heat (cross-story signal boosting via embedding similarity)
    compute_cause_heat(&client, 0.7).await?;

    Ok(())
}
