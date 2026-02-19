use neo4rs::query;
use tracing::{info, warn};

use crate::GraphClient;

/// Run idempotent schema migrations: constraints, indexes.
/// Uses Neo4j 5+ syntax with IF NOT EXISTS for idempotent operations.
pub async fn migrate(client: &GraphClient) -> Result<(), neo4rs::Error> {
    let g = &client.graph;

    info!("Running schema migrations...");

    // --- UUID uniqueness constraints ---
    let constraints = [
        "CREATE CONSTRAINT event_id_unique IF NOT EXISTS FOR (n:Event) REQUIRE n.id IS UNIQUE",
        "CREATE CONSTRAINT give_id_unique IF NOT EXISTS FOR (n:Give) REQUIRE n.id IS UNIQUE",
        "CREATE CONSTRAINT ask_id_unique IF NOT EXISTS FOR (n:Ask) REQUIRE n.id IS UNIQUE",
        "CREATE CONSTRAINT notice_id_unique IF NOT EXISTS FOR (n:Notice) REQUIRE n.id IS UNIQUE",
        "CREATE CONSTRAINT tension_id_unique IF NOT EXISTS FOR (n:Tension) REQUIRE n.id IS UNIQUE",
        "CREATE CONSTRAINT evidence_id_unique IF NOT EXISTS FOR (n:Evidence) REQUIRE n.id IS UNIQUE",
    ];

    for c in &constraints {
        g.run(query(c)).await?;
    }
    info!("UUID uniqueness constraints created");

    // --- Existence (NOT NULL) constraints ---
    let existence = [
        // Event
        "CREATE CONSTRAINT event_sensitivity_exists IF NOT EXISTS FOR (n:Event) REQUIRE n.sensitivity IS NOT NULL",
        "CREATE CONSTRAINT event_confidence_exists IF NOT EXISTS FOR (n:Event) REQUIRE n.confidence IS NOT NULL",
        // Give
        "CREATE CONSTRAINT give_sensitivity_exists IF NOT EXISTS FOR (n:Give) REQUIRE n.sensitivity IS NOT NULL",
        "CREATE CONSTRAINT give_confidence_exists IF NOT EXISTS FOR (n:Give) REQUIRE n.confidence IS NOT NULL",
        // Ask
        "CREATE CONSTRAINT ask_sensitivity_exists IF NOT EXISTS FOR (n:Ask) REQUIRE n.sensitivity IS NOT NULL",
        "CREATE CONSTRAINT ask_confidence_exists IF NOT EXISTS FOR (n:Ask) REQUIRE n.confidence IS NOT NULL",
        // Notice
        "CREATE CONSTRAINT notice_sensitivity_exists IF NOT EXISTS FOR (n:Notice) REQUIRE n.sensitivity IS NOT NULL",
        "CREATE CONSTRAINT notice_confidence_exists IF NOT EXISTS FOR (n:Notice) REQUIRE n.confidence IS NOT NULL",
        // Tension
        "CREATE CONSTRAINT tension_sensitivity_exists IF NOT EXISTS FOR (n:Tension) REQUIRE n.sensitivity IS NOT NULL",
        "CREATE CONSTRAINT tension_confidence_exists IF NOT EXISTS FOR (n:Tension) REQUIRE n.confidence IS NOT NULL",
    ];

    for e in &existence {
        g.run(query(e)).await?;
    }
    info!("Existence constraints created");

    // --- Property indexes (lat/lng for bounding box queries) ---
    let indexes = [
        "CREATE INDEX event_lat IF NOT EXISTS FOR (n:Event) ON (n.lat)",
        "CREATE INDEX event_lng IF NOT EXISTS FOR (n:Event) ON (n.lng)",
        "CREATE INDEX give_lat IF NOT EXISTS FOR (n:Give) ON (n.lat)",
        "CREATE INDEX give_lng IF NOT EXISTS FOR (n:Give) ON (n.lng)",
        "CREATE INDEX ask_lat IF NOT EXISTS FOR (n:Ask) ON (n.lat)",
        "CREATE INDEX ask_lng IF NOT EXISTS FOR (n:Ask) ON (n.lng)",
        "CREATE INDEX notice_lat IF NOT EXISTS FOR (n:Notice) ON (n.lat)",
        "CREATE INDEX notice_lng IF NOT EXISTS FOR (n:Notice) ON (n.lng)",
        "CREATE INDEX tension_lat IF NOT EXISTS FOR (n:Tension) ON (n.lat)",
        "CREATE INDEX tension_lng IF NOT EXISTS FOR (n:Tension) ON (n.lng)",
    ];

    for idx in &indexes {
        g.run(query(idx)).await?;
    }
    info!("Property indexes created");

    // --- Backfill lat/lng from point() locations, then drop point() property ---
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
        "CREATE INDEX event_source_diversity IF NOT EXISTS FOR (n:Event) ON (n.source_diversity)",
        "CREATE INDEX give_source_diversity IF NOT EXISTS FOR (n:Give) ON (n.source_diversity)",
        "CREATE INDEX ask_source_diversity IF NOT EXISTS FOR (n:Ask) ON (n.source_diversity)",
        "CREATE INDEX notice_source_diversity IF NOT EXISTS FOR (n:Notice) ON (n.source_diversity)",
        "CREATE INDEX tension_source_diversity IF NOT EXISTS FOR (n:Tension) ON (n.source_diversity)",
    ];

    for idx in &diversity_indexes {
        g.run(query(idx)).await?;
    }
    info!("Source diversity indexes created");

    // --- Cause heat indexes ---
    let heat_indexes = [
        "CREATE INDEX event_cause_heat IF NOT EXISTS FOR (n:Event) ON (n.cause_heat)",
        "CREATE INDEX give_cause_heat IF NOT EXISTS FOR (n:Give) ON (n.cause_heat)",
        "CREATE INDEX ask_cause_heat IF NOT EXISTS FOR (n:Ask) ON (n.cause_heat)",
        "CREATE INDEX notice_cause_heat IF NOT EXISTS FOR (n:Notice) ON (n.cause_heat)",
        "CREATE INDEX tension_cause_heat IF NOT EXISTS FOR (n:Tension) ON (n.cause_heat)",
    ];

    for idx in &heat_indexes {
        g.run(query(idx)).await?;
    }
    info!("Cause heat indexes created");

    // --- Full-text indexes ---
    let fulltext = [
        "CREATE FULLTEXT INDEX event_text IF NOT EXISTS FOR (n:Event) ON EACH [n.title, n.summary]",
        "CREATE FULLTEXT INDEX give_text IF NOT EXISTS FOR (n:Give) ON EACH [n.title, n.summary]",
        "CREATE FULLTEXT INDEX ask_text IF NOT EXISTS FOR (n:Ask) ON EACH [n.title, n.summary]",
        "CREATE FULLTEXT INDEX notice_text IF NOT EXISTS FOR (n:Notice) ON EACH [n.title, n.summary]",
        "CREATE FULLTEXT INDEX tension_text IF NOT EXISTS FOR (n:Tension) ON EACH [n.title, n.summary]",
    ];

    for f in &fulltext {
        g.run(query(f)).await?;
    }
    info!("Full-text indexes created");

    // --- Vector indexes (1024-dim for Voyage embeddings) ---
    let vector = [
        "CREATE VECTOR INDEX event_embedding IF NOT EXISTS FOR (n:Event) ON (n.embedding) OPTIONS {indexConfig: {`vector.dimensions`: 1024, `vector.similarity_function`: 'cosine'}}",
        "CREATE VECTOR INDEX give_embedding IF NOT EXISTS FOR (n:Give) ON (n.embedding) OPTIONS {indexConfig: {`vector.dimensions`: 1024, `vector.similarity_function`: 'cosine'}}",
        "CREATE VECTOR INDEX ask_embedding IF NOT EXISTS FOR (n:Ask) ON (n.embedding) OPTIONS {indexConfig: {`vector.dimensions`: 1024, `vector.similarity_function`: 'cosine'}}",
        "CREATE VECTOR INDEX notice_embedding IF NOT EXISTS FOR (n:Notice) ON (n.embedding) OPTIONS {indexConfig: {`vector.dimensions`: 1024, `vector.similarity_function`: 'cosine'}}",
        "CREATE VECTOR INDEX tension_embedding IF NOT EXISTS FOR (n:Tension) ON (n.embedding) OPTIONS {indexConfig: {`vector.dimensions`: 1024, `vector.similarity_function`: 'cosine'}}",
    ];

    for v in &vector {
        g.run(query(v)).await?;
    }
    info!("Vector indexes created");

    // --- Story and ClusterSnapshot constraints ---
    let story_constraints = [
        "CREATE CONSTRAINT story_id_unique IF NOT EXISTS FOR (n:Story) REQUIRE n.id IS UNIQUE",
        "CREATE CONSTRAINT clustersnapshot_id_unique IF NOT EXISTS FOR (n:ClusterSnapshot) REQUIRE n.id IS UNIQUE",
    ];

    for c in &story_constraints {
        g.run(query(c)).await?;
    }
    info!("Story/ClusterSnapshot constraints created");

    // --- Story indexes ---
    let story_indexes = [
        "CREATE INDEX story_energy IF NOT EXISTS FOR (n:Story) ON (n.energy)",
        "CREATE INDEX story_status IF NOT EXISTS FOR (n:Story) ON (n.status)",
        "CREATE INDEX story_last_updated IF NOT EXISTS FOR (n:Story) ON (n.last_updated)",
    ];

    for idx in &story_indexes {
        g.run(query(idx)).await?;
    }
    info!("Story indexes created");

    // --- Story synthesis indexes ---
    let synthesis_indexes = [
        "CREATE INDEX story_arc IF NOT EXISTS FOR (n:Story) ON (n.arc)",
        "CREATE INDEX story_category IF NOT EXISTS FOR (n:Story) ON (n.category)",
    ];

    for idx in &synthesis_indexes {
        g.run(query(idx)).await?;
    }
    info!("Story synthesis indexes created");

    // --- Actor constraints and indexes ---
    let actor_constraints = [
        "CREATE CONSTRAINT actor_id_unique IF NOT EXISTS FOR (a:Actor) REQUIRE a.id IS UNIQUE",
        "CREATE CONSTRAINT actor_entity_id_unique IF NOT EXISTS FOR (a:Actor) REQUIRE a.entity_id IS UNIQUE",
    ];

    for c in &actor_constraints {
        g.run(query(c)).await?;
    }

    let actor_indexes = [
        "CREATE INDEX actor_name IF NOT EXISTS FOR (a:Actor) ON (a.name)",
        "CREATE INDEX actor_city IF NOT EXISTS FOR (a:Actor) ON (a.city)",
    ];

    for idx in &actor_indexes {
        g.run(query(idx)).await?;
    }
    info!("Actor constraints and indexes created");

    // --- Edge index for SIMILAR_TO weight ---
    g.run(query(
        "CREATE INDEX similar_to_weight IF NOT EXISTS FOR ()-[r:SIMILAR_TO]-() ON (r.weight)",
    ))
    .await?;
    info!("Edge indexes created");

    // --- City node constraints and indexes ---
    let city_constraints = [
        "CREATE CONSTRAINT city_id_unique IF NOT EXISTS FOR (c:City) REQUIRE c.id IS UNIQUE",
        "CREATE CONSTRAINT city_slug_unique IF NOT EXISTS FOR (c:City) REQUIRE c.slug IS UNIQUE",
    ];

    for c in &city_constraints {
        g.run(query(c)).await?;
    }

    g.run(query(
        "CREATE INDEX city_active IF NOT EXISTS FOR (c:City) ON (c.active)",
    ))
    .await?;
    info!("City constraints and indexes created");

    // --- Source node constraints and indexes ---
    let source_constraints = [
        "CREATE CONSTRAINT source_id_unique IF NOT EXISTS FOR (s:Source) REQUIRE s.id IS UNIQUE",
        "CREATE CONSTRAINT source_canonical_key_unique IF NOT EXISTS FOR (s:Source) REQUIRE s.canonical_key IS UNIQUE",
    ];

    for c in &source_constraints {
        g.run(query(c)).await?;
    }

    // Drop legacy url uniqueness constraint (canonical_key is the new identity)
    drop_constraint_if_exists(g, "source_url_unique").await;

    let source_indexes = [
        "CREATE INDEX source_city IF NOT EXISTS FOR (s:Source) ON (s.city)",
        "CREATE INDEX source_active IF NOT EXISTS FOR (s:Source) ON (s.active)",
        "CREATE INDEX source_url IF NOT EXISTS FOR (s:Source) ON (s.url)",
        "CREATE INDEX source_type IF NOT EXISTS FOR (s:Source) ON (s.source_type)",
        "CREATE INDEX source_weight IF NOT EXISTS FOR (s:Source) ON (s.weight)",
    ];

    for idx in &source_indexes {
        g.run(query(idx)).await?;
    }
    info!("Source constraints and indexes created");

    // --- BlockedSource constraint ---
    g.run(query("CREATE CONSTRAINT blockedsource_url_pattern_unique IF NOT EXISTS FOR (b:BlockedSource) REQUIRE b.url_pattern IS UNIQUE")).await?;
    info!("BlockedSource constraint created");

    // --- Supervisor node constraints and indexes ---
    let supervisor_constraints = [
        "CREATE CONSTRAINT supervisorstate_id_unique IF NOT EXISTS FOR (s:SupervisorState) REQUIRE s.id IS UNIQUE",
        "CREATE CONSTRAINT extractionrule_id_unique IF NOT EXISTS FOR (r:ExtractionRule) REQUIRE r.id IS UNIQUE",
        "CREATE CONSTRAINT validationissue_id_unique IF NOT EXISTS FOR (v:ValidationIssue) REQUIRE v.id IS UNIQUE",
    ];

    for c in &supervisor_constraints {
        g.run(query(c)).await?;
    }

    let supervisor_indexes = [
        "CREATE INDEX validationissue_status IF NOT EXISTS FOR (v:ValidationIssue) ON (v.status)",
        "CREATE INDEX validationissue_city IF NOT EXISTS FOR (v:ValidationIssue) ON (v.city)",
        "CREATE INDEX validationissue_target_id IF NOT EXISTS FOR (v:ValidationIssue) ON (v.target_id)",
        "CREATE INDEX extractionrule_city IF NOT EXISTS FOR (r:ExtractionRule) ON (r.city)",
        "CREATE INDEX extractionrule_approved IF NOT EXISTS FOR (r:ExtractionRule) ON (r.approved)",
        "CREATE INDEX supervisorstate_city IF NOT EXISTS FOR (s:SupervisorState) ON (s.city)",
    ];

    for idx in &supervisor_indexes {
        g.run(query(idx)).await?;
    }
    info!("Supervisor constraints and indexes created");

    // --- Source role index ---
    g.run(query(
        "CREATE INDEX source_role IF NOT EXISTS FOR (s:Source) ON (s.source_role)",
    ))
    .await?;
    info!("Source role index created");

    // --- Place node constraints and indexes ---
    g.run(query(
        "CREATE CONSTRAINT place_id_unique IF NOT EXISTS FOR (p:Place) REQUIRE p.id IS UNIQUE",
    ))
    .await?;
    g.run(query(
        "CREATE INDEX place_slug IF NOT EXISTS FOR (p:Place) ON (p.slug)",
    ))
    .await?;
    g.run(query(
        "CREATE INDEX place_city IF NOT EXISTS FOR (p:Place) ON (p.city)",
    ))
    .await?;
    info!("Place constraints and indexes created");

    // --- Resource node constraints and indexes ---
    g.run(query("CREATE CONSTRAINT resource_id_unique IF NOT EXISTS FOR (r:Resource) REQUIRE r.id IS UNIQUE")).await?;
    g.run(query("CREATE CONSTRAINT resource_slug_unique IF NOT EXISTS FOR (r:Resource) REQUIRE r.slug IS UNIQUE")).await?;
    g.run(query(
        "CREATE INDEX resource_name IF NOT EXISTS FOR (r:Resource) ON (r.name)",
    ))
    .await?;
    g.run(query("CREATE VECTOR INDEX resource_embedding IF NOT EXISTS FOR (r:Resource) ON (r.embedding) OPTIONS {indexConfig: {`vector.dimensions`: 1024, `vector.similarity_function`: 'cosine'}}")).await?;
    info!("Resource constraints and indexes created");

    // --- Convert RESPONDS_TO gathering edges to DRAWN_TO ---
    convert_gathering_edges(client).await?;

    // --- Reclassify query sources: web → *_query for listing pages ---
    reclassify_query_sources(client).await?;

    // --- Deduplicate evidence + recompute corroboration ---
    deduplicate_evidence(client).await?;

    // --- Backfill event dates: clean up string/empty dates ---
    backfill_event_dates(client).await?;

    // --- Rename tavily_query → web_query in Source nodes ---
    rename_tavily_to_web_query(client).await?;

    // --- Source query embedding vector index ---
    g.run(query(
        "CREATE VECTOR INDEX source_query_embedding IF NOT EXISTS \
         FOR (s:Source) ON (s.query_embedding) \
         OPTIONS {indexConfig: {`vector.dimensions`: 1024, `vector.similarity_function`: 'cosine'}}"
    )).await?;
    info!("Source query embedding vector index created");

    // --- Backfill source_role on existing Source nodes ---
    backfill_source_roles(client).await?;

    // --- Backfill scrape_count on existing Source nodes ---
    backfill_scrape_count(client).await?;

    // --- Deactivate orphaned web query sources ---
    deactivate_orphaned_web_queries(client).await?;

    // --- Story pipeline consolidation: delete Leiden-created stories (no Tension in CONTAINS set) ---
    cleanup_leiden_stories(client).await?;

    // --- New story metric indexes ---
    let metric_indexes = [
        "CREATE INDEX story_cause_heat IF NOT EXISTS FOR (n:Story) ON (n.cause_heat)",
        "CREATE INDEX story_gap_score IF NOT EXISTS FOR (n:Story) ON (n.gap_score)",
    ];
    for idx in &metric_indexes {
        g.run(query(idx)).await?;
    }
    info!("Story metric indexes created");

    info!("Schema migration complete");
    Ok(())
}

