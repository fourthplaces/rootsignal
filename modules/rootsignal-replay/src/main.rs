use anyhow::Result;
use clap::Parser;
use rootsignal_common::{EmbeddingLookup, TextEmbedder};
use rootsignal_events::EventStore;
use rootsignal_graph::{connect_graph, query, embedding_store::EmbeddingStore, GraphClient, GraphProjector};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
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
    /// Full rebuild: wipe Neo4j and replay all events from scratch.
    #[arg(long)]
    commit: bool,

    /// Incremental: resume from last checkpoint, projecting only new events.
    #[arg(long)]
    resume: bool,
}

const PROJECTOR_NAME: &str = "neo4j";
const BATCH_SIZE: usize = 1000;
const CHECKPOINT_INTERVAL: i64 = 500;

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

    if !cli.commit && !cli.resume {
        let database_url = std::env::var("DATABASE_URL")?;
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await?;
        let store = EventStore::new(pool.clone());
        let total = store.latest_seq().await?;
        let checkpoint = read_checkpoint(&pool).await?;
        info!("Event store contains {total} events");
        if let Some(seq) = checkpoint {
            info!("Last checkpoint: seq {seq} ({} events to catch up)", total - seq);
        } else {
            info!("No checkpoint found");
        }
        info!("Pass --commit for full rebuild or --resume for incremental replay.");
        return Ok(());
    }

    if cli.commit && cli.resume {
        anyhow::bail!("Cannot use --commit and --resume together");
    }

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
        pool.clone(),
        Arc::new(NoOpEmbedder),
        "voyage-3-large".to_string(),
    ));
    let projector = GraphProjector::new(graph.clone())
        .with_embedding_store(embedding_store);

    let total = store.latest_seq().await?;
    info!("Event store contains {total} events");

    let started = Instant::now();

    let start_seq = if cli.commit {
        reset_checkpoint(&pool).await?;
        full_rebuild(&graph).await?;
        1
    } else {
        let checkpoint = read_checkpoint(&pool).await?;
        match checkpoint {
            Some(seq) => {
                info!("Resuming from checkpoint seq {seq}");
                seq + 1
            }
            None => {
                info!("No checkpoint found, starting from seq 1");
                1
            }
        }
    };

    let (applied, no_op, errors) = replay_loop(&store, &projector, &pool, start_seq, total).await?;

    let elapsed = started.elapsed();
    info!(
        "Done in {:.1}s — applied: {applied}, no-op: {no_op}, errors: {errors}",
        elapsed.as_secs_f64()
    );

    Ok(())
}

async fn full_rebuild(graph: &GraphClient) -> Result<()> {
    info!("Running Neo4j migrations...");
    rootsignal_graph::migrate::migrate(graph).await?;
    info!("Migrations complete.");

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
    Ok(())
}

async fn replay_loop(
    store: &EventStore,
    projector: &GraphProjector,
    pool: &PgPool,
    start_seq: i64,
    total: i64,
) -> Result<(i64, i64, i64)> {
    let mut cursor = start_seq;
    let mut applied: i64 = 0;
    let mut no_op: i64 = 0;
    let mut errors: i64 = 0;
    let mut since_checkpoint: i64 = 0;

    loop {
        let events = store.read_from(cursor, BATCH_SIZE).await?;
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

        let last_seq = events.last().unwrap().seq;
        cursor = last_seq + 1;
        since_checkpoint += events.len() as i64;

        if since_checkpoint >= CHECKPOINT_INTERVAL {
            write_checkpoint(pool, last_seq).await?;
            since_checkpoint = 0;
        }

        let processed = applied + no_op + errors;
        info!("Replayed {processed}/{total} events (cursor: {cursor})");
    }

    // Final checkpoint
    if cursor > start_seq {
        write_checkpoint(pool, cursor - 1).await?;
    }

    Ok((applied, no_op, errors))
}

async fn read_checkpoint(pool: &PgPool) -> Result<Option<i64>> {
    let result = sqlx::query_as::<_, (i64,)>(
        "SELECT last_seq FROM replay_checkpoints WHERE projector_name = $1",
    )
    .bind(PROJECTOR_NAME)
    .fetch_optional(pool)
    .await;

    match result {
        Ok(row) => Ok(row.map(|(seq,)| seq)),
        Err(e) => {
            // Table may not exist if migration 018 hasn't been applied
            let msg = e.to_string();
            if msg.contains("replay_checkpoints") && msg.contains("does not exist") {
                Ok(None)
            } else {
                Err(e.into())
            }
        }
    }
}

async fn write_checkpoint(pool: &PgPool, seq: i64) -> Result<()> {
    let result = sqlx::query(
        "INSERT INTO replay_checkpoints (projector_name, last_seq, updated_at)
         VALUES ($1, $2, now())
         ON CONFLICT (projector_name)
         DO UPDATE SET last_seq = $2, updated_at = now()",
    )
    .bind(PROJECTOR_NAME)
    .bind(seq)
    .execute(pool)
    .await;

    // Skip silently if table doesn't exist (migration 018 not applied)
    if let Err(e) = &result {
        let msg = e.to_string();
        if msg.contains("replay_checkpoints") && msg.contains("does not exist") {
            return Ok(());
        }
    }
    result?;
    Ok(())
}

async fn reset_checkpoint(pool: &PgPool) -> Result<()> {
    let result = sqlx::query(
        "DELETE FROM replay_checkpoints WHERE projector_name = $1",
    )
    .bind(PROJECTOR_NAME)
    .execute(pool)
    .await;

    // Ignore if table doesn't exist
    if let Err(e) = &result {
        let msg = e.to_string();
        if msg.contains("replay_checkpoints") && msg.contains("does not exist") {
            return Ok(());
        }
    }
    result?;
    Ok(())
}
