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

use rootsignal_common::events::{Event, Location, WorldEvent};
use rootsignal_common::system_events::SystemEvent;
use rootsignal_common::{DiscoveryMethod, GeoPoint, GeoPrecision, SourceRole};
use rootsignal_events::StoredEvent;
use rootsignal_graph::{query, BBox, GraphClient, GraphWriter, Pipeline};

/// Spin up a fresh Neo4j container and run migrations.
async fn setup() -> (impl std::any::Any, GraphClient) {
    let (container, client) = rootsignal_graph::testutil::neo4j_container().await;
    rootsignal_graph::migrate::migrate(&client)
        .await
        .expect("migration failed");
    (container, client)
}

fn stored(seq: i64, event: &Event) -> StoredEvent {
    StoredEvent {
        seq,
        ts: Utc::now(),
        event_type: event.event_type().to_string(),
        parent_seq: None,
        caused_by_seq: None,
        run_id: Some("test".to_string()),
        actor: None,
        payload: serde_json::to_value(event).expect("serialize event"),
        schema_v: 1,
    }
}

fn tc_bbox() -> BBox {
    BBox {
        min_lat: 44.0,
        max_lat: 46.0,
        min_lng: -94.0,
        max_lng: -92.0,
    }
}

fn mpls() -> Option<Location> {
    Some(Location {
        point: Some(GeoPoint {
            lat: 44.9778,
            lng: -93.2650,
            precision: GeoPrecision::Exact,
        }),
        name: Some("Minneapolis".into()),
        address: None,
    })
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
async fn create_signal(client: &GraphClient, label: &str, id: Uuid, title: &str, source_url: &str) {
    create_signal_at(client, label, id, title, source_url, 44.9778, -93.2650).await;
}

async fn create_signal_at(
    client: &GraphClient,
    label: &str,
    id: Uuid,
    title: &str,
    source_url: &str,
    lat: f64,
    lng: f64,
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
            lat: {lat},
            lng: {lng},
            embedding: {emb}
        }})"
    );

    let q = query(&cypher)
        .param("id", id.to_string())
        .param("title", title)
        .param("summary", format!("Test signal: {title}"))
        .param("source_url", source_url)
        .param("now", now);

    client
        .inner()
        .run(q)
        .await
        .expect("Failed to create signal");
}

/// Helper: create a Gathering with a specific starts_at using the CASE/datetime pattern.
async fn create_gathering_with_date(
    client: &GraphClient,
    id: Uuid,
    title: &str,
    starts_at: &str, // ISO datetime string, or "" for missing
) {
    let now = neo4j_dt(&Utc::now());
    let emb = dummy_embedding();
    let cypher = format!(
        "CREATE (e:Gathering {{
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
        .param("summary", format!("Test gathering: {title}"))
        .param("starts_at", starts_at)
        .param("now", now);

    client
        .inner()
        .run(q)
        .await
        .expect("Failed to create gathering");
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
            description: 'Test actor',
            signal_count: 1,
            first_seen: datetime($now),
            last_active: datetime($now),
            typical_roles: [$role]
        })",
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

    client
        .inner()
        .run(q)
        .await
        .expect("Failed to link actor to signal");
}

// ---------------------------------------------------------------------------
// Test 1: Gathering date stored as proper datetime
// ---------------------------------------------------------------------------

