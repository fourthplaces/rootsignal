//! Integration tests for litmus-test scenarios.
//!
//! Validates datetime storage, keyword search, geo queries, source diversity,
//! actor linking, and cross-type topic search against a real Neo4j instance.
//!
//! Requirements: Docker (for Neo4j via testcontainers)
//!
//! Run with: cargo test -p rootsignal-graph --features test-utils --test litmus_test

#![cfg(feature = "test-utils")]

use chrono::Utc;
use uuid::Uuid;

use rootsignal_common::{DiscoveryMethod, EvidenceNode, SourceNode, SourceRole, SourceType};
use rootsignal_graph::{query, GraphClient, GraphWriter};

/// Spin up a fresh Neo4j container and run migrations.
async fn setup() -> (impl std::any::Any, GraphClient) {
    let (container, client) = rootsignal_graph::testutil::neo4j_container().await;
    rootsignal_graph::migrate::migrate(&client).await.expect("migration failed");
    (container, client)
}

fn neo4j_dt(dt: &chrono::DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
}

/// Build a 1024-dimensional dummy embedding as a Cypher list literal.
/// Matches the dimension configured in the vector index migration.
fn dummy_embedding() -> String {
    let mut parts = vec!["0.1".to_string()];
    parts.extend(std::iter::repeat("0.0".to_string()).take(1023));
    format!("[{}]", parts.join(","))
}

/// Helper: create a signal node with minimal required properties.
async fn create_signal(
    client: &GraphClient,
    label: &str,
    id: Uuid,
    title: &str,
    source_url: &str,
) {
    let now = neo4j_dt(&Utc::now());
    let emb = dummy_embedding();
    let cypher = format!(
        "CREATE (n:{label} {{
            id: $id,
            title: $title,
            summary: $summary,
            sensitivity: 'general',
            confidence: 0.8,
            freshness_score: 0.8,
            corroboration_count: 0,
            source_diversity: 1,
            external_ratio: 0.0,
            cause_heat: 0.0,
            source_url: $source_url,
            extracted_at: datetime($now),
            last_confirmed_active: datetime($now),
            location_name: '',
            lat: 44.9778,
            lng: -93.2650,
            embedding: {emb}
        }})"
    );

    let q = query(&cypher)
        .param("id", id.to_string())
        .param("title", title)
        .param("summary", format!("Test signal: {title}"))
        .param("source_url", source_url)
        .param("now", now);

    client.inner().run(q).await.expect("Failed to create signal");
}

/// Helper: create an Event with a specific starts_at using the CASE/datetime pattern.
async fn create_event_with_date(
    client: &GraphClient,
    id: Uuid,
    title: &str,
    starts_at: &str, // ISO datetime string, or "" for missing
) {
    let now = neo4j_dt(&Utc::now());
    let emb = dummy_embedding();
    let cypher = format!(
        "CREATE (e:Event {{
            id: $id,
            title: $title,
            summary: $summary,
            sensitivity: 'general',
            confidence: 0.8,
            freshness_score: 0.8,
            corroboration_count: 0,
            source_diversity: 1,
            external_ratio: 0.0,
            cause_heat: 0.0,
            source_url: 'https://test.com',
            extracted_at: datetime($now),
            last_confirmed_active: datetime($now),
            location_name: '',
            starts_at: CASE WHEN $starts_at = '' THEN null ELSE datetime($starts_at) END,
            ends_at: null,
            action_url: '',
            organizer: '',
            is_recurring: false,
            lat: 44.9778,
            lng: -93.2650,
            embedding: {emb}
        }})"
    );
    let q = query(&cypher)
    .param("id", id.to_string())
    .param("title", title)
    .param("summary", format!("Test event: {title}"))
    .param("starts_at", starts_at)
    .param("now", now);

    client.inner().run(q).await.expect("Failed to create event");
}

/// Helper: create an Actor and link it to a signal via ACTED_IN.
async fn create_actor_and_link(
    client: &GraphClient,
    actor_id: Uuid,
    name: &str,
    signal_id: Uuid,
    signal_label: &str,
    role: &str,
) {
    let now = neo4j_dt(&Utc::now());
    let q = query(
        "CREATE (a:Actor {
            id: $id,
            entity_id: $entity_id,
            name: $name,
            actor_type: 'org',
            domains: [],
            social_urls: [],
            city: 'twincities',
            description: 'Test actor',
            signal_count: 1,
            first_seen: datetime($now),
            last_active: datetime($now),
            typical_roles: [$role]
        })"
    )
    .param("id", actor_id.to_string())
    .param("entity_id", format!("test-entity-{}", actor_id))
    .param("name", name)
    .param("now", now)
    .param("role", role);

    client.inner().run(q).await.expect("Failed to create actor");

    let cypher = format!(
        "MATCH (a:Actor {{id: $actor_id}}), (n:{signal_label} {{id: $signal_id}})
         MERGE (a)-[:ACTED_IN {{role: $role}}]->(n)"
    );
    let q = query(&cypher)
        .param("actor_id", actor_id.to_string())
        .param("signal_id", signal_id.to_string())
        .param("role", role);

    client.inner().run(q).await.expect("Failed to link actor to signal");
}

// ---------------------------------------------------------------------------
// Test 1: Event date stored as proper datetime
// ---------------------------------------------------------------------------

#[tokio::test]
async fn event_date_stored_as_datetime() {
    let (_container, client) = setup().await;

    let id = Uuid::new_v4();
    create_event_with_date(&client, id, "Test dated event", "2026-03-15T18:00:00.000000").await;

    let q = query(
        "MATCH (e:Event {id: $id})
         RETURN valueType(e.starts_at) AS vtype, e.starts_at IS NOT NULL AS has_date"
    )
    .param("id", id.to_string());

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream error").expect("no row");

    let vtype: String = row.get("vtype").expect("no vtype");
    let has_date: bool = row.get("has_date").expect("no has_date");

    assert!(has_date, "starts_at should be non-null");
    assert!(
        vtype.contains("DATE") || vtype.contains("date"),
        "starts_at should be a datetime type, got: {vtype}"
    );
}

// ---------------------------------------------------------------------------
// Test 2: Empty date stored as null
// ---------------------------------------------------------------------------

#[tokio::test]
async fn event_null_date_stored_as_null() {
    let (_container, client) = setup().await;

    let id = Uuid::new_v4();
    create_event_with_date(&client, id, "No-date event", "").await;

    let q = query(
        "MATCH (e:Event {id: $id})
         RETURN e.starts_at IS NULL AS is_null"
    )
    .param("id", id.to_string());

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream error").expect("no row");

    let is_null: bool = row.get("is_null").expect("no is_null");
    assert!(is_null, "starts_at should be null for empty string input");
}

// ---------------------------------------------------------------------------
// Test 3: Events sortable by starts_at (no crash on mixed types)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn events_sortable_by_starts_at() {
    let (_container, client) = setup().await;

    // Create events: one with date, one without, one with later date
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let id3 = Uuid::new_v4();
    create_event_with_date(&client, id1, "March event", "2026-03-10T10:00:00.000000").await;
    create_event_with_date(&client, id2, "No-date event", "").await;
    create_event_with_date(&client, id3, "April event", "2026-04-01T14:00:00.000000").await;

    // ORDER BY on non-null dates should not crash
    let q = query(
        "MATCH (e:Event)
         WHERE e.starts_at IS NOT NULL
         RETURN e.title AS title
         ORDER BY e.starts_at"
    );

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let mut titles = Vec::new();
    while let Some(row) = stream.next().await.expect("stream error") {
        let title: String = row.get("title").expect("no title");
        titles.push(title);
    }

    assert_eq!(titles.len(), 2, "should get 2 dated events, got {}", titles.len());
    assert_eq!(titles[0], "March event", "March should sort before April");
    assert_eq!(titles[1], "April event", "April should sort after March");
}

