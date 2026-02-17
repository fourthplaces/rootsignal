use neo4rs::query;
use tracing::{info, warn};

use crate::GraphClient;

/// Run idempotent schema migrations: constraints, indexes.
/// Memgraph does not support IF NOT EXISTS — we ignore "already exists" errors.
pub async fn migrate(client: &GraphClient) -> Result<(), neo4rs::Error> {
    let g = &client.graph;

    info!("Running schema migrations...");

    // --- UUID uniqueness constraints ---
    let constraints = [
        "CREATE CONSTRAINT ON (n:Event) ASSERT n.id IS UNIQUE",
        "CREATE CONSTRAINT ON (n:Give) ASSERT n.id IS UNIQUE",
        "CREATE CONSTRAINT ON (n:Ask) ASSERT n.id IS UNIQUE",
        "CREATE CONSTRAINT ON (n:Notice) ASSERT n.id IS UNIQUE",
        "CREATE CONSTRAINT ON (n:Tension) ASSERT n.id IS UNIQUE",
        "CREATE CONSTRAINT ON (n:Evidence) ASSERT n.id IS UNIQUE",
    ];

    for c in &constraints {
        run_ignoring_exists(g, c).await?;
    }
    info!("UUID uniqueness constraints created");

    // --- Existence (NOT NULL) constraints ---
    // Memgraph supports these for free (Neo4j required Enterprise).
    let existence = [
        // Event
        "CREATE CONSTRAINT ON (n:Event) ASSERT EXISTS (n.sensitivity)",
        "CREATE CONSTRAINT ON (n:Event) ASSERT EXISTS (n.confidence)",
        // Give
        "CREATE CONSTRAINT ON (n:Give) ASSERT EXISTS (n.sensitivity)",
        "CREATE CONSTRAINT ON (n:Give) ASSERT EXISTS (n.confidence)",
        // Ask
        "CREATE CONSTRAINT ON (n:Ask) ASSERT EXISTS (n.sensitivity)",
        "CREATE CONSTRAINT ON (n:Ask) ASSERT EXISTS (n.confidence)",
        // Notice
        "CREATE CONSTRAINT ON (n:Notice) ASSERT EXISTS (n.sensitivity)",
        "CREATE CONSTRAINT ON (n:Notice) ASSERT EXISTS (n.confidence)",
        // Tension
        "CREATE CONSTRAINT ON (n:Tension) ASSERT EXISTS (n.sensitivity)",
        "CREATE CONSTRAINT ON (n:Tension) ASSERT EXISTS (n.confidence)",
    ];

    for e in &existence {
        run_ignoring_exists(g, e).await?;
    }
    info!("Existence constraints created");

    // --- Property indexes (lat/lng for bounding box queries) ---
    let indexes = [
        "CREATE INDEX ON :Event(lat)",
        "CREATE INDEX ON :Event(lng)",
        "CREATE INDEX ON :Give(lat)",
        "CREATE INDEX ON :Give(lng)",
        "CREATE INDEX ON :Ask(lat)",
        "CREATE INDEX ON :Ask(lng)",
        "CREATE INDEX ON :Notice(lat)",
        "CREATE INDEX ON :Notice(lng)",
        "CREATE INDEX ON :Tension(lat)",
        "CREATE INDEX ON :Tension(lng)",
    ];

    for idx in &indexes {
        run_ignoring_exists(g, idx).await?;
    }
    info!("Property indexes created");

    // --- Backfill lat/lng from point() locations, then drop point() property ---
    // neo4rs can't deserialize nodes that contain point() values (Memgraph serialization quirk),
    // so we store lat/lng as plain floats and remove the point() property entirely.
    let backfill = [
        "MATCH (n:Event) WHERE n.location IS NOT NULL AND n.lat IS NULL SET n.lat = n.location.y, n.lng = n.location.x",
        "MATCH (n:Give) WHERE n.location IS NOT NULL AND n.lat IS NULL SET n.lat = n.location.y, n.lng = n.location.x",
        "MATCH (n:Ask) WHERE n.location IS NOT NULL AND n.lat IS NULL SET n.lat = n.location.y, n.lng = n.location.x",
        "MATCH (n:Notice) WHERE n.location IS NOT NULL AND n.lat IS NULL SET n.lat = n.location.y, n.lng = n.location.x",
        "MATCH (n:Tension) WHERE n.location IS NOT NULL AND n.lat IS NULL SET n.lat = n.location.y, n.lng = n.location.x",
        "MATCH (n) WHERE n.location IS NOT NULL REMOVE n.location",
    ];

    for b in &backfill {
        match g.run(query(b)).await {
            Ok(_) => {}
            Err(e) => warn!("Backfill failed (non-fatal): {e}"),
        }
    }
    info!("Location backfill complete");

    // --- Full-text indexes ---
    let fulltext = [
        "CREATE TEXT INDEX event_text ON :Event(title, summary)",
        "CREATE TEXT INDEX give_text ON :Give(title, summary)",
        "CREATE TEXT INDEX ask_text ON :Ask(title, summary)",
        "CREATE TEXT INDEX notice_text ON :Notice(title, summary)",
        "CREATE TEXT INDEX tension_text ON :Tension(title, summary)",
    ];

    for f in &fulltext {
        run_ignoring_exists(g, f).await?;
    }
    info!("Full-text indexes created");

    // --- Vector indexes (1024-dim for Voyage embeddings) ---
    let vector = [
        r#"CREATE VECTOR INDEX event_embedding ON :Event(embedding) WITH CONFIG {"dimension": 1024, "capacity": 100000, "metric": "cos"}"#,
        r#"CREATE VECTOR INDEX give_embedding ON :Give(embedding) WITH CONFIG {"dimension": 1024, "capacity": 100000, "metric": "cos"}"#,
        r#"CREATE VECTOR INDEX ask_embedding ON :Ask(embedding) WITH CONFIG {"dimension": 1024, "capacity": 100000, "metric": "cos"}"#,
        r#"CREATE VECTOR INDEX notice_embedding ON :Notice(embedding) WITH CONFIG {"dimension": 1024, "capacity": 100000, "metric": "cos"}"#,
        r#"CREATE VECTOR INDEX tension_embedding ON :Tension(embedding) WITH CONFIG {"dimension": 1024, "capacity": 100000, "metric": "cos"}"#,
    ];

    for v in &vector {
        run_ignoring_exists(g, v).await?;
    }
    info!("Vector indexes created");

    // --- Story and ClusterSnapshot constraints ---
    let story_constraints = [
        "CREATE CONSTRAINT ON (n:Story) ASSERT n.id IS UNIQUE",
        "CREATE CONSTRAINT ON (n:ClusterSnapshot) ASSERT n.id IS UNIQUE",
    ];

    for c in &story_constraints {
        run_ignoring_exists(g, c).await?;
    }
    info!("Story/ClusterSnapshot constraints created");

    // --- Story indexes ---
    let story_indexes = [
        "CREATE INDEX ON :Story(energy)",
        "CREATE INDEX ON :Story(status)",
        "CREATE INDEX ON :Story(last_updated)",
    ];

    for idx in &story_indexes {
        run_ignoring_exists(g, idx).await?;
    }
    info!("Story indexes created");

    // --- Edge index for SIMILAR_TO weight ---
    // Memgraph supports edge indexes on properties
    let edge_indexes = [
        "CREATE INDEX ON :SIMILAR_TO(weight)",
    ];

    for idx in &edge_indexes {
        run_ignoring_exists(g, idx).await?;
    }
    info!("Edge indexes created");

    info!("Schema migration complete");
    Ok(())
}

/// Run a Cypher statement, ignoring errors that indicate the constraint/index already exists.
async fn run_ignoring_exists(
    g: &neo4rs::Graph,
    cypher: &str,
) -> Result<(), neo4rs::Error> {
    match g.run(query(cypher)).await {
        Ok(_) => Ok(()),
        Err(e) => {
            let msg = e.to_string().to_lowercase();
            if msg.contains("already exists") || msg.contains("equivalent") {
                warn!("Already exists (skipped): {}", cypher.chars().take(80).collect::<String>());
                Ok(())
            } else {
                Err(e)
            }
        }
    }
}
