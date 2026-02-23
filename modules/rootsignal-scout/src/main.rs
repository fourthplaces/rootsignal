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
    GraphClient, GraphWriter, PublicGraphReader, SimilarityBuilder,
};

use rootsignal_scout::infra::embedder::{Embedder, TextEmbedder};
use rootsignal_scout::pipeline::extractor::{Extractor, SignalExtractor};
use rootsignal_scout::pipeline::scrape_pipeline::ScrapePipeline;
use rootsignal_scout::scheduling::budget::{BudgetTracker, OperationCost};
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

    // Post-run: merge near-duplicate tensions before computing heat
    let merged = writer
        .merge_duplicate_tensions(0.85, min_lat, max_lat, min_lng, max_lng)
        .await?;
    if merged > 0 {
        info!(merged, "Merged duplicate tensions");
    }

    // Actor extraction — extract actors from signals that have none
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

    // Compute cause heat (cross-story signal boosting via embedding similarity)
    compute_cause_heat(&client, 0.7, min_lat, max_lat, min_lng, max_lng).await?;

    Ok(())
}

/// Run a full scout cycle: scrape → synthesis → situation weaving.
async fn run_full_scout(
    deps: &ScoutDeps,
    region: ScoutScope,
) -> Result<rootsignal_scout::pipeline::stats::ScoutStats> {
    let writer = GraphWriter::new(deps.graph_client.clone());
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

    // === Scrape pipeline ===
    let pipeline = ScrapePipeline::new(
        writer.clone(),
        extractor,
        embedder.clone(),
        archive.clone(),
        deps.anthropic_api_key.clone(),
        region.clone(),
        &budget,
        cancelled.clone(),
        run_id.clone(),
        deps.pg_pool.clone(),
    );
    let stats = pipeline.run_all().await?;

    // === Synthesis ===
    info!("Starting parallel synthesis...");

    let run_response_mapping = budget.has_budget(OperationCost::CLAUDE_HAIKU_SYNTHESIS * 10);
    let run_tension_linker = budget.has_budget(
        OperationCost::CLAUDE_HAIKU_TENSION_LINKER + OperationCost::SEARCH_TENSION_LINKER,
    );
    let run_response_finder = budget.has_budget(
        OperationCost::CLAUDE_HAIKU_RESPONSE_FINDER + OperationCost::SEARCH_RESPONSE_FINDER,
    );
    let run_gathering_finder = budget.has_budget(
        OperationCost::CLAUDE_HAIKU_GATHERING_FINDER + OperationCost::SEARCH_GATHERING_FINDER,
    );
    let run_investigation = budget.has_budget(
        OperationCost::CLAUDE_HAIKU_INVESTIGATION + OperationCost::SEARCH_INVESTIGATION,
    );

    let run_id_owned = run_id.clone();

    tokio::join!(
        async {
            info!("Building similarity edges...");
            let similarity = SimilarityBuilder::new(deps.graph_client.clone());
            similarity.clear_edges().await.unwrap_or_else(|e| {
                tracing::warn!(error = %e, "Failed to clear similarity edges");
                0
            });
            match similarity.build_edges().await {
                Ok(edges) => info!(edges, "Similarity edges built"),
                Err(e) => tracing::warn!(error = %e, "Similarity edge building failed (non-fatal)"),
            }
        },
        async {
            if run_response_mapping {
                info!("Starting response mapping...");
                let response_mapper = rootsignal_graph::response::ResponseMapper::new(
                    deps.graph_client.clone(),
                    &deps.anthropic_api_key,
                    region.center_lat,
                    region.center_lng,
                    region.radius_km,
                );
                match response_mapper.map_responses().await {
                    Ok(rm_stats) => info!("{rm_stats}"),
                    Err(e) => tracing::warn!(error = %e, "Response mapping failed (non-fatal)"),
                }
            } else if budget.is_active() {
                info!("Skipping response mapping (budget exhausted)");
            }
        },
        async {
            if run_tension_linker {
                info!("Starting tension linker...");
                let tension_linker = rootsignal_scout::discovery::tension_linker::TensionLinker::new(
                    &writer,
                    archive.clone(),
                    &*embedder,
                    &deps.anthropic_api_key,
                    region.clone(),
                    cancelled.clone(),
                    run_id_owned.clone(),
                );
                let tl_stats = tension_linker.run().await;
                info!("{tl_stats}");
            } else if budget.is_active() {
                info!("Skipping tension linker (budget exhausted)");
            }
        },
        async {
            if run_response_finder {
                info!("Starting response finder...");
                let response_finder = rootsignal_scout::discovery::response_finder::ResponseFinder::new(
                    &writer,
                    archive.clone(),
                    &*embedder,
                    &deps.anthropic_api_key,
                    region.clone(),
                    cancelled.clone(),
                    run_id_owned.clone(),
                );
                let rf_stats = response_finder.run().await;
                info!("{rf_stats}");
            } else if budget.is_active() {
                info!("Skipping response finder (budget exhausted)");
            }
        },
        async {
            if run_gathering_finder {
                info!("Starting gathering finder...");
                let gathering_finder = rootsignal_scout::discovery::gathering_finder::GatheringFinder::new(
                    &writer,
                    archive.clone(),
                    &*embedder,
                    &deps.anthropic_api_key,
                    region.clone(),
                    cancelled.clone(),
                    run_id_owned.clone(),
                );
                let gf_stats = gathering_finder.run().await;
                info!("{gf_stats}");
            } else if budget.is_active() {
                info!("Skipping gathering finder (budget exhausted)");
            }
        },
        async {
            if run_investigation {
                info!("Starting investigation phase...");
                let investigator = rootsignal_scout::enrichment::investigator::Investigator::new(
                    &writer,
                    archive.clone(),
                    &deps.anthropic_api_key,
                    &region,
                    cancelled.clone(),
                );
                let investigation_stats = investigator.run().await;
                info!("{investigation_stats}");
            } else if budget.is_active() {
                info!("Skipping investigation (budget exhausted)");
            }
        },
    );
    info!("Parallel synthesis complete");

    // === Situation weaving ===
    info!("Starting situation weaving...");
    let situation_weaver = rootsignal_graph::SituationWeaver::new(
        deps.graph_client.clone(),
        &deps.anthropic_api_key,
        embedder.clone(),
        region.clone(),
    );
    let has_situation_budget = budget.has_budget(OperationCost::CLAUDE_HAIKU_STORY_WEAVE);
    match situation_weaver.run(&run_id, has_situation_budget).await {
        Ok(sit_stats) => info!("{sit_stats}"),
        Err(e) => tracing::warn!(error = %e, "Situation weaving failed (non-fatal)"),
    }

    // Source boost for hot situations
    match writer.get_situation_landscape(20).await {
        Ok(situations) => {
            let hot: Vec<_> = situations.iter()
                .filter(|s| s.temperature >= 0.6 && s.sensitivity != "SENSITIVE" && s.sensitivity != "RESTRICTED")
                .collect();
            if !hot.is_empty() {
                info!(count = hot.len(), "Hot situations boosting source cadence");
                for sit in &hot {
                    if let Err(e) = writer.boost_sources_for_situation_headline(&sit.headline, 1.2).await {
                        tracing::warn!(error = %e, headline = sit.headline.as_str(), "Failed to boost sources");
                    }
                }
            }
        }
        Err(e) => tracing::warn!(error = %e, "Failed to fetch situation landscape"),
    }

    // Curiosity re-investigation
    match writer.trigger_situation_curiosity().await {
        Ok(0) => {}
        Ok(n) => info!(count = n, "Situations triggered curiosity re-investigation"),
        Err(e) => tracing::warn!(error = %e, "Failed to trigger situation curiosity"),
    }

    budget.log_status();
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