#[tokio::test]
async fn event_date_stored_as_datetime() {
    let (_container, client) = setup().await;

    let id = Uuid::new_v4();
    create_gathering_with_date(
        &client,
        id,
        "Test dated event",
        "2026-03-15T18:00:00.000000",
    )
    .await;

    let q = query(
        "MATCH (e:Gathering {id: $id})
         RETURN valueType(e.starts_at) AS vtype, e.starts_at IS NOT NULL AS has_date",
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
    create_gathering_with_date(&client, id, "No-date event", "").await;

    let q = query(
        "MATCH (e:Gathering {id: $id})
         RETURN e.starts_at IS NULL AS is_null",
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
    create_gathering_with_date(&client, id1, "March event", "2026-03-10T10:00:00.000000").await;
    create_gathering_with_date(&client, id2, "No-date event", "").await;
    create_gathering_with_date(&client, id3, "April event", "2026-04-01T14:00:00.000000").await;

    // ORDER BY on non-null dates should not crash
    let q = query(
        "MATCH (e:Gathering)
         WHERE e.starts_at IS NOT NULL
         RETURN e.title AS title
         ORDER BY e.starts_at",
    );

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let mut titles = Vec::new();
    while let Some(row) = stream.next().await.expect("stream error") {
        let title: String = row.get("title").expect("no title");
        titles.push(title);
    }

    assert_eq!(
        titles.len(),
        2,
        "should get 2 dated events, got {}",
        titles.len()
    );
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
    create_signal(
        &client,
        "Aid",
        id1,
        "Free food at community center",
        "https://food.org",
    )
    .await;
    create_signal(
        &client,
        "Need",
        id2,
        "Volunteers needed for food drive",
        "https://drive.org",
    )
    .await;
    create_signal(
        &client,
        "Gathering",
        id3,
        "Housing forum downtown",
        "https://housing.org",
    )
    .await;

    let q = query(
        "MATCH (n)
         WHERE (n:Aid OR n:Need OR n:Gathering) AND toLower(n.title) CONTAINS 'food'
         RETURN n.title AS title",
    );

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let mut found = Vec::new();
    while let Some(row) = stream.next().await.expect("stream error") {
        let title: String = row.get("title").expect("no title");
        found.push(title);
    }

    assert_eq!(
        found.len(),
        2,
        "should find 2 food-related signals, got {}",
        found.len()
    );
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
        "CREATE (n:Gathering {{
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
        "CREATE (n:Gathering {{
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
        "MATCH (n:Gathering)
         WHERE n.lat > 44.9 AND n.lat < 45.0
           AND n.lng > -93.3 AND n.lng < -93.2
         RETURN n.title AS title",
    );

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let mut found = Vec::new();
    while let Some(row) = stream.next().await.expect("stream error") {
        let title: String = row.get("title").expect("no title");
        found.push(title);
    }

    assert_eq!(
        found.len(),
        1,
        "should find 1 in-range event, got {}",
        found.len()
    );
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
    for (diversity, title) in [
        (5, "Multi-source signal"),
        (1, "Single-source signal"),
        (3, "Mid-source signal"),
    ] {
        let id = Uuid::new_v4();
        let cypher = format!(
            "CREATE (n:Gathering {{
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
        "MATCH (n:Gathering)
         RETURN n.title AS title, n.source_diversity AS diversity
         ORDER BY n.source_diversity DESC",
    );

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let mut diversities = Vec::new();
    while let Some(row) = stream.next().await.expect("stream error") {
        let d: i64 = row.get("diversity").expect("no diversity");
        diversities.push(d);
    }

    assert_eq!(
        diversities,
        vec![5, 3, 1],
        "should be sorted DESC by source_diversity"
    );
}

// ---------------------------------------------------------------------------
// Test 7: Actor linked via ACTED_IN
// ---------------------------------------------------------------------------

#[tokio::test]
async fn actor_linked_via_acted_in() {
    let (_container, client) = setup().await;

    let signal_id = Uuid::new_v4();
    create_signal(
        &client,
        "Gathering",
        signal_id,
        "Community cleanup",
        "https://cleanup.org",
    )
    .await;

    let actor_id = Uuid::new_v4();
    create_actor_and_link(
        &client,
        actor_id,
        "Neighborhood Council",
        signal_id,
        "Gathering",
        "organizer",
    )
    .await;

    let q = query(
        "MATCH (a:Actor)-[r:ACTED_IN]->(n:Gathering {id: $signal_id})
         RETURN a.name AS name, r.role AS role",
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
// Test 8: Cross-type topic search (Need + Aid about same topic)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cross_type_topic_search() {
    let (_container, client) = setup().await;

    let need_id = Uuid::new_v4();
    let aid_id = Uuid::new_v4();
    let unrelated_id = Uuid::new_v4();
    create_signal(
        &client,
        "Need",
        need_id,
        "Winter coats needed for families",
        "https://need.org",
    )
    .await;
    create_signal(
        &client,
        "Aid",
        aid_id,
        "Free winter coats available",
        "https://give.org",
    )
    .await;
    create_signal(
        &client,
        "Aid",
        unrelated_id,
        "Free tutoring available",
        "https://tutor.org",
    )
    .await;

    let q = query(
        "MATCH (n)
         WHERE (n:Need OR n:Aid) AND toLower(n.title) CONTAINS 'coat'
         RETURN labels(n)[0] AS type, n.title AS title",
    );

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let mut results: Vec<(String, String)> = Vec::new();
    while let Some(row) = stream.next().await.expect("stream error") {
        let label: String = row.get("type").expect("no type");
        let title: String = row.get("title").expect("no title");
        results.push((label, title));
    }

    assert_eq!(
        results.len(),
        2,
        "should find both Need and Aid about coats, got {}",
        results.len()
    );

    let types: Vec<&str> = results.iter().map(|(t, _)| t.as_str()).collect();
    assert!(types.contains(&"Need"), "should include Need");
    assert!(types.contains(&"Aid"), "should include Aid");
}

// ---------------------------------------------------------------------------
// Test 9: Same-source evidence is idempotent (MERGE, not CREATE)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn same_source_no_duplicate_evidence() {
    let (_container, client) = setup().await;
    let pipeline = Pipeline::new(client.clone(), 0.3);

    let signal_id = Uuid::new_v4();
    let ev1_id = Uuid::new_v4();
    let ev2_id = Uuid::new_v4();
    let ev3_id = Uuid::new_v4();

    let events = vec![
        stored(
            1,
            &Event::World(WorldEvent::GatheringDiscovered {
                id: signal_id,
                title: "Cleanup day".into(),
                summary: "Community cleanup".into(),
                confidence: 0.8,
                source_url: "https://source-a.org".into(),
                extracted_at: Utc::now(),
                content_date: None,
                location: mpls(),
                from_location: None,
                mentioned_actors: vec![],
                author_actor: None,
                schedule: None,
                action_url: None,
                organizer: None,
            }),
        ),
        // Three citations from same URL — simulates re-scrapes
        stored(
            2,
            &Event::World(WorldEvent::CitationRecorded {
                citation_id: ev1_id,
                signal_id,
                url: "https://source-a.org".into(),
                content_hash: "hash_v1".into(),
                snippet: Some("First scrape".into()),
                relevance: None,
                channel_type: None,
                evidence_confidence: None,
            }),
        ),
        stored(
            3,
            &Event::World(WorldEvent::CitationRecorded {
                citation_id: ev2_id,
                signal_id,
                url: "https://source-a.org".into(),
                content_hash: "hash_v2".into(),
                snippet: Some("Second scrape".into()),
                relevance: None,
                channel_type: None,
                evidence_confidence: None,
            }),
        ),
        stored(
            4,
            &Event::World(WorldEvent::CitationRecorded {
                citation_id: ev3_id,
                signal_id,
                url: "https://source-a.org".into(),
                content_hash: "hash_v3".into(),
                snippet: Some("Third scrape".into()),
                relevance: None,
                channel_type: None,
                evidence_confidence: None,
            }),
        ),
    ];

    pipeline
        .process(&events, &tc_bbox(), &[])
        .await
        .expect("pipeline failed");

    // Should have exactly 1 evidence node, not 3 (MERGE on source_url)
    let q = query(
        "MATCH (n:Gathering {id: $id})-[:SOURCED_FROM]->(ev:Evidence)
         RETURN count(ev) AS cnt, ev.content_hash AS hash",
    )
    .param("id", signal_id.to_string());

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream error").expect("no row");

    let cnt: i64 = row.get("cnt").expect("no cnt");
    let hash: String = row.get("hash").expect("no hash");

    assert_eq!(
        cnt, 1,
        "should have exactly 1 evidence node from same source, got {cnt}"
    );
    assert_eq!(
        hash, "hash_v3",
        "content_hash should be updated to latest scrape"
    );
}

// ---------------------------------------------------------------------------
// Test 10: Cross-source evidence creates separate nodes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cross_source_creates_new_evidence() {
    let (_container, client) = setup().await;
    let pipeline = Pipeline::new(client.clone(), 0.3);

    let signal_id = Uuid::new_v4();

    let events = vec![
        stored(
            1,
            &Event::World(WorldEvent::GatheringDiscovered {
                id: signal_id,
                title: "Community meeting".into(),
                summary: "Neighborhood meeting".into(),
                confidence: 0.8,
                source_url: "https://source-a.org".into(),
                extracted_at: Utc::now(),
                content_date: None,
                location: mpls(),
                from_location: None,
                mentioned_actors: vec![],
                author_actor: None,
                schedule: None,
                action_url: None,
                organizer: None,
            }),
        ),
        stored(
            2,
            &Event::World(WorldEvent::CitationRecorded {
                citation_id: Uuid::new_v4(),
                signal_id,
                url: "https://source-a.org".into(),
                content_hash: "hash_a".into(),
                snippet: Some("Source A".into()),
                relevance: None,
                channel_type: None,
                evidence_confidence: None,
            }),
        ),
        stored(
            3,
            &Event::World(WorldEvent::CitationRecorded {
                citation_id: Uuid::new_v4(),
                signal_id,
                url: "https://source-b.org".into(),
                content_hash: "hash_b".into(),
                snippet: Some("Source B".into()),
                relevance: None,
                channel_type: None,
                evidence_confidence: None,
            }),
        ),
        stored(
            4,
            &Event::World(WorldEvent::CitationRecorded {
                citation_id: Uuid::new_v4(),
                signal_id,
                url: "https://source-c.org".into(),
                content_hash: "hash_c".into(),
                snippet: Some("Source C".into()),
                relevance: None,
                channel_type: None,
                evidence_confidence: None,
            }),
        ),
    ];

    pipeline
        .process(&events, &tc_bbox(), &[])
        .await
        .expect("pipeline failed");

    // Should have 3 evidence nodes (one per source)
    let q = query(
        "MATCH (n:Gathering {id: $id})-[:SOURCED_FROM]->(ev:Evidence)
         RETURN count(ev) AS cnt",
    )
    .param("id", signal_id.to_string());

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream error").expect("no row");

    let cnt: i64 = row.get("cnt").expect("no cnt");
    assert_eq!(
        cnt, 3,
        "should have 3 evidence nodes from 3 different sources, got {cnt}"
    );
}

// ---------------------------------------------------------------------------
// Test 11: Same-source refresh does not inflate corroboration_count
// ---------------------------------------------------------------------------

#[tokio::test]
async fn same_source_does_not_inflate_corroboration() {
    let (_container, client) = setup().await;
    let pipeline = Pipeline::new(client.clone(), 0.3);

    let signal_id = Uuid::new_v4();

    // Create signal + initial evidence from same source
    let mut events = vec![
        stored(
            1,
            &Event::World(WorldEvent::GatheringDiscovered {
                id: signal_id,
                title: "Annual parade".into(),
                summary: "City parade".into(),
                confidence: 0.8,
                source_url: "https://parade.org".into(),
                extracted_at: Utc::now(),
                content_date: None,
                location: mpls(),
                from_location: None,
                mentioned_actors: vec![],
                author_actor: None,
                schedule: None,
                action_url: None,
                organizer: None,
            }),
        ),
        stored(
            2,
            &Event::World(WorldEvent::CitationRecorded {
                citation_id: Uuid::new_v4(),
                signal_id,
                url: "https://parade.org".into(),
                content_hash: "hash_v1".into(),
                snippet: None,
                relevance: None,
                channel_type: None,
                evidence_confidence: None,
            }),
        ),
    ];

    // Simulate 5 same-source re-scrapes: FreshnessConfirmed + CitationRecorded (MERGE)
    for i in 0..5u32 {
        let seq = (i as i64) * 2 + 3;
        events.push(stored(
            seq,
            &Event::System(
                SystemEvent::FreshnessConfirmed {
                    signal_ids: vec![signal_id],
                    node_type: rootsignal_common::NodeType::Gathering,
                    confirmed_at: Utc::now(),
                },
            ),
        ));
        events.push(stored(
            seq + 1,
            &Event::World(WorldEvent::CitationRecorded {
                citation_id: Uuid::new_v4(),
                signal_id,
                url: "https://parade.org".into(),
                content_hash: format!("hash_v{}", i + 2),
                snippet: None,
                relevance: None,
                channel_type: None,
                evidence_confidence: None,
            }),
        ));
    }

    pipeline
        .process(&events, &tc_bbox(), &[])
        .await
        .expect("pipeline failed");

    // corroboration_count should still be 0 (initial value, never incremented)
    let q = query(
        "MATCH (n:Gathering {id: $id})
         RETURN n.corroboration_count AS corr",
    )
    .param("id", signal_id.to_string());

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream error").expect("no row");
    let corr: i64 = row.get("corr").expect("no corr");
    assert_eq!(
        corr, 0,
        "corroboration_count should stay 0 after same-source refreshes, got {corr}"
    );

    // Should still have exactly 1 evidence node
    let q = query(
        "MATCH (n:Gathering {id: $id})-[:SOURCED_FROM]->(ev:Evidence)
         RETURN count(ev) AS cnt",
    )
    .param("id", signal_id.to_string());

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream error").expect("no row");
    let cnt: i64 = row.get("cnt").expect("no cnt");
    assert_eq!(
        cnt, 1,
        "should have exactly 1 evidence node after 5 same-source refreshes, got {cnt}"
    );

    // Now simulate a REAL cross-source corroboration via ObservationCorroborated + CitationRecorded
    let corr_events = vec![
        stored(
            13,
            &Event::World(WorldEvent::ObservationCorroborated {
                signal_id,
                node_type: rootsignal_common::NodeType::Gathering,
                new_source_url: "https://independent-news.org".into(),
                summary: None,
            }),
        ),
        stored(
            14,
            &Event::World(WorldEvent::CitationRecorded {
                citation_id: Uuid::new_v4(),
                signal_id,
                url: "https://independent-news.org".into(),
                content_hash: "cross_hash".into(),
                snippet: None,
                relevance: None,
                channel_type: None,
                evidence_confidence: None,
            }),
        ),
    ];

    pipeline
        .process(&corr_events, &tc_bbox(), &[])
        .await
        .expect("corroboration pipeline failed");

    // Now corroboration_count should be 1, evidence count should be 2
    let q = query(
        "MATCH (n:Gathering {id: $id})
         OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
         RETURN n.corroboration_count AS corr, count(ev) AS ev_cnt",
    )
    .param("id", signal_id.to_string());

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream error").expect("no row");
    let corr: i64 = row.get("corr").expect("no corr");
    let ev_cnt: i64 = row.get("ev_cnt").expect("no ev_cnt");
    assert_eq!(
        corr, 1,
        "corroboration_count should be 1 after one real cross-source, got {corr}"
    );
    assert_eq!(
        ev_cnt, 2,
        "should have 2 evidence nodes (1 same-source + 1 cross-source), got {ev_cnt}"
    );
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
        "CREATE (n:Gathering {{
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
            "MATCH (n:Gathering {id: $signal_id})
             CREATE (ev:Evidence {
                 id: $ev_id,
                 source_url: 'https://source-a.org',
                 retrieved_at: datetime($now),
                 content_hash: $hash,
                 snippet: '',
                 relevance: '',
                 evidence_confidence: 0.0
             })
             CREATE (n)-[:SOURCED_FROM]->(ev)",
        )
        .param("signal_id", signal_id.to_string())
        .param("ev_id", ev_id.to_string())
        .param("now", now.clone())
        .param("hash", format!("hash_{i}"));
        client
            .inner()
            .run(q)
            .await
            .expect("create duplicate evidence");
    }

    // Also add 1 legitimate cross-source evidence
    let cross_ev_id = Uuid::new_v4();
    let q = query(
        "MATCH (n:Gathering {id: $signal_id})
         CREATE (ev:Evidence {
             id: $ev_id,
             source_url: 'https://independent.org',
             retrieved_at: datetime($now),
             content_hash: 'cross_hash',
             snippet: '',
             relevance: '',
             evidence_confidence: 0.0
         })
         CREATE (n)-[:SOURCED_FROM]->(ev)",
    )
    .param("signal_id", signal_id.to_string())
    .param("ev_id", cross_ev_id.to_string())
    .param("now", now);
    client
        .inner()
        .run(q)
        .await
        .expect("create cross-source evidence");

    // Verify the mess: 15 evidence nodes, corroboration_count = 13
    let q = query(
        "MATCH (n:Gathering {id: $id})-[:SOURCED_FROM]->(ev:Evidence)
         RETURN n.corroboration_count AS corr, count(ev) AS ev_cnt",
    )
    .param("id", signal_id.to_string());
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream error").expect("no row");
    let ev_cnt: i64 = row.get("ev_cnt").expect("no ev_cnt");
    assert_eq!(
        ev_cnt, 15,
        "pre-migration: should have 15 evidence nodes, got {ev_cnt}"
    );

    // Run the dedup migration
    rootsignal_graph::migrate::deduplicate_evidence(&client)
        .await
        .expect("deduplicate_evidence failed");

    // After migration: should have 2 evidence nodes (1 per unique source_url)
    // and corroboration_count = 1 (2 evidence - 1 = 1 real corroboration)
    let q = query(
        "MATCH (n:Gathering {id: $id})
         OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
         RETURN n.corroboration_count AS corr, count(ev) AS ev_cnt",
    )
    .param("id", signal_id.to_string());
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream error").expect("no row");

    let corr: i64 = row.get("corr").expect("no corr");
    let ev_cnt: i64 = row.get("ev_cnt").expect("no ev_cnt");
    assert_eq!(
        ev_cnt, 2,
        "post-migration: should have 2 evidence nodes (1 per source), got {ev_cnt}"
    );
    assert_eq!(
        corr, 1,
        "post-migration: corroboration_count should be 1 (2 sources - 1), got {corr}"
    );

    // Verify the distinct source URLs are correct
    let q = query(
        "MATCH (n:Gathering {id: $id})-[:SOURCED_FROM]->(ev:Evidence)
         RETURN ev.source_url AS url ORDER BY url",
    )
    .param("id", signal_id.to_string());
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let mut urls = Vec::new();
    while let Some(row) = stream.next().await.expect("stream error") {
        let url: String = row.get("url").expect("no url");
        urls.push(url);
    }
    assert_eq!(
        urls,
        vec!["https://independent.org", "https://source-a.org"]
    );
}