// ---------------------------------------------------------------------------
// Test 4: Topic keyword search
// ---------------------------------------------------------------------------

#[tokio::test]
async fn topic_keyword_search() {
    let (_container, client) = setup().await;

    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let id3 = Uuid::new_v4();
    create_signal(&client, "Give", id1, "Free food at community center", "https://food.org").await;
    create_signal(&client, "Ask", id2, "Volunteers needed for food drive", "https://drive.org").await;
    create_signal(&client, "Event", id3, "Housing forum downtown", "https://housing.org").await;

    let q = query(
        "MATCH (n)
         WHERE (n:Give OR n:Ask OR n:Event) AND toLower(n.title) CONTAINS 'food'
         RETURN n.title AS title"
    );

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let mut found = Vec::new();
    while let Some(row) = stream.next().await.expect("stream error") {
        let title: String = row.get("title").expect("no title");
        found.push(title);
    }

    assert_eq!(found.len(), 2, "should find 2 food-related signals, got {}", found.len());
    assert!(found.iter().all(|t| t.to_lowercase().contains("food")));
}

// ---------------------------------------------------------------------------
// Test 5: Geo bounding box query
// ---------------------------------------------------------------------------

#[tokio::test]
async fn geo_bounding_box_query() {
    let (_container, client) = setup().await;

    let now = neo4j_dt(&Utc::now());
    let emb = dummy_embedding();

    // Signal inside bounding box (downtown Minneapolis)
    let inside_id = Uuid::new_v4();
    let cypher = format!(
        "CREATE (n:Event {{
            id: $id, title: 'Inside event', summary: 'In range',
            sensitivity: 'general', confidence: 0.8, freshness_score: 0.8,
            corroboration_count: 0, source_diversity: 1, external_ratio: 0.0,
            cause_heat: 0.0, source_url: 'https://test.com',
            extracted_at: datetime($now), last_confirmed_active: datetime($now),
            location_name: '', starts_at: null, ends_at: null,
            action_url: '', organizer: '', is_recurring: false,
            lat: 44.975, lng: -93.265,
            embedding: {emb}
        }})"
    );
    let q = query(&cypher)
        .param("id", inside_id.to_string())
        .param("now", now.clone());
    client.inner().run(q).await.expect("create inside event");

    // Signal outside bounding box (Duluth)
    let outside_id = Uuid::new_v4();
    let cypher = format!(
        "CREATE (n:Event {{
            id: $id, title: 'Outside event', summary: 'Out of range',
            sensitivity: 'general', confidence: 0.8, freshness_score: 0.8,
            corroboration_count: 0, source_diversity: 1, external_ratio: 0.0,
            cause_heat: 0.0, source_url: 'https://test.com',
            extracted_at: datetime($now), last_confirmed_active: datetime($now),
            location_name: '', starts_at: null, ends_at: null,
            action_url: '', organizer: '', is_recurring: false,
            lat: 46.786, lng: -92.100,
            embedding: {emb}
        }})"
    );
    let q = query(&cypher)
        .param("id", outside_id.to_string())
        .param("now", now);
    client.inner().run(q).await.expect("create outside event");

    // Bounding box around downtown Minneapolis
    let q = query(
        "MATCH (n:Event)
         WHERE n.lat > 44.9 AND n.lat < 45.0
           AND n.lng > -93.3 AND n.lng < -93.2
         RETURN n.title AS title"
    );

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let mut found = Vec::new();
    while let Some(row) = stream.next().await.expect("stream error") {
        let title: String = row.get("title").expect("no title");
        found.push(title);
    }

    assert_eq!(found.len(), 1, "should find 1 in-range event, got {}", found.len());
    assert_eq!(found[0], "Inside event");
}

// ---------------------------------------------------------------------------
// Test 6: Source diversity ranking
// ---------------------------------------------------------------------------

#[tokio::test]
async fn source_diversity_ranking() {
    let (_container, client) = setup().await;

    let now = neo4j_dt(&Utc::now());
    let emb = dummy_embedding();

    // Create signals with different source_diversity values
    for (diversity, title) in [(5, "Multi-source signal"), (1, "Single-source signal"), (3, "Mid-source signal")] {
        let id = Uuid::new_v4();
        let cypher = format!(
            "CREATE (n:Event {{
                id: $id, title: $title, summary: 'test',
                sensitivity: 'general', confidence: 0.8, freshness_score: 0.8,
                corroboration_count: 0, source_diversity: $diversity, external_ratio: 0.0,
                cause_heat: 0.0, source_url: 'https://test.com',
                extracted_at: datetime($now), last_confirmed_active: datetime($now),
                location_name: '', starts_at: null, ends_at: null,
                action_url: '', organizer: '', is_recurring: false,
                lat: 44.9778, lng: -93.2650,
                embedding: {emb}
            }})"
        );
        let q = query(&cypher)
            .param("id", id.to_string())
            .param("title", title)
            .param("diversity", diversity as i64)
            .param("now", now.clone());
        client.inner().run(q).await.expect("create signal");
    }

    let q = query(
        "MATCH (n:Event)
         RETURN n.title AS title, n.source_diversity AS diversity
         ORDER BY n.source_diversity DESC"
    );

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let mut diversities = Vec::new();
    while let Some(row) = stream.next().await.expect("stream error") {
        let d: i64 = row.get("diversity").expect("no diversity");
        diversities.push(d);
    }

    assert_eq!(diversities, vec![5, 3, 1], "should be sorted DESC by source_diversity");
}

// ---------------------------------------------------------------------------
// Test 7: Actor linked via ACTED_IN
// ---------------------------------------------------------------------------

#[tokio::test]
async fn actor_linked_via_acted_in() {
    let (_container, client) = setup().await;

    let signal_id = Uuid::new_v4();
    create_signal(&client, "Event", signal_id, "Community cleanup", "https://cleanup.org").await;

    let actor_id = Uuid::new_v4();
    create_actor_and_link(&client, actor_id, "Neighborhood Council", signal_id, "Event", "organizer").await;

    let q = query(
        "MATCH (a:Actor)-[r:ACTED_IN]->(n:Event {id: $signal_id})
         RETURN a.name AS name, r.role AS role"
    )
    .param("signal_id", signal_id.to_string());

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream error").expect("no row");

    let name: String = row.get("name").expect("no name");
    let role: String = row.get("role").expect("no role");

    assert_eq!(name, "Neighborhood Council");
    assert_eq!(role, "organizer");
}

// ---------------------------------------------------------------------------
// Test 8: Cross-type topic search (Ask + Give about same topic)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cross_type_topic_search() {
    let (_container, client) = setup().await;

    let ask_id = Uuid::new_v4();
    let give_id = Uuid::new_v4();
    let unrelated_id = Uuid::new_v4();
    create_signal(&client, "Ask", ask_id, "Winter coats needed for families", "https://ask.org").await;
    create_signal(&client, "Give", give_id, "Free winter coats available", "https://give.org").await;
    create_signal(&client, "Give", unrelated_id, "Free tutoring available", "https://tutor.org").await;

    let q = query(
        "MATCH (n)
         WHERE (n:Ask OR n:Give) AND toLower(n.title) CONTAINS 'coat'
         RETURN labels(n)[0] AS type, n.title AS title"
    );

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let mut results: Vec<(String, String)> = Vec::new();
    while let Some(row) = stream.next().await.expect("stream error") {
        let label: String = row.get("type").expect("no type");
        let title: String = row.get("title").expect("no title");
        results.push((label, title));
    }

    assert_eq!(results.len(), 2, "should find both Ask and Give about coats, got {}", results.len());

    let types: Vec<&str> = results.iter().map(|(t, _)| t.as_str()).collect();
    assert!(types.contains(&"Ask"), "should include Ask");
    assert!(types.contains(&"Give"), "should include Give");
}

