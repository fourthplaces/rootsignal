//! Integration tests for litmus-test scenarios.
//!
//! Validates datetime storage, keyword search, geo queries, source diversity,
//! actor linking, and cross-type topic search against a real Memgraph instance.
//!
//! Requirements: Docker (for Memgraph via testcontainers)
//!
//! Run with: cargo test -p rootsignal-graph --features test-utils --test litmus_test

#![cfg(feature = "test-utils")]

use chrono::Utc;
use uuid::Uuid;

use rootsignal_common::EvidenceNode;
use rootsignal_graph::{query, GraphClient, GraphWriter};

/// Spin up a fresh Memgraph container and run migrations.
async fn setup() -> (impl std::any::Any, GraphClient) {
    let (container, client) = rootsignal_graph::testutil::memgraph_container().await;
    rootsignal_graph::migrate::migrate(&client).await.expect("migration failed");
    (container, client)
}

fn memgraph_dt(dt: &chrono::DateTime<Utc>) -> String {
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
    let now = memgraph_dt(&Utc::now());
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
    let now = memgraph_dt(&Utc::now());
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
    let now = memgraph_dt(&Utc::now());
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

    let now = memgraph_dt(&Utc::now());
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

    let now = memgraph_dt(&Utc::now());
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
    let now = memgraph_dt(&Utc::now());
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
