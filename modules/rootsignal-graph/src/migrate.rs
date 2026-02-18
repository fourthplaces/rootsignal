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
    // Memgraph supports these natively.
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

    // --- Cause heat indexes ---
    let heat_indexes = [
        "CREATE INDEX ON :Event(cause_heat)",
        "CREATE INDEX ON :Give(cause_heat)",
        "CREATE INDEX ON :Ask(cause_heat)",
        "CREATE INDEX ON :Notice(cause_heat)",
        "CREATE INDEX ON :Tension(cause_heat)",
    ];

    for idx in &heat_indexes {
        run_ignoring_exists(g, idx).await?;
    }
    info!("Cause heat indexes created");

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

    // --- City node constraints and indexes ---
    let city_constraints = [
        "CREATE CONSTRAINT ON (c:City) ASSERT c.id IS UNIQUE",
        "CREATE CONSTRAINT ON (c:City) ASSERT c.slug IS UNIQUE",
    ];

    for c in &city_constraints {
        run_ignoring_exists(g, c).await?;
    }

    run_ignoring_exists(g, "CREATE INDEX ON :City(active)").await?;
    info!("City constraints and indexes created");

    // --- Source node constraints and indexes ---
    let source_constraints = [
        "CREATE CONSTRAINT ON (s:Source) ASSERT s.id IS UNIQUE",
        "CREATE CONSTRAINT ON (s:Source) ASSERT s.canonical_key IS UNIQUE",
    ];

    for c in &source_constraints {
        run_ignoring_exists(g, c).await?;
    }

    // Drop legacy url uniqueness constraint (canonical_key is the new identity)
    drop_ignoring_missing(g, "DROP CONSTRAINT ON (s:Source) ASSERT s.url IS UNIQUE").await;

    let source_indexes = [
        "CREATE INDEX ON :Source(city)",
        "CREATE INDEX ON :Source(active)",
        "CREATE INDEX ON :Source(url)",
        "CREATE INDEX ON :Source(source_type)",
        "CREATE INDEX ON :Source(weight)",
    ];

    for idx in &source_indexes {
        run_ignoring_exists(g, idx).await?;
    }
    info!("Source constraints and indexes created");

    // --- BlockedSource constraint ---
    run_ignoring_exists(g, "CREATE CONSTRAINT ON (b:BlockedSource) ASSERT b.url_pattern IS UNIQUE").await?;
    info!("BlockedSource constraint created");

    // --- Supervisor node constraints and indexes ---
    let supervisor_constraints = [
        "CREATE CONSTRAINT ON (s:SupervisorState) ASSERT s.id IS UNIQUE",
        "CREATE CONSTRAINT ON (r:ExtractionRule) ASSERT r.id IS UNIQUE",
        "CREATE CONSTRAINT ON (v:ValidationIssue) ASSERT v.id IS UNIQUE",
    ];

    for c in &supervisor_constraints {
        run_ignoring_exists(g, c).await?;
    }

    let supervisor_indexes = [
        "CREATE INDEX ON :ValidationIssue(status)",
        "CREATE INDEX ON :ValidationIssue(city)",
        "CREATE INDEX ON :ValidationIssue(target_id)",
        "CREATE INDEX ON :ExtractionRule(city)",
        "CREATE INDEX ON :ExtractionRule(approved)",
        "CREATE INDEX ON :SupervisorState(city)",
    ];

    for idx in &supervisor_indexes {
        run_ignoring_exists(g, idx).await?;
    }
    info!("Supervisor constraints and indexes created");

    // --- Deduplicate evidence + recompute corroboration ---
    deduplicate_evidence(client).await?;

    // --- Backfill event dates: clean up string/empty dates ---
    backfill_event_dates(client).await?;

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

/// Drop a constraint/index, ignoring errors if it doesn't exist.
async fn drop_ignoring_missing(g: &neo4rs::Graph, cypher: &str) {
    match g.run(query(cypher)).await {
        Ok(_) => info!("Dropped: {}", cypher.chars().take(80).collect::<String>()),
        Err(e) => {
            let msg = e.to_string().to_lowercase();
            if msg.contains("doesn't exist") || msg.contains("does not exist") || msg.contains("not found") {
                warn!("Already dropped (skipped): {}", cypher.chars().take(80).collect::<String>());
            } else {
                warn!("Drop failed (non-fatal): {e}");
            }
        }
    }
}