// ---------------------------------------------------------------------------
// Test 9: Same-source evidence is idempotent (MERGE, not CREATE)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn same_source_no_duplicate_evidence() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    // Create a signal
    let signal_id = Uuid::new_v4();
    create_signal(&client, "Event", signal_id, "Cleanup day", "https://source-a.org").await;

    // Create evidence from URL A — first time
    let ev1 = EvidenceNode {
        id: Uuid::new_v4(),
        source_url: "https://source-a.org".to_string(),
        retrieved_at: Utc::now(),
        content_hash: "hash_v1".to_string(),
        snippet: Some("First scrape".to_string()),
        relevance: None,
        evidence_confidence: None,
    };
    writer.create_evidence(&ev1, signal_id).await.expect("create evidence 1");

    // Create evidence from URL A again — simulates re-scrape with changed content
    let ev2 = EvidenceNode {
        id: Uuid::new_v4(),
        source_url: "https://source-a.org".to_string(),
        retrieved_at: Utc::now(),
        content_hash: "hash_v2".to_string(),
        snippet: Some("Second scrape".to_string()),
        relevance: None,
        evidence_confidence: None,
    };
    writer.create_evidence(&ev2, signal_id).await.expect("create evidence 2");

    // Call it a third time for good measure
    let ev3 = EvidenceNode {
        id: Uuid::new_v4(),
        source_url: "https://source-a.org".to_string(),
        retrieved_at: Utc::now(),
        content_hash: "hash_v3".to_string(),
        snippet: Some("Third scrape".to_string()),
        relevance: None,
        evidence_confidence: None,
    };
    writer.create_evidence(&ev3, signal_id).await.expect("create evidence 3");

    // Should have exactly 1 evidence node, not 3
    let q = query(
        "MATCH (n:Event {id: $id})-[:SOURCED_FROM]->(ev:Evidence)
         RETURN count(ev) AS cnt, ev.content_hash AS hash"
    )
    .param("id", signal_id.to_string());

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream error").expect("no row");

    let cnt: i64 = row.get("cnt").expect("no cnt");
    let hash: String = row.get("hash").expect("no hash");

    assert_eq!(cnt, 1, "should have exactly 1 evidence node from same source, got {cnt}");
    assert_eq!(hash, "hash_v3", "content_hash should be updated to latest scrape");
}

// ---------------------------------------------------------------------------
// Test 10: Cross-source evidence creates separate nodes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cross_source_creates_new_evidence() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let signal_id = Uuid::new_v4();
    create_signal(&client, "Event", signal_id, "Community meeting", "https://source-a.org").await;

    // Evidence from URL A
    let ev_a = EvidenceNode {
        id: Uuid::new_v4(),
        source_url: "https://source-a.org".to_string(),
        retrieved_at: Utc::now(),
        content_hash: "hash_a".to_string(),
        snippet: Some("Source A".to_string()),
        relevance: None,
        evidence_confidence: None,
    };
    writer.create_evidence(&ev_a, signal_id).await.expect("create evidence A");

    // Evidence from URL B — different source, should create a new evidence node
    let ev_b = EvidenceNode {
        id: Uuid::new_v4(),
        source_url: "https://source-b.org".to_string(),
        retrieved_at: Utc::now(),
        content_hash: "hash_b".to_string(),
        snippet: Some("Source B".to_string()),
        relevance: None,
        evidence_confidence: None,
    };
    writer.create_evidence(&ev_b, signal_id).await.expect("create evidence B");

    // Evidence from URL C — third independent source
    let ev_c = EvidenceNode {
        id: Uuid::new_v4(),
        source_url: "https://source-c.org".to_string(),
        retrieved_at: Utc::now(),
        content_hash: "hash_c".to_string(),
        snippet: Some("Source C".to_string()),
        relevance: None,
        evidence_confidence: None,
    };
    writer.create_evidence(&ev_c, signal_id).await.expect("create evidence C");

    // Should have 3 evidence nodes (one per source)
    let q = query(
        "MATCH (n:Event {id: $id})-[:SOURCED_FROM]->(ev:Evidence)
         RETURN count(ev) AS cnt"
    )
    .param("id", signal_id.to_string());

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream error").expect("no row");

    let cnt: i64 = row.get("cnt").expect("no cnt");
    assert_eq!(cnt, 3, "should have 3 evidence nodes from 3 different sources, got {cnt}");
}

// ---------------------------------------------------------------------------
// Test 11: Same-source refresh does not inflate corroboration_count
// ---------------------------------------------------------------------------

#[tokio::test]
async fn same_source_does_not_inflate_corroboration() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let signal_id = Uuid::new_v4();
    create_signal(&client, "Event", signal_id, "Annual parade", "https://parade.org").await;

    // Initial evidence
    let ev = EvidenceNode {
        id: Uuid::new_v4(),
        source_url: "https://parade.org".to_string(),
        retrieved_at: Utc::now(),
        content_hash: "hash_v1".to_string(),
        snippet: None,
        relevance: None,
        evidence_confidence: None,
    };
    writer.create_evidence(&ev, signal_id).await.expect("create evidence");

    // Simulate 5 same-source re-scrapes: refresh_signal (not corroborate) + create_evidence (MERGE)
    for i in 0..5 {
        writer
            .refresh_signal(signal_id, rootsignal_common::NodeType::Event, Utc::now())
            .await
            .expect("refresh failed");

        let ev = EvidenceNode {
            id: Uuid::new_v4(),
            source_url: "https://parade.org".to_string(),
            retrieved_at: Utc::now(),
            content_hash: format!("hash_v{}", i + 2),
            snippet: None,
            relevance: None,
            evidence_confidence: None,
        };
        writer.create_evidence(&ev, signal_id).await.expect("create evidence");
    }

    // corroboration_count should still be 0 (initial value, never incremented)
    let q = query(
        "MATCH (n:Event {id: $id})
         RETURN n.corroboration_count AS corr"
    )
    .param("id", signal_id.to_string());

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream error").expect("no row");
    let corr: i64 = row.get("corr").expect("no corr");
    assert_eq!(corr, 0, "corroboration_count should stay 0 after same-source refreshes, got {corr}");

    // Should still have exactly 1 evidence node
    let q = query(
        "MATCH (n:Event {id: $id})-[:SOURCED_FROM]->(ev:Evidence)
         RETURN count(ev) AS cnt"
    )
    .param("id", signal_id.to_string());

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream error").expect("no row");
    let cnt: i64 = row.get("cnt").expect("no cnt");
    assert_eq!(cnt, 1, "should have exactly 1 evidence node after 5 same-source refreshes, got {cnt}");

    // Now simulate a REAL cross-source corroboration
    let entity_mappings = vec![];
    writer
        .corroborate(signal_id, rootsignal_common::NodeType::Event, Utc::now(), &entity_mappings)
        .await
        .expect("corroborate failed");

    let ev_cross = EvidenceNode {
        id: Uuid::new_v4(),
        source_url: "https://independent-news.org".to_string(),
        retrieved_at: Utc::now(),
        content_hash: "cross_hash".to_string(),
        snippet: None,
        relevance: None,
        evidence_confidence: None,
    };
    writer.create_evidence(&ev_cross, signal_id).await.expect("cross-source evidence");

    // Now corroboration_count should be 1, evidence count should be 2
    let q = query(
        "MATCH (n:Event {id: $id})
         OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
         RETURN n.corroboration_count AS corr, count(ev) AS ev_cnt"
    )
    .param("id", signal_id.to_string());

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream error").expect("no row");
    let corr: i64 = row.get("corr").expect("no corr");
    let ev_cnt: i64 = row.get("ev_cnt").expect("no ev_cnt");
    assert_eq!(corr, 1, "corroboration_count should be 1 after one real cross-source, got {corr}");
    assert_eq!(ev_cnt, 2, "should have 2 evidence nodes (1 same-source + 1 cross-source), got {ev_cnt}");
}