/// Reclassify Source nodes that were stored as "web" but are actually query sources
/// (Eventbrite, VolunteerMatch, GoFundMe listing pages). Updates both source_type
/// and canonical_key. Also deletes signals with listing-page source_urls (garbage data).
/// Idempotent — WHERE clauses match nothing after the first run.
pub async fn reclassify_query_sources(client: &GraphClient) -> Result<(), neo4rs::Error> {
    let g = &client.graph;

    info!("Reclassifying query sources...");

    let reclassifications = [
        ("eventbrite.com", "eventbrite_query"),
        ("volunteermatch.org", "volunteermatch_query"),
        ("gofundme.com", "gofundme_query"),
    ];

    for (domain, new_type) in &reclassifications {
        let q = query(
            "MATCH (s:Source) WHERE s.source_type = 'web' AND s.url CONTAINS $domain \
             SET s.source_type = $new_type, \
                 s.canonical_key = s.city + ':' + $new_type + ':' + s.canonical_value \
             RETURN count(s) AS updated",
        )
        .param("domain", *domain)
        .param("new_type", *new_type);

        match g.execute(q).await {
            Ok(mut stream) => {
                if let Some(row) = stream.next().await? {
                    let updated: i64 = row.get("updated").unwrap_or(0);
                    if updated > 0 {
                        info!(updated, domain, new_type, "Reclassified sources");
                    }
                }
            }
            Err(e) => warn!(domain, "Source reclassification failed (non-fatal): {e}"),
        }
    }

    // Delete signals with listing-page source_urls (misattributed garbage data)
    let cleanup = query(
        "MATCH (n) WHERE n.source_url CONTAINS 'eventbrite.com/d/' \
         DETACH DELETE n \
         RETURN count(n) AS deleted",
    );

    match g.execute(cleanup).await {
        Ok(mut stream) => {
            if let Some(row) = stream.next().await? {
                let deleted: i64 = row.get("deleted").unwrap_or(0);
                if deleted > 0 {
                    info!(deleted, "Deleted signals with listing-page source_urls");
                }
            }
        }
        Err(e) => warn!("Listing-page signal cleanup failed (non-fatal): {e}"),
    }

    info!("Query source reclassification complete");
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

/// Drop a constraint by name if it exists (Neo4j 5+ syntax).
async fn drop_constraint_if_exists(g: &neo4rs::Graph, name: &str) {
    let cypher = format!("DROP CONSTRAINT {name} IF EXISTS");
    match g.run(query(&cypher)).await {
        Ok(_) => info!("Dropped constraint (if existed): {name}"),
        Err(e) => warn!("Drop constraint {name} failed (non-fatal): {e}"),
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
         RETURN count(dup) AS deleted",
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
        // Convert remaining string-typed dates to datetime (Neo4j 5.11+ type predicate syntax)
        "MATCH (e:Event) WHERE e.starts_at IS NOT NULL AND e.starts_at IS :: STRING SET e.starts_at = datetime(e.starts_at)",
        "MATCH (e:Event) WHERE e.ends_at IS NOT NULL AND e.ends_at IS :: STRING SET e.ends_at = datetime(e.ends_at)",
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
        (
            "Twin Cities (Minneapolis-St. Paul, Minnesota)",
            "twincities",
        ),
        ("New York City", "nyc"),
        ("Portland, Oregon", "portland"),
        ("Berlin, Germany", "berlin"),
    ];
    for (name, slug) in &city_mappings {
        let q = query("MATCH (s:Source) WHERE s.city = $name SET s.city = $slug")
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
             s.avg_signals_per_scrape = CASE WHEN s.avg_signals_per_scrape IS NULL THEN 0.0 ELSE s.avg_signals_per_scrape END
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

/// Backfill `source_role` on existing Source nodes using heuristic classification.
/// Idempotent — only touches nodes where source_role IS NULL.
///
/// Heuristics:
/// - Reddit, forums, news → tension
/// - Nonprofit .org, EventbriteQuery, VolunteerMatchQuery → response
/// - Discovery sources with gap_context containing "response"/"resource" → response
/// - Discovery sources with gap_context containing "tension"/"problem" → tension
/// - Everything else → mixed
pub async fn backfill_source_roles(client: &GraphClient) -> Result<(), neo4rs::Error> {
    let g = &client.graph;

    info!("Backfilling source roles...");

    // Step 1: Reddit / forums / news → tension
    let tension_q = query(
        "MATCH (s:Source) WHERE s.source_role IS NULL AND \
         (s.source_type = 'reddit' OR \
          s.url CONTAINS 'reddit.com' OR \
          s.url CONTAINS 'nextdoor.com' OR \
          s.url CONTAINS 'startribune.com' OR \
          s.url CONTAINS 'minnpost.com') \
         SET s.source_role = 'tension' \
         RETURN count(s) AS updated",
    );
    match g.execute(tension_q).await {
        Ok(mut stream) => {
            if let Some(row) = stream.next().await? {
                let updated: i64 = row.get("updated").unwrap_or(0);
                if updated > 0 {
                    info!(updated, "Classified tension sources");
                }
            }
        }
        Err(e) => warn!("Tension source classification failed (non-fatal): {e}"),
    }

    // Step 2: Eventbrite, VolunteerMatch, GoFundMe, .org nonprofits → response
    let response_q = query(
        "MATCH (s:Source) WHERE s.source_role IS NULL AND \
         (s.source_type = 'eventbrite_query' OR \
          s.source_type = 'volunteermatch_query' OR \
          s.source_type = 'gofundme_query' OR \
          (s.url IS NOT NULL AND s.url ENDS WITH '.org' AND s.source_type = 'web') OR \
          (s.url IS NOT NULL AND s.url CONTAINS '.org/' AND s.source_type = 'web')) \
         SET s.source_role = 'response' \
         RETURN count(s) AS updated",
    );
    match g.execute(response_q).await {
        Ok(mut stream) => {
            if let Some(row) = stream.next().await? {
                let updated: i64 = row.get("updated").unwrap_or(0);
                if updated > 0 {
                    info!(updated, "Classified response sources");
                }
            }
        }
        Err(e) => warn!("Response source classification failed (non-fatal): {e}"),
    }

    // Step 3: Discovery sources — classify by gap_context keywords
    let gap_response_q = query(
        "MATCH (s:Source) WHERE s.source_role IS NULL AND s.gap_context IS NOT NULL AND \
         (toLower(s.gap_context) CONTAINS 'response' OR \
          toLower(s.gap_context) CONTAINS 'resource' OR \
          toLower(s.gap_context) CONTAINS 'volunteer' OR \
          toLower(s.gap_context) CONTAINS 'program') \
         SET s.source_role = 'response' \
         RETURN count(s) AS updated",
    );
    match g.execute(gap_response_q).await {
        Ok(mut stream) => {
            if let Some(row) = stream.next().await? {
                let updated: i64 = row.get("updated").unwrap_or(0);
                if updated > 0 {
                    info!(updated, "Classified gap-response sources");
                }
            }
        }
        Err(e) => warn!("Gap-response classification failed (non-fatal): {e}"),
    }

    let gap_tension_q = query(
        "MATCH (s:Source) WHERE s.source_role IS NULL AND s.gap_context IS NOT NULL AND \
         (toLower(s.gap_context) CONTAINS 'tension' OR \
          toLower(s.gap_context) CONTAINS 'problem' OR \
          toLower(s.gap_context) CONTAINS 'complaint') \
         SET s.source_role = 'tension' \
         RETURN count(s) AS updated",
    );
    match g.execute(gap_tension_q).await {
        Ok(mut stream) => {
            if let Some(row) = stream.next().await? {
                let updated: i64 = row.get("updated").unwrap_or(0);
                if updated > 0 {
                    info!(updated, "Classified gap-tension sources");
                }
            }
        }
        Err(e) => warn!("Gap-tension classification failed (non-fatal): {e}"),
    }

    // Step 4: Everything remaining → mixed
    let mixed_q = query(
        "MATCH (s:Source) WHERE s.source_role IS NULL \
         SET s.source_role = 'mixed' \
         RETURN count(s) AS updated",
    );
    match g.execute(mixed_q).await {
        Ok(mut stream) => {
            if let Some(row) = stream.next().await? {
                let updated: i64 = row.get("updated").unwrap_or(0);
                if updated > 0 {
                    info!(updated, "Classified remaining sources as mixed");
                }
            }
        }
        Err(e) => warn!("Mixed source classification failed (non-fatal): {e}"),
    }

    info!("Source role backfill complete");
    Ok(())
}

/// Backfill `scrape_count` on existing Source nodes.
/// Sets scrape_count = 0 where it is null. Idempotent.
pub async fn backfill_scrape_count(client: &GraphClient) -> Result<(), neo4rs::Error> {
    let g = &client.graph;

    info!("Backfilling scrape_count...");

    let q = query(
        "MATCH (s:Source) WHERE s.scrape_count IS NULL \
         SET s.scrape_count = 0 \
         RETURN count(s) AS updated",
    );

    match g.execute(q).await {
        Ok(mut stream) => {
            if let Some(row) = stream.next().await? {
                let updated: i64 = row.get("updated").unwrap_or(0);
                if updated > 0 {
                    info!(updated, "Backfilled scrape_count on existing sources");
                }
            }
        }
        Err(e) => warn!("scrape_count backfill failed (non-fatal): {e}"),
    }

    info!("scrape_count backfill complete");
    Ok(())
}

/// Convert existing RESPONDS_TO edges with gathering_type to DRAWN_TO edges.
/// Idempotent — WHERE clause matches nothing after the first run.
pub async fn convert_gathering_edges(client: &GraphClient) -> Result<(), neo4rs::Error> {
    let g = &client.graph;

    info!("Converting gathering edges from RESPONDS_TO to DRAWN_TO...");

    let q = query(
        "MATCH (sig)-[old:RESPONDS_TO]->(t:Tension)
         WHERE old.gathering_type IS NOT NULL
         MERGE (sig)-[new:DRAWN_TO]->(t)
         SET new.match_strength = old.match_strength,
             new.explanation = old.explanation,
             new.gathering_type = old.gathering_type
         DELETE old
         RETURN count(old) AS converted",
    );

    match g.execute(q).await {
        Ok(mut stream) => {
            if let Some(row) = stream.next().await? {
                let converted: i64 = row.get("converted").unwrap_or(0);
                if converted > 0 {
                    info!(
                        converted,
                        "Converted gathering RESPONDS_TO edges to DRAWN_TO"
                    );
                }
            }
        }
        Err(e) => warn!("Gathering edge conversion failed (non-fatal): {e}"),
    }

    info!("Gathering edge conversion complete");
    Ok(())
}

/// Rename `tavily_query` → `web_query` in Source nodes.
/// Updates both `source_type` and `canonical_key`. Idempotent — WHERE clause
/// matches nothing after the first run.
pub async fn rename_tavily_to_web_query(client: &GraphClient) -> Result<(), neo4rs::Error> {
    let g = &client.graph;

    info!("Renaming tavily_query → web_query in Source nodes...");

    let q = query(
        "MATCH (s:Source {source_type: 'tavily_query'})
         SET s.source_type = 'web_query',
             s.canonical_key = replace(s.canonical_key, ':tavily_query:', ':web_query:')
         RETURN count(s) AS updated",
    );

    match g.execute(q).await {
        Ok(mut stream) => {
            if let Some(row) = stream.next().await? {
                let updated: i64 = row.get("updated").unwrap_or(0);
                if updated > 0 {
                    info!(updated, "Renamed tavily_query → web_query");
                }
            }
        }
        Err(e) => warn!("tavily_query rename failed (non-fatal): {e}"),
    }

    info!("tavily_query rename complete");
    Ok(())
}

/// Deactivate orphaned WebQuery sources that were created before the attribution
/// fix but never scraped. These queries accumulated because the feedback loop was
/// broken — they look "never scraped" forever.
///
/// Criteria: web_query + never scraped + older than 7 days + not curated/human.
/// Idempotent — WHERE clause matches nothing after cleanup.
pub async fn deactivate_orphaned_web_queries(client: &GraphClient) -> Result<(), neo4rs::Error> {
    let g = &client.graph;

    info!("Deactivating orphaned web query sources...");

    let q = query(
        "MATCH (s:Source {source_type: 'web_query', active: true})
         WHERE s.last_scraped IS NULL
           AND s.created_at < datetime() - duration('P7D')
           AND s.discovery_method <> 'curated'
           AND s.discovery_method <> 'human_submission'
         SET s.active = false
         RETURN count(s) AS deactivated",
    );

    match g.execute(q).await {
        Ok(mut stream) => {
            if let Some(row) = stream.next().await? {
                let deactivated: i64 = row.get("deactivated").unwrap_or(0);
                if deactivated > 0 {
                    info!(deactivated, "Deactivated orphaned web query sources");
                }
            }
        }
        Err(e) => warn!("Orphaned web query deactivation failed (non-fatal): {e}"),
    }

    info!("Orphaned web query deactivation complete");
    Ok(())
}

/// Delete stories created by Leiden clustering that don't contain a Tension node.
/// StoryWeaver always anchors stories on a Tension, so stories without one are Leiden artifacts.
/// Idempotent — matches nothing after cleanup.
pub async fn cleanup_leiden_stories(client: &GraphClient) -> Result<(), neo4rs::Error> {
    let g = &client.graph;

    info!("Cleaning up Leiden-created stories (no Tension in CONTAINS set)...");

    let q = query(
        "MATCH (s:Story)
         WHERE NOT (s)-[:CONTAINS]->(:Tension)
         DETACH DELETE s
         RETURN count(s) AS deleted",
    );

    match g.execute(q).await {
        Ok(mut stream) => {
            if let Some(row) = stream.next().await? {
                let deleted: i64 = row.get("deleted").unwrap_or(0);
                if deleted > 0 {
                    info!(deleted, "Deleted Leiden-created stories without Tension");
                }
            }
        }
        Err(e) => warn!("Leiden story cleanup failed (non-fatal): {e}"),
    }

    info!("Leiden story cleanup complete");
    Ok(())
}
