use anyhow::Result;
use clap::Parser;
use rootsignal_common::{EmbeddingLookup, TextEmbedder};
use rootsignal_events::EventStore;
use rootsignal_graph::{connect_graph, query, embedding_store::EmbeddingStore, GraphClient, GraphProjector};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info};

/// No-op embedder for replay — the EmbeddingStore cache serves cached embeddings;
/// cache misses return empty and the projector skips them.
struct NoOpEmbedder;

#[async_trait::async_trait]
impl TextEmbedder for NoOpEmbedder {
    async fn embed(&self, _text: &str) -> Result<Vec<f32>> {
        Ok(vec![])
    }
    async fn embed_batch(&self, _texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        Ok(vec![])
    }
}

#[derive(Parser)]
#[command(name = "replay", about = "Replay events from Postgres into Neo4j")]
struct Cli {
    /// Actually execute the replay. Without this flag, only prints event count.
    #[arg(long)]
    commit: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "replay=info".into()),
        )
        .init();

    let cli = Cli::parse();

    let database_url = std::env::var("DATABASE_URL")?;
    let neo4j_uri = std::env::var("NEO4J_URI")?;
    let neo4j_user = std::env::var("NEO4J_USER")?;
    let neo4j_password = std::env::var("NEO4J_PASSWORD")?;

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;

    let graph = connect_graph(&neo4j_uri, &neo4j_user, &neo4j_password).await?;
    let store = EventStore::new(pool.clone());

    let embedding_store: Arc<dyn EmbeddingLookup> = Arc::new(EmbeddingStore::new(
        pool,
        Arc::new(NoOpEmbedder),
        "voyage-3-large".to_string(),
    ));
    let projector = GraphProjector::new(graph.clone())
        .with_embedding_store(embedding_store);

    let total = store.latest_seq().await?;
    info!("Event store contains {total} events");

    if !cli.commit {
        info!("Dry run: {total} events would be replayed. Pass --commit to execute.");
        return Ok(());
    }

    let started = Instant::now();

    // Ensure schema constraints and indexes exist before replaying.
    info!("Running Neo4j migrations...");
    rootsignal_graph::migrate::migrate(&graph).await?;
    info!("Migrations complete.");

    // Batched wipe — delete in chunks to avoid Neo4j memory pressure.
    info!("Wiping Neo4j graph...");
    loop {
        let mut result = graph
            .execute(query("MATCH (n) WITH n LIMIT 10000 DETACH DELETE n RETURN count(*)"))
            .await?;
        let row = result.next().await?;
        let deleted: i64 = row.map(|r| r.get("count(*)").unwrap_or(0)).unwrap_or(0);
        if deleted == 0 {
            break;
        }
        info!("  deleted {deleted} nodes...");
    }
    info!("Graph wiped.");

    // Replay loop
    let batch_size = 1000;
    let mut cursor: i64 = 1;
    let mut applied: i64 = 0;
    let mut no_op: i64 = 0;
    let mut errors: i64 = 0;

    loop {
        let events = store.read_from(cursor, batch_size).await?;
        if events.is_empty() {
            break;
        }

        for event in &events {
            match projector.project(event).await {
                Ok(rootsignal_graph::ApplyResult::Applied) => applied += 1,
                Ok(rootsignal_graph::ApplyResult::NoOp) => no_op += 1,
                Ok(rootsignal_graph::ApplyResult::DeserializeError(msg)) => {
                    error!("seq {}: deserialize error: {msg}", event.seq);
                    errors += 1;
                }
                Err(e) => {
                    error!("seq {}: projection error: {e:#}", event.seq);
                    errors += 1;
                }
            }
        }

        cursor = events.last().unwrap().seq + 1;
        let processed = applied + no_op + errors;
        info!("Replayed {processed}/{total} events");
    }

    let elapsed = started.elapsed();
    info!(
        "Done in {:.1}s — applied: {applied}, no-op: {no_op}, errors: {errors}",
        elapsed.as_secs_f64()
    );

    Ok(())
}
