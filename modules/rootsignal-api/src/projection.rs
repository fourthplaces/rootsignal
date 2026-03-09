//! Projection lifecycle — unified live + replay behind `seesaw_replay::ProjectionStream`.
//!
//! `REPLAY=1 server` creates a fresh Neo4j database, replays all events, health checks, promotes, exits.
//! Normal `server` catches up from the pointer, then tails via PG NOTIFY.

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use sqlx::PgPool;
use tracing::{error, info, warn};

use rootsignal_common::{EmbeddingLookup, TextEmbedder};
use rootsignal_graph::{connect_graph, embedding_store::EmbeddingStore, query, GraphClient, GraphProjector};
use rootsignal_scout::core::postgres_store::PostgresStore;
use seesaw_replay::{Mode, PgNotifyTailSource, PgPointerStore, ProjectionStream};
use uuid::Uuid;

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

/// Resolve which Neo4j database to use based on mode.
///
/// Replay: create a fresh database with a timestamp name.
/// Live: use NEO4J_DB env var or default to "neo4j".
pub async fn resolve_neo4j_db(config: &rootsignal_common::Config) -> Result<String> {
    if Mode::from_env() == Mode::Replay {
        let db_name = format!(
            "projection_{}",
            chrono::Utc::now().format("%Y%m%d_%H%M%S")
        );
        let system = connect_graph(
            &config.neo4j_uri,
            &config.neo4j_user,
            &config.neo4j_password,
            "system",
        )
        .await?;
        ensure_neo4j_db(&system, &db_name).await?;
        Ok(db_name)
    } else {
        Ok(std::env::var("NEO4J_DB").unwrap_or_else(|_| "neo4j".into()))
    }
}

/// Create a Neo4j database if it doesn't exist and wait for it to come online.
async fn ensure_neo4j_db(system: &GraphClient, name: &str) -> Result<()> {
    info!(db = name, "Creating Neo4j database...");
    let cypher = format!("CREATE DATABASE `{name}` IF NOT EXISTS");
    system.run(query(&cypher)).await?;

    let deadline = Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let mut result = system
            .execute(query(&format!(
                "SHOW DATABASE `{name}` YIELD currentStatus RETURN currentStatus"
            )))
            .await?;
        if let Some(row) = result.next().await? {
            let status: String = row.get("currentStatus").unwrap_or_default();
            if status == "online" {
                info!(db = name, "Database online");
                return Ok(());
            }
            info!(db = name, status = status.as_str(), "Waiting for database...");
        }
        if Instant::now() > deadline {
            anyhow::bail!("Database {name} did not come online within 30s");
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

/// Health check: verify Source nodes exist and key constraints are present.
async fn health_check(graph: &GraphClient) -> Result<bool> {
    info!("Running health checks...");
    let mut passed = true;

    let labels = [
        "Source",
        "Gathering",
        "Resource",
        "HelpRequest",
        "Announcement",
        "Concern",
        "Actor",
    ];
    for label in labels {
        let cypher = format!("MATCH (n:{label}) RETURN count(n) AS c");
        let mut result = graph.execute(query(&cypher)).await?;
        let count: i64 = result
            .next()
            .await?
            .map(|r| r.get("c").unwrap_or(0))
            .unwrap_or(0);
        if count > 0 {
            info!("  {label}: {count} nodes");
        } else {
            warn!("  {label}: 0 nodes");
        }
    }

    let cypher = "MATCH (s:Source) RETURN count(s) AS c";
    let mut result = graph.execute(query(cypher)).await?;
    let source_count: i64 = result
        .next()
        .await?
        .map(|r| r.get("c").unwrap_or(0))
        .unwrap_or(0);
    if source_count == 0 {
        error!("FAIL: No Source nodes projected — projection is empty");
        passed = false;
    }

    let mut result = graph
        .execute(query("SHOW CONSTRAINTS YIELD name RETURN name"))
        .await?;
    let mut constraint_names: Vec<String> = Vec::new();
    while let Some(row) = result.next().await? {
        let name: String = row.get("name").unwrap_or_default();
        constraint_names.push(name);
    }
    for required in [
        "source_id_unique",
        "actor_id_unique",
        "gathering_id_unique",
    ] {
        if constraint_names.iter().any(|n| n == required) {
            info!("  Constraint {required}: present");
        } else {
            error!("FAIL: Constraint {required} missing");
            passed = false;
        }
    }

    if passed {
        info!("Health checks passed");
    }
    Ok(passed)
}

/// Run the full projection lifecycle (blocking).
///
/// In replay mode: creates fresh Neo4j DB, replays all events, health checks, promotes, exits.
/// In live mode: catches up from pointer, then tails indefinitely.
pub async fn run(pool: PgPool, graph: GraphClient) -> Result<()> {
    let embedding_store: Arc<dyn EmbeddingLookup> = Arc::new(EmbeddingStore::new(
        pool.clone(),
        Arc::new(NoOpEmbedder),
        "voyage-3-large".to_string(),
    ));
    let projector = Arc::new(
        GraphProjector::new(graph.clone()).with_embedding_store(embedding_store),
    );

    let log = PostgresStore::new(pool.clone(), Uuid::nil());
    let pointer = PgPointerStore::new(pool.clone()).await?;
    let tail = PgNotifyTailSource::new(&pool, "events").await?;

    let graph_for_gate = graph.clone();
    ProjectionStream::new(&log, &pointer)
        .tail(Box::new(tail))
        .promote_if(move || {
            let g = graph_for_gate.clone();
            async move { health_check(&g).await }
        })
        .run(|event| {
            let p = projector.clone();
            let event = event.clone();
            async move {
                p.project(&event).await?;
                Ok(())
            }
        })
        .await
}

/// Spawn projection stream as a background task (non-blocking).
pub fn spawn(pool: PgPool, graph: GraphClient) {
    tokio::spawn(async move {
        if let Err(e) = run(pool, graph).await {
            tracing::error!(error = %e, "ProjectionStream exited with error");
        }
    });
}
