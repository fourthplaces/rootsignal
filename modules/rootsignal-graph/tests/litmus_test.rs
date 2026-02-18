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

use rootsignal_graph::{query, GraphClient};

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