/// Deduplicate evidence nodes: for each (signal, source_url) pair, keep one evidence
/// node and delete the rest. Then recompute corroboration_count from actual unique
/// evidence edges. Fixes inflated metrics from same-source re-scrapes.
/// Idempotent — safe to run on every migration (no-ops when no duplicates exist).
pub async fn deduplicate_evidence(client: &GraphClient) -> Result<(), neo4rs::Error> {
    let g = &client.graph;

    info!("Deduplicating evidence nodes...");

    // Step 1: For each (signal, source_url) with multiple evidence nodes, delete extras.
    // Keeps evs[0] (arbitrary but stable), deletes evs[1..].
    let dedup_q = query(
        "MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
         WHERE n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension
         WITH n, ev.source_url AS src, collect(ev) AS evs
         WHERE size(evs) > 1
         UNWIND evs[1..] AS dup
         DETACH DELETE dup
         RETURN count(dup) AS deleted"
    );

    match g.execute(dedup_q).await {
        Ok(mut stream) => {
            if let Some(row) = stream.next().await? {
                let deleted: i64 = row.get("deleted").unwrap_or(0);
                if deleted > 0 {
                    info!(deleted, "Deleted duplicate evidence nodes");
                }
            }
        }
        Err(e) => warn!("Evidence dedup failed (non-fatal): {e}"),
    }

    // Step 2: Recompute corroboration_count = (evidence_count - 1) for all signals.
    // After dedup, each evidence node = one unique source URL.
    // The original source isn't a corroboration, so subtract 1.
    for label in &["Event", "Give", "Ask", "Notice", "Tension"] {
        let recount_q = query(&format!(
            "MATCH (n:{label})
             OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
             WITH n, count(ev) AS ev_count
             SET n.corroboration_count = CASE WHEN ev_count > 0 THEN ev_count - 1 ELSE 0 END"
        ));

        match g.run(recount_q).await {
            Ok(_) => {}
            Err(e) => warn!("Corroboration recount for {label} failed (non-fatal): {e}"),
        }
    }

    info!("Evidence dedup and corroboration recount complete");
    Ok(())
}

/// Backfill event dates: null out empty strings, fix scrape-timestamp-as-event-date,
/// and convert remaining string-typed dates to proper datetime values.
/// Idempotent — safe to run on every migration.
pub async fn backfill_event_dates(client: &GraphClient) -> Result<(), neo4rs::Error> {
    let g = &client.graph;

    info!("Backfilling event dates...");

    let steps = [
        // Null out empty strings
        "MATCH (e:Event) WHERE e.starts_at = '' SET e.starts_at = null",
        "MATCH (e:Event) WHERE e.ends_at = '' SET e.ends_at = null",
        // Null out starts_at that equals extracted_at (scrape timestamp mistaken for event date)
        "MATCH (e:Event) WHERE e.starts_at IS NOT NULL AND e.starts_at = e.extracted_at SET e.starts_at = null",
        // Convert remaining string-typed dates to datetime
        "MATCH (e:Event) WHERE e.starts_at IS NOT NULL AND valueType(e.starts_at) = 'STRING' SET e.starts_at = datetime(e.starts_at)",
        "MATCH (e:Event) WHERE e.ends_at IS NOT NULL AND valueType(e.ends_at) = 'STRING' SET e.ends_at = datetime(e.ends_at)",
    ];

    for step in &steps {
        match g.run(query(step)).await {
            Ok(_) => {}
            Err(e) => warn!("Event date backfill step failed (non-fatal): {e}"),
        }
    }

    info!("Event date backfill complete");
    Ok(())
}

/// Backfill canonical_key on existing Source nodes and normalize city to slug.
/// Idempotent — skips sources that already have canonical_key.
pub async fn backfill_source_canonical_keys(client: &GraphClient) -> Result<(), neo4rs::Error> {
    let g = &client.graph;

    info!("Backfilling source canonical keys...");

    // Step 1: Normalize city names to slugs
    let city_mappings = [
        ("Twin Cities (Minneapolis-St. Paul, Minnesota)", "twincities"),
        ("New York City", "nyc"),
        ("Portland, Oregon", "portland"),
        ("Berlin, Germany", "berlin"),
    ];
    for (name, slug) in &city_mappings {
        let q = query(
            "MATCH (s:Source) WHERE s.city = $name SET s.city = $slug"
        )
        .param("name", *name)
        .param("slug", *slug);
        match g.run(q).await {
            Ok(_) => {}
            Err(e) => warn!("City slug backfill failed for {name}: {e}"),
        }
    }

    // Step 2: Generate canonical_key for sources that don't have one
    let q = query(
        "MATCH (s:Source) WHERE s.canonical_key IS NULL
         SET s.canonical_key = s.city + ':' + s.source_type + ':' + s.url,
             s.canonical_value = s.url,
             s.weight = CASE WHEN s.weight IS NULL THEN 0.5 ELSE s.weight END,
             s.avg_signals_per_scrape = CASE WHEN s.avg_signals_per_scrape IS NULL THEN 0.0 ELSE s.avg_signals_per_scrape END,
             s.total_cost_cents = CASE WHEN s.total_cost_cents IS NULL THEN 0 ELSE s.total_cost_cents END,
             s.last_cost_cents = CASE WHEN s.last_cost_cents IS NULL THEN 0 ELSE s.last_cost_cents END
         RETURN count(s) AS updated"
    );
    match g.execute(q).await {
        Ok(mut stream) => {
            if let Some(row) = stream.next().await? {
                let updated: i64 = row.get("updated").unwrap_or(0);
                if updated > 0 {
                    info!(updated, "Backfilled canonical keys on existing sources");
                }
            }
        }
        Err(e) => warn!("Canonical key backfill failed (non-fatal): {e}"),
    }

    info!("Source canonical key backfill complete");
    Ok(())
}
