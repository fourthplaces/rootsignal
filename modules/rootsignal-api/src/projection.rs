//! Projection lifecycle — unified live + replay behind `seesaw_replay::ProjectionStream`.
//!
//! `REPLAY=1 server` replays all events into a versioned Neo4j database,
//! health checks, promotes, exits.
//! Normal `server` catches up from the promoted pointer, then tails via PG NOTIFY.
//!
//! The position IS the version. `neo4j.v48050` means "built from the first
//! 48050 events in the log." `stream.version()` handles both modes.

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use sqlx::PgPool;
use tracing::{error, info, warn};

use rootsignal_common::{EmbeddingLookup, TextEmbedder};
use rootsignal_graph::{connect_graph, embedding_store::EmbeddingStore, query, GraphClient, GraphProjector};
use rootsignal_scout::core::postgres_store::PostgresStore;
use seesaw_replay::{PgNotifyTailSource, PgPointerStore, PointerStore, ProjectionStream};
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
/// Builds the stream, derives the DB version, creates/connects Neo4j,
/// then runs. Same code path for replay and live — `stream.version()`
/// handles the mode difference.
pub async fn run(pool: PgPool, config: &rootsignal_common::Config) -> Result<GraphClient> {
    let log = PostgresStore::new(pool.clone(), Uuid::nil());
    let pointer = PgPointerStore::new(pool.clone()).await?;
    let tail = PgNotifyTailSource::new(&pool, "events").await?;

    let stream = ProjectionStream::new(&log, &pointer)
        .tail(Box::new(tail));

    let version = stream.version().await?;
    let neo4j_db = match std::env::var("NEO4J_DB") {
        Ok(db) => db,
        Err(_) => format!("neo4j.v{version}"),
    };
    info!(db = neo4j_db.as_str(), version, "Projection target");

    let system = connect_graph(
        &config.neo4j_uri,
        &config.neo4j_user,
        &config.neo4j_password,
        "system",
    )
    .await?;
    ensure_neo4j_db(&system, &neo4j_db).await?;

    let graph = connect_graph(
        &config.neo4j_uri,
        &config.neo4j_user,
        &config.neo4j_password,
        &neo4j_db,
    )
    .await?;

    rootsignal_graph::migrate::migrate(&graph)
        .await
        .map_err(|e| anyhow::anyhow!("Neo4j migration failed: {e}"))?;

    let embedding_store: Arc<dyn EmbeddingLookup> = Arc::new(EmbeddingStore::new(
        pool.clone(),
        Arc::new(NoOpEmbedder),
        "voyage-3-large".to_string(),
    ));
    let projector = Arc::new(
        GraphProjector::new(graph.clone()).with_embedding_store(embedding_store),
    );

    let pb = indicatif::ProgressBar::new(version)
        .with_style(
            indicatif::ProgressStyle::with_template(
                "{bar:40.cyan/blue} {pos}/{len} events ({percent}%) [{elapsed_precise}]",
            )
            .unwrap(),
        );

    let graph_for_gate = graph.clone();
    stream
        .batch_size(5000)
        .on_progress(move |p| {
            pb.set_position(p.position);
        })
        .promote_if(move || {
            let g = graph_for_gate.clone();
            async move { health_check(&g).await }
        })
        .run_batch(|events| {
            let p = projector.clone();
            let owned = events.to_vec();
            async move { p.project_batch(&owned).await }
        })
        .await?;

    Ok(graph)
}

/// Spawn projection stream as a background task (non-blocking).
/// Returns the GraphClient connected to the versioned DB.
pub async fn start(pool: PgPool, config: &rootsignal_common::Config) -> Result<GraphClient> {
    let pointer = PgPointerStore::new(pool.clone()).await?;
    let version = pointer.version().await?.unwrap_or(0);
    let neo4j_db = match std::env::var("NEO4J_DB") {
        Ok(db) => db,
        Err(_) => format!("neo4j.v{version}"),
    };
    info!(db = neo4j_db.as_str(), version, "Projection target");

    let system = connect_graph(
        &config.neo4j_uri,
        &config.neo4j_user,
        &config.neo4j_password,
        "system",
    )
    .await?;
    ensure_neo4j_db(&system, &neo4j_db).await?;

    let graph = connect_graph(
        &config.neo4j_uri,
        &config.neo4j_user,
        &config.neo4j_password,
        &neo4j_db,
    )
    .await?;

    rootsignal_graph::migrate::migrate(&graph)
        .await
        .map_err(|e| anyhow::anyhow!("Neo4j migration failed: {e}"))?;

    let embedding_store: Arc<dyn EmbeddingLookup> = Arc::new(EmbeddingStore::new(
        pool.clone(),
        Arc::new(NoOpEmbedder),
        "voyage-3-large".to_string(),
    ));
    let projector = Arc::new(
        GraphProjector::new(graph.clone()).with_embedding_store(embedding_store),
    );

    let graph_ret = graph.clone();
    let graph_for_gate = graph.clone();
    tokio::spawn(async move {
        let log = PostgresStore::new(pool.clone(), Uuid::nil());
        let tail = match PgNotifyTailSource::new(&pool, "events").await {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(error = %e, "Failed to create tail source");
                return;
            }
        };

        let stream = ProjectionStream::new(&log, &pointer)
            .tail(Box::new(tail));

        let result = stream
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
            .await;

        if let Err(e) = result {
            tracing::error!(error = %e, "ProjectionStream exited with error");
        }
    });

    Ok(graph_ret)
}