// ---------------------------------------------------------------------------
// Test: Bbox proximity signal lookup (list_recent_in_bbox)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bbox_proximity_signal_lookup() {
    let (_container, client) = setup().await;

    let now = neo4j_dt(&Utc::now());
    let emb = dummy_embedding();

    // Minneapolis center: 44.9778, -93.2650
    // Create signal in downtown Minneapolis (inside ~50km radius)
    let mpls_id = Uuid::new_v4();
    let cypher = format!(
        "CREATE (n:Aid {{
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
    client
        .inner()
        .run(
            query(&cypher)
                .param("id", mpls_id.to_string())
                .param("now", now.clone()),
        )
        .await
        .expect("create mpls signal");

    // Create signal in St. Paul (inside ~50km radius of Minneapolis center)
    let stp_id = Uuid::new_v4();
    let cypher = format!(
        "CREATE (n:Gathering {{
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
    client
        .inner()
        .run(
            query(&cypher)
                .param("id", stp_id.to_string())
                .param("now", now.clone()),
        )
        .await
        .expect("create stp signal");

    // Create signal in Duluth (outside ~50km radius — ~250km away)
    let duluth_id = Uuid::new_v4();
    let cypher = format!(
        "CREATE (n:Gathering {{
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
    client
        .inner()
        .run(
            query(&cypher)
                .param("id", duluth_id.to_string())
                .param("now", now.clone()),
        )
        .await
        .expect("create duluth signal");

    // Create signal at (0,0) — should be excluded
    let zero_id = Uuid::new_v4();
    let cypher = format!(
        "CREATE (n:Need {{
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
    client
        .inner()
        .run(
            query(&cypher)
                .param("id", zero_id.to_string())
                .param("now", now),
        )
        .await
        .expect("create zero signal");

    // Query via PublicGraphReader with Minneapolis center + 50km radius
    let reader = rootsignal_graph::PublicGraphReader::new(client.clone());
    let results = reader
        .list_recent_in_bbox(44.9778, -93.2650, 50.0, 100)
        .await
        .expect("list_recent_in_bbox failed");

    let titles: Vec<&str> = results
        .iter()
        .filter_map(|n| n.meta().map(|m| m.title.as_str()))
        .collect();

    // Should include Minneapolis and St. Paul signals
    assert!(
        titles.contains(&"Minneapolis food shelf"),
        "Missing Minneapolis signal; got: {:?}",
        titles
    );
    assert!(
        titles.contains(&"St Paul community event"),
        "Missing St Paul signal; got: {:?}",
        titles
    );

    // Should NOT include Duluth or zero-coordinate signal
    assert!(
        !titles.contains(&"Duluth harbor event"),
        "Duluth signal should be outside radius; got: {:?}",
        titles
    );
    assert!(
        !titles.contains(&"Zero-coordinate signal"),
        "Zero-coordinate signal should be excluded; got: {:?}",
        titles
    );
}

// ---------------------------------------------------------------------------
// Test: Source last_scraped survives datetime() round-trip
// ---------------------------------------------------------------------------

#[tokio::test]
async fn source_last_scraped_round_trip() {
    let (_container, client) = setup().await;
    let pipeline = Pipeline::new(client.clone(), 0.3);
    let writer = GraphWriter::new(client.clone());

    let source_id = Uuid::new_v4();
    let events = vec![stored(
        1,
        &Event::System(SystemEvent::SourceRegistered {
            source_id,
            canonical_key: "https://example.org".into(),
            canonical_value: "https://example.org".into(),
            url: Some("https://example.org".into()),
            discovery_method: DiscoveryMethod::Curated,
            weight: 0.5,
            source_role: SourceRole::Mixed,
            gap_context: None,
        }),
    )];

    pipeline
        .process(&events, &tc_bbox(), &[])
        .await
        .expect("pipeline failed");

    // Record a scrape (stores last_scraped via Cypher datetime()) — still uses writer
    let scrape_time = Utc::now();
    writer
        .record_source_scrape("https://example.org", 3, scrape_time)
        .await
        .expect("record_source_scrape failed");

    // Read back via get_active_sources — the bug caused last_scraped to be None
    // because row.get::<String>() silently failed on Neo4j DateTime types
    let sources = writer
        .get_active_sources()
        .await
        .expect("get_active_sources failed");
    assert_eq!(sources.len(), 1, "should find 1 source");

    let s = &sources[0];
    assert!(
        s.last_scraped.is_some(),
        "last_scraped should be Some after record_source_scrape, got None"
    );
    assert!(
        s.last_produced_signal.is_some(),
        "last_produced_signal should be Some when signals > 0"
    );
    assert_eq!(s.signals_produced, 3);
    assert_eq!(s.scrape_count, 1);
    assert_eq!(s.consecutive_empty_runs, 0);

    // Also verify created_at survived the round-trip (not just defaulting to now)
    let age = Utc::now() - s.created_at;
    assert!(
        age.num_seconds() < 60,
        "created_at should be recent, not a fallback value"
    );
}

/// Helper: create a Tension with a specific embedding vector at given coordinates.
async fn create_tension_with_embedding_at(
    client: &GraphClient,
    id: Uuid,
    title: &str,
    embedding: &[f64],
    lat: f64,
    lng: f64,
) {
    let now = neo4j_dt(&Utc::now());
    let emb_str = format!(
        "[{}]",
        embedding
            .iter()
            .map(|v| format!("{v}"))
            .collect::<Vec<_>>()
            .join(",")
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
            location_name: '',
            severity: 'high',
            category: 'safety',
            what_would_help: 'more resources',
            lat: {lat},
            lng: {lng},
            embedding: {emb_str}
        }})"
    );

    let q = query(&cypher)
        .param("id", id.to_string())
        .param("title", title)
        .param("summary", format!("Test tension: {title}"))
        .param("now", now);

    client
        .inner()
        .run(q)
        .await
        .expect("Failed to create tension");
}

/// Helper: create a Tension with a specific embedding vector at Twin Cities coordinates.
async fn create_tension_with_embedding(
    client: &GraphClient,
    id: Uuid,
    title: &str,
    embedding: &[f64],
) {
    create_tension_with_embedding_at(client, id, title, embedding, 44.9778, -93.2650).await;
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

    create_tension_with_embedding(
        &client,
        id1,
        "Youth Violence in North Minneapolis",
        &base_emb,
    )
    .await;
    create_tension_with_embedding(
        &client,
        id2,
        "Youth Violence Spike in North Minneapolis",
        &emb2,
    )
    .await;
    create_tension_with_embedding(
        &client,
        id3,
        "Youth Violence and Lack of Safe Spaces",
        &emb3,
    )
    .await;

    // Create one unrelated tension
    let id_unrelated = Uuid::new_v4();
    let mut unrelated_emb = vec![0.0f64; 1024];
    unrelated_emb[500] = 1.0; // completely different
    create_tension_with_embedding(
        &client,
        id_unrelated,
        "Housing Affordability Crisis",
        &unrelated_emb,
    )
    .await;

    // Create a signal that RESPONDS_TO one of the duplicates
    let signal_id = Uuid::new_v4();
    create_signal(
        &client,
        "Aid",
        signal_id,
        "NAZ Tutoring",
        "https://example.com/naz",
    )
    .await;
    let q = query(
        "MATCH (g:Aid {id: $gid}), (t:Tension {id: $tid})
         CREATE (g)-[:RESPONDS_TO {match_strength: 0.9, explanation: 'test'}]->(t)",
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

    // Run merge (bbox covering Twin Cities test data)
    let merged = writer
        .merge_duplicate_tensions(0.85, 44.0, 46.0, -94.0, -92.0)
        .await
        .expect("merge failed");
    assert_eq!(merged, 2, "Should merge 2 duplicates (keep 1 of 3)");

    // Verify: 2 tensions after merge (1 youth violence survivor + 1 housing)
    let q = query("MATCH (t:Tension) RETURN count(t) AS cnt");
    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().unwrap();
    let count: i64 = row.get("cnt").unwrap();
    assert_eq!(count, 2, "Should have 2 tensions after merge");

    // Verify: the RESPONDS_TO edge was re-pointed to the survivor (id1, the oldest)
    let q = query(
        "MATCH (g:Aid {id: $gid})-[:RESPONDS_TO]->(t:Tension)
         RETURN t.id AS tid",
    )
    .param("gid", signal_id.to_string());
    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream
        .next()
        .await
        .unwrap()
        .expect("Should have RESPONDS_TO edge");
    let tid: String = row.get("tid").unwrap();
    assert_eq!(
        tid,
        id1.to_string(),
        "Edge should point to survivor (oldest tension)"
    );

    // Verify: survivor got corroboration bumped
    let q = query("MATCH (t:Tension {id: $id}) RETURN t.corroboration_count AS cnt")
        .param("id", id1.to_string());
    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().unwrap();
    let corr: i64 = row.get("cnt").unwrap();
    assert_eq!(
        corr, 2,
        "Survivor should have corroboration_count = 2 (absorbed 2 dupes)"
    );

    // Verify: unrelated tension untouched
    let q = query("MATCH (t:Tension {id: $id}) RETURN t.title AS title")
        .param("id", id_unrelated.to_string());
    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream
        .next()
        .await
        .unwrap()
        .expect("Unrelated tension should survive");
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

    let merged = writer
        .merge_duplicate_tensions(0.85, 44.0, 46.0, -94.0, -92.0)
        .await
        .expect("merge failed");
    assert_eq!(merged, 0, "No duplicates should be merged");

    let q = query("MATCH (t:Tension) RETURN count(t) AS cnt");
    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().unwrap();
    let count: i64 = row.get("cnt").unwrap();
    assert_eq!(count, 2, "Both tensions should survive");
}

#[tokio::test]
async fn merge_duplicate_tensions_preserves_cross_region_signals() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    // Create two near-identical tensions (cosine >0.85) in different cities
    let mut base_emb = vec![0.0f64; 1024];
    base_emb[0] = 1.0;
    base_emb[1] = 0.5;

    let mut chi_emb = base_emb.clone();
    chi_emb[2] = 0.05; // tiny perturbation, still >0.85 cosine

    let tc_id = Uuid::new_v4();
    let chi_id = Uuid::new_v4();

    // Twin Cities
    create_tension_with_embedding_at(
        &client,
        tc_id,
        "ICE Enforcement Actions",
        &base_emb,
        44.9778,
        -93.2650,
    )
    .await;

    // Chicago
    create_tension_with_embedding_at(
        &client,
        chi_id,
        "ICE Enforcement Actions",
        &chi_emb,
        41.8781,
        -87.6298,
    )
    .await;

    // Merge scoped to Twin Cities bbox — should not touch Chicago
    let merged = writer
        .merge_duplicate_tensions(0.85, 44.0, 46.0, -94.0, -92.0)
        .await
        .expect("merge failed");
    assert_eq!(merged, 0, "Cross-region tensions must not be merged");

    // Both tensions survive
    let q = query("MATCH (t:Tension) RETURN count(t) AS cnt");
    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().unwrap();
    let count: i64 = row.get("cnt").unwrap();
    assert_eq!(count, 2, "Both region tensions should survive");
}

#[tokio::test]
async fn merge_duplicate_tensions_repoints_situation_edges() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    // Two near-identical tensions (cosine >0.85)
    let mut base_emb = vec![0.0f64; 1024];
    base_emb[0] = 1.0;
    base_emb[1] = 0.5;

    let mut dup_emb = base_emb.clone();
    dup_emb[2] = 0.05; // tiny perturbation

    let survivor_id = Uuid::new_v4();
    let dup_id = Uuid::new_v4();

    create_tension_with_embedding(
        &client,
        survivor_id,
        "Youth Violence in North Minneapolis",
        &base_emb,
    )
    .await;
    create_tension_with_embedding(
        &client,
        dup_id,
        "Youth Violence Spike in North Minneapolis",
        &dup_emb,
    )
    .await;

    // Create a Situation and link the duplicate to it via PART_OF
    let situation_id = Uuid::new_v4();
    let now = neo4j_dt(&Utc::now());
    let q = query(
        "CREATE (s:Situation {
            id: $id,
            headline: 'Youth Violence Situation',
            lede: 'test',
            arc: 'escalating',
            temperature: 0.5,
            tension_heat: 0.5,
            entity_velocity: 0.0,
            amplification: 0.0,
            response_coverage: 0.0,
            clarity_need: 0.5,
            clarity: 'murky',
            centroid_lat: 44.9778,
            centroid_lng: -93.2650,
            location_name: 'Minneapolis',
            structured_state: '{}',
            signal_count: 1,
            tension_count: 1,
            dispatch_count: 0,
            first_seen: datetime($now),
            last_updated: datetime($now),
            sensitivity: 'general',
            category: 'safety'
        })",
    )
    .param("id", situation_id.to_string())
    .param("now", now);
    client.inner().run(q).await.expect("Create situation");

    let q = query(
        "MATCH (t:Tension {id: $tid}), (s:Situation {id: $sid})
         CREATE (t)-[:PART_OF]->(s)",
    )
    .param("tid", dup_id.to_string())
    .param("sid", situation_id.to_string());
    client.inner().run(q).await.expect("Create PART_OF edge");

    // Run merge
    let merged = writer
        .merge_duplicate_tensions(0.85, 44.0, 46.0, -94.0, -92.0)
        .await
        .expect("merge failed");
    assert_eq!(merged, 1, "Should merge 1 duplicate");

    // Survivor now has PART_OF edge to Situation
    let q = query(
        "MATCH (t:Tension {id: $tid})-[:PART_OF]->(s:Situation {id: $sid})
         RETURN t.id AS tid",
    )
    .param("tid", survivor_id.to_string())
    .param("sid", situation_id.to_string());
    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream
        .next()
        .await
        .unwrap()
        .expect("Survivor should have PART_OF edge to Situation");
    let tid: String = row.get("tid").unwrap();
    assert_eq!(tid, survivor_id.to_string());

    // Duplicate is deleted
    let q = query("MATCH (t:Tension {id: $id}) RETURN t.id AS id")
        .param("id", dup_id.to_string());
    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap();
    assert!(row.is_none(), "Duplicate tension should be deleted");
}

// =============================================================================
// Response Scout writer method tests
// =============================================================================

/// Helper: create a tension with specific confidence and optional response_scouted_at.
async fn create_tension_for_response_finder(
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

    client
        .inner()
        .run(q)
        .await
        .expect("Failed to create tension");
}