// ---------------------------------------------------------------------------
// Test 12: deduplicate_evidence migration cleans up legacy duplicate evidence
// ---------------------------------------------------------------------------

#[tokio::test]
async fn deduplicate_evidence_migration() {
    let (_container, client) = setup().await;

    // Create a signal with corroboration_count already inflated
    let signal_id = Uuid::new_v4();
    let now = neo4j_dt(&Utc::now());
    let emb = dummy_embedding();
    let cypher = format!(
        "CREATE (n:Event {{
            id: $id, title: 'Inflated signal', summary: 'test',
            sensitivity: 'general', confidence: 0.8, freshness_score: 0.8,
            corroboration_count: 13, source_diversity: 1, external_ratio: 0.0,
            cause_heat: 0.0, source_url: 'https://source-a.org',
            extracted_at: datetime($now), last_confirmed_active: datetime($now),
            location_name: '', starts_at: null, ends_at: null,
            action_url: '', organizer: '', is_recurring: false,
            lat: 44.9778, lng: -93.2650,
            embedding: {emb}
        }})"
    );
    let q = query(&cypher)
        .param("id", signal_id.to_string())
        .param("now", now.clone());
    client.inner().run(q).await.expect("create signal");

    // Simulate the bug: manually CREATE 14 evidence nodes from the same source URL
    // (bypassing the MERGE safety net to reproduce legacy data)
    for i in 0..14 {
        let ev_id = Uuid::new_v4();
        let q = query(
            "MATCH (n:Event {id: $signal_id})
             CREATE (ev:Evidence {
                 id: $ev_id,
                 source_url: 'https://source-a.org',
                 retrieved_at: datetime($now),
                 content_hash: $hash,
                 snippet: '',
                 relevance: '',
                 evidence_confidence: 0.0
             })
             CREATE (n)-[:SOURCED_FROM]->(ev)"
        )
        .param("signal_id", signal_id.to_string())
        .param("ev_id", ev_id.to_string())
        .param("now", now.clone())
        .param("hash", format!("hash_{i}"));
        client.inner().run(q).await.expect("create duplicate evidence");
    }

    // Also add 1 legitimate cross-source evidence
    let cross_ev_id = Uuid::new_v4();
    let q = query(
        "MATCH (n:Event {id: $signal_id})
         CREATE (ev:Evidence {
             id: $ev_id,
             source_url: 'https://independent.org',
             retrieved_at: datetime($now),
             content_hash: 'cross_hash',
             snippet: '',
             relevance: '',
             evidence_confidence: 0.0
         })
         CREATE (n)-[:SOURCED_FROM]->(ev)"
    )
    .param("signal_id", signal_id.to_string())
    .param("ev_id", cross_ev_id.to_string())
    .param("now", now);
    client.inner().run(q).await.expect("create cross-source evidence");

    // Verify the mess: 15 evidence nodes, corroboration_count = 13
    let q = query(
        "MATCH (n:Event {id: $id})-[:SOURCED_FROM]->(ev:Evidence)
         RETURN n.corroboration_count AS corr, count(ev) AS ev_cnt"
    )
    .param("id", signal_id.to_string());
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream error").expect("no row");
    let ev_cnt: i64 = row.get("ev_cnt").expect("no ev_cnt");
    assert_eq!(ev_cnt, 15, "pre-migration: should have 15 evidence nodes, got {ev_cnt}");

    // Run the dedup migration
    rootsignal_graph::migrate::deduplicate_evidence(&client)
        .await
        .expect("deduplicate_evidence failed");

    // After migration: should have 2 evidence nodes (1 per unique source_url)
    // and corroboration_count = 1 (2 evidence - 1 = 1 real corroboration)
    let q = query(
        "MATCH (n:Event {id: $id})
         OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
         RETURN n.corroboration_count AS corr, count(ev) AS ev_cnt"
    )
    .param("id", signal_id.to_string());
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream error").expect("no row");

    let corr: i64 = row.get("corr").expect("no corr");
    let ev_cnt: i64 = row.get("ev_cnt").expect("no ev_cnt");
    assert_eq!(ev_cnt, 2, "post-migration: should have 2 evidence nodes (1 per source), got {ev_cnt}");
    assert_eq!(corr, 1, "post-migration: corroboration_count should be 1 (2 sources - 1), got {corr}");

    // Verify the distinct source URLs are correct
    let q = query(
        "MATCH (n:Event {id: $id})-[:SOURCED_FROM]->(ev:Evidence)
         RETURN ev.source_url AS url ORDER BY url"
    )
    .param("id", signal_id.to_string());
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let mut urls = Vec::new();
    while let Some(row) = stream.next().await.expect("stream error") {
        let url: String = row.get("url").expect("no url");
        urls.push(url);
    }
    assert_eq!(urls, vec!["https://independent.org", "https://source-a.org"]);
}

// ---------------------------------------------------------------------------
// Test: City proximity signal lookup (list_recent_for_city)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn city_proximity_signal_lookup() {
    let (_container, client) = setup().await;

    let now = neo4j_dt(&Utc::now());
    let emb = dummy_embedding();

    // Minneapolis center: 44.9778, -93.2650
    // Create signal in downtown Minneapolis (inside ~50km radius)
    let mpls_id = Uuid::new_v4();
    let cypher = format!(
        "CREATE (n:Give {{
            id: $id, title: 'Minneapolis food shelf', summary: 'Free meals',
            sensitivity: 'general', confidence: 0.8, freshness_score: 0.8,
            corroboration_count: 0, source_diversity: 1, external_ratio: 0.0,
            cause_heat: 0.0, source_url: 'https://test.com/mpls',
            extracted_at: datetime($now), last_confirmed_active: datetime($now),
            location_name: 'North Minneapolis', action_url: '', availability: '',
            is_ongoing: true,
            lat: 44.975, lng: -93.265,
            embedding: {emb}
        }})"
    );
    client.inner().run(
        query(&cypher).param("id", mpls_id.to_string()).param("now", now.clone())
    ).await.expect("create mpls signal");

    // Create signal in St. Paul (inside ~50km radius of Minneapolis center)
    let stp_id = Uuid::new_v4();
    let cypher = format!(
        "CREATE (n:Event {{
            id: $id, title: 'St Paul community event', summary: 'Block party',
            sensitivity: 'general', confidence: 0.8, freshness_score: 0.8,
            corroboration_count: 0, source_diversity: 1, external_ratio: 0.0,
            cause_heat: 0.0, source_url: 'https://test.com/stp',
            extracted_at: datetime($now), last_confirmed_active: datetime($now),
            location_name: 'St Paul', starts_at: null, ends_at: null,
            action_url: '', organizer: '', is_recurring: false,
            lat: 44.9537, lng: -93.0900,
            embedding: {emb}
        }})"
    );
    client.inner().run(
        query(&cypher).param("id", stp_id.to_string()).param("now", now.clone())
    ).await.expect("create stp signal");

    // Create signal in Duluth (outside ~50km radius — ~250km away)
    let duluth_id = Uuid::new_v4();
    let cypher = format!(
        "CREATE (n:Event {{
            id: $id, title: 'Duluth harbor event', summary: 'Far away',
            sensitivity: 'general', confidence: 0.8, freshness_score: 0.8,
            corroboration_count: 0, source_diversity: 1, external_ratio: 0.0,
            cause_heat: 0.0, source_url: 'https://test.com/duluth',
            extracted_at: datetime($now), last_confirmed_active: datetime($now),
            location_name: 'Duluth', starts_at: null, ends_at: null,
            action_url: '', organizer: '', is_recurring: false,
            lat: 46.786, lng: -92.100,
            embedding: {emb}
        }})"
    );
    client.inner().run(
        query(&cypher).param("id", duluth_id.to_string()).param("now", now.clone())
    ).await.expect("create duluth signal");

    // Create signal at (0,0) — should be excluded
    let zero_id = Uuid::new_v4();
    let cypher = format!(
        "CREATE (n:Ask {{
            id: $id, title: 'Zero-coordinate signal', summary: 'Bad data',
            sensitivity: 'general', confidence: 0.8, freshness_score: 0.8,
            corroboration_count: 0, source_diversity: 1, external_ratio: 0.0,
            cause_heat: 0.0, source_url: 'https://test.com/zero',
            extracted_at: datetime($now), last_confirmed_active: datetime($now),
            location_name: '', urgency: 'medium', what_needed: '', action_url: '', goal: '',
            lat: 0.0, lng: 0.0,
            embedding: {emb}
        }})"
    );
    client.inner().run(
        query(&cypher).param("id", zero_id.to_string()).param("now", now)
    ).await.expect("create zero signal");

    // Query via PublicGraphReader with Minneapolis center + 50km radius
    let reader = rootsignal_graph::PublicGraphReader::new(client.clone());
    let results = reader
        .list_recent_for_city(44.9778, -93.2650, 50.0, 100)
        .await
        .expect("list_recent_for_city failed");

    let titles: Vec<&str> = results.iter().filter_map(|n| n.meta().map(|m| m.title.as_str())).collect();

    // Should include Minneapolis and St. Paul signals
    assert!(titles.contains(&"Minneapolis food shelf"), "Missing Minneapolis signal; got: {:?}", titles);
    assert!(titles.contains(&"St Paul community event"), "Missing St Paul signal; got: {:?}", titles);

    // Should NOT include Duluth or zero-coordinate signal
    assert!(!titles.contains(&"Duluth harbor event"), "Duluth signal should be outside radius; got: {:?}", titles);
    assert!(!titles.contains(&"Zero-coordinate signal"), "Zero-coordinate signal should be excluded; got: {:?}", titles);
}

