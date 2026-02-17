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

    // --- Source diversity indexes ---
    let diversity_indexes = [
        "CREATE INDEX ON :Event(source_diversity)",
        "CREATE INDEX ON :Give(source_diversity)",
        "CREATE INDEX ON :Ask(source_diversity)",
        "CREATE INDEX ON :Notice(source_diversity)",
        "CREATE INDEX ON :Tension(source_diversity)",
    ];

    for idx in &diversity_indexes {
        run_ignoring_exists(g, idx).await?;
    }
    info!("Source diversity indexes created");

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

    // --- Story synthesis indexes ---
    let synthesis_indexes = [
        "CREATE INDEX ON :Story(arc)",
        "CREATE INDEX ON :Story(category)",
    ];

    for idx in &synthesis_indexes {
        run_ignoring_exists(g, idx).await?;
    }
    info!("Story synthesis indexes created");

    // --- Actor constraints and indexes ---
    let actor_constraints = [
        "CREATE CONSTRAINT ON (a:Actor) ASSERT a.id IS UNIQUE",
        "CREATE CONSTRAINT ON (a:Actor) ASSERT a.entity_id IS UNIQUE",
    ];

    for c in &actor_constraints {
        run_ignoring_exists(g, c).await?;
    }

    let actor_indexes = [
        "CREATE INDEX ON :Actor(name)",
        "CREATE INDEX ON :Actor(city)",
    ];

    for idx in &actor_indexes {
        run_ignoring_exists(g, idx).await?;
    }
    info!("Actor constraints and indexes created");

    // --- Edition constraints and indexes ---
    let edition_constraints = [
        "CREATE CONSTRAINT ON (e:Edition) ASSERT e.id IS UNIQUE",
    ];

    for c in &edition_constraints {
        run_ignoring_exists(g, c).await?;
    }

    let edition_indexes = [
        "CREATE INDEX ON :Edition(city)",
        "CREATE INDEX ON :Edition(period)",
    ];

    for idx in &edition_indexes {
        run_ignoring_exists(g, idx).await?;
    }
    info!("Edition constraints and indexes created");

    // --- Edge index for SIMILAR_TO weight ---
    // Memgraph supports edge indexes on properties
    let edge_indexes = [
        "CREATE INDEX ON :SIMILAR_TO(weight)",
    ];

    for idx in &edge_indexes {
        run_ignoring_exists(g, idx).await?;
    }
    info!("Edge indexes created");

    // --- Source node constraints and indexes ---
    let source_constraints = [
        "CREATE CONSTRAINT ON (s:Source) ASSERT s.id IS UNIQUE",
        "CREATE CONSTRAINT ON (s:Source) ASSERT s.url IS UNIQUE",
    ];

    for c in &source_constraints {
        run_ignoring_exists(g, c).await?;
    }

    let source_indexes = [
        "CREATE INDEX ON :Source(city)",
        "CREATE INDEX ON :Source(active)",
    ];

    for idx in &source_indexes {
        run_ignoring_exists(g, idx).await?;
    }
    info!("Source constraints and indexes created");

    // --- BlockedSource constraint ---
    run_ignoring_exists(g, "CREATE CONSTRAINT ON (b:BlockedSource) ASSERT b.url_pattern IS UNIQUE").await?;
    info!("BlockedSource constraint created");

    info!("Schema migration complete");
    Ok(())
}

/// Backfill source_diversity and external_ratio for all existing signal nodes.
/// Traverses SOURCED_FROM edges to count unique entity sources per signal.
pub async fn backfill_source_diversity(
    client: &GraphClient,
    entity_mappings: &[rootsignal_common::EntityMappingOwned],
) -> Result<(), neo4rs::Error> {
    let g = &client.graph;

    info!("Backfilling source diversity...");

    let mut total = 0u32;
    let mut updated = 0u32;

    for label in &["Event", "Give", "Ask", "Notice", "Tension"] {
        let q = query(&format!(
            "MATCH (n:{label})
             OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
             RETURN n.id AS id, n.source_url AS self_url,
                    collect(ev.source_url) AS evidence_urls"
        ));

        let mut stream = g.execute(q).await?;
        while let Some(row) = stream.next().await? {
            total += 1;
            let id: String = row.get("id").unwrap_or_default();
            let self_url: String = row.get("self_url").unwrap_or_default();
            let evidence_urls: Vec<String> = row.get("evidence_urls").unwrap_or_default();

            let self_entity = rootsignal_common::resolve_entity(&self_url, entity_mappings);

            let mut entities = std::collections::HashSet::new();
            let mut external_count = 0u32;
            let evidence_total = evidence_urls.len() as u32;

            for url in &evidence_urls {
                let entity = rootsignal_common::resolve_entity(url, entity_mappings);
                entities.insert(entity.clone());
                if entity != self_entity {
                    external_count += 1;
                }
            }

            let diversity = entities.len().max(1) as u32;
            let external_ratio = if evidence_total > 0 {
                external_count as f64 / evidence_total as f64
            } else {
                0.0
            };

            let update = query(&format!(
                "MATCH (n:{label} {{id: $id}})
                 SET n.source_diversity = $diversity, n.external_ratio = $ratio"
            ))
            .param("id", id)
            .param("diversity", diversity as i64)
            .param("ratio", external_ratio);

            g.run(update).await?;
            updated += 1;
        }
    }

    info!(total, updated, "Source diversity backfill complete");
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