#[tokio::test]
async fn response_finder_targets_finds_unscouted_tensions() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let t1 = Uuid::new_v4();
    let t2 = Uuid::new_v4();
    let t3 = Uuid::new_v4();

    // t1: never scouted, high confidence — should be found
    create_tension_for_response_finder(&client, t1, "ICE Enforcement Fear", 0.7, None).await;
    // t2: scouted recently — should NOT be found
    create_tension_for_response_finder(
        &client,
        t2,
        "Housing Crisis",
        0.8,
        Some("2026-02-17T00:00:00"),
    )
    .await;
    // t3: low confidence (below 0.5) — should NOT be found
    create_tension_for_response_finder(&client, t3, "Emergent Tension", 0.3, None).await;

    let targets = writer
        .find_response_finder_targets(10, -90.0, 90.0, -180.0, 180.0)
        .await
        .expect("query failed");

    assert_eq!(targets.len(), 1, "Only 1 target should qualify");
    assert_eq!(targets[0].tension_id, t1);
    assert_eq!(targets[0].title, "ICE Enforcement Fear");
    assert_eq!(targets[0].response_count, 0);
}

#[tokio::test]
async fn response_finder_targets_includes_stale_scouted_tensions() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let t1 = Uuid::new_v4();

    // Scouted 30 days ago — should be found (>14 day threshold)
    create_tension_for_response_finder(
        &client,
        t1,
        "Old Tension",
        0.7,
        Some("2026-01-15T00:00:00"),
    )
    .await;

    let targets = writer
        .find_response_finder_targets(10, -90.0, 90.0, -180.0, 180.0)
        .await
        .expect("query failed");
    assert_eq!(
        targets.len(),
        1,
        "Stale-scouted tension should be re-eligible"
    );
    assert_eq!(targets[0].tension_id, t1);
}

#[tokio::test]
async fn response_finder_targets_sorted_by_response_count_then_heat() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let t1 = Uuid::new_v4();
    let t2 = Uuid::new_v4();

    create_tension_for_response_finder(&client, t1, "Well-served", 0.7, None).await;
    create_tension_for_response_finder(&client, t2, "Neglected", 0.7, None).await;

    // Give t1 a response edge, t2 has none
    let aid_id = Uuid::new_v4();
    create_signal(
        &client,
        "Aid",
        aid_id,
        "Food Shelf",
        "https://example.com/food",
    )
    .await;
    let edge_q = query(
        "MATCH (g:Aid {id: $gid}), (t:Tension {id: $tid})
         CREATE (g)-[:RESPONDS_TO {match_strength: 0.8, explanation: 'test'}]->(t)",
    )
    .param("gid", aid_id.to_string())
    .param("tid", t1.to_string());
    client
        .inner()
        .run(edge_q)
        .await
        .expect("edge creation failed");

    let targets = writer
        .find_response_finder_targets(10, -90.0, 90.0, -180.0, 180.0)
        .await
        .expect("query failed");
    assert_eq!(targets.len(), 2);
    // t2 (0 responses) should come first
    assert_eq!(
        targets[0].tension_id, t2,
        "Neglected tension should sort first"
    );
    assert_eq!(targets[0].response_count, 0);
    assert_eq!(targets[1].tension_id, t1);
    assert_eq!(targets[1].response_count, 1);
}

#[tokio::test]
async fn get_existing_responses_returns_heuristics() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let tension_id = Uuid::new_v4();
    create_tension_for_response_finder(&client, tension_id, "Housing Crisis", 0.7, None).await;

    let aid_id = Uuid::new_v4();
    create_signal(
        &client,
        "Aid",
        aid_id,
        "Rent Assistance Program",
        "https://example.com/rent",
    )
    .await;

    let edge_q = query(
        "MATCH (g:Aid {id: $gid}), (t:Tension {id: $tid})
         CREATE (g)-[:RESPONDS_TO {match_strength: 0.9, explanation: 'provides rent help'}]->(t)",
    )
    .param("gid", aid_id.to_string())
    .param("tid", tension_id.to_string());
    client
        .inner()
        .run(edge_q)
        .await
        .expect("edge creation failed");

    let heuristics = writer
        .get_existing_responses(tension_id)
        .await
        .expect("query failed");
    assert_eq!(heuristics.len(), 1);
    assert_eq!(heuristics[0].title, "Rent Assistance Program");
    assert_eq!(heuristics[0].signal_type, "Aid");
}

#[tokio::test]
async fn mark_response_found_sets_timestamp() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let tension_id = Uuid::new_v4();
    create_tension_for_response_finder(&client, tension_id, "Test Tension", 0.7, None).await;

    // Before marking — should be a target
    let targets = writer
        .find_response_finder_targets(10, -90.0, 90.0, -180.0, 180.0)
        .await
        .expect("query failed");
    assert_eq!(targets.len(), 1);

    // Mark as scouted
    writer
        .mark_response_found(tension_id)
        .await
        .expect("mark failed");

    // After marking — should NOT be a target (scouted < 14 days ago)
    let targets = writer
        .find_response_finder_targets(10, -90.0, 90.0, -180.0, 180.0)
        .await
        .expect("query failed");
    assert_eq!(
        targets.len(),
        0,
        "Recently scouted tension should not be a target"
    );
}

#[tokio::test]
async fn create_response_edge_wires_give_to_tension() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let tension_id = Uuid::new_v4();
    create_tension_for_response_finder(&client, tension_id, "Test Tension", 0.7, None).await;

    let aid_id = Uuid::new_v4();
    create_signal(
        &client,
        "Aid",
        aid_id,
        "Mutual Aid Network",
        "https://example.com/aid",
    )
    .await;

    writer
        .create_response_edge(aid_id, tension_id, 0.85, "provides mutual aid")
        .await
        .expect("create_response_edge failed");

    // Verify edge exists
    let q = query(
        "MATCH (g:Aid {id: $gid})-[rel:RESPONDS_TO]->(t:Tension {id: $tid})
         RETURN rel.match_strength AS strength, rel.explanation AS explanation",
    )
    .param("gid", aid_id.to_string())
    .param("tid", tension_id.to_string());

    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().expect("Edge should exist");
    let strength: f64 = row.get("strength").unwrap();
    let explanation: String = row.get("explanation").unwrap();

    assert!(
        (strength - 0.85).abs() < 0.001,
        "match_strength should be 0.85"
    );
    assert_eq!(explanation, "provides mutual aid");
}

// =============================================================================
// Gravity Scout integration tests
// =============================================================================

/// Helper: create a tension with specific confidence, cause_heat, and optional gravity scouting state.
async fn create_tension_for_gathering_finder(
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

    client
        .inner()
        .run(q)
        .await
        .expect("Failed to create tension for gravity scout");
}

#[tokio::test]
async fn gathering_finder_targets_requires_minimum_heat() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let t1 = Uuid::new_v4();
    let t2 = Uuid::new_v4();

    // t1: has heat — should be found
    create_tension_for_gathering_finder(&client, t1, "ICE Enforcement Fear", 0.7, 0.5, None, None)
        .await;
    // t2: no heat (0.0) — should NOT be found
    create_tension_for_gathering_finder(&client, t2, "Cold Tension", 0.7, 0.0, None, None).await;

    let targets = writer
        .find_gathering_finder_targets(10, -90.0, 90.0, -180.0, 180.0)
        .await
        .expect("query failed");
    assert_eq!(targets.len(), 1, "Only hot tension should qualify");
    assert_eq!(targets[0].tension_id, t1);
}

#[tokio::test]
async fn gathering_finder_targets_sorted_by_heat_desc() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let t1 = Uuid::new_v4();
    let t2 = Uuid::new_v4();

    // t1: moderate heat
    create_tension_for_gathering_finder(&client, t1, "Moderate", 0.7, 0.3, None, None).await;
    // t2: high heat
    create_tension_for_gathering_finder(&client, t2, "Hot", 0.7, 0.9, None, None).await;

    let targets = writer
        .find_gathering_finder_targets(10, -90.0, 90.0, -180.0, 180.0)
        .await
        .expect("query failed");
    assert_eq!(targets.len(), 2);
    // Hottest first
    assert_eq!(
        targets[0].tension_id, t2,
        "Hottest tension should sort first"
    );
    assert_eq!(targets[1].tension_id, t1);
}

#[tokio::test]
async fn gathering_finder_respects_scouted_timestamp() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let t1 = Uuid::new_v4();

    // Scouted 3 days ago — should NOT be found (7-day base window)
    create_tension_for_gathering_finder(
        &client,
        t1,
        "Recent",
        0.7,
        0.5,
        Some("2026-02-15T00:00:00"),
        Some(0),
    )
    .await;

    let targets = writer
        .find_gathering_finder_targets(10, -90.0, 90.0, -180.0, 180.0)
        .await
        .expect("query failed");
    assert_eq!(
        targets.len(),
        0,
        "Recently scouted tension should not be a target"
    );
}

#[tokio::test]
async fn gathering_finder_backoff_on_consecutive_misses() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let t1 = Uuid::new_v4();

    // Scouted 15 days ago with miss_count=2 — needs 21 days, so should NOT be found
    create_tension_for_gathering_finder(
        &client,
        t1,
        "Two misses",
        0.7,
        0.5,
        Some("2026-02-03T00:00:00"),
        Some(2),
    )
    .await;

    let targets = writer
        .find_gathering_finder_targets(10, -90.0, 90.0, -180.0, 180.0)
        .await
        .expect("query failed");
    assert_eq!(
        targets.len(),
        0,
        "miss_count=2 requires 21-day window, only 15 days elapsed"
    );

    // Now try with miss_count=1 — needs 14 days, 15 days elapsed, should be found
    let t2 = Uuid::new_v4();
    create_tension_for_gathering_finder(
        &client,
        t2,
        "One miss",
        0.7,
        0.5,
        Some("2026-02-03T00:00:00"),
        Some(1),
    )
    .await;

    let targets = writer
        .find_gathering_finder_targets(10, -90.0, 90.0, -180.0, 180.0)
        .await
        .expect("query failed");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].tension_id, t2);
}

#[tokio::test]
async fn gathering_finder_backoff_resets_on_success() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let t1 = Uuid::new_v4();

    // Start with miss_count=3
    create_tension_for_gathering_finder(
        &client,
        t1,
        "Was cold",
        0.7,
        0.5,
        Some("2026-01-01T00:00:00"),
        Some(3),
    )
    .await;

    // Mark as scouted with success — should reset miss_count to 0
    writer
        .mark_gathering_found(t1, true)
        .await
        .expect("mark failed");

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
    create_tension_for_gathering_finder(&client, tension_id, "ICE Fear", 0.7, 0.5, None, None)
        .await;

    let gathering_id = Uuid::new_v4();
    create_signal(
        &client,
        "Gathering",
        gathering_id,
        "Singing Rebellion",
        "https://example.com/singing",
    )
    .await;

    writer
        .create_drawn_to_edge(
            gathering_id,
            tension_id,
            0.9,
            "solidarity through singing",
            "singing",
        )
        .await
        .expect("create_drawn_to_edge failed");

    // Verify DRAWN_TO edge exists with gathering_type
    let q = query(
        "MATCH (e:Gathering {id: $eid})-[rel:DRAWN_TO]->(t:Tension {id: $tid})
         RETURN rel.match_strength AS strength, rel.gathering_type AS gt",
    )
    .param("eid", gathering_id.to_string())
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
    create_tension_for_gathering_finder(
        &client,
        tension_id,
        "Housing Crisis",
        0.7,
        0.5,
        None,
        None,
    )
    .await;

    let aid_id = Uuid::new_v4();
    create_signal(
        &client,
        "Aid",
        aid_id,
        "Tenant Solidarity Fund",
        "https://example.com/fund",
    )
    .await;

    // First: create a regular response edge
    writer
        .create_response_edge(aid_id, tension_id, 0.8, "provides rent assistance")
        .await
        .expect("create_response_edge failed");

    // Then: create a DRAWN_TO edge for the same signal→tension
    // These are now separate edge types, so both should exist
    writer
        .create_drawn_to_edge(
            aid_id,
            tension_id,
            0.9,
            "solidarity fund",
            "solidarity fund",
        )
        .await
        .expect("create_drawn_to_edge failed");

    // Verify RESPONDS_TO edge exists
    let q1 = query(
        "MATCH (g:Aid {id: $gid})-[rel:RESPONDS_TO]->(t:Tension {id: $tid})
         RETURN count(rel) AS edge_count",
    )
    .param("gid", aid_id.to_string())
    .param("tid", tension_id.to_string());

    let mut stream = client.inner().execute(q1).await.unwrap();
    let row = stream.next().await.unwrap().expect("Should have results");
    let resp_count: i64 = row.get("edge_count").unwrap();
    assert_eq!(resp_count, 1, "Should have exactly one RESPONDS_TO edge");

    // Verify DRAWN_TO edge exists separately
    let q2 = query(
        "MATCH (g:Aid {id: $gid})-[rel:DRAWN_TO]->(t:Tension {id: $tid})
         RETURN count(rel) AS edge_count, rel.gathering_type AS gt",
    )
    .param("gid", aid_id.to_string())
    .param("tid", tension_id.to_string());

    let mut stream = client.inner().execute(q2).await.unwrap();
    let row = stream.next().await.unwrap().expect("Should have results");
    let drawn_count: i64 = row.get("edge_count").unwrap();
    let gt: String = row.get("gt").unwrap();
    assert_eq!(drawn_count, 1, "Should have exactly one DRAWN_TO edge");
    assert_eq!(gt, "solidarity fund");
}