// ---------------------------------------------------------------------------
// Test: City proximity story lookup (top_stories_for_city)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn city_proximity_story_lookup() {
    let (_container, client) = setup().await;

    let now = neo4j_dt(&Utc::now());

    // Story with centroid in Minneapolis
    let mpls_story_id = Uuid::new_v4();
    let q = query(
        "CREATE (s:Story {
            id: $id, headline: 'Minneapolis housing crisis', summary: 'Rents rising',
            signal_count: 3, first_seen: datetime($now), last_updated: datetime($now),
            velocity: 0.5, energy: 8.0,
            centroid_lat: 44.975, centroid_lng: -93.265,
            dominant_type: 'Tension', sensitivity: 'general',
            source_count: 2, entity_count: 1, type_diversity: 2,
            source_domains: ['reddit.com'], corroboration_depth: 1,
            status: 'active'
        })"
    )
    .param("id", mpls_story_id.to_string())
    .param("now", now.clone());
    client.inner().run(q).await.expect("create mpls story");

    // Story with centroid in Duluth
    let duluth_story_id = Uuid::new_v4();
    let q = query(
        "CREATE (s:Story {
            id: $id, headline: 'Duluth port expansion', summary: 'New docks',
            signal_count: 1, first_seen: datetime($now), last_updated: datetime($now),
            velocity: 0.1, energy: 3.0,
            centroid_lat: 46.786, centroid_lng: -92.100,
            dominant_type: 'Notice', sensitivity: 'general',
            source_count: 1, entity_count: 1, type_diversity: 1,
            source_domains: ['duluthnewstribune.com'], corroboration_depth: 0,
            status: 'emerging'
        })"
    )
    .param("id", duluth_story_id.to_string())
    .param("now", now.clone());
    client.inner().run(q).await.expect("create duluth story");

    // Story with no centroid
    let no_geo_story_id = Uuid::new_v4();
    let q = query(
        "CREATE (s:Story {
            id: $id, headline: 'Online-only discussion', summary: 'No location',
            signal_count: 2, first_seen: datetime($now), last_updated: datetime($now),
            velocity: 0.2, energy: 5.0,
            dominant_type: 'Ask', sensitivity: 'general',
            source_count: 1, entity_count: 0, type_diversity: 1,
            source_domains: ['reddit.com'], corroboration_depth: 0,
            status: 'emerging'
        })"
    )
    .param("id", no_geo_story_id.to_string())
    .param("now", now);
    client.inner().run(q).await.expect("create no-geo story");

    // Query stories near Minneapolis center, 50km radius
    let reader = rootsignal_graph::PublicGraphReader::new(client.clone());
    let results = reader
        .top_stories_for_city(44.9778, -93.2650, 50.0, 100)
        .await
        .expect("top_stories_for_city failed");

    let headlines: Vec<&str> = results.iter().map(|s| s.headline.as_str()).collect();

    assert!(headlines.contains(&"Minneapolis housing crisis"), "Missing Minneapolis story; got: {:?}", headlines);
    assert!(!headlines.contains(&"Duluth port expansion"), "Duluth story should be outside radius; got: {:?}", headlines);
    assert!(!headlines.contains(&"Online-only discussion"), "No-centroid story should be excluded; got: {:?}", headlines);
}

// ---------------------------------------------------------------------------
// Test: Source last_scraped survives datetime() round-trip
// ---------------------------------------------------------------------------

#[tokio::test]
async fn source_last_scraped_round_trip() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let source = SourceNode {
        id: Uuid::new_v4(),
        canonical_key: "test-city:web:https://example.org".to_string(),
        canonical_value: "https://example.org".to_string(),
        url: Some("https://example.org".to_string()),
        source_type: SourceType::Web,
        discovery_method: DiscoveryMethod::Curated,
        city: "test-city".to_string(),
        created_at: Utc::now(),
        last_scraped: None,
        last_produced_signal: None,
        signals_produced: 0,
        signals_corroborated: 0,
        consecutive_empty_runs: 0,
        active: true,
        gap_context: None,
        weight: 0.5,
        cadence_hours: None,
        avg_signals_per_scrape: 0.0,
        quality_penalty: 1.0,
        source_role: SourceRole::Mixed,
        scrape_count: 0,
    };

    writer.upsert_source(&source).await.expect("upsert_source failed");

    // Record a scrape (stores last_scraped via Cypher datetime())
    let scrape_time = Utc::now();
    writer
        .record_source_scrape(&source.canonical_key, 3, scrape_time)
        .await
        .expect("record_source_scrape failed");

    // Read back via get_active_sources — the bug caused last_scraped to be None
    // because row.get::<String>() silently failed on Neo4j DateTime types
    let sources = writer.get_active_sources("test-city").await.expect("get_active_sources failed");
    assert_eq!(sources.len(), 1, "should find 1 source");

    let s = &sources[0];
    assert!(s.last_scraped.is_some(), "last_scraped should be Some after record_source_scrape, got None");
    assert!(s.last_produced_signal.is_some(), "last_produced_signal should be Some when signals > 0");
    assert_eq!(s.signals_produced, 3);
    assert_eq!(s.scrape_count, 1);
    assert_eq!(s.consecutive_empty_runs, 0);

    // Also verify created_at survived the round-trip (not just defaulting to now)
    let age = Utc::now() - s.created_at;
    assert!(age.num_seconds() < 60, "created_at should be recent, not a fallback value");
}

