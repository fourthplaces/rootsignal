use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use serde::Serialize;
use sqlx::postgres::PgPoolOptions;
use tracing::info;
use tracing_subscriber::EnvFilter;

use rootsignal_common::{Config, Node, NodeType, ScoutScope, SituationNode};
use rootsignal_graph::{
    cause_heat::compute_cause_heat,
    migrate::{backfill_source_canonical_keys, backfill_source_diversity, migrate},
    query,
    reader::{node_type_label, row_to_node},
    GraphClient, GraphWriter, PublicGraphReader,
};
use rootsignal_scout::scout::Scout;

#[derive(Parser)]
#[command(about = "Run the Root Signal scout for a region")]
struct Cli {
    /// Region slug (e.g. "minneapolis"). Overrides REGION env var.
    region: Option<String>,

    /// Dump raw graph data (situations + signals) as JSON to stdout instead of running the scout.
    #[arg(long)]
    dump: bool,
}

#[derive(Serialize)]
struct DumpOutput {
    region: String,
    situations: Vec<SituationDump>,
    ungrouped_signals: Vec<Node>,
}

#[derive(Serialize)]
struct SituationDump {
    #[serde(flatten)]
    situation: SituationNode,
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

    // Load config, with optional CLI region override
    let cli = Cli::parse();
    let mut config = Config::scout_from_env();
    if let Some(region) = cli.region {
        config.region = region;
    }

    // Connect to Neo4j
    let client = GraphClient::connect(
        &config.neo4j_uri,
        &config.neo4j_user,
        &config.neo4j_password,
    )
    .await?;

    if cli.dump {
        return dump_region(&client, &config.region).await;
    }

    config.log_redacted();

    // Run migrations
    migrate(&client).await?;

    // Construct ScoutScope from env vars
    let region_name = config.region_name.as_deref().unwrap_or(&config.region);
    let center_lat = config
        .region_lat
        .expect("REGION_LAT required");
    let center_lng = config
        .region_lng
        .expect("REGION_LNG required");
    let radius_km = config.region_radius_km.unwrap_or(30.0);

    let region = ScoutScope {
        center_lat,
        center_lng,
        radius_km,
        name: region_name.to_string(),
        geo_terms: vec![region_name.to_string()],
    };

    info!(
        name = region.name.as_str(),
        lat = center_lat,
        lng = center_lng,
        radius_km,
        "Constructed ScoutScope from env vars"
    );

    // Backfill canonical keys on existing Source nodes (idempotent migration)
    backfill_source_canonical_keys(&client).await?;

    // Backfill source diversity for existing signals (no entity mappings — domain fallback handles it)
    backfill_source_diversity(&client, &[]).await?;

    // Save region geo bounds before moving region into Scout
    let region_name_key = region.name.clone();
    let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();

    // Connect to Postgres for the web archive
    let database_url = std::env::var("DATABASE_URL")
        .context("DATABASE_URL required for web archive")?;
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .context("Failed to connect to Postgres")?;

    // Create and run scout
    let scout = Scout::new(
        client.clone(),
        pool,
        &config.anthropic_api_key,
        &config.voyage_api_key,
        &config.serper_api_key,
        &config.apify_api_key,
        region,
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
        &region_name_key,
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

/// Dump all situations and signals for a region as raw JSON to stdout.
async fn dump_region(client: &GraphClient, region_slug: &str) -> Result<()> {
    // Construct geo bounds from env vars (same as main scout flow)
    let config = Config::scout_from_env();
    let center_lat = config
        .region_lat
        .context("REGION_LAT required for dump")?;
    let center_lng = config
        .region_lng
        .context("REGION_LNG required for dump")?;
    let radius_km = config.region_radius_km.unwrap_or(30.0);

    let scope = ScoutScope {
        center_lat,
        center_lng,
        radius_km,
        name: region_slug.to_string(),
        geo_terms: vec![region_slug.to_string()],
    };
    let (min_lat, max_lat, min_lng, max_lng) = scope.bounding_box();

    // Fetch all situations in the region's bounding box
    let reader = PublicGraphReader::new(client.clone());
    let situation_nodes = reader
        .situations_in_bounds(min_lat, max_lat, min_lng, max_lng, 500, None)
        .await?;

    let mut situations: Vec<SituationDump> = Vec::new();
    let mut grouped_signal_ids = std::collections::HashSet::new();

    // For each situation, fetch its signals via EVIDENCES edges
    for situation in situation_nodes {
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
                "MATCH (n:{label})-[:EVIDENCES]->(s:Situation {{id: $id}}) RETURN n ORDER BY n.confidence DESC"
            );
            let q = query(&cypher).param("id", situation.id.to_string());
            let mut sig_stream = client.inner().execute(q).await?;
            while let Some(row) = sig_stream.next().await? {
                if let Some(node) = row_to_node(&row, *nt) {
                    grouped_signal_ids.insert(node.id());
                    signals.push(node);
                }
            }
        }
        situations.push(SituationDump { situation, signals });
    }

    // Fetch ungrouped signals (not evidencing any situation) within the bounding box
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
               AND NOT (n)-[:EVIDENCES]->(:Situation)
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
        region: region_slug.to_string(),
        situations,
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