#[tokio::test]
async fn get_existing_gathering_signals_filters_by_bbox() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let tension_id = Uuid::new_v4();
    create_tension_for_gathering_finder(
        &client,
        tension_id,
        "Immigration Fear",
        0.7,
        0.5,
        None,
        None,
    )
    .await;

    // Minneapolis gathering (lat 44.9778, lng -93.2650)
    let mpls_event = Uuid::new_v4();
    create_signal_at(
        &client,
        "Gathering",
        mpls_event,
        "Singing Rebellion at Lake Street Church",
        "https://example.com/singing",
        44.9778,
        -93.2650,
    )
    .await;
    writer
        .create_drawn_to_edge(
            mpls_event,
            tension_id,
            0.9,
            "solidarity through singing",
            "singing",
        )
        .await
        .expect("edge failed");

    // NYC gathering (lat 40.7128, lng -74.0060)
    let nyc_event = Uuid::new_v4();
    create_signal_at(
        &client,
        "Gathering",
        nyc_event,
        "Union Square Immigration Vigil",
        "https://example.com/vigil",
        40.7128,
        -74.0060,
    )
    .await;
    writer
        .create_drawn_to_edge(nyc_event, tension_id, 0.85, "solidarity vigil", "vigil")
        .await
        .expect("edge failed");

    // When querying from NYC, should only see the NYC gathering
    let nyc_results = writer
        .get_existing_gathering_signals(tension_id, 40.7128, -74.0060, 50.0)
        .await
        .expect("query failed");
    assert_eq!(
        nyc_results.len(),
        1,
        "Should only see NYC gathering, not Minneapolis"
    );
    assert_eq!(nyc_results[0].title, "Union Square Immigration Vigil");

    // When querying from Minneapolis, should only see the Minneapolis gathering
    let mpls_results = writer
        .get_existing_gathering_signals(tension_id, 44.9778, -93.2650, 50.0)
        .await
        .expect("query failed");
    assert_eq!(
        mpls_results.len(),
        1,
        "Should only see Minneapolis gathering, not NYC"
    );
    assert_eq!(
        mpls_results[0].title,
        "Singing Rebellion at Lake Street Church"
    );
}

// =============================================================================
// Signal Expansion integration tests
// =============================================================================

/// Helper: create an Aid node with implied_queries stored as a native Neo4j list.
async fn create_aid_with_implied_queries(
    client: &GraphClient,
    id: Uuid,
    title: &str,
    implied_queries: &[&str],
) {
    let now = neo4j_dt(&Utc::now());
    let emb = dummy_embedding();
    let cypher = format!(
        "CREATE (g:Aid {{
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
            source_url: 'https://example.com/give',
            extracted_at: datetime($now),
            last_confirmed_active: datetime($now),
            location_name: 'Minneapolis',
            action_url: '',
            availability: '',
            is_ongoing: true,
            implied_queries: $queries,
            lat: 44.9778,
            lng: -93.2650,
            embedding: {emb}
        }})"
    );

    let queries: Vec<String> = implied_queries.iter().map(|s| s.to_string()).collect();
    let q = query(&cypher)
        .param("id", id.to_string())
        .param("title", title)
        .param("summary", format!("Test aid: {title}"))
        .param("now", now)
        .param("queries", queries);

    client
        .inner()
        .run(q)
        .await
        .expect("Failed to create aid with implied_queries");
}

/// Helper: create a WebQuery source node.
async fn create_web_query_source(
    client: &GraphClient,
    _region: &str,
    query_text: &str,
    active: bool,
) {
    let now = neo4j_dt(&Utc::now());
    let id = Uuid::new_v4();
    let q = query(
        "CREATE (s:Source {
            id: $id,
            canonical_key: $key,
            canonical_value: $query,
            discovery_method: 'curated',
            created_at: datetime($now),
            signals_produced: 0,
            signals_corroborated: 0,
            consecutive_empty_runs: 0,
            active: $active,
            weight: 0.5,
            avg_signals_per_scrape: 0.0,
            quality_penalty: 1.0,
            source_role: 'mixed',
            scrape_count: 0
        })",
    )
    .param("id", id.to_string())
    .param("key", query_text)
    .param("query", query_text)
    .param("now", now)
    .param("active", active);

    client
        .inner()
        .run(q)
        .await
        .expect("Failed to create web query source");
}

// ---------------------------------------------------------------------------
// Test: get_tension_response_shape returns correct breakdown
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tension_response_shape_correct_breakdown() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    // Create a high-heat tension
    let tension_id = Uuid::new_v4();
    create_tension_for_response_finder(
        &client,
        tension_id,
        "Immigration Enforcement Fear",
        0.7,
        None,
    )
    .await;
    // Bump cause_heat above 0.1 (create_tension_for_response_finder sets 0.5)

    // Create 2 Gives and 1 Need linked via RESPONDS_TO
    let aid1_id = Uuid::new_v4();
    let aid2_id = Uuid::new_v4();
    let need_id = Uuid::new_v4();
    create_signal(
        &client,
        "Aid",
        aid1_id,
        "ILCM Legal Clinic",
        "https://example.com/ilcm",
    )
    .await;
    create_signal(
        &client,
        "Aid",
        aid2_id,
        "Emergency Bail Fund",
        "https://example.com/bail",
    )
    .await;
    create_signal(
        &client,
        "Need",
        need_id,
        "Volunteer interpreters needed",
        "https://example.com/interpreters",
    )
    .await;

    for (sig_id, label) in [(aid1_id, "Aid"), (aid2_id, "Aid"), (need_id, "Need")] {
        let cypher = format!(
            "MATCH (s:{label} {{id: $sid}}), (t:Tension {{id: $tid}})
             CREATE (s)-[:RESPONDS_TO {{match_strength: 0.8, explanation: 'test'}}]->(t)"
        );
        let q = query(&cypher)
            .param("sid", sig_id.to_string())
            .param("tid", tension_id.to_string());
        client.inner().run(q).await.expect("edge creation failed");
    }

    let shapes = writer
        .get_tension_response_shape(10)
        .await
        .expect("query failed");
    assert_eq!(shapes.len(), 1, "Should have 1 tension with responses");

    let shape = &shapes[0];
    assert_eq!(shape.title, "Immigration Enforcement Fear");
    assert_eq!(shape.aid_count, 2, "Should have 2 Aid responses");
    assert_eq!(
        shape.gathering_count, 0,
        "Should have 0 Gathering responses"
    );
    assert_eq!(shape.need_count, 1, "Should have 1 Need response");
    assert!(
        shape.cause_heat >= 0.1,
        "Tension should have heat above threshold"
    );
    assert_eq!(shape.sample_titles.len(), 3, "Should have 3 sample titles");
    assert!(shape
        .sample_titles
        .contains(&"ILCM Legal Clinic".to_string()));
    assert!(shape
        .sample_titles
        .contains(&"Emergency Bail Fund".to_string()));
    assert!(shape
        .sample_titles
        .contains(&"Volunteer interpreters needed".to_string()));
}

// ---------------------------------------------------------------------------
// Test: get_tension_response_shape filters by heat and confidence
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tension_response_shape_filters_low_heat_and_confidence() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    // Hot tension with responses — should appear
    let hot_id = Uuid::new_v4();
    create_tension_for_response_finder(&client, hot_id, "Hot Tension", 0.7, None).await;
    let aid_id = Uuid::new_v4();
    create_signal(
        &client,
        "Aid",
        aid_id,
        "Some Service",
        "https://example.com/svc",
    )
    .await;
    let q = query(
        "MATCH (g:Aid {id: $gid}), (t:Tension {id: $tid})
         CREATE (g)-[:RESPONDS_TO {match_strength: 0.8, explanation: 'test'}]->(t)",
    )
    .param("gid", aid_id.to_string())
    .param("tid", hot_id.to_string());
    client.inner().run(q).await.expect("edge failed");

    // Cold tension (cause_heat = 0.0) with responses — should NOT appear
    let cold_id = Uuid::new_v4();
    create_tension_for_gathering_finder(&client, cold_id, "Cold Tension", 0.7, 0.0, None, None)
        .await;
    let aid2_id = Uuid::new_v4();
    create_signal(
        &client,
        "Aid",
        aid2_id,
        "Cold Service",
        "https://example.com/cold",
    )
    .await;
    let q = query(
        "MATCH (g:Aid {id: $gid}), (t:Tension {id: $tid})
         CREATE (g)-[:RESPONDS_TO {match_strength: 0.8, explanation: 'test'}]->(t)",
    )
    .param("gid", aid2_id.to_string())
    .param("tid", cold_id.to_string());
    client.inner().run(q).await.expect("edge failed");

    // Low confidence tension (0.3, below 0.5 threshold) — should NOT appear
    let low_conf_id = Uuid::new_v4();
    let now = neo4j_dt(&Utc::now());
    let emb = dummy_embedding();
    let cypher = format!(
        "CREATE (t:Tension {{
            id: $id, title: 'Low Confidence', summary: 'test',
            sensitivity: 'general', confidence: 0.3, freshness_score: 1.0,
            corroboration_count: 0, source_diversity: 1, external_ratio: 1.0,
            cause_heat: 0.5, source_url: 'https://example.com',
            extracted_at: datetime($now), last_confirmed_active: datetime($now),
            location_name: 'Minneapolis', severity: 'high', category: 'safety',
            what_would_help: 'resources', lat: 44.9778, lng: -93.2650,
            embedding: {emb}
        }})"
    );
    let q = query(&cypher)
        .param("id", low_conf_id.to_string())
        .param("now", now);
    client.inner().run(q).await.expect("create tension");
    let give3_id = Uuid::new_v4();
    create_signal(
        &client,
        "Aid",
        give3_id,
        "Low Conf Service",
        "https://example.com/lowconf",
    )
    .await;
    let q = query(
        "MATCH (g:Aid {id: $gid}), (t:Tension {id: $tid})
         CREATE (g)-[:RESPONDS_TO {match_strength: 0.8, explanation: 'test'}]->(t)",
    )
    .param("gid", give3_id.to_string())
    .param("tid", low_conf_id.to_string());
    client.inner().run(q).await.expect("edge failed");

    let shapes = writer
        .get_tension_response_shape(10)
        .await
        .expect("query failed");
    assert_eq!(
        shapes.len(),
        1,
        "Only the hot, high-confidence tension should appear"
    );
    assert_eq!(shapes[0].title, "Hot Tension");
}

// ---------------------------------------------------------------------------
// Test: get_tension_response_shape excludes tensions with zero responses
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tension_response_shape_excludes_zero_responses() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    // Hot tension with NO responses — should NOT appear
    let t1 = Uuid::new_v4();
    create_tension_for_response_finder(&client, t1, "No Responses", 0.7, None).await;

    let shapes = writer
        .get_tension_response_shape(10)
        .await
        .expect("query failed");
    assert_eq!(
        shapes.len(),
        0,
        "Tension with no responses should not appear in response shape"
    );
}

// ---------------------------------------------------------------------------
// Test: get_recently_linked_signals_with_queries collects and clears queries
// ---------------------------------------------------------------------------

