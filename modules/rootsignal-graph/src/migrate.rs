use neo4rs::query;
use tracing::{info, warn};

use crate::GraphClient;

/// Run idempotent schema migrations: constraints, indexes.
/// Uses Neo4j 5+ syntax with IF NOT EXISTS for idempotent operations.
pub async fn migrate(client: &GraphClient) -> Result<(), neo4rs::Error> {
    let g = &client.graph;

    info!("Running schema migrations...");

    // --- Ask → Need rename: drop old constraints/indexes and relabel nodes ---
    let ask_drops = [
        "DROP CONSTRAINT ask_id_unique IF EXISTS",
        "DROP CONSTRAINT ask_sensitivity_exists IF EXISTS",
        "DROP CONSTRAINT ask_confidence_exists IF EXISTS",
        "DROP INDEX ask_lat IF EXISTS",
        "DROP INDEX ask_lng IF EXISTS",
        "DROP INDEX ask_source_diversity IF EXISTS",
        "DROP INDEX ask_cause_heat IF EXISTS",
        "DROP INDEX ask_text IF EXISTS",
        "DROP INDEX ask_embedding IF EXISTS",
    ];

    for d in &ask_drops {
        match g.run(query(d)).await {
            Ok(_) => {}
            Err(e) => warn!("Ask→Need drop step failed (non-fatal): {e}"),
        }
    }

    // Relabel existing Ask nodes to Need
    match g.run(query("MATCH (n:Ask) SET n:Need REMOVE n:Ask")).await {
        Ok(_) => {}
        Err(e) => warn!("Ask→Need relabel failed (non-fatal): {e}"),
    }
    info!("Ask→Need migration steps complete");

    // --- Event → Gathering rename: drop old constraints/indexes and relabel nodes ---
    let event_drops = [
        "DROP CONSTRAINT event_id_unique IF EXISTS",
        "DROP CONSTRAINT event_sensitivity_exists IF EXISTS",
        "DROP CONSTRAINT event_confidence_exists IF EXISTS",
        "DROP INDEX event_lat IF EXISTS",
        "DROP INDEX event_lng IF EXISTS",
        "DROP INDEX event_source_diversity IF EXISTS",
        "DROP INDEX event_cause_heat IF EXISTS",
        "DROP INDEX event_text IF EXISTS",
        "DROP INDEX event_embedding IF EXISTS",
    ];

    for d in &event_drops {
        match g.run(query(d)).await {
            Ok(_) => {}
            Err(e) => warn!("Event→Gathering drop step failed (non-fatal): {e}"),
        }
    }

    // Relabel existing Event nodes to Gathering
    match g
        .run(query(
            "MATCH (n:Event) SET n:Gathering REMOVE n:Event",
        ))
        .await
    {
        Ok(_) => {}
        Err(e) => warn!("Event→Gathering relabel failed (non-fatal): {e}"),
    }
    info!("Event→Gathering migration steps complete");

    // --- Give → Aid rename: drop old constraints/indexes and relabel nodes ---
    let give_drops = [
        "DROP CONSTRAINT give_id_unique IF EXISTS",
        "DROP CONSTRAINT give_sensitivity_exists IF EXISTS",
        "DROP CONSTRAINT give_confidence_exists IF EXISTS",
        "DROP INDEX give_lat IF EXISTS",
        "DROP INDEX give_lng IF EXISTS",
        "DROP INDEX give_source_diversity IF EXISTS",
        "DROP INDEX give_cause_heat IF EXISTS",
        "DROP INDEX give_text IF EXISTS",
        "DROP INDEX give_embedding IF EXISTS",
    ];

    for d in &give_drops {
        match g.run(query(d)).await {
            Ok(_) => {}
            Err(e) => warn!("Give→Aid drop step failed (non-fatal): {e}"),
        }
    }

    // Relabel existing Give nodes to Aid
    match g
        .run(query("MATCH (n:Give) SET n:Aid REMOVE n:Give"))
        .await
    {
        Ok(_) => {}
        Err(e) => warn!("Give→Aid relabel failed (non-fatal): {e}"),
    }
    info!("Give→Aid migration steps complete");

    // --- UUID uniqueness constraints ---
    let constraints = [
        "CREATE CONSTRAINT gathering_id_unique IF NOT EXISTS FOR (n:Gathering) REQUIRE n.id IS UNIQUE",
        "CREATE CONSTRAINT aid_id_unique IF NOT EXISTS FOR (n:Aid) REQUIRE n.id IS UNIQUE",
        "CREATE CONSTRAINT need_id_unique IF NOT EXISTS FOR (n:Need) REQUIRE n.id IS UNIQUE",
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
        // Gathering
        "CREATE CONSTRAINT gathering_sensitivity_exists IF NOT EXISTS FOR (n:Gathering) REQUIRE n.sensitivity IS NOT NULL",
        "CREATE CONSTRAINT gathering_confidence_exists IF NOT EXISTS FOR (n:Gathering) REQUIRE n.confidence IS NOT NULL",
        // Aid
        "CREATE CONSTRAINT aid_sensitivity_exists IF NOT EXISTS FOR (n:Aid) REQUIRE n.sensitivity IS NOT NULL",
        "CREATE CONSTRAINT aid_confidence_exists IF NOT EXISTS FOR (n:Aid) REQUIRE n.confidence IS NOT NULL",
        // Need
        "CREATE CONSTRAINT need_sensitivity_exists IF NOT EXISTS FOR (n:Need) REQUIRE n.sensitivity IS NOT NULL",
        "CREATE CONSTRAINT need_confidence_exists IF NOT EXISTS FOR (n:Need) REQUIRE n.confidence IS NOT NULL",
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
        "CREATE INDEX gathering_lat IF NOT EXISTS FOR (n:Gathering) ON (n.lat)",
        "CREATE INDEX gathering_lng IF NOT EXISTS FOR (n:Gathering) ON (n.lng)",
        "CREATE INDEX aid_lat IF NOT EXISTS FOR (n:Aid) ON (n.lat)",
        "CREATE INDEX aid_lng IF NOT EXISTS FOR (n:Aid) ON (n.lng)",
        "CREATE INDEX need_lat IF NOT EXISTS FOR (n:Need) ON (n.lat)",
        "CREATE INDEX need_lng IF NOT EXISTS FOR (n:Need) ON (n.lng)",
        "CREATE INDEX notice_lat IF NOT EXISTS FOR (n:Notice) ON (n.lat)",
        "CREATE INDEX notice_lng IF NOT EXISTS FOR (n:Notice) ON (n.lng)",
        "CREATE INDEX tension_lat IF NOT EXISTS FOR (n:Tension) ON (n.lat)",
        "CREATE INDEX tension_lng IF NOT EXISTS FOR (n:Tension) ON (n.lng)",
    ];

    for idx in &indexes {
        g.run(query(idx)).await?;
    }
    info!("Property indexes created");

    // --- Composite lat/lng indexes for bounding-box queries ---
    let composite_geo_indexes = [
        "CREATE INDEX gathering_lat_lng IF NOT EXISTS FOR (n:Gathering) ON (n.lat, n.lng)",
        "CREATE INDEX aid_lat_lng IF NOT EXISTS FOR (n:Aid) ON (n.lat, n.lng)",
        "CREATE INDEX need_lat_lng IF NOT EXISTS FOR (n:Need) ON (n.lat, n.lng)",
        "CREATE INDEX notice_lat_lng IF NOT EXISTS FOR (n:Notice) ON (n.lat, n.lng)",
        "CREATE INDEX tension_lat_lng IF NOT EXISTS FOR (n:Tension) ON (n.lat, n.lng)",
    ];

    for idx in &composite_geo_indexes {
        g.run(query(idx)).await?;
    }
    info!("Composite geo indexes created");

    // --- Backfill lat/lng from point() locations, then drop point() property ---
    let backfill = [
        "MATCH (n:Gathering) WHERE n.location IS NOT NULL AND n.lat IS NULL SET n.lat = n.location.y, n.lng = n.location.x",
        "MATCH (n:Aid) WHERE n.location IS NOT NULL AND n.lat IS NULL SET n.lat = n.location.y, n.lng = n.location.x",
        "MATCH (n:Need) WHERE n.location IS NOT NULL AND n.lat IS NULL SET n.lat = n.location.y, n.lng = n.location.x",
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
        "CREATE INDEX gathering_source_diversity IF NOT EXISTS FOR (n:Gathering) ON (n.source_diversity)",
        "CREATE INDEX aid_source_diversity IF NOT EXISTS FOR (n:Aid) ON (n.source_diversity)",
        "CREATE INDEX need_source_diversity IF NOT EXISTS FOR (n:Need) ON (n.source_diversity)",
        "CREATE INDEX notice_source_diversity IF NOT EXISTS FOR (n:Notice) ON (n.source_diversity)",
        "CREATE INDEX tension_source_diversity IF NOT EXISTS FOR (n:Tension) ON (n.source_diversity)",
    ];

    for idx in &diversity_indexes {
        g.run(query(idx)).await?;
    }
    info!("Source diversity indexes created");

    // --- Cause heat indexes ---
    let heat_indexes = [
        "CREATE INDEX gathering_cause_heat IF NOT EXISTS FOR (n:Gathering) ON (n.cause_heat)",
        "CREATE INDEX aid_cause_heat IF NOT EXISTS FOR (n:Aid) ON (n.cause_heat)",
        "CREATE INDEX need_cause_heat IF NOT EXISTS FOR (n:Need) ON (n.cause_heat)",
        "CREATE INDEX notice_cause_heat IF NOT EXISTS FOR (n:Notice) ON (n.cause_heat)",
        "CREATE INDEX tension_cause_heat IF NOT EXISTS FOR (n:Tension) ON (n.cause_heat)",
    ];

    for idx in &heat_indexes {
        g.run(query(idx)).await?;
    }
    info!("Cause heat indexes created");

    // --- Full-text indexes ---
    let fulltext = [
        "CREATE FULLTEXT INDEX gathering_text IF NOT EXISTS FOR (n:Gathering) ON EACH [n.title, n.summary]",
        "CREATE FULLTEXT INDEX aid_text IF NOT EXISTS FOR (n:Aid) ON EACH [n.title, n.summary]",
        "CREATE FULLTEXT INDEX need_text IF NOT EXISTS FOR (n:Need) ON EACH [n.title, n.summary]",
        "CREATE FULLTEXT INDEX notice_text IF NOT EXISTS FOR (n:Notice) ON EACH [n.title, n.summary]",
        "CREATE FULLTEXT INDEX tension_text IF NOT EXISTS FOR (n:Tension) ON EACH [n.title, n.summary]",
    ];

    for f in &fulltext {
        g.run(query(f)).await?;
    }
    info!("Full-text indexes created");

    // --- Vector indexes (1024-dim for Voyage embeddings) ---
    let vector = [
        "CREATE VECTOR INDEX gathering_embedding IF NOT EXISTS FOR (n:Gathering) ON (n.embedding) OPTIONS {indexConfig: {`vector.dimensions`: 1024, `vector.similarity_function`: 'cosine'}}",
        "CREATE VECTOR INDEX aid_embedding IF NOT EXISTS FOR (n:Aid) ON (n.embedding) OPTIONS {indexConfig: {`vector.dimensions`: 1024, `vector.similarity_function`: 'cosine'}}",
        "CREATE VECTOR INDEX need_embedding IF NOT EXISTS FOR (n:Need) ON (n.embedding) OPTIONS {indexConfig: {`vector.dimensions`: 1024, `vector.similarity_function`: 'cosine'}}",
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
        // actor_city index removed — actors are linked to regions via :DISCOVERED relationship
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

    // --- ScoutTask node constraints and indexes ---
    let scout_task_schema = [
        "CREATE CONSTRAINT scouttask_id IF NOT EXISTS FOR (t:ScoutTask) REQUIRE t.id IS UNIQUE",
        "CREATE INDEX scouttask_status IF NOT EXISTS FOR (t:ScoutTask) ON (t.status)",
        "CREATE INDEX scouttask_priority IF NOT EXISTS FOR (t:ScoutTask) ON (t.priority)",
    ];
    for s in &scout_task_schema {
        g.run(query(s)).await?;
    }
    info!("ScoutTask constraints and indexes created");

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
        "CREATE INDEX validationissue_region IF NOT EXISTS FOR (v:ValidationIssue) ON (v.region)",
        "CREATE INDEX validationissue_target_id IF NOT EXISTS FOR (v:ValidationIssue) ON (v.target_id)",
        "CREATE INDEX extractionrule_region IF NOT EXISTS FOR (r:ExtractionRule) ON (r.region)",
        "CREATE INDEX extractionrule_approved IF NOT EXISTS FOR (r:ExtractionRule) ON (r.approved)",
        "CREATE INDEX supervisorstate_region IF NOT EXISTS FOR (s:SupervisorState) ON (s.region)",
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

    // --- Tag node constraints and indexes ---
    let tag_constraints = [
        "CREATE CONSTRAINT tag_id_unique IF NOT EXISTS FOR (t:Tag) REQUIRE t.id IS UNIQUE",
        "CREATE CONSTRAINT tag_slug_unique IF NOT EXISTS FOR (t:Tag) REQUIRE t.slug IS UNIQUE",
        "CREATE CONSTRAINT tag_slug_exists IF NOT EXISTS FOR (t:Tag) REQUIRE t.slug IS NOT NULL",
        "CREATE CONSTRAINT tag_name_exists IF NOT EXISTS FOR (t:Tag) REQUIRE t.name IS NOT NULL",
    ];

    for c in &tag_constraints {
        g.run(query(c)).await?;
    }
    info!("Tag constraints created");

    // --- Story geo composite index (benefits all story geo queries) ---
    g.run(query(
        "CREATE INDEX story_centroid_lat_lng IF NOT EXISTS FOR (s:Story) ON (s.centroid_lat, s.centroid_lng)",
    ))
    .await?;
    info!("Story geo composite index created");

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

    // NOTE: cleanup_off_geo_signals and deactivate_duplicate_cities removed
    // as part of city concept removal (demand-driven scout swarm).

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

    // --- extracted_at indexes for supervisor time-window queries ---
    let extracted_at_indexes = [
        "CREATE INDEX gathering_extracted_at IF NOT EXISTS FOR (n:Gathering) ON (n.extracted_at)",
        "CREATE INDEX aid_extracted_at IF NOT EXISTS FOR (n:Aid) ON (n.extracted_at)",
        "CREATE INDEX need_extracted_at IF NOT EXISTS FOR (n:Need) ON (n.extracted_at)",
        "CREATE INDEX notice_extracted_at IF NOT EXISTS FOR (n:Notice) ON (n.extracted_at)",
        "CREATE INDEX tension_extracted_at IF NOT EXISTS FOR (n:Tension) ON (n.extracted_at)",
    ];
    for idx in &extracted_at_indexes {
        g.run(query(idx)).await?;
    }
    info!("Signal extracted_at indexes created");

    // --- review_status indexes for quality gate (staged/live/rejected) ---
    let review_status_indexes = [
        "CREATE INDEX gathering_review_status IF NOT EXISTS FOR (n:Gathering) ON (n.review_status)",
        "CREATE INDEX aid_review_status IF NOT EXISTS FOR (n:Aid) ON (n.review_status)",
        "CREATE INDEX need_review_status IF NOT EXISTS FOR (n:Need) ON (n.review_status)",
        "CREATE INDEX notice_review_status IF NOT EXISTS FOR (n:Notice) ON (n.review_status)",
        "CREATE INDEX tension_review_status IF NOT EXISTS FOR (n:Tension) ON (n.review_status)",
        "CREATE INDEX story_review_status IF NOT EXISTS FOR (n:Story) ON (n.review_status)",
    ];
    for idx in &review_status_indexes {
        g.run(query(idx)).await?;
    }
    info!("Review status indexes created");

    // --- Backfill existing signals and stories as 'live' (already visible to users) ---
    backfill_review_status(client).await?;

    // --- Migrate canonical keys: remove source_type from key format, add domain to social values ---
    migrate_canonical_keys_remove_source_type(client).await?;

    // --- Drop legacy source_type index (property kept as read-only breadcrumb) ---
    match g.run(query("DROP INDEX source_type IF EXISTS")).await {
        Ok(_) => {}
        Err(e) => warn!("Drop source_type index failed (non-fatal): {e}"),
    }

    // NOTE: migrate_region_relationships removed as part of RegionNode deletion.

    // --- Channel diversity backfill and indexes ---
    backfill_channel_diversity(client).await?;

    // --- Remove city concept: drop legacy indexes and properties ---
    remove_city_concept(client).await?;

    // --- Situation and Dispatch node constraints and indexes ---
    let situation_constraints = [
        "CREATE CONSTRAINT situation_id_unique IF NOT EXISTS FOR (n:Situation) REQUIRE n.id IS UNIQUE",
        "CREATE CONSTRAINT dispatch_id_unique IF NOT EXISTS FOR (n:Dispatch) REQUIRE n.id IS UNIQUE",
    ];
    for c in &situation_constraints {
        g.run(query(c)).await?;
    }

    let situation_indexes = [
        "CREATE INDEX situation_arc IF NOT EXISTS FOR (n:Situation) ON (n.arc)",
        "CREATE INDEX situation_temperature IF NOT EXISTS FOR (n:Situation) ON (n.temperature)",
        "CREATE INDEX situation_category IF NOT EXISTS FOR (n:Situation) ON (n.category)",
        "CREATE INDEX situation_sensitivity IF NOT EXISTS FOR (n:Situation) ON (n.sensitivity)",
        "CREATE INDEX situation_last_updated IF NOT EXISTS FOR (n:Situation) ON (n.last_updated)",
        "CREATE INDEX situation_centroid_lat_lng IF NOT EXISTS FOR (n:Situation) ON (n.centroid_lat, n.centroid_lng)",
        "CREATE INDEX dispatch_situation_id IF NOT EXISTS FOR (n:Dispatch) ON (n.situation_id)",
        "CREATE INDEX dispatch_created_at IF NOT EXISTS FOR (n:Dispatch) ON (n.created_at)",
        "CREATE INDEX dispatch_flagged IF NOT EXISTS FOR (n:Dispatch) ON (n.flagged_for_review)",
    ];
    for idx in &situation_indexes {
        g.run(query(idx)).await?;
    }

    // Dual vector indexes for situation embeddings (1024-dim Voyage AI)
    let situation_vector_indexes = [
        "CREATE VECTOR INDEX situation_narrative_embedding IF NOT EXISTS FOR (n:Situation) ON (n.narrative_embedding) OPTIONS {indexConfig: {`vector.dimensions`: 1024, `vector.similarity_function`: 'cosine'}}",
        "CREATE VECTOR INDEX situation_causal_embedding IF NOT EXISTS FOR (n:Situation) ON (n.causal_embedding) OPTIONS {indexConfig: {`vector.dimensions`: 1024, `vector.similarity_function`: 'cosine'}}",
    ];
    for idx in &situation_vector_indexes {
        g.run(query(idx)).await?;
    }

    // EVIDENCES edge index (for signal→situation lookups)
    g.run(query(
        "CREATE INDEX evidences_match_confidence IF NOT EXISTS FOR ()-[r:EVIDENCES]-() ON (r.match_confidence)",
    )).await?;

    info!("Situation/Dispatch constraints and indexes created");

    info!("Schema migration complete");
    Ok(())
}

/// Backfill `channel_diversity = 1` on existing signal and story nodes where null.
/// Also creates property indexes for channel_diversity.
/// Idempotent — WHERE clause matches nothing after the first run.
async fn backfill_channel_diversity(client: &GraphClient) -> Result<(), neo4rs::Error> {
    let g = &client.graph;

    info!("Backfilling channel_diversity...");

    let labels = ["Gathering", "Aid", "Need", "Notice", "Tension", "Story"];
    for label in &labels {
        let cypher = format!(
            "MATCH (n:{label}) WHERE n.channel_diversity IS NULL SET n.channel_diversity = 1 RETURN count(n) AS updated"
        );
        match g.execute(query(&cypher)).await {
            Ok(mut stream) => {
                if let Some(row) = stream.next().await? {
                    let updated: i64 = row.get("updated").unwrap_or(0);
                    if updated > 0 {
                        info!(label, updated, "Backfilled channel_diversity = 1");
                    }
                }
            }
            Err(e) => warn!(label, "channel_diversity backfill failed (non-fatal): {e}"),
        }
    }

    // Channel diversity indexes
    let channel_indexes = [
        "CREATE INDEX gathering_channel_diversity IF NOT EXISTS FOR (n:Gathering) ON (n.channel_diversity)",
        "CREATE INDEX aid_channel_diversity IF NOT EXISTS FOR (n:Aid) ON (n.channel_diversity)",
        "CREATE INDEX need_channel_diversity IF NOT EXISTS FOR (n:Need) ON (n.channel_diversity)",
        "CREATE INDEX notice_channel_diversity IF NOT EXISTS FOR (n:Notice) ON (n.channel_diversity)",
        "CREATE INDEX tension_channel_diversity IF NOT EXISTS FOR (n:Tension) ON (n.channel_diversity)",
        "CREATE INDEX story_channel_diversity IF NOT EXISTS FOR (n:Story) ON (n.channel_diversity)",
    ];

    for idx in &channel_indexes {
        g.run(query(idx)).await?;
    }
    info!("Channel diversity indexes created");

    info!("Channel diversity backfill complete");
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

    for label in &["Gathering", "Aid", "Need", "Notice", "Tension"] {
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
         WHERE n:Gathering OR n:Aid OR n:Need OR n:Notice OR n:Tension
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
    for label in &["Gathering", "Aid", "Need", "Notice", "Tension"] {
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
        "MATCH (e:Gathering) WHERE e.starts_at = '' SET e.starts_at = null",
        "MATCH (e:Gathering) WHERE e.ends_at = '' SET e.ends_at = null",
        // Null out starts_at that equals extracted_at (scrape timestamp mistaken for event date)
        "MATCH (e:Gathering) WHERE e.starts_at IS NOT NULL AND e.starts_at = e.extracted_at SET e.starts_at = null",
        // Convert remaining string-typed dates to datetime (Neo4j 5.11+ type predicate syntax)
        "MATCH (e:Gathering) WHERE e.starts_at IS NOT NULL AND e.starts_at IS :: STRING SET e.starts_at = datetime(e.starts_at)",
        "MATCH (e:Gathering) WHERE e.ends_at IS NOT NULL AND e.ends_at IS :: STRING SET e.ends_at = datetime(e.ends_at)",
    ];

    for step in &steps {
        match g.run(query(step)).await {
            Ok(_) => {}
            Err(e) => warn!("Gathering date backfill step failed (non-fatal): {e}"),
        }
    }

    info!("Gathering date backfill complete");
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

/// Backfill existing signals and stories with review_status = 'live'.
/// Only sets the field on nodes where it is not already set (idempotent).
async fn backfill_review_status(client: &GraphClient) -> Result<(), neo4rs::Error> {
    let g = &client.graph;

    info!("Backfilling review_status on existing signals and stories...");

    let labels = ["Gathering", "Aid", "Need", "Notice", "Tension", "Story"];
    for label in &labels {
        let cypher = format!(
            "MATCH (n:{label}) WHERE n.review_status IS NULL SET n.review_status = 'live' RETURN count(n) AS updated"
        );
        match g.execute(query(&cypher)).await {
            Ok(mut stream) => {
                if let Some(row) = stream.next().await? {
                    let updated: i64 = row.get("updated").unwrap_or(0);
                    if updated > 0 {
                        info!(label, updated, "Backfilled review_status = 'live'");
                    }
                }
            }
            Err(e) => warn!(label, "review_status backfill failed (non-fatal): {e}"),
        }
    }

    info!("Review status backfill complete");
    Ok(())
}

/// Migrate canonical_keys to remove source_type from the key format, and add domain
/// prefixes to social source canonical_values to prevent collisions.
///
/// Old format: `city:source_type:canonical_value` (e.g. `twincities:instagram:lakestreetstories`)
/// Intermediate format: `city:canonical_value` (e.g. `twincities:instagram.com/lakestreetstories`)
/// Final format (after remove_city_concept): just `canonical_value`
///
/// Idempotent — skips sources whose canonical_value already contains a domain prefix.
async fn migrate_canonical_keys_remove_source_type(
    client: &GraphClient,
) -> Result<(), neo4rs::Error> {
    let g = &client.graph;

    // Guard: check if migration is needed by looking for old-format keys (city:type:value)
    let check = query(
        "MATCH (s:Source)
         WHERE s.source_type IS NOT NULL
           AND s.canonical_key CONTAINS ':' + s.source_type + ':'
         RETURN count(s) AS cnt",
    );
    let mut stream = g.execute(check).await?;
    let needs_migration = match stream.next().await? {
        Some(row) => row.get::<i64>("cnt").unwrap_or(0) > 0,
        None => false,
    };

    if !needs_migration {
        info!("canonical_key migration: already complete, skipping");
        return Ok(());
    }

    info!("Migrating canonical_keys: removing source_type from key format...");

    // Pre-migration collision audit: check if the new keys would collide
    let collision_check = query(
        "MATCH (s:Source)
         WITH s.city + ':' +
           CASE
             WHEN s.source_type = 'instagram' AND NOT s.canonical_value STARTS WITH 'instagram.com/'
               THEN 'instagram.com/' + s.canonical_value
             WHEN s.source_type = 'reddit' AND NOT s.canonical_value STARTS WITH 'reddit.com/'
               THEN 'reddit.com/r/' + s.canonical_value
             WHEN s.source_type = 'twitter' AND NOT s.canonical_value STARTS WITH 'x.com/'
               THEN 'x.com/' + s.canonical_value
             WHEN s.source_type = 'tiktok' AND NOT s.canonical_value STARTS WITH 'tiktok.com/'
               THEN 'tiktok.com/' + s.canonical_value
             ELSE s.canonical_value
           END AS new_key, collect(s.id) AS ids, count(*) AS cnt
         WHERE cnt > 1
         RETURN new_key, ids, cnt",
    );
    let mut stream = g.execute(collision_check).await?;
    let mut has_collisions = false;
    while let Some(row) = stream.next().await? {
        let new_key: String = row.get("new_key").unwrap_or_default();
        let cnt: i64 = row.get("cnt").unwrap_or(0);
        warn!(new_key, cnt, "COLLISION detected in canonical_key migration — aborting");
        has_collisions = true;
    }

    if has_collisions {
        warn!("canonical_key migration aborted due to collisions — resolve manually");
        return Ok(());
    }

    // Step 1: Rewrite canonical_value for social sources to include domain
    let social_rewrites = [
        ("instagram", "instagram.com/", "instagram.com/"),
        ("reddit", "reddit.com/r/", "reddit.com/"),
        ("twitter", "x.com/", "x.com/"),
        ("tiktok", "tiktok.com/", "tiktok.com/"),
    ];

    for (source_type, prefix, guard) in &social_rewrites {
        let q = query(
            "MATCH (s:Source)
             WHERE s.source_type = $source_type
               AND NOT s.canonical_value STARTS WITH $guard
             SET s.canonical_value = $prefix + s.canonical_value
             RETURN count(s) AS updated",
        )
        .param("source_type", *source_type)
        .param("prefix", *prefix)
        .param("guard", *guard);

        match g.execute(q).await {
            Ok(mut stream) => {
                if let Some(row) = stream.next().await? {
                    let updated: i64 = row.get("updated").unwrap_or(0);
                    if updated > 0 {
                        info!(source_type, updated, "Rewrote canonical_value with domain prefix");
                    }
                }
            }
            Err(e) => warn!(source_type, "canonical_value rewrite failed: {e}"),
        }
    }

    // Step 2: Rewrite all canonical_keys to new format (city:canonical_value)
    let rewrite_keys = query(
        "MATCH (s:Source)
         WHERE s.source_type IS NOT NULL
           AND s.canonical_key CONTAINS ':' + s.source_type + ':'
         SET s.canonical_key = s.city + ':' + s.canonical_value
         RETURN count(s) AS updated",
    );
    match g.execute(rewrite_keys).await {
        Ok(mut stream) => {
            if let Some(row) = stream.next().await? {
                let updated: i64 = row.get("updated").unwrap_or(0);
                info!(updated, "Rewrote canonical_keys to city:value format");
            }
        }
        Err(e) => warn!("canonical_key rewrite failed: {e}"),
    }

    // Step 3: Fix GoFundMe bootstrap sources (url should be null for query sources)
    let fix_gofundme = query(
        "MATCH (s:Source)
         WHERE s.source_type = 'web_query'
           AND s.url IS NOT NULL AND s.url <> ''
           AND NOT (s.url STARTS WITH 'http://' OR s.url STARTS WITH 'https://')
         SET s.url = null
         RETURN count(s) AS updated",
    );
    match g.execute(fix_gofundme).await {
        Ok(mut stream) => {
            if let Some(row) = stream.next().await? {
                let updated: i64 = row.get("updated").unwrap_or(0);
                if updated > 0 {
                    info!(updated, "Fixed non-URL values stored in Source.url");
                }
            }
        }
        Err(e) => warn!("GoFundMe url fix failed (non-fatal): {e}"),
    }

    info!("canonical_key migration complete");
    Ok(())
}

/// Remove the city concept from the graph: drop city indexes, rewrite canonical_keys
/// to remove city prefix, rename city→region on ScoutLock/ValidationIssue/SupervisorState,
/// and delete :City nodes.
async fn remove_city_concept(client: &GraphClient) -> Result<(), neo4rs::Error> {
    let g = &client.graph;

    // Guard: check if migration is needed
    let check = query("MATCH (s:Source) WHERE s.city IS NOT NULL RETURN count(s) AS cnt");
    let mut stream = g.execute(check).await?;
    let needs_migration = match stream.next().await? {
        Some(row) => row.get::<i64>("cnt").unwrap_or(0) > 0,
        None => false,
    };

    if !needs_migration {
        info!("remove_city_concept: already complete, skipping");
        return Ok(());
    }

    info!("Removing city concept from graph...");

    // Step 1: Drop legacy city indexes
    let city_index_drops = [
        "DROP INDEX source_city IF EXISTS",
        "DROP INDEX validationissue_city IF EXISTS",
        "DROP INDEX extractionrule_city IF EXISTS",
        "DROP INDEX supervisorstate_city IF EXISTS",
        "DROP INDEX place_city IF EXISTS",
    ];
    for d in &city_index_drops {
        match g.run(query(d)).await {
            Ok(_) => {}
            Err(e) => warn!("City index drop failed (non-fatal): {e}"),
        }
    }
    info!("Dropped legacy city indexes");

    // Step 2: Rewrite canonical_key from "city:canonical_value" → "canonical_value"
    // Keep the one with more signals_produced if there are collisions
    let rewrite = query(
        "MATCH (s:Source) WHERE s.city IS NOT NULL AND s.canonical_key CONTAINS ':'
         SET s.canonical_key = s.canonical_value
         RETURN count(s) AS updated",
    );
    match g.execute(rewrite).await {
        Ok(mut stream) => {
            if let Some(row) = stream.next().await? {
                let updated: i64 = row.get("updated").unwrap_or(0);
                info!(updated, "Rewrote canonical_keys to remove city prefix");
            }
        }
        Err(e) => warn!("canonical_key rewrite failed: {e}"),
    }

    // Step 3: Rename city→region on ScoutLock nodes
    let rename_lock = query(
        "MATCH (lock:ScoutLock) WHERE lock.city IS NOT NULL
         SET lock.region = lock.city REMOVE lock.city
         RETURN count(lock) AS updated",
    );
    match g.execute(rename_lock).await {
        Ok(mut s) => {
            if let Some(row) = s.next().await? {
                let u: i64 = row.get("updated").unwrap_or(0);
                if u > 0 { info!(u, "Renamed ScoutLock.city → region"); }
            }
        }
        Err(e) => warn!("ScoutLock rename failed (non-fatal): {e}"),
    }

    // Step 4: Copy city→region on ValidationIssue/ExtractionRule/SupervisorState (where region is null)
    for label in &["ValidationIssue", "ExtractionRule", "SupervisorState"] {
        let cypher = format!(
            "MATCH (n:{label}) WHERE n.city IS NOT NULL AND n.region IS NULL \
             SET n.region = n.city RETURN count(n) AS updated"
        );
        match g.execute(query(&cypher)).await {
            Ok(mut s) => {
                if let Some(row) = s.next().await? {
                    let u: i64 = row.get("updated").unwrap_or(0);
                    if u > 0 { info!(u, label, "Backfilled region from city"); }
                }
            }
            Err(e) => warn!("Region backfill for {label} failed (non-fatal): {e}"),
        }
    }

    // Step 5: Remove city property from all node types
    for label in &["Source", "Place", "ScoutLock", "ValidationIssue", "ExtractionRule", "SupervisorState", "Submission"] {
        let cypher = format!(
            "MATCH (n:{label}) WHERE n.city IS NOT NULL REMOVE n.city RETURN count(n) AS updated"
        );
        match g.execute(query(&cypher)).await {
            Ok(mut s) => {
                if let Some(row) = s.next().await? {
                    let u: i64 = row.get("updated").unwrap_or(0);
                    if u > 0 { info!(u, label, "Removed city property"); }
                }
            }
            Err(e) => warn!("Remove city from {label} failed (non-fatal): {e}"),
        }
    }

    // Step 6: Delete :City nodes and their edges
    match g.run(query("MATCH (c:City) DETACH DELETE c")).await {
        Ok(_) => info!("Deleted :City nodes"),
        Err(e) => warn!("Delete :City nodes failed (non-fatal): {e}"),
    }

    info!("City concept removal complete");
    Ok(())
}
