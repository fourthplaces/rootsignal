use neo4rs::query;
use tracing::info;

use crate::GraphClient;

/// Ensure Neo4j schema exists: constraints, indexes, vector indexes, fulltext indexes.
///
/// All statements use IF NOT EXISTS / IF EXISTS — fully idempotent.
/// Neo4j is a derived projection; data comes from replaying events, not from
/// migrations. This function only creates the structural schema the projector needs.
pub async fn migrate(client: &GraphClient) -> Result<(), neo4rs::Error> {
    let g = client;

    info!("Ensuring Neo4j schema...");

    // ── Signal node constraints ──────────────────────────────────────────

    let signal_labels = [
        "Gathering",
        "Resource",
        "HelpRequest",
        "Announcement",
        "Concern",
        "Condition",
    ];

    // UUID uniqueness
    for label in &signal_labels {
        let c = format!(
            "CREATE CONSTRAINT {l}_id_unique IF NOT EXISTS FOR (n:{l}) REQUIRE n.id IS UNIQUE",
            l = label.to_lowercase()
        );
        // Constraint names use lowercase to avoid conflicts with legacy names
        g.run(query(&c)).await?;
    }

    // Sensitivity + confidence NOT NULL (skip Condition — it doesn't carry these)
    for label in &["Gathering", "Resource", "HelpRequest", "Announcement", "Concern"] {
        let l = label.to_lowercase();
        g.run(query(&format!(
            "CREATE CONSTRAINT {l}_sensitivity_exists IF NOT EXISTS FOR (n:{label}) REQUIRE n.sensitivity IS NOT NULL"
        ))).await?;
        g.run(query(&format!(
            "CREATE CONSTRAINT {l}_confidence_exists IF NOT EXISTS FOR (n:{label}) REQUIRE n.confidence IS NOT NULL"
        ))).await?;
    }

    info!("Signal constraints created");

    // ── Signal property indexes ──────────────────────────────────────────

    for label in &signal_labels {
        let l = label.to_lowercase();
        for prop in &["lat", "lng", "source_diversity", "cause_heat", "extracted_at", "review_status", "channel_diversity"] {
            g.run(query(&format!(
                "CREATE INDEX {l}_{prop} IF NOT EXISTS FOR (n:{label}) ON (n.{prop})"
            ))).await?;
        }
    }

    info!("Signal property indexes created");

    // ── Signal fulltext indexes ──────────────────────────────────────────

    for label in &signal_labels {
        let l = label.to_lowercase();
        g.run(query(&format!(
            "CREATE FULLTEXT INDEX {l}_text IF NOT EXISTS FOR (n:{label}) ON EACH [n.title, n.summary]"
        ))).await?;
    }

    info!("Signal fulltext indexes created");

    // ── Signal vector indexes (1024-dim Voyage embeddings) ───────────────

    for label in &signal_labels {
        let l = label.to_lowercase();
        g.run(query(&format!(
            "CREATE VECTOR INDEX {l}_embedding IF NOT EXISTS FOR (n:{label}) ON (n.embedding) \
             OPTIONS {{indexConfig: {{`vector.dimensions`: 1024, `vector.similarity_function`: 'cosine'}}}}"
        ))).await?;
    }

    info!("Signal vector indexes created");

    // ── Citation constraints ─────────────────────────────────────────────

    g.run(query(
        "CREATE CONSTRAINT citation_id_unique IF NOT EXISTS FOR (n:Citation) REQUIRE n.id IS UNIQUE",
    )).await?;

    // ── Actor constraints and indexes ────────────────────────────────────

    g.run(query("CREATE CONSTRAINT actor_id_unique IF NOT EXISTS FOR (a:Actor) REQUIRE a.id IS UNIQUE")).await?;
    g.run(query("CREATE CONSTRAINT actor_canonical_key_unique IF NOT EXISTS FOR (a:Actor) REQUIRE a.canonical_key IS UNIQUE")).await?;
    g.run(query("CREATE INDEX actor_name IF NOT EXISTS FOR (a:Actor) ON (a.name)")).await?;

    info!("Actor constraints and indexes created");

    // ── Pin constraints and indexes ──────────────────────────────────────

    g.run(query("CREATE CONSTRAINT pin_id_unique IF NOT EXISTS FOR (p:Pin) REQUIRE p.id IS UNIQUE")).await?;
    g.run(query("CREATE INDEX pin_location IF NOT EXISTS FOR (p:Pin) ON (p.location_lat, p.location_lng)")).await?;

    // ── Edge indexes ─────────────────────────────────────────────────────

    g.run(query("CREATE INDEX similar_to_weight IF NOT EXISTS FOR ()-[r:SIMILAR_TO]-() ON (r.weight)")).await?;
    g.run(query("CREATE INDEX part_of_match_confidence IF NOT EXISTS FOR ()-[r:PART_OF]-() ON (r.match_confidence)")).await?;
    g.run(query("CREATE INDEX evidence_of_match_strength IF NOT EXISTS FOR ()-[r:EVIDENCE_OF]-() ON (r.match_strength)")).await?;

    info!("Edge indexes created");

    // ── Region constraints and indexes ──────────────────────────────────

    g.run(query("CREATE CONSTRAINT region_id IF NOT EXISTS FOR (r:Region) REQUIRE r.id IS UNIQUE")).await?;
    g.run(query("CREATE INDEX region_is_leaf IF NOT EXISTS FOR (r:Region) ON (r.is_leaf)")).await?;

    info!("Region constraints and indexes created");

    // ── Source constraints and indexes ────────────────────────────────────

    g.run(query("CREATE CONSTRAINT source_id_unique IF NOT EXISTS FOR (s:Source) REQUIRE s.id IS UNIQUE")).await?;
    g.run(query("CREATE CONSTRAINT source_canonical_key_unique IF NOT EXISTS FOR (s:Source) REQUIRE s.canonical_key IS UNIQUE")).await?;
    g.run(query("CREATE INDEX source_active IF NOT EXISTS FOR (s:Source) ON (s.active)")).await?;
    g.run(query("CREATE INDEX source_url IF NOT EXISTS FOR (s:Source) ON (s.url)")).await?;
    g.run(query("CREATE INDEX source_weight IF NOT EXISTS FOR (s:Source) ON (s.weight)")).await?;
    g.run(query("CREATE INDEX source_role IF NOT EXISTS FOR (s:Source) ON (s.source_role)")).await?;
    g.run(query(
        "CREATE VECTOR INDEX source_query_embedding IF NOT EXISTS FOR (s:Source) ON (s.query_embedding) \
         OPTIONS {indexConfig: {`vector.dimensions`: 1024, `vector.similarity_function`: 'cosine'}}"
    )).await?;

    info!("Source constraints and indexes created");

    // ── BlockedSource constraint ─────────────────────────────────────────

    g.run(query("CREATE CONSTRAINT blockedsource_url_pattern_unique IF NOT EXISTS FOR (b:BlockedSource) REQUIRE b.url_pattern IS UNIQUE")).await?;

    // ── Supervisor-related constraints and indexes ────────────────────────

    g.run(query("CREATE CONSTRAINT extractionrule_id_unique IF NOT EXISTS FOR (r:ExtractionRule) REQUIRE r.id IS UNIQUE")).await?;
    g.run(query("CREATE CONSTRAINT validationissue_id_unique IF NOT EXISTS FOR (v:ValidationIssue) REQUIRE v.id IS UNIQUE")).await?;
    g.run(query("CREATE INDEX validationissue_status IF NOT EXISTS FOR (v:ValidationIssue) ON (v.status)")).await?;
    g.run(query("CREATE INDEX validationissue_region IF NOT EXISTS FOR (v:ValidationIssue) ON (v.region)")).await?;
    g.run(query("CREATE INDEX validationissue_target_id IF NOT EXISTS FOR (v:ValidationIssue) ON (v.target_id)")).await?;
    g.run(query("CREATE INDEX extractionrule_region IF NOT EXISTS FOR (r:ExtractionRule) ON (r.region)")).await?;
    g.run(query("CREATE INDEX extractionrule_approved IF NOT EXISTS FOR (r:ExtractionRule) ON (r.approved)")).await?;

    info!("Supervisor constraints and indexes created");

    // ── Place constraints and indexes ────────────────────────────────────

    g.run(query("CREATE CONSTRAINT place_id_unique IF NOT EXISTS FOR (p:Place) REQUIRE p.id IS UNIQUE")).await?;
    g.run(query("CREATE INDEX place_slug IF NOT EXISTS FOR (p:Place) ON (p.slug)")).await?;

    // ── Tag constraints ──────────────────────────────────────────────────

    g.run(query("CREATE CONSTRAINT tag_id_unique IF NOT EXISTS FOR (t:Tag) REQUIRE t.id IS UNIQUE")).await?;
    g.run(query("CREATE CONSTRAINT tag_slug_unique IF NOT EXISTS FOR (t:Tag) REQUIRE t.slug IS UNIQUE")).await?;
    g.run(query("CREATE CONSTRAINT tag_slug_exists IF NOT EXISTS FOR (t:Tag) REQUIRE t.slug IS NOT NULL")).await?;
    g.run(query("CREATE CONSTRAINT tag_name_exists IF NOT EXISTS FOR (t:Tag) REQUIRE t.name IS NOT NULL")).await?;

    info!("Tag constraints created");

    // ── Situation and Dispatch constraints and indexes ────────────────────

    g.run(query("CREATE CONSTRAINT situation_id_unique IF NOT EXISTS FOR (n:Situation) REQUIRE n.id IS UNIQUE")).await?;
    g.run(query("CREATE CONSTRAINT dispatch_id_unique IF NOT EXISTS FOR (n:Dispatch) REQUIRE n.id IS UNIQUE")).await?;
    g.run(query("CREATE INDEX situation_arc IF NOT EXISTS FOR (n:Situation) ON (n.arc)")).await?;
    g.run(query("CREATE INDEX situation_temperature IF NOT EXISTS FOR (n:Situation) ON (n.temperature)")).await?;
    g.run(query("CREATE INDEX situation_category IF NOT EXISTS FOR (n:Situation) ON (n.category)")).await?;
    g.run(query("CREATE INDEX situation_sensitivity IF NOT EXISTS FOR (n:Situation) ON (n.sensitivity)")).await?;
    g.run(query("CREATE INDEX situation_last_updated IF NOT EXISTS FOR (n:Situation) ON (n.last_updated)")).await?;
    g.run(query("CREATE INDEX situation_centroid_lat_lng IF NOT EXISTS FOR (n:Situation) ON (n.centroid_lat, n.centroid_lng)")).await?;
    g.run(query("CREATE INDEX dispatch_situation_id IF NOT EXISTS FOR (n:Dispatch) ON (n.situation_id)")).await?;
    g.run(query("CREATE INDEX dispatch_created_at IF NOT EXISTS FOR (n:Dispatch) ON (n.created_at)")).await?;
    g.run(query("CREATE INDEX dispatch_flagged IF NOT EXISTS FOR (n:Dispatch) ON (n.flagged_for_review)")).await?;
    g.run(query(
        "CREATE VECTOR INDEX situation_narrative_embedding IF NOT EXISTS FOR (n:Situation) ON (n.narrative_embedding) \
         OPTIONS {indexConfig: {`vector.dimensions`: 1024, `vector.similarity_function`: 'cosine'}}"
    )).await?;
    g.run(query(
        "CREATE VECTOR INDEX situation_causal_embedding IF NOT EXISTS FOR (n:Situation) ON (n.causal_embedding) \
         OPTIONS {indexConfig: {`vector.dimensions`: 1024, `vector.similarity_function`: 'cosine'}}"
    )).await?;

    info!("Situation/Dispatch constraints and indexes created");

    // ── SignalGroup constraints and indexes ────────────────────────────

    g.run(query("CREATE CONSTRAINT signalgroup_id_unique IF NOT EXISTS FOR (n:SignalGroup) REQUIRE n.id IS UNIQUE")).await?;
    g.run(query("CREATE INDEX signalgroup_created_at IF NOT EXISTS FOR (n:SignalGroup) ON (n.created_at)")).await?;

    info!("SignalGroup constraints and indexes created");

    // ── Schedule constraints and indexes ──────────────────────────────────

    g.run(query("CREATE CONSTRAINT schedule_id_unique IF NOT EXISTS FOR (n:Schedule) REQUIRE n.id IS UNIQUE")).await?;
    g.run(query("CREATE INDEX schedule_dtstart IF NOT EXISTS FOR (n:Schedule) ON (n.dtstart)")).await?;

    // ── DomainVerdict constraint ─────────────────────────────────────────

    g.run(query("CREATE CONSTRAINT domainverdict_domain_unique IF NOT EXISTS FOR (d:DomainVerdict) REQUIRE d.domain IS UNIQUE")).await?;

    // ── Location node indexes ────────────────────────────────────────────

    g.run(query("CREATE INDEX location_lat_lng IF NOT EXISTS FOR (l:Location) ON (l.lat, l.lng)")).await?;
    g.run(query("CREATE INDEX location_normalized_name IF NOT EXISTS FOR (l:Location) ON (l.normalized_name)")).await?;

    // ── Data migrations (idempotent) ─────────────────────────────────────

    // Rename review_status 'live' → 'accepted' across all signal + situation nodes
    for label in &["Gathering", "Resource", "HelpRequest", "Announcement", "Concern", "Condition", "Situation"] {
        g.run(query(&format!(
            "MATCH (n:{label}) WHERE n.review_status = 'live' SET n.review_status = 'accepted'"
        ))).await?;
    }

    info!("Neo4j schema ready");
    Ok(())
}