#[tokio::test]
async fn recently_linked_signals_collects_and_clears_queries() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    // Create a heated tension
    let tension_id = Uuid::new_v4();
    create_tension_for_gathering_finder(
        &client,
        tension_id,
        "Housing Crisis",
        0.7,
        0.5,
        None,
        None,
    )
    .await;

    // Create an Aid with implied_queries
    let aid_id = Uuid::new_v4();
    create_aid_with_implied_queries(
        &client,
        aid_id,
        "Rent Assistance Program",
        &[
            "emergency housing Minneapolis",
            "tenant legal aid Minneapolis",
        ],
    )
    .await;

    // Link Aid → Tension via RESPONDS_TO
    let q = query(
        "MATCH (g:Aid {id: $gid}), (t:Tension {id: $tid})
         CREATE (g)-[:RESPONDS_TO {match_strength: 0.8, explanation: 'provides rent help'}]->(t)",
    )
    .param("gid", aid_id.to_string())
    .param("tid", tension_id.to_string());
    client.inner().run(q).await.expect("edge creation failed");

    // First call: should collect the queries
    let queries = writer
        .get_recently_linked_signals_with_queries()
        .await
        .expect("query failed");
    assert_eq!(queries.len(), 2, "Should collect 2 implied queries");
    assert!(queries.contains(&"emergency housing Minneapolis".to_string()));
    assert!(queries.contains(&"tenant legal aid Minneapolis".to_string()));

    // Second call: should return empty — queries were cleared after first collection
    let queries_again = writer
        .get_recently_linked_signals_with_queries()
        .await
        .expect("query failed");
    assert_eq!(
        queries_again.len(),
        0,
        "Queries should be cleared after collection"
    );

    // Verify the property is null on the node
    let q = query(
        "MATCH (g:Aid {id: $id})
         RETURN g.implied_queries IS NULL AS is_null",
    )
    .param("id", aid_id.to_string());
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream error").expect("no row");
    let is_null: bool = row.get("is_null").expect("no is_null");
    assert!(is_null, "implied_queries should be null after collection");
}

// ---------------------------------------------------------------------------
// Test: get_recently_linked_signals ignores cold tensions (heat < 0.1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn recently_linked_signals_ignores_cold_tensions() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    // Create a COLD tension (heat = 0.0)
    let tension_id = Uuid::new_v4();
    create_tension_for_gathering_finder(&client, tension_id, "Cold Tension", 0.7, 0.0, None, None)
        .await;

    // Create an Aid with implied_queries linked to the cold tension
    let aid_id = Uuid::new_v4();
    create_aid_with_implied_queries(
        &client,
        aid_id,
        "Some Service",
        &["should not be collected"],
    )
    .await;

    let q = query(
        "MATCH (g:Aid {id: $gid}), (t:Tension {id: $tid})
         CREATE (g)-[:RESPONDS_TO {match_strength: 0.8, explanation: 'test'}]->(t)",
    )
    .param("gid", aid_id.to_string())
    .param("tid", tension_id.to_string());
    client.inner().run(q).await.expect("edge failed");

    let queries = writer
        .get_recently_linked_signals_with_queries()
        .await
        .expect("query failed");
    assert_eq!(
        queries.len(),
        0,
        "Should not collect queries from signals linked to cold tensions"
    );
}

// ---------------------------------------------------------------------------
// Test: get_recently_linked_signals works with DRAWN_TO edges (gravity)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn recently_linked_signals_works_with_drawn_to() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    // Create a heated tension
    let tension_id = Uuid::new_v4();
    create_tension_for_gathering_finder(&client, tension_id, "ICE Fear", 0.7, 0.5, None, None)
        .await;

    // Create a Gathering with implied_queries
    let now = neo4j_dt(&Utc::now());
    let emb = dummy_embedding();
    let gathering_id = Uuid::new_v4();
    let cypher = format!(
        "CREATE (e:Gathering {{
            id: $id, title: 'Solidarity Vigil', summary: 'test',
            sensitivity: 'general', confidence: 0.8, freshness_score: 0.8,
            corroboration_count: 0, source_diversity: 1, external_ratio: 0.0,
            cause_heat: 0.0, source_url: 'https://example.com/vigil',
            extracted_at: datetime($now), last_confirmed_active: datetime($now),
            location_name: 'Minneapolis', starts_at: null, ends_at: null,
            action_url: '', organizer: '', is_recurring: false,
            implied_queries: $queries,
            lat: 44.9778, lng: -93.2650,
            embedding: {emb}
        }})"
    );
    let queries = vec![
        "immigration vigil Minneapolis".to_string(),
        "ICE response events".to_string(),
    ];
    let q = query(&cypher)
        .param("id", gathering_id.to_string())
        .param("now", now)
        .param("queries", queries);
    client.inner().run(q).await.expect("create gathering");

    // Link via DRAWN_TO (gravity scout edge)
    writer
        .create_drawn_to_edge(gathering_id, tension_id, 0.85, "solidarity", "vigil")
        .await
        .expect("create_drawn_to failed");

    let collected = writer
        .get_recently_linked_signals_with_queries()
        .await
        .expect("query failed");
    assert_eq!(
        collected.len(),
        2,
        "Should collect queries from DRAWN_TO-linked gatherings"
    );
    assert!(collected.contains(&"immigration vigil Minneapolis".to_string()));
}

// ---------------------------------------------------------------------------
// Test: get_active_web_queries returns active queries only
// ---------------------------------------------------------------------------

#[tokio::test]
async fn active_web_queries_filters_inactive() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    create_web_query_source(
        &client,
        "twincities",
        "immigration services Minneapolis",
        true,
    )
    .await;
    create_web_query_source(
        &client,
        "twincities",
        "food shelf volunteer Minneapolis",
        true,
    )
    .await;
    create_web_query_source(&client, "twincities", "deactivated old query", false).await;
    create_web_query_source(&client, "nyc", "immigration services NYC", true).await;

    let queries = writer.get_active_web_queries().await.expect("query failed");
    assert_eq!(
        queries.len(),
        3,
        "Should find 3 active queries (2 twincities + 1 nyc), got {}",
        queries.len()
    );
    assert!(queries.contains(&"immigration services Minneapolis".to_string()));
    assert!(queries.contains(&"food shelf volunteer Minneapolis".to_string()));
    assert!(queries.contains(&"immigration services NYC".to_string()));
    assert!(
        !queries.iter().any(|q| q.contains("deactivated")),
        "Should not include inactive queries"
    );
    assert!(
        !queries.iter().any(|q| q.contains("deactivated")),
        "Should not include inactive queries from any region"
    );
}

// ---------------------------------------------------------------------------
// Test: implied_queries round-trip through Neo4j as native List<String>
// ---------------------------------------------------------------------------

#[tokio::test]
async fn implied_queries_round_trip_neo4j() {
    let (_container, client) = setup().await;

    let aid_id = Uuid::new_v4();
    create_aid_with_implied_queries(
        &client,
        aid_id,
        "Test Aid",
        &["query one", "query two", "query three"],
    )
    .await;

    // Read back as native list
    let q = query(
        "MATCH (g:Aid {id: $id})
         RETURN g.implied_queries AS queries, size(g.implied_queries) AS len",
    )
    .param("id", aid_id.to_string());

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream error").expect("no row");
    let queries: Vec<String> = row.get("queries").expect("no queries");
    let len: i64 = row.get("len").expect("no len");

    assert_eq!(len, 3, "Should store 3 queries");
    assert_eq!(queries, vec!["query one", "query two", "query three"]);
}

// ---------------------------------------------------------------------------
// Test: empty implied_queries stored as null (not empty list)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn empty_implied_queries_stored_as_null() {
    let (_container, client) = setup().await;

    // Create an Aid with empty implied_queries via Cypher CASE pattern
    let aid_id = Uuid::new_v4();
    let now = neo4j_dt(&Utc::now());
    let emb = dummy_embedding();
    let cypher = format!(
        "CREATE (g:Aid {{
            id: $id, title: 'No Queries Aid', summary: 'test',
            sensitivity: 'general', confidence: 0.8, freshness_score: 0.8,
            corroboration_count: 0, source_diversity: 1, external_ratio: 0.0,
            cause_heat: 0.0, source_url: 'https://example.com',
            extracted_at: datetime($now), last_confirmed_active: datetime($now),
            location_name: '', action_url: '', availability: '', is_ongoing: true,
            implied_queries: CASE WHEN size($queries) > 0 THEN $queries ELSE null END,
            lat: 44.9778, lng: -93.2650,
            embedding: {emb}
        }})"
    );
    let empty_queries: Vec<String> = vec![];
    let q = query(&cypher)
        .param("id", aid_id.to_string())
        .param("now", now)
        .param("queries", empty_queries);
    client.inner().run(q).await.expect("create aid");

    let q = query(
        "MATCH (g:Aid {id: $id})
         RETURN g.implied_queries IS NULL AS is_null",
    )
    .param("id", aid_id.to_string());
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream error").expect("no row");
    let is_null: bool = row.get("is_null").expect("no is_null");
    assert!(
        is_null,
        "Empty implied_queries should be stored as null, not empty list"
    );
}

// ---------------------------------------------------------------------------
// Test: Signal expansion source creation via upsert_source
// ---------------------------------------------------------------------------

#[tokio::test]
async fn signal_expansion_source_created_with_correct_method() {
    let (_container, client) = setup().await;
    let pipeline = Pipeline::new(client.clone(), 0.3);

    let source_id = Uuid::new_v4();
    let canonical_key = "twincities:web_query:emergency housing Minneapolis";

    let events = vec![stored(
        1,
        &Event::System(SystemEvent::SourceRegistered {
            source_id,
            canonical_key: canonical_key.into(),
            canonical_value: "emergency housing Minneapolis".into(),
            url: None,
            discovery_method: DiscoveryMethod::SignalExpansion,
            weight: 0.5,
            source_role: SourceRole::Mixed,
            gap_context: Some("Expanded from: Emergency bail fund for detained immigrants".into()),
        }),
    )];

    pipeline
        .process(&events, &tc_bbox(), &[])
        .await
        .expect("pipeline failed");

    // Verify the source was created with SignalExpansion discovery method
    let q = query(
        "MATCH (s:Source {canonical_key: $key})
         RETURN s.discovery_method AS dm, s.gap_context AS gc, s.active AS active",
    )
    .param("key", canonical_key);

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream error").expect("no row");
    let dm: String = row.get("dm").expect("no dm");
    let gc: String = row.get("gc").expect("no gc");
    let active: bool = row.get("active").expect("no active");

    assert_eq!(
        dm, "signal_expansion",
        "Discovery method should be signal_expansion"
    );
    assert!(
        gc.contains("Emergency bail fund"),
        "Gap context should reference originating signal"
    );
    assert!(active, "Source should be active");
}

// =============================================================================
// Resource Capability Matching tests
// =============================================================================

/// Helper: create a 1024-dim f32 embedding with a specific value in the first slot.
fn make_embedding(first: f32) -> Vec<f32> {
    let mut emb = vec![0.0f32; 1024];
    emb[0] = first;
    emb
}

/// Helper: create a similar embedding (high cosine similarity to make_embedding(first)).
fn make_similar_embedding(first: f32) -> Vec<f32> {
    let mut emb = vec![0.0f32; 1024];
    emb[0] = first;
    emb[1] = 0.05; // small perturbation → high similarity
    emb
}

/// Helper: create a dissimilar embedding (low cosine similarity).
fn make_dissimilar_embedding() -> Vec<f32> {
    let mut emb = vec![0.0f32; 1024];
    emb[500] = 1.0; // orthogonal direction
    emb
}

#[tokio::test]
async fn resource_find_or_create_is_idempotent() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let emb = make_embedding(0.5);
    let id1 = writer
        .find_or_create_resource("Vehicle", "vehicle", "A car or truck", &emb)
        .await
        .expect("first create failed");

    let id2 = writer
        .find_or_create_resource("Vehicle", "vehicle", "A car or truck", &emb)
        .await
        .expect("second create failed");

    assert_eq!(id1, id2, "Same slug should return same UUID");

    // signal_count should be 2 after two calls
    let q = query("MATCH (r:Resource {slug: 'vehicle'}) RETURN r.signal_count AS sc");
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream err").expect("no row");
    let sc: i64 = row.get("sc").expect("no sc");
    assert_eq!(sc, 2, "signal_count should increment on each MERGE");
}