/// Helper: create a Tension with a specific embedding vector.
async fn create_tension_with_embedding(
    client: &GraphClient,
    id: Uuid,
    title: &str,
    embedding: &[f64],
) {
    let now = neo4j_dt(&Utc::now());
    let emb_str = format!(
        "[{}]",
        embedding.iter().map(|v| format!("{v}")).collect::<Vec<_>>().join(",")
    );
    let cypher = format!(
        "CREATE (t:Tension {{
            id: $id,
            title: $title,
            summary: $summary,
            sensitivity: 'general',
            confidence: 0.7,
            freshness_score: 1.0,
            corroboration_count: 0,
            source_diversity: 1,
            external_ratio: 1.0,
            cause_heat: 0.0,
            source_url: 'https://example.com',
            extracted_at: datetime($now),
            last_confirmed_active: datetime($now),
            location_name: 'Minneapolis',
            severity: 'high',
            category: 'safety',
            what_would_help: 'more resources',
            lat: 44.9778,
            lng: -93.2650,
            embedding: {emb_str}
        }})"
    );

    let q = query(&cypher)
        .param("id", id.to_string())
        .param("title", title)
        .param("summary", format!("Test tension: {title}"))
        .param("now", now);

    client.inner().run(q).await.expect("Failed to create tension");
}

#[tokio::test]
async fn merge_duplicate_tensions_collapses_near_dupes() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    // Create 3 near-identical youth violence tensions (would be >0.85 cosine)
    let mut base_emb = vec![0.0f64; 1024];
    base_emb[0] = 1.0;
    base_emb[1] = 0.5;

    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let id3 = Uuid::new_v4();

    let mut emb2 = base_emb.clone();
    emb2[2] = 0.05; // tiny perturbation
    let mut emb3 = base_emb.clone();
    emb3[3] = 0.05; // tiny perturbation

    create_tension_with_embedding(&client, id1, "Youth Violence in North Minneapolis", &base_emb).await;
    create_tension_with_embedding(&client, id2, "Youth Violence Spike in North Minneapolis", &emb2).await;
    create_tension_with_embedding(&client, id3, "Youth Violence and Lack of Safe Spaces", &emb3).await;

    // Create one unrelated tension
    let id_unrelated = Uuid::new_v4();
    let mut unrelated_emb = vec![0.0f64; 1024];
    unrelated_emb[500] = 1.0; // completely different
    create_tension_with_embedding(&client, id_unrelated, "Housing Affordability Crisis", &unrelated_emb).await;

    // Create a signal that RESPONDS_TO one of the duplicates
    let signal_id = Uuid::new_v4();
    create_signal(&client, "Give", signal_id, "NAZ Tutoring", "https://example.com/naz").await;
    let q = query(
        "MATCH (g:Give {id: $gid}), (t:Tension {id: $tid})
         CREATE (g)-[:RESPONDS_TO {match_strength: 0.9, explanation: 'test'}]->(t)"
    )
    .param("gid", signal_id.to_string())
    .param("tid", id2.to_string());
    client.inner().run(q).await.expect("Failed to create edge");

    // Verify: 4 tensions before merge
    let q = query("MATCH (t:Tension) RETURN count(t) AS cnt");
    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().unwrap();
    let count: i64 = row.get("cnt").unwrap();
    assert_eq!(count, 4, "Should have 4 tensions before merge");

    // Run merge
    let merged = writer.merge_duplicate_tensions(0.85).await.expect("merge failed");
    assert_eq!(merged, 2, "Should merge 2 duplicates (keep 1 of 3)");

    // Verify: 2 tensions after merge (1 youth violence survivor + 1 housing)
    let q = query("MATCH (t:Tension) RETURN count(t) AS cnt");
    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().unwrap();
    let count: i64 = row.get("cnt").unwrap();
    assert_eq!(count, 2, "Should have 2 tensions after merge");

    // Verify: the RESPONDS_TO edge was re-pointed to the survivor (id1, the oldest)
    let q = query(
        "MATCH (g:Give {id: $gid})-[:RESPONDS_TO]->(t:Tension)
         RETURN t.id AS tid"
    )
    .param("gid", signal_id.to_string());
    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().expect("Should have RESPONDS_TO edge");
    let tid: String = row.get("tid").unwrap();
    assert_eq!(tid, id1.to_string(), "Edge should point to survivor (oldest tension)");

    // Verify: survivor got corroboration bumped
    let q = query("MATCH (t:Tension {id: $id}) RETURN t.corroboration_count AS cnt")
        .param("id", id1.to_string());
    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().unwrap();
    let corr: i64 = row.get("cnt").unwrap();
    assert_eq!(corr, 2, "Survivor should have corroboration_count = 2 (absorbed 2 dupes)");

    // Verify: unrelated tension untouched
    let q = query("MATCH (t:Tension {id: $id}) RETURN t.title AS title")
        .param("id", id_unrelated.to_string());
    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().expect("Unrelated tension should survive");
    let title: String = row.get("title").unwrap();
    assert_eq!(title, "Housing Affordability Crisis");
}

#[tokio::test]
async fn merge_duplicate_tensions_noop_when_no_dupes() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    // Create two very different tensions
    let mut emb1 = vec![0.0f64; 1024];
    emb1[0] = 1.0;
    let mut emb2 = vec![0.0f64; 1024];
    emb2[500] = 1.0;

    create_tension_with_embedding(&client, Uuid::new_v4(), "Youth Violence", &emb1).await;
    create_tension_with_embedding(&client, Uuid::new_v4(), "Housing Crisis", &emb2).await;

    let merged = writer.merge_duplicate_tensions(0.85).await.expect("merge failed");
    assert_eq!(merged, 0, "No duplicates should be merged");

    let q = query("MATCH (t:Tension) RETURN count(t) AS cnt");
    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().unwrap();
    let count: i64 = row.get("cnt").unwrap();
    assert_eq!(count, 2, "Both tensions should survive");
}

// =============================================================================
// Response Scout writer method tests
// =============================================================================

/// Helper: create a tension with specific confidence and optional response_scouted_at.
async fn create_tension_for_response_scout(
    client: &GraphClient,
    id: Uuid,
    title: &str,
    confidence: f64,
    response_scouted_at: Option<&str>,
) {
    let now = neo4j_dt(&Utc::now());
    let emb = dummy_embedding();
    let scouted_clause = match response_scouted_at {
        Some(dt) => format!(", response_scouted_at: datetime('{dt}')"),
        None => String::new(),
    };
    let cypher = format!(
        "CREATE (t:Tension {{
            id: $id,
            title: $title,
            summary: $summary,
            sensitivity: 'general',
            confidence: $confidence,
            freshness_score: 1.0,
            corroboration_count: 0,
            source_diversity: 1,
            external_ratio: 1.0,
            cause_heat: $cause_heat,
            source_url: 'https://example.com',
            extracted_at: datetime($now),
            last_confirmed_active: datetime($now),
            location_name: 'Minneapolis',
            severity: 'high',
            category: 'safety',
            what_would_help: 'more resources',
            lat: 44.9778,
            lng: -93.2650,
            embedding: {emb}
            {scouted_clause}
        }})"
    );

    let q = query(&cypher)
        .param("id", id.to_string())
        .param("title", title)
        .param("summary", format!("Test tension: {title}"))
        .param("now", now)
        .param("confidence", confidence)
        .param("cause_heat", 0.5);

    client.inner().run(q).await.expect("Failed to create tension");
}

