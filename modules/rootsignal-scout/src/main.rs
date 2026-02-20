use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use serde::Serialize;
use tracing::info;
use tracing_subscriber::EnvFilter;

use rootsignal_common::{CityNode, Config, Node, NodeType, StoryNode};
use rootsignal_graph::{
    cause_heat::compute_cause_heat,
    migrate::{backfill_source_canonical_keys, backfill_source_diversity, migrate},
    query,
    reader::{node_type_label, row_to_node, row_to_story},
    GraphClient, GraphWriter,
};
use rootsignal_scout::{bootstrap, scout::Scout, scraper::SerperSearcher};

#[derive(Parser)]
#[command(about = "Run the Root Signal scout for a city")]
struct Cli {
    /// City slug (e.g. "minneapolis"). Overrides CITY env var.
    city: Option<String>,

    /// Dump raw graph data (stories + signals) as JSON to stdout instead of running the scout.
    #[arg(long)]
    dump: bool,
}

#[derive(Serialize)]
struct DumpOutput {
    city: String,
    stories: Vec<StoryDump>,
    ungrouped_signals: Vec<Node>,
}

#[derive(Serialize)]
struct StoryDump {
    #[serde(flatten)]
    story: StoryNode,
    signals: Vec<Node>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("rootsignal=info".parse()?))
        .init();

    info!("Root Signal Scout starting...");

    // Load .env from workspace root (doesn't override existing env vars)
    dotenv_load();

    // Load config, with optional CLI city override
    let cli = Cli::parse();
    let mut config = Config::scout_from_env();
    if let Some(city) = cli.city {
        config.city = city;
    }

    // Connect to Neo4j
    let client = GraphClient::connect(
        &config.neo4j_uri,
        &config.neo4j_user,
        &config.neo4j_password,
    )
    .await?;

    if cli.dump {
        return dump_city(&client, &config.city).await;
    }

    config.log_redacted();

    // Run migrations
    migrate(&client).await?;

    // Load or seed CityNode from graph
    let writer = GraphWriter::new(client.clone());
    let city_node = match writer.get_city(&config.city).await? {
        Some(node) => {
            info!(
                slug = node.slug.as_str(),
                name = node.name.as_str(),
                "Loaded city from graph"
            );
            node
        }
        None => {
            // Cold start — create from env vars + bootstrap
            let city_name = config.city_name.as_deref().unwrap_or(&config.city);
            let center_lat = config
                .city_lat
                .expect("CITY_LAT required for cold start (no city in graph)");
            let center_lng = config
                .city_lng
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
                last_scout_completed_at: None,
            };
            writer.upsert_city(&node).await?;

            // Run cold start bootstrapper to generate seed sources
            let searcher = SerperSearcher::new(&config.serper_api_key);
            let bootstrapper = bootstrap::Bootstrapper::new(
                &writer,
                &searcher,
                &config.anthropic_api_key,
                node.clone(),
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

    // Save city geo bounds before moving city_node into Scout
    let city_name = city_node.name.clone();
    let lat_delta = city_node.radius_km / 111.0;
    let lng_delta = city_node.radius_km / (111.0 * city_node.center_lat.to_radians().cos());
    let min_lat = city_node.center_lat - lat_delta;
    let max_lat = city_node.center_lat + lat_delta;
    let min_lng = city_node.center_lng - lng_delta;
    let max_lng = city_node.center_lng + lng_delta;

    // Create and run scout
    let scout = Scout::new(
        client.clone(),
        &config.anthropic_api_key,
        &config.voyage_api_key,
        &config.serper_api_key,
        &config.apify_api_key,
        city_node,
        config.daily_budget_cents,
        Arc::new(AtomicBool::new(false)),
    )?;

    let stats = scout.run().await?;
    info!("Scout run complete. {stats}");

    // Merge near-duplicate tensions before computing heat
    let writer_ref = GraphWriter::new(client.clone());
    let merged = writer_ref
        .merge_duplicate_tensions(0.85, min_lat, max_lat, min_lng, max_lng)
        .await?;
    if merged > 0 {
        info!(merged, "Merged duplicate tensions");
    }

    // Actor extraction — extract actors from signals that have none
    info!("Starting actor extraction...");
    let sweep_stats = rootsignal_scout::actor_extractor::run_actor_extraction(
        &writer_ref,
        &client,
        &config.anthropic_api_key,
        &city_name,
        min_lat,
        max_lat,
        min_lng,
        max_lng,
    )
    .await;
    info!("{sweep_stats}");

    // Compute cause heat (cross-story signal boosting via embedding similarity)
    compute_cause_heat(&client, 0.7, min_lat, max_lat, min_lng, max_lng).await?;

    Ok(())
}

/// Dump all stories and signals for a city as raw JSON to stdout.
async fn dump_city(client: &GraphClient, city_slug: &str) -> Result<()> {
    // Load city node to get geo bounds
    let writer = GraphWriter::new(client.clone());
    let city = writer
        .get_city(city_slug)
        .await?
        .with_context(|| format!("City '{}' not found in graph", city_slug))?;

    let lat_delta = city.radius_km / 111.0;
    let lng_delta = city.radius_km / (111.0 * city.center_lat.to_radians().cos());
    let min_lat = city.center_lat - lat_delta;
    let max_lat = city.center_lat + lat_delta;
    let min_lng = city.center_lng - lng_delta;
    let max_lng = city.center_lng + lng_delta;

    // Fetch all stories in the city's bounding box
    let story_q = query(
        "MATCH (s:Story)
         WHERE s.centroid_lat IS NOT NULL
           AND s.centroid_lat >= $min_lat AND s.centroid_lat <= $max_lat
           AND s.centroid_lng >= $min_lng AND s.centroid_lng <= $max_lng
         RETURN s
         ORDER BY s.energy DESC",
    )
    .param("min_lat", min_lat)
    .param("max_lat", max_lat)
    .param("min_lng", min_lng)
    .param("max_lng", max_lng);

    let mut stories: Vec<StoryDump> = Vec::new();
    let mut grouped_signal_ids = std::collections::HashSet::new();

    let mut stream = client.inner().execute(story_q).await?;
    let mut story_nodes = Vec::new();
    while let Some(row) = stream.next().await? {
        if let Some(story) = row_to_story(&row) {
            story_nodes.push(story);
        }
    }

    // For each story, fetch its raw signals (no filtering, no fuzzing)
    for story in story_nodes {
        let mut signals = Vec::new();
        for nt in &[
            NodeType::Gathering,
            NodeType::Aid,
            NodeType::Need,
            NodeType::Notice,
            NodeType::Tension,
        ] {
            let label = node_type_label(*nt);
            let cypher = format!(
                "MATCH (s:Story {{id: $id}})-[:CONTAINS]->(n:{label}) RETURN n ORDER BY n.confidence DESC"
            );
            let q = query(&cypher).param("id", story.id.to_string());
            let mut sig_stream = client.inner().execute(q).await?;
            while let Some(row) = sig_stream.next().await? {
                if let Some(node) = row_to_node(&row, *nt) {
                    grouped_signal_ids.insert(node.id());
                    signals.push(node);
                }
            }
        }
        stories.push(StoryDump { story, signals });
    }

    // Fetch ungrouped signals (not in any story) within the bounding box
    let mut ungrouped = Vec::new();
    for nt in &[
        NodeType::Gathering,
        NodeType::Aid,
        NodeType::Need,
        NodeType::Notice,
        NodeType::Tension,
    ] {
        let label = node_type_label(*nt);
        let cypher = format!(
            "MATCH (n:{label})
             WHERE n.lat >= $min_lat AND n.lat <= $max_lat
               AND n.lng >= $min_lng AND n.lng <= $max_lng
               AND NOT (n)<-[:CONTAINS]-(:Story)
             RETURN n
             ORDER BY n.confidence DESC"
        );
        let q = query(&cypher)
            .param("min_lat", min_lat)
            .param("max_lat", max_lat)
            .param("min_lng", min_lng)
            .param("max_lng", max_lng);
        let mut stream = client.inner().execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Some(node) = row_to_node(&row, *nt) {
                ungrouped.push(node);
            }
        }
    }

    let output = DumpOutput {
        city: city_slug.to_string(),
        stories,
        ungrouped_signals: ungrouped,
    };

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn dotenv_load() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(".env");
    if let Ok(content) = std::fs::read_to_string(&path) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                if std::env::var(key.trim()).is_err() {
                    std::env::set_var(key.trim(), value.trim());
                }
            }
        }
    }
}