#[tokio::test]
async fn resource_find_by_slug() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let emb = make_embedding(0.3);
    let created_id = writer
        .find_or_create_resource("Food", "food", "Food assistance", &emb)
        .await
        .expect("create failed");

    let found = writer
        .find_resource_by_slug("food")
        .await
        .expect("lookup failed");

    assert_eq!(found, Some(created_id), "Should find resource by slug");

    let not_found = writer
        .find_resource_by_slug("nonexistent")
        .await
        .expect("lookup failed");

    assert_eq!(not_found, None, "Should return None for unknown slug");
}

#[tokio::test]
async fn resource_find_by_embedding_similarity() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let emb = make_embedding(0.7);
    writer
        .find_or_create_resource("Legal Expertise", "legal-expertise", "Legal aid", &emb)
        .await
        .expect("create failed");

    // Similar embedding should match above threshold
    let similar = make_similar_embedding(0.7);
    let found = writer
        .find_resource_by_embedding(&similar, 0.85)
        .await
        .expect("search failed");

    assert!(found.is_some(), "Should find similar resource");
    let (_, sim) = found.unwrap();
    assert!(sim >= 0.85, "Similarity should be >= 0.85, got {sim}");

    // Dissimilar embedding should NOT match
    let dissimilar = make_dissimilar_embedding();
    let not_found = writer
        .find_resource_by_embedding(&dissimilar, 0.85)
        .await
        .expect("search failed");

    assert!(not_found.is_none(), "Should not find dissimilar resource");
}

#[tokio::test]
async fn resource_requires_edge_creation() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    // Create a Need signal
    let need_id = Uuid::new_v4();
    create_signal(
        &client,
        "Need",
        need_id,
        "Need drivers for food delivery",
        "https://test.com/ask1",
    )
    .await;

    // Create a Resource
    let emb = make_embedding(0.4);
    let resource_id = writer
        .find_or_create_resource("Vehicle", "vehicle", "A car or truck", &emb)
        .await
        .expect("create failed");

    // Create REQUIRES edge
    writer
        .create_requires_edge(
            need_id,
            resource_id,
            0.9,
            Some("Saturday mornings"),
            Some("10 volunteers"),
        )
        .await
        .expect("edge creation failed");

    // Verify edge exists with properties
    let q = query(
        "MATCH (s:Need {id: $sid})-[e:REQUIRES]->(r:Resource {id: $rid})
         RETURN e.confidence AS conf, e.quantity AS qty, e.notes AS notes",
    )
    .param("sid", need_id.to_string())
    .param("rid", resource_id.to_string());

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream
        .next()
        .await
        .expect("stream err")
        .expect("no REQUIRES edge found");
    let conf: f64 = row.get("conf").expect("no confidence");
    let qty: String = row.get("qty").expect("no quantity");
    let notes: String = row.get("notes").expect("no notes");

    assert!((conf - 0.9).abs() < 0.01, "Confidence should be 0.9");
    assert_eq!(qty, "Saturday mornings");
    assert_eq!(notes, "10 volunteers");
}

#[tokio::test]
async fn resource_prefers_edge_creation() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let need_id = Uuid::new_v4();
    create_signal(
        &client,
        "Need",
        need_id,
        "Court date transport",
        "https://test.com/ask2",
    )
    .await;

    let emb = make_embedding(0.6);
    let resource_id = writer
        .find_or_create_resource(
            "Bilingual Spanish",
            "bilingual-spanish",
            "Spanish speaker",
            &emb,
        )
        .await
        .expect("create failed");

    writer
        .create_prefers_edge(need_id, resource_id, 0.7)
        .await
        .expect("edge creation failed");

    let q = query(
        "MATCH (s:Need {id: $sid})-[e:PREFERS]->(r:Resource {id: $rid})
         RETURN e.confidence AS conf",
    )
    .param("sid", need_id.to_string())
    .param("rid", resource_id.to_string());

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream
        .next()
        .await
        .expect("stream err")
        .expect("no PREFERS edge found");
    let conf: f64 = row.get("conf").expect("no confidence");
    assert!((conf - 0.7).abs() < 0.01, "Confidence should be 0.7");
}

#[tokio::test]
async fn resource_offers_edge_creation() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let aid_id = Uuid::new_v4();
    create_signal(
        &client,
        "Aid",
        aid_id,
        "Emergency food pantry",
        "https://test.com/aid1",
    )
    .await;

    let emb = make_embedding(0.3);
    let resource_id = writer
        .find_or_create_resource("Food", "food", "Food assistance", &emb)
        .await
        .expect("create failed");

    writer
        .create_offers_edge(aid_id, resource_id, 0.95, Some("Mon-Fri 9-5"))
        .await
        .expect("edge creation failed");

    let q = query(
        "MATCH (s:Aid {id: $sid})-[e:OFFERS]->(r:Resource {id: $rid})
         RETURN e.confidence AS conf, e.capacity AS cap",
    )
    .param("sid", aid_id.to_string())
    .param("rid", resource_id.to_string());

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream
        .next()
        .await
        .expect("stream err")
        .expect("no OFFERS edge found");
    let conf: f64 = row.get("conf").expect("no confidence");
    let cap: String = row.get("cap").expect("no capacity");

    assert!((conf - 0.95).abs() < 0.01, "Confidence should be 0.95");
    assert_eq!(cap, "Mon-Fri 9-5");
}

#[tokio::test]
async fn resource_edges_are_idempotent() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let need_id = Uuid::new_v4();
    create_signal(
        &client,
        "Need",
        need_id,
        "Need volunteers",
        "https://test.com/ask3",
    )
    .await;

    let emb = make_embedding(0.2);
    let resource_id = writer
        .find_or_create_resource("Physical Labor", "physical-labor", "Manual work", &emb)
        .await
        .expect("create failed");

    // Create same REQUIRES edge twice
    writer
        .create_requires_edge(need_id, resource_id, 0.8, None, None)
        .await
        .expect("first edge failed");
    writer
        .create_requires_edge(need_id, resource_id, 0.9, Some("updated"), None)
        .await
        .expect("second edge failed");

    // Should have exactly ONE edge (MERGE), with updated confidence
    let q = query(
        "MATCH (s:Need {id: $sid})-[e:REQUIRES]->(r:Resource {id: $rid})
         RETURN count(e) AS edge_count, e.confidence AS conf",
    )
    .param("sid", need_id.to_string())
    .param("rid", resource_id.to_string());

    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream err").expect("no row");
    let count: i64 = row.get("edge_count").expect("no count");
    let conf: f64 = row.get("conf").expect("no conf");

    assert_eq!(count, 1, "MERGE should create only one edge");
    assert!(
        (conf - 0.9).abs() < 0.01,
        "Confidence should be updated to 0.9"
    );
}

#[tokio::test]
async fn find_needs_by_single_resource() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());
    let reader = rootsignal_graph::PublicGraphReader::new(client.clone());

    // Create two Needs and one Aid, all needing "vehicle"
    let need1 = Uuid::new_v4();
    let need2 = Uuid::new_v4();
    let aid1 = Uuid::new_v4();
    create_signal(
        &client,
        "Need",
        need1,
        "Deliver meals to elderly",
        "https://test.com/a1",
    )
    .await;
    create_signal(
        &client,
        "Need",
        need2,
        "Drive kids to camp",
        "https://test.com/a2",
    )
    .await;
    create_signal(
        &client,
        "Aid",
        aid1,
        "Free car service",
        "https://test.com/g1",
    )
    .await;

    let emb = make_embedding(0.5);
    let vehicle_id = writer
        .find_or_create_resource("Vehicle", "vehicle", "Car or truck", &emb)
        .await
        .expect("create failed");

    writer
        .create_requires_edge(need1, vehicle_id, 0.9, None, None)
        .await
        .unwrap();
    writer
        .create_requires_edge(need2, vehicle_id, 0.85, None, None)
        .await
        .unwrap();
    writer
        .create_offers_edge(aid1, vehicle_id, 0.9, None)
        .await
        .unwrap();

    // Query: "I have a car" — should find Needs, not Gives
    let matches = reader
        .find_needs_by_resource("vehicle", 44.9778, -93.2650, 50.0, 50)
        .await
        .expect("query failed");

    assert_eq!(matches.len(), 2, "Should find 2 Needs requiring vehicle");
    for m in &matches {
        assert!(m.score > 0.0, "Score should be positive");
        assert!(
            m.matched_requires.contains(&"vehicle".to_string()),
            "Should match vehicle"
        );
    }
}

#[tokio::test]
async fn find_aids_by_resource() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());
    let reader = rootsignal_graph::PublicGraphReader::new(client.clone());

    let aid1 = Uuid::new_v4();
    let aid2 = Uuid::new_v4();
    create_signal(
        &client,
        "Aid",
        aid1,
        "Food shelf downtown",
        "https://test.com/g1",
    )
    .await;
    create_signal(
        &client,
        "Aid",
        aid2,
        "Free grocery delivery",
        "https://test.com/g2",
    )
    .await;

    let emb = make_embedding(0.3);
    let food_id = writer
        .find_or_create_resource("Food", "food", "Food assistance", &emb)
        .await
        .expect("create failed");

    writer
        .create_offers_edge(aid1, food_id, 0.9, Some("Mon-Fri"))
        .await
        .unwrap();
    writer
        .create_offers_edge(aid2, food_id, 0.85, None)
        .await
        .unwrap();

    // Query: "I need food" — should find Aids
    let matches = reader
        .find_aids_by_resource("food", 44.9778, -93.2650, 50.0, 50)
        .await
        .expect("query failed");

    assert_eq!(matches.len(), 2, "Should find 2 Aids offering food");
    for m in &matches {
        assert_eq!(m.score, 1.0, "Aid matches should have score 1.0");
    }
}

#[tokio::test]
async fn multi_resource_fuzzy_and_scoring() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());
    let reader = rootsignal_graph::PublicGraphReader::new(client.clone());

    // Create resources
    let emb_v = make_embedding(0.5);
    let emb_s = make_embedding(0.6);
    let vehicle_id = writer
        .find_or_create_resource("Vehicle", "vehicle", "", &emb_v)
        .await
        .unwrap();
    let spanish_id = writer
        .find_or_create_resource("Bilingual Spanish", "bilingual-spanish", "", &emb_s)
        .await
        .unwrap();

    // Need 1: Requires(vehicle) + Requires(bilingual-spanish) — full match for car+Spanish person
    let need1 = Uuid::new_v4();
    create_signal(
        &client,
        "Need",
        need1,
        "Court date transport (bilingual)",
        "https://test.com/a1",
    )
    .await;
    writer
        .create_requires_edge(need1, vehicle_id, 0.9, None, None)
        .await
        .unwrap();
    writer
        .create_requires_edge(need1, spanish_id, 0.85, None, None)
        .await
        .unwrap();

    // Need 2: Requires(vehicle) only — partial match for car+Spanish person
    let need2 = Uuid::new_v4();
    create_signal(
        &client,
        "Need",
        need2,
        "Meal delivery drivers",
        "https://test.com/a2",
    )
    .await;
    writer
        .create_requires_edge(need2, vehicle_id, 0.9, None, None)
        .await
        .unwrap();

    // Need 3: Requires(vehicle) + Prefers(bilingual-spanish) — full Requires match + Prefers bonus
    let need3 = Uuid::new_v4();
    create_signal(
        &client,
        "Need",
        need3,
        "Transport to ICE check-in",
        "https://test.com/a3",
    )
    .await;
    writer
        .create_requires_edge(need3, vehicle_id, 0.9, None, None)
        .await
        .unwrap();
    writer
        .create_prefers_edge(need3, spanish_id, 0.7)
        .await
        .unwrap();

    // Query: "I have a car AND speak Spanish"
    let matches = reader
        .find_needs_by_resources(
            &["vehicle".to_string(), "bilingual-spanish".to_string()],
            44.9778,
            -93.2650,
            50.0,
            50,
        )
        .await
        .expect("query failed");

    assert_eq!(matches.len(), 3, "Should find all 3 Needs");

    // Find each by title to check scores
    let need1_match = matches
        .iter()
        .find(|m| m.node.title() == "Court date transport (bilingual)")
        .expect("need1 not found");
    let need2_match = matches
        .iter()
        .find(|m| m.node.title() == "Meal delivery drivers")
        .expect("need2 not found");
    let need3_match = matches
        .iter()
        .find(|m| m.node.title() == "Transport to ICE check-in")
        .expect("need3 not found");

    // Need 1: 2/2 Requires matched = 1.0
    assert!(
        (need1_match.score - 1.0).abs() < 0.01,
        "Need1 score should be 1.0, got {}",
        need1_match.score
    );
    assert!(
        need1_match.unmatched_requires.is_empty(),
        "Need1 should have no unmatched requires"
    );

    // Need 2: 1/1 Requires matched = 1.0 (vehicle is the only requirement)
    assert!(
        (need2_match.score - 1.0).abs() < 0.01,
        "Need2 score should be 1.0, got {}",
        need2_match.score
    );

    // Need 3: 1/1 Requires matched = 1.0, +0.2 for Prefers match = 1.2
    assert!(
        (need3_match.score - 1.2).abs() < 0.01,
        "Need3 score should be 1.2, got {}",
        need3_match.score
    );

    // Results should be sorted by score descending
    assert!(
        matches[0].score >= matches[1].score,
        "Should be sorted by score desc"
    );
}