#[tokio::test]
async fn response_scout_targets_finds_unscouted_tensions() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let t1 = Uuid::new_v4();
    let t2 = Uuid::new_v4();
    let t3 = Uuid::new_v4();

    // t1: never scouted, high confidence — should be found
    create_tension_for_response_scout(&client, t1, "ICE Enforcement Fear", 0.7, None).await;
    // t2: scouted recently — should NOT be found
    create_tension_for_response_scout(
        &client, t2, "Housing Crisis", 0.8,
        Some("2026-02-17T00:00:00"),
    ).await;
    // t3: low confidence (below 0.5) — should NOT be found
    create_tension_for_response_scout(&client, t3, "Emergent Tension", 0.3, None).await;

    let targets = writer.find_response_scout_targets(10).await.expect("query failed");

    assert_eq!(targets.len(), 1, "Only 1 target should qualify");
    assert_eq!(targets[0].tension_id, t1);
    assert_eq!(targets[0].title, "ICE Enforcement Fear");
    assert_eq!(targets[0].response_count, 0);
}

#[tokio::test]
async fn response_scout_targets_includes_stale_scouted_tensions() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let t1 = Uuid::new_v4();

    // Scouted 30 days ago — should be found (>14 day threshold)
    create_tension_for_response_scout(
        &client, t1, "Old Tension", 0.7,
        Some("2026-01-15T00:00:00"),
    ).await;

    let targets = writer.find_response_scout_targets(10).await.expect("query failed");
    assert_eq!(targets.len(), 1, "Stale-scouted tension should be re-eligible");
    assert_eq!(targets[0].tension_id, t1);
}

#[tokio::test]
async fn response_scout_targets_sorted_by_response_count_then_heat() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let t1 = Uuid::new_v4();
    let t2 = Uuid::new_v4();

    create_tension_for_response_scout(&client, t1, "Well-served", 0.7, None).await;
    create_tension_for_response_scout(&client, t2, "Neglected", 0.7, None).await;

    // Give t1 a response edge, t2 has none
    let give_id = Uuid::new_v4();
    create_signal(&client, "Give", give_id, "Food Shelf", "https://example.com/food").await;
    let edge_q = query(
        "MATCH (g:Give {id: $gid}), (t:Tension {id: $tid})
         CREATE (g)-[:RESPONDS_TO {match_strength: 0.8, explanation: 'test'}]->(t)",
    )
    .param("gid", give_id.to_string())
    .param("tid", t1.to_string());
    client.inner().run(edge_q).await.expect("edge creation failed");

    let targets = writer.find_response_scout_targets(10).await.expect("query failed");
    assert_eq!(targets.len(), 2);
    // t2 (0 responses) should come first
    assert_eq!(targets[0].tension_id, t2, "Neglected tension should sort first");
    assert_eq!(targets[0].response_count, 0);
    assert_eq!(targets[1].tension_id, t1);
    assert_eq!(targets[1].response_count, 1);
}

#[tokio::test]
async fn get_existing_responses_returns_heuristics() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let tension_id = Uuid::new_v4();
    create_tension_for_response_scout(&client, tension_id, "Housing Crisis", 0.7, None).await;

    let give_id = Uuid::new_v4();
    create_signal(&client, "Give", give_id, "Rent Assistance Program", "https://example.com/rent").await;

    let edge_q = query(
        "MATCH (g:Give {id: $gid}), (t:Tension {id: $tid})
         CREATE (g)-[:RESPONDS_TO {match_strength: 0.9, explanation: 'provides rent help'}]->(t)",
    )
    .param("gid", give_id.to_string())
    .param("tid", tension_id.to_string());
    client.inner().run(edge_q).await.expect("edge creation failed");

    let heuristics = writer.get_existing_responses(tension_id).await.expect("query failed");
    assert_eq!(heuristics.len(), 1);
    assert_eq!(heuristics[0].title, "Rent Assistance Program");
    assert_eq!(heuristics[0].signal_type, "Give");
}

#[tokio::test]
async fn mark_response_scouted_sets_timestamp() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let tension_id = Uuid::new_v4();
    create_tension_for_response_scout(&client, tension_id, "Test Tension", 0.7, None).await;

    // Before marking — should be a target
    let targets = writer.find_response_scout_targets(10).await.expect("query failed");
    assert_eq!(targets.len(), 1);

    // Mark as scouted
    writer.mark_response_scouted(tension_id).await.expect("mark failed");

    // After marking — should NOT be a target (scouted < 14 days ago)
    let targets = writer.find_response_scout_targets(10).await.expect("query failed");
    assert_eq!(targets.len(), 0, "Recently scouted tension should not be a target");
}

#[tokio::test]
async fn create_response_edge_wires_give_to_tension() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let tension_id = Uuid::new_v4();
    create_tension_for_response_scout(&client, tension_id, "Test Tension", 0.7, None).await;

    let give_id = Uuid::new_v4();
    create_signal(&client, "Give", give_id, "Mutual Aid Network", "https://example.com/aid").await;

    writer.create_response_edge(give_id, tension_id, 0.85, "provides mutual aid").await
        .expect("create_response_edge failed");

    // Verify edge exists
    let q = query(
        "MATCH (g:Give {id: $gid})-[rel:RESPONDS_TO]->(t:Tension {id: $tid})
         RETURN rel.match_strength AS strength, rel.explanation AS explanation",
    )
    .param("gid", give_id.to_string())
    .param("tid", tension_id.to_string());

    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().expect("Edge should exist");
    let strength: f64 = row.get("strength").unwrap();
    let explanation: String = row.get("explanation").unwrap();

    assert!((strength - 0.85).abs() < 0.001, "match_strength should be 0.85");
    assert_eq!(explanation, "provides mutual aid");
}

// =============================================================================
// Gravity Scout integration tests
// =============================================================================

/// Helper: create a tension with specific confidence, cause_heat, and optional gravity scouting state.
async fn create_tension_for_gravity_scout(
    client: &GraphClient,
    id: Uuid,
    title: &str,
    confidence: f64,
    cause_heat: f64,
    gravity_scouted_at: Option<&str>,
    gravity_scout_miss_count: Option<i64>,
) {
    let now = neo4j_dt(&Utc::now());
    let emb = dummy_embedding();
    let mut extra_props = String::new();
    if let Some(dt) = gravity_scouted_at {
        extra_props.push_str(&format!(", gravity_scouted_at: datetime('{dt}')"));
    }
    if let Some(mc) = gravity_scout_miss_count {
        extra_props.push_str(&format!(", gravity_scout_miss_count: {mc}"));
    }
    let cypher = format!(
        "CREATE (t:Tension {{
            id: $id,
            title: $title,
            summary: $summary,
            sensitivity: 'general',
            confidence: $confidence,
            freshness_score: 1.0,
            corroboration_count: 0,
            source_diversity: 1,
            external_ratio: 1.0,
            cause_heat: $cause_heat,
            source_url: 'https://example.com',
            extracted_at: datetime($now),
            last_confirmed_active: datetime($now),
            location_name: 'Minneapolis',
            severity: 'high',
            category: 'safety',
            what_would_help: 'community solidarity',
            lat: 44.9778,
            lng: -93.2650,
            embedding: {emb}
            {extra_props}
        }})"
    );

    let q = query(&cypher)
        .param("id", id.to_string())
        .param("title", title)
        .param("summary", format!("Test tension: {title}"))
        .param("now", now)
        .param("confidence", confidence)
        .param("cause_heat", cause_heat);

    client.inner().run(q).await.expect("Failed to create tension for gravity scout");
}

#[tokio::test]
async fn gravity_scout_targets_requires_minimum_heat() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let t1 = Uuid::new_v4();
    let t2 = Uuid::new_v4();

    // t1: has heat — should be found
    create_tension_for_gravity_scout(&client, t1, "ICE Enforcement Fear", 0.7, 0.5, None, None).await;
    // t2: no heat (0.0) — should NOT be found
    create_tension_for_gravity_scout(&client, t2, "Cold Tension", 0.7, 0.0, None, None).await;

    let targets = writer.find_gravity_scout_targets(10).await.expect("query failed");
    assert_eq!(targets.len(), 1, "Only hot tension should qualify");
    assert_eq!(targets[0].tension_id, t1);
}

