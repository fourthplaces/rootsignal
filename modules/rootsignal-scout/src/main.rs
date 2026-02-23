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
    migrate::{backfill_source_canonical_keys, backfill_source_diversity, migrate},
    query,
    reader::{node_type_label, row_to_node},
    GraphClient, GraphWriter, PublicGraphReader,
};

use rootsignal_scout::infra::embedder::{Embedder, TextEmbedder};
use rootsignal_scout::pipeline::extractor::{Extractor, SignalExtractor};
use rootsignal_scout::pipeline::scrape_pipeline::ScrapePipeline;
use rootsignal_scout::scheduling::budget::BudgetTracker;
use rootsignal_scout::workflows::{create_archive, ScoutDeps};

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

    // Save region geo bounds before moving region into pipeline
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

    // Build shared deps (same struct the Restate workflows use)
    let deps = ScoutDeps::builder()
        .graph_client(client.clone())
        .pg_pool(pool)
        .anthropic_api_key(config.anthropic_api_key.clone())
        .voyage_api_key(config.voyage_api_key.clone())
        .serper_api_key(config.serper_api_key.clone())
        .apify_api_key(config.apify_api_key.clone())
        .daily_budget_cents(config.daily_budget_cents)
        .browserless_url(config.browserless_url.clone())
        .browserless_token(config.browserless_token.clone())
        .build();

    let writer = GraphWriter::new(deps.graph_client.clone());
    let region_slug = rootsignal_common::slugify(&region.name);

    // Transition region status to running (acts as lock)
    let allowed = &["idle", "bootstrap_complete", "actor_discovery_complete",
        "scrape_complete", "synthesis_complete", "situation_weaver_complete", "complete"];
    if !writer
        .transition_region_status(&region_slug, allowed, "running_bootstrap")
        .await
        .context("Failed to check region run status")?
    {
        anyhow::bail!("Another scout run is in progress for {}", region.name);
    }

    let result = run_full_scout(&deps, region).await;

    // Set status to complete or reset to idle on failure
    let final_status = if result.is_ok() { "complete" } else { "idle" };
    if let Err(e) = writer.set_region_run_status(&region_slug, final_status).await {
        tracing::error!("Failed to set region run status: {e}");
    }

    let stats = result?;
    info!("Scout run complete. {stats}");

    // Actor extraction — extract actors from signals that have none.
    // Not yet part of any workflow, so it runs here post-run.
    info!("Starting actor extraction...");
    let sweep_stats = rootsignal_scout::enrichment::actor_extractor::run_actor_extraction(
        &writer,
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

    Ok(())
}

/// Run a full scout cycle: scrape → synthesis → situation weaving → supervisor.
///
/// Delegates to the same functions that the Restate workflows use, avoiding duplication.
async fn run_full_scout(
    deps: &ScoutDeps,
    region: ScoutScope,
) -> Result<rootsignal_scout::pipeline::stats::ScoutStats> {
    let extractor: Arc<dyn SignalExtractor> = Arc::new(Extractor::new(
        &deps.anthropic_api_key,
        region.name.as_str(),
        region.center_lat,
        region.center_lng,
    ));
    let embedder: Arc<dyn TextEmbedder> =
        Arc::new(Embedder::new(&deps.voyage_api_key));
    let archive = create_archive(deps);
    let budget = BudgetTracker::new(deps.daily_budget_cents);
    let cancelled = Arc::new(AtomicBool::new(false));
    let run_id = uuid::Uuid::new_v4().to_string();
    let writer = GraphWriter::new(deps.graph_client.clone());

    // === Scrape pipeline ===
    let pipeline = ScrapePipeline::new(
        writer,
        extractor,
        embedder,
        archive,
        deps.anthropic_api_key.clone(),
        region.clone(),
        &budget,
        cancelled,
        run_id,
        deps.pg_pool.clone(),
    );
    let stats = pipeline.run_all().await?;

    let spent_so_far = budget.total_spent();

    // === Synthesis (parallel finders + similarity edges) ===
    let synthesis_result = rootsignal_scout::workflows::synthesis::run_synthesis_from_deps(
        deps, &region, spent_so_far,
    ).await?;

    // === Situation weaving + source boost + curiosity re-investigation ===
    let _weaver_result = rootsignal_scout::workflows::situation_weaver::run_situation_weaving_from_deps(
        deps, &region, synthesis_result.spent_cents,
    ).await?;

    // === Supervisor (merge tensions, compute cause heat, detect beacons) ===
    let _supervisor_result = rootsignal_scout::workflows::supervisor::run_supervisor_pipeline(
        deps, &region,
    ).await?;

    Ok(stats)
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