#[tokio::test]
async fn list_resources_sorted_by_signal_count() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());
    let reader = rootsignal_graph::PublicGraphReader::new(client.clone());

    let emb1 = make_embedding(0.1);
    let emb2 = make_embedding(0.2);

    // Create "food" with 3 signal_count bumps
    writer
        .find_or_create_resource("Food", "food", "", &emb1)
        .await
        .unwrap();
    writer
        .find_or_create_resource("Food", "food", "", &emb1)
        .await
        .unwrap();
    writer
        .find_or_create_resource("Food", "food", "", &emb1)
        .await
        .unwrap();

    // Create "vehicle" with 1 signal_count
    writer
        .find_or_create_resource("Vehicle", "vehicle", "", &emb2)
        .await
        .unwrap();

    let resources = reader.list_resources(10).await.expect("list failed");
    assert_eq!(resources.len(), 2);
    assert_eq!(
        resources[0].slug, "food",
        "Food should be first (highest signal_count)"
    );
    assert_eq!(resources[0].signal_count, 3);
    assert_eq!(resources[1].slug, "vehicle");
    assert_eq!(resources[1].signal_count, 1);
}

#[tokio::test]
async fn resource_gap_analysis_shows_unmet_needs() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());
    let reader = rootsignal_graph::PublicGraphReader::new(client.clone());

    let emb_v = make_embedding(0.5);
    let emb_f = make_embedding(0.3);
    let vehicle_id = writer
        .find_or_create_resource("Vehicle", "vehicle", "", &emb_v)
        .await
        .unwrap();
    let food_id = writer
        .find_or_create_resource("Food", "food", "", &emb_f)
        .await
        .unwrap();

    // 3 Needs require vehicle, 0 Gives offer it → gap = 3
    for i in 0..3 {
        let need = Uuid::new_v4();
        create_signal(
            &client,
            "Need",
            need,
            &format!("Driver needed {i}"),
            &format!("https://test.com/a{i}"),
        )
        .await;
        writer
            .create_requires_edge(need, vehicle_id, 0.9, None, None)
            .await
            .unwrap();
    }

    // 2 Needs require food, 2 Gives offer it → gap = 0
    for i in 0..2 {
        let need = Uuid::new_v4();
        create_signal(
            &client,
            "Need",
            need,
            &format!("Food needed {i}"),
            &format!("https://test.com/fa{i}"),
        )
        .await;
        writer
            .create_requires_edge(need, food_id, 0.9, None, None)
            .await
            .unwrap();
    }
    for i in 0..2 {
        let aid = Uuid::new_v4();
        create_signal(
            &client,
            "Aid",
            aid,
            &format!("Food shelf {i}"),
            &format!("https://test.com/fg{i}"),
        )
        .await;
        writer
            .create_offers_edge(aid, food_id, 0.9, None)
            .await
            .unwrap();
    }

    let gaps = reader
        .resource_gap_analysis()
        .await
        .expect("gap analysis failed");
    assert!(gaps.len() >= 2, "Should have at least 2 resources");

    // Vehicle should be the biggest gap
    let vehicle_gap = gaps
        .iter()
        .find(|g| g.resource_slug == "vehicle")
        .expect("vehicle not found");
    assert_eq!(vehicle_gap.requires_count, 3);
    assert_eq!(vehicle_gap.offers_count, 0);
    assert_eq!(vehicle_gap.gap, 3);

    // Food should be balanced
    let food_gap = gaps
        .iter()
        .find(|g| g.resource_slug == "food")
        .expect("food not found");
    assert_eq!(food_gap.requires_count, 2);
    assert_eq!(food_gap.offers_count, 2);
    assert_eq!(food_gap.gap, 0);

    // Vehicle should appear before food (sorted by gap descending)
    let v_idx = gaps
        .iter()
        .position(|g| g.resource_slug == "vehicle")
        .unwrap();
    let f_idx = gaps.iter().position(|g| g.resource_slug == "food").unwrap();
    assert!(
        v_idx < f_idx,
        "Vehicle (gap=3) should rank before food (gap=0)"
    );
}

#[tokio::test]
async fn consolidate_resources_merges_similar() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    // Create two similar resources (should merge)
    let emb1 = make_embedding(0.8);
    let emb2 = make_similar_embedding(0.8); // high similarity to emb1
    let id1 = writer
        .find_or_create_resource("Vehicle", "vehicle", "Car", &emb1)
        .await
        .unwrap();
    let id2 = writer
        .find_or_create_resource("Car", "car", "Automobile", &emb2)
        .await
        .unwrap();

    // Create a dissimilar resource (should NOT merge)
    let emb3 = make_dissimilar_embedding();
    let id3 = writer
        .find_or_create_resource("Food", "food", "Food assistance", &emb3)
        .await
        .unwrap();

    // Create edges to the duplicate
    let need1 = Uuid::new_v4();
    create_signal(
        &client,
        "Need",
        need1,
        "Need a driver",
        "https://test.com/a1",
    )
    .await;
    writer
        .create_requires_edge(need1, id2, 0.9, Some("weekends"), None)
        .await
        .unwrap();

    let need2 = Uuid::new_v4();
    create_signal(
        &client,
        "Need",
        need2,
        "Need transport",
        "https://test.com/a2",
    )
    .await;
    writer
        .create_requires_edge(need2, id1, 0.85, None, None)
        .await
        .unwrap();

    // Run consolidation
    let stats = writer
        .consolidate_resources(0.85)
        .await
        .expect("consolidation failed");
    assert!(
        stats.clusters_found >= 1,
        "Should find at least 1 merge cluster"
    );
    assert!(stats.nodes_merged >= 1, "Should merge at least 1 node");
    assert!(
        stats.edges_redirected >= 1,
        "Should redirect at least 1 edge"
    );

    // Verify: "car" resource should be deleted
    let q = query("MATCH (r:Resource {slug: 'car'}) RETURN count(r) AS c");
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("err").expect("no row");
    let count: i64 = row.get("c").expect("no c");
    assert_eq!(count, 0, "Duplicate 'car' resource should be deleted");

    // Verify: "vehicle" resource should still exist
    let q = query("MATCH (r:Resource {slug: 'vehicle'}) RETURN r.signal_count AS sc");
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("err").expect("no row");
    let sc: i64 = row.get("sc").expect("no sc");
    assert!(
        sc >= 2,
        "Canonical should have summed signal_count, got {sc}"
    );

    // Verify: "food" resource should still exist (dissimilar, not merged)
    let food_found = writer
        .find_resource_by_slug("food")
        .await
        .expect("lookup failed");
    assert_eq!(food_found, Some(id3), "Food should survive consolidation");

    // Verify: need1's REQUIRES edge now points to vehicle (canonical), not car (deleted)
    let q = query(
        "MATCH (s:Need {id: $sid})-[:REQUIRES]->(r:Resource)
         RETURN r.slug AS slug",
    )
    .param("sid", need1.to_string());
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("err").expect("no row");
    let slug: String = row.get("slug").expect("no slug");
    assert_eq!(
        slug, "vehicle",
        "Edge should be re-pointed to canonical 'vehicle'"
    );
}

#[tokio::test]
async fn consolidate_resources_below_threshold_not_merged() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    // Create two dissimilar resources
    let emb1 = make_embedding(1.0);
    let emb2 = make_dissimilar_embedding();
    writer
        .find_or_create_resource("Vehicle", "vehicle", "Car", &emb1)
        .await
        .unwrap();
    writer
        .find_or_create_resource("Food", "food", "Groceries", &emb2)
        .await
        .unwrap();

    let stats = writer
        .consolidate_resources(0.85)
        .await
        .expect("consolidation failed");
    assert_eq!(
        stats.clusters_found, 0,
        "Dissimilar resources should not merge"
    );
    assert_eq!(stats.nodes_merged, 0);

    // Both should still exist
    let q = query("MATCH (r:Resource) RETURN count(r) AS c");
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("err").expect("no row");
    let count: i64 = row.get("c").expect("no c");
    assert_eq!(count, 2, "Both resources should survive");
}

#[tokio::test]
async fn consolidate_resources_preserves_edge_properties() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    // Create two similar resources
    let emb1 = make_embedding(0.9);
    let emb2 = make_similar_embedding(0.9);
    let _canonical_id = writer
        .find_or_create_resource("Vehicle", "vehicle", "", &emb1)
        .await
        .unwrap();
    let dup_id = writer
        .find_or_create_resource("Car", "car", "", &emb2)
        .await
        .unwrap();

    // Create edges with properties to the duplicate
    let need = Uuid::new_v4();
    create_signal(&client, "Need", need, "Need ride", "https://test.com/r1").await;
    writer
        .create_requires_edge(need, dup_id, 0.88, Some("10 volunteers"), Some("urgent"))
        .await
        .unwrap();

    let aid = Uuid::new_v4();
    create_signal(&client, "Aid", aid, "Free rides", "https://test.com/r2").await;
    writer
        .create_offers_edge(aid, dup_id, 0.92, Some("evenings only"))
        .await
        .unwrap();

    let gathering = Uuid::new_v4();
    create_signal(
        &client,
        "Gathering",
        gathering,
        "Carpool meetup",
        "https://test.com/r3",
    )
    .await;
    writer
        .create_prefers_edge(gathering, dup_id, 0.75)
        .await
        .unwrap();

    // Run consolidation
    writer
        .consolidate_resources(0.85)
        .await
        .expect("consolidation failed");

    // Verify REQUIRES edge properties preserved on canonical
    let q = query(
        "MATCH (s:Need {id: $sid})-[e:REQUIRES]->(r:Resource {slug: 'vehicle'})
         RETURN e.confidence AS conf, e.quantity AS qty, e.notes AS notes",
    )
    .param("sid", need.to_string());
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream
        .next()
        .await
        .expect("err")
        .expect("REQUIRES edge not found on canonical");
    let conf: f64 = row.get("conf").expect("no conf");
    let qty: String = row.get("qty").expect("no qty");
    assert!(
        (conf - 0.88).abs() < 0.01,
        "REQUIRES confidence should be preserved"
    );
    assert_eq!(
        qty, "10 volunteers",
        "REQUIRES quantity should be preserved"
    );

    // Verify OFFERS edge properties preserved
    let q = query(
        "MATCH (s:Aid {id: $sid})-[e:OFFERS]->(r:Resource {slug: 'vehicle'})
         RETURN e.confidence AS conf, e.capacity AS cap",
    )
    .param("sid", aid.to_string());
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream
        .next()
        .await
        .expect("err")
        .expect("OFFERS edge not found on canonical");
    let cap: String = row.get("cap").expect("no cap");
    assert_eq!(cap, "evenings only", "OFFERS capacity should be preserved");

    // Verify PREFERS edge preserved
    let q = query(
        "MATCH (s:Gathering {id: $sid})-[e:PREFERS]->(r:Resource {slug: 'vehicle'})
         RETURN e.confidence AS conf",
    )
    .param("sid", gathering.to_string());
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream
        .next()
        .await
        .expect("err")
        .expect("PREFERS edge not found on canonical");
    let conf: f64 = row.get("conf").expect("no conf");
    assert!(
        (conf - 0.75).abs() < 0.01,
        "PREFERS confidence should be preserved"
    );
}