#[tokio::test]
async fn gravity_scout_targets_sorted_by_heat_desc() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let t1 = Uuid::new_v4();
    let t2 = Uuid::new_v4();

    // t1: moderate heat
    create_tension_for_gravity_scout(&client, t1, "Moderate", 0.7, 0.3, None, None).await;
    // t2: high heat
    create_tension_for_gravity_scout(&client, t2, "Hot", 0.7, 0.9, None, None).await;

    let targets = writer.find_gravity_scout_targets(10).await.expect("query failed");
    assert_eq!(targets.len(), 2);
    // Hottest first
    assert_eq!(targets[0].tension_id, t2, "Hottest tension should sort first");
    assert_eq!(targets[1].tension_id, t1);
}

#[tokio::test]
async fn gravity_scout_respects_scouted_timestamp() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let t1 = Uuid::new_v4();

    // Scouted 3 days ago — should NOT be found (7-day base window)
    create_tension_for_gravity_scout(
        &client, t1, "Recent", 0.7, 0.5,
        Some("2026-02-15T00:00:00"), Some(0),
    ).await;

    let targets = writer.find_gravity_scout_targets(10).await.expect("query failed");
    assert_eq!(targets.len(), 0, "Recently scouted tension should not be a target");
}

#[tokio::test]
async fn gravity_scout_backoff_on_consecutive_misses() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let t1 = Uuid::new_v4();

    // Scouted 15 days ago with miss_count=2 — needs 21 days, so should NOT be found
    create_tension_for_gravity_scout(
        &client, t1, "Two misses", 0.7, 0.5,
        Some("2026-02-03T00:00:00"), Some(2),
    ).await;

    let targets = writer.find_gravity_scout_targets(10).await.expect("query failed");
    assert_eq!(targets.len(), 0, "miss_count=2 requires 21-day window, only 15 days elapsed");

    // Now try with miss_count=1 — needs 14 days, 15 days elapsed, should be found
    let t2 = Uuid::new_v4();
    create_tension_for_gravity_scout(
        &client, t2, "One miss", 0.7, 0.5,
        Some("2026-02-03T00:00:00"), Some(1),
    ).await;

    let targets = writer.find_gravity_scout_targets(10).await.expect("query failed");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].tension_id, t2);
}

#[tokio::test]
async fn gravity_scout_backoff_resets_on_success() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let t1 = Uuid::new_v4();

    // Start with miss_count=3
    create_tension_for_gravity_scout(
        &client, t1, "Was cold", 0.7, 0.5,
        Some("2026-01-01T00:00:00"), Some(3),
    ).await;

    // Mark as scouted with success — should reset miss_count to 0
    writer.mark_gravity_scouted(t1, true).await.expect("mark failed");

    // Verify miss_count is 0
    let q = query(
        "MATCH (t:Tension {id: $id})
         RETURN t.gravity_scout_miss_count AS mc",
    )
    .param("id", t1.to_string());
    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().expect("should have row");
    let mc: i64 = row.get("mc").unwrap();
    assert_eq!(mc, 0, "Success should reset miss_count to 0");
}

#[tokio::test]
async fn create_drawn_to_edge_includes_gathering_type() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let tension_id = Uuid::new_v4();
    create_tension_for_gravity_scout(&client, tension_id, "ICE Fear", 0.7, 0.5, None, None).await;

    let event_id = Uuid::new_v4();
    create_signal(&client, "Event", event_id, "Singing Rebellion", "https://example.com/singing").await;

    writer
        .create_drawn_to_edge(event_id, tension_id, 0.9, "solidarity through singing", "singing")
        .await
        .expect("create_drawn_to_edge failed");

    // Verify DRAWN_TO edge exists with gathering_type
    let q = query(
        "MATCH (e:Event {id: $eid})-[rel:DRAWN_TO]->(t:Tension {id: $tid})
         RETURN rel.match_strength AS strength, rel.gathering_type AS gt",
    )
    .param("eid", event_id.to_string())
    .param("tid", tension_id.to_string());

    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().expect("Edge should exist");
    let strength: f64 = row.get("strength").unwrap();
    let gt: String = row.get("gt").unwrap();

    assert!((strength - 0.9).abs() < 0.001);
    assert_eq!(gt, "singing");
}

#[tokio::test]
async fn drawn_to_edge_coexists_with_response_edge() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let tension_id = Uuid::new_v4();
    create_tension_for_gravity_scout(&client, tension_id, "Housing Crisis", 0.7, 0.5, None, None).await;

    let give_id = Uuid::new_v4();
    create_signal(&client, "Give", give_id, "Tenant Solidarity Fund", "https://example.com/fund").await;

    // First: create a regular response edge
    writer
        .create_response_edge(give_id, tension_id, 0.8, "provides rent assistance")
        .await
        .expect("create_response_edge failed");

    // Then: create a DRAWN_TO edge for the same signal→tension
    // These are now separate edge types, so both should exist
    writer
        .create_drawn_to_edge(give_id, tension_id, 0.9, "solidarity fund", "solidarity fund")
        .await
        .expect("create_drawn_to_edge failed");

    // Verify RESPONDS_TO edge exists
    let q1 = query(
        "MATCH (g:Give {id: $gid})-[rel:RESPONDS_TO]->(t:Tension {id: $tid})
         RETURN count(rel) AS edge_count",
    )
    .param("gid", give_id.to_string())
    .param("tid", tension_id.to_string());

    let mut stream = client.inner().execute(q1).await.unwrap();
    let row = stream.next().await.unwrap().expect("Should have results");
    let resp_count: i64 = row.get("edge_count").unwrap();
    assert_eq!(resp_count, 1, "Should have exactly one RESPONDS_TO edge");

    // Verify DRAWN_TO edge exists separately
    let q2 = query(
        "MATCH (g:Give {id: $gid})-[rel:DRAWN_TO]->(t:Tension {id: $tid})
         RETURN count(rel) AS edge_count, rel.gathering_type AS gt",
    )
    .param("gid", give_id.to_string())
    .param("tid", tension_id.to_string());

    let mut stream = client.inner().execute(q2).await.unwrap();
    let row = stream.next().await.unwrap().expect("Should have results");
    let drawn_count: i64 = row.get("edge_count").unwrap();
    let gt: String = row.get("gt").unwrap();
    assert_eq!(drawn_count, 1, "Should have exactly one DRAWN_TO edge");
    assert_eq!(gt, "solidarity fund");
}

#[tokio::test]
async fn touch_signal_timestamp_refreshes_last_confirmed_active() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let event_id = Uuid::new_v4();
    create_signal(&client, "Event", event_id, "Weekly Vigil", "https://example.com/vigil").await;

    // Get initial timestamp
    let q = query(
        "MATCH (e:Event {id: $id})
         RETURN e.last_confirmed_active AS lca",
    )
    .param("id", event_id.to_string());
    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().expect("Should have row");
    let initial_lca: String = format!("{:?}", row.get::<chrono::NaiveDateTime>("lca"));

    // Wait a tiny bit and touch
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    writer.touch_signal_timestamp(event_id).await.expect("touch failed");

    // Verify timestamp was updated
    let q2 = query(
        "MATCH (e:Event {id: $id})
         RETURN e.last_confirmed_active AS lca",
    )
    .param("id", event_id.to_string());
    let mut stream2 = client.inner().execute(q2).await.unwrap();
    let row2 = stream2.next().await.unwrap().expect("Should have row");
    let updated_lca: String = format!("{:?}", row2.get::<chrono::NaiveDateTime>("lca"));

    assert_ne!(initial_lca, updated_lca, "last_confirmed_active should have been updated");
}
