//! Integration test: verify that multiple LocationGeocoded events with the same
//! canonical Mapbox address converge to a single Location node in Neo4j.
//!
//! All test nodes use unique prefixed names to avoid colliding with real data.
//!
//! Run with: cargo test -p rootsignal-graph --test location_convergence_test -- --ignored --nocapture

use rootsignal_graph::{connect_graph, query, GraphClient, GraphProjector};
use rootsignal_common::events::{Event, SystemEvent};
use chrono::Utc;
use causal::types::PersistedEvent;
use serde_json::json;
use uuid::Uuid;

fn load_env() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(".env");
    if let Ok(content) = std::fs::read_to_string(&path) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                if std::env::var(key.trim()).is_err() {
                    std::env::set_var(key.trim(), value.trim());
                }
            }
        }
    }
}

async fn connect() -> GraphClient {
    load_env();
    let uri = std::env::var("NEO4J_URI").expect("NEO4J_URI required");
    let user = std::env::var("NEO4J_USER").expect("NEO4J_USER required");
    let password = std::env::var("NEO4J_PASSWORD").expect("NEO4J_PASSWORD required");

    assert!(
        uri.contains("localhost") || uri.contains("127.0.0.1"),
        "Refusing to run integration tests against non-local Neo4j: {uri}"
    );

    let db = std::env::var("NEO4J_DB").unwrap_or_else(|_| "neo4j".to_string());
    connect_graph(&uri, &user, &password, &db)
        .await
        .expect("Failed to connect to Neo4j")
}

fn persisted(event: &Event, position: i64) -> PersistedEvent {
    PersistedEvent {
        position: position as u64,
        event_id: Uuid::new_v4(),
        parent_id: None,
        correlation_id: Uuid::new_v4(),
        event_type: event.event_type().to_string(),
        payload: event.to_payload(),
        created_at: Utc::now(),
        aggregate_type: None,
        aggregate_id: None,
        version: None,
        metadata: {
            let mut m = serde_json::Map::new();
            m.insert("run_id".into(), json!("test-run"));
            m.insert("schema_v".into(), json!(1));
            m.insert("actor".into(), json!("test-actor"));
            m
        },
        ephemeral: None,
        persistent: true,
    }
}

/// Generate a test-unique location name that won't collide with real data.
fn test_loc(tag: &str, name: &str) -> String {
    format!("__test_{tag}_{name}")
}

/// Clean up all test nodes by tag.
async fn cleanup(client: &GraphClient, tag: &str) {
    let prefix = format!("__test_{tag}_");
    // Delete test Gathering nodes
    client.run(
        query("MATCH (n) WHERE n._test_tag = $tag DETACH DELETE n")
            .param("tag", tag),
    ).await.unwrap();
    // Delete test Location nodes by prefix
    client.run(
        query("MATCH (l:Location) WHERE l.normalized_name STARTS WITH $prefix DETACH DELETE l")
            .param("prefix", prefix.as_str()),
    ).await.unwrap();
    // Delete test Location nodes by canonical_address prefix
    client.run(
        query("MATCH (l:Location) WHERE l.canonical_address STARTS WITH $prefix DETACH DELETE l")
            .param("prefix", prefix.as_str()),
    ).await.unwrap();
}

/// Create a Gathering signal node with a stub Location (as WorldEvent projection does today).
async fn create_signal_with_location_stub(
    client: &GraphClient,
    signal_id: &Uuid,
    title: &str,
    location_name: &str,
    edge_type: &str,
    tag: &str,
) {
    let normalized_name = location_name.trim().to_lowercase();
    let q = query(&format!(
        "CREATE (g:Gathering {{id: $id, title: $title, sensitivity: 'general', confidence: 0.5, _test_tag: $tag}})
         MERGE (l:Location {{normalized_name: $normalized_name}})
         ON CREATE SET l.name = $location_name
         CREATE (g)-[:{edge_type}]->(l)",
    ))
    .param("id", signal_id.to_string())
    .param("title", title)
    .param("normalized_name", normalized_name.as_str())
    .param("location_name", location_name)
    .param("tag", tag);

    client.run(q).await.unwrap();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Three signals reference different LLM name variants of the same city.
/// After LocationGeocoded events (all resolving to the same canonical Mapbox address),
/// only ONE Location node should remain, with all three signals connected.
#[tokio::test]
#[ignore]
async fn different_location_names_converge_to_one_canonical_node() {
    let client = connect().await;
    let tag = "conv1";
    cleanup(&client, tag).await;

    let sig1 = Uuid::new_v4();
    let sig2 = Uuid::new_v4();
    let sig3 = Uuid::new_v4();

    let name1 = test_loc(tag, "minneapolis");
    let name2 = test_loc(tag, "north minneapolis");
    let name3 = test_loc(tag, "minneapolis central");

    // WorldEvent projection creates 3 stubs with different names
    create_signal_with_location_stub(&client, &sig1, "Free meals at church", &name1, "HELD_AT", tag).await;
    create_signal_with_location_stub(&client, &sig2, "Winter coat drive", &name2, "HELD_AT", tag).await;
    create_signal_with_location_stub(&client, &sig3, "Community meeting", &name3, "HELD_AT", tag).await;

    // Verify: 3 separate Location stubs exist
    let prefix = format!("__test_{tag}_");
    let mut r = client.execute(
        query("MATCH (l:Location) WHERE l.normalized_name STARTS WITH $prefix RETURN count(l) AS cnt")
            .param("prefix", prefix.as_str()),
    ).await.unwrap();
    let stub_count: i64 = r.next().await.unwrap().unwrap().get("cnt").unwrap();
    assert_eq!(stub_count, 3, "Expected 3 location stubs before geocoding");

    // Project LocationGeocoded for each — all resolve to same Mapbox canonical address
    let canonical_address = test_loc(tag, "Minneapolis, Minnesota, United States");
    let projector = GraphProjector::new(client.clone());

    for (signal_id, location_name) in [
        (sig1, name1.as_str()),
        (sig2, name2.as_str()),
        (sig3, name3.as_str()),
    ] {
        let event = Event::System(SystemEvent::LocationGeocoded {
            signal_id,
            location_name: location_name.to_string(),
            lat: 44.9778,
            lng: -93.2650,
            address: Some(canonical_address.clone()),
            precision: "approximate".to_string(),
            timezone: Some("America/Chicago".to_string()),
            city: None, state: None, country_code: None,
        });

        let plan = projector.plan(&persisted(&event, 100));
        projector.execute(plan).await.unwrap();
    }

    // Assert: exactly 1 Location node remains for these test signals
    let mut r = client.execute(
        query(
            "MATCH (g)-[]->(l:Location)
             WHERE g._test_tag = $tag
             WITH DISTINCT l
             RETURN count(l) AS cnt"
        ).param("tag", tag),
    ).await.unwrap();
    let final_count: i64 = r.next().await.unwrap().unwrap().get("cnt").unwrap();
    assert_eq!(final_count, 1, "All location variants should converge to 1 canonical node");

    // Assert: the canonical node has the Mapbox address and geocoded coordinates
    let mut r = client.execute(
        query(
            "MATCH (g)-[]->(l:Location)
             WHERE g._test_tag = $tag
             WITH DISTINCT l
             RETURN l.canonical_address AS addr, l.lat AS lat, l.lng AS lng,
                    l.precision AS prec, l.timezone AS tz, l.geocoded AS geo"
        ).param("tag", tag),
    ).await.unwrap();
    let row = r.next().await.unwrap().unwrap();
    let addr: String = row.get("addr").unwrap();
    let lat: f64 = row.get("lat").unwrap();
    let prec: String = row.get("prec").unwrap();
    let tz: String = row.get("tz").unwrap();
    let geo: bool = row.get("geo").unwrap();

    assert_eq!(addr, canonical_address);
    assert!((lat - 44.9778).abs() < 0.001);
    assert_eq!(prec, "approximate");
    assert_eq!(tz, "America/Chicago");
    assert!(geo);

    // Assert: all 3 signals are connected to the canonical Location
    let mut r = client.execute(
        query(
            "MATCH (g)-[]->(l:Location)
             WHERE g._test_tag = $tag
             RETURN count(g) AS cnt"
        ).param("tag", tag),
    ).await.unwrap();
    let edge_count: i64 = r.next().await.unwrap().unwrap().get("cnt").unwrap();
    assert_eq!(edge_count, 3, "All 3 signals should be connected to the canonical Location");

    // Assert: the original HELD_AT edge type is preserved
    let mut r = client.execute(
        query(
            "MATCH (g)-[r:HELD_AT]->(l:Location)
             WHERE g._test_tag = $tag
             RETURN count(r) AS cnt"
        ).param("tag", tag),
    ).await.unwrap();
    let held_at_count: i64 = r.next().await.unwrap().unwrap().get("cnt").unwrap();
    assert_eq!(held_at_count, 3, "HELD_AT edge type should be preserved through merge");

    cleanup(&client, tag).await;
    println!("  ✓ 3 location variants converged to 1 canonical node with all edges preserved");
}

/// Two different places that happen to have the same input name but geocode to different
/// addresses should remain as separate Location nodes.
#[tokio::test]
#[ignore]
async fn different_canonical_addresses_stay_separate() {
    let client = connect().await;
    let tag = "conv2";
    cleanup(&client, tag).await;

    let sig1 = Uuid::new_v4();
    let sig2 = Uuid::new_v4();

    // Both signals reference "Portland" — same normalized_name, same stub node
    let portland = test_loc(tag, "portland");
    create_signal_with_location_stub(&client, &sig1, "Portland event", &portland, "HELD_AT", tag).await;
    create_signal_with_location_stub(&client, &sig2, "Another Portland event", &portland, "HELD_AT", tag).await;

    let projector = GraphProjector::new(client.clone());

    // First signal's location geocodes to Portland, Oregon
    let e1 = Event::System(SystemEvent::LocationGeocoded {
        signal_id: sig1,
        location_name: portland.clone(),
        lat: 45.5152,
        lng: -122.6784,
        address: Some(test_loc(tag, "Portland, Oregon, United States")),
        precision: "approximate".to_string(),
        timezone: Some("America/Los_Angeles".to_string()),
        city: None, state: None, country_code: None,
    });
    let plan = projector.plan(&persisted(&e1, 200));
    projector.execute(plan).await.unwrap();

    // Second signal's location geocodes to Portland, Maine
    let e2 = Event::System(SystemEvent::LocationGeocoded {
        signal_id: sig2,
        location_name: portland.clone(),
        lat: 43.6591,
        lng: -70.2568,
        address: Some(test_loc(tag, "Portland, Maine, United States")),
        precision: "approximate".to_string(),
        timezone: Some("America/New_York".to_string()),
        city: None, state: None, country_code: None,
    });
    let plan = projector.plan(&persisted(&e2, 201));
    projector.execute(plan).await.unwrap();

    // Assert: 2 distinct Location nodes (different canonical addresses)
    let mut r = client.execute(
        query(
            "MATCH (g)-[]->(l:Location)
             WHERE g._test_tag = $tag
             WITH DISTINCT l
             RETURN count(l) AS cnt"
        ).param("tag", tag),
    ).await.unwrap();
    let count: i64 = r.next().await.unwrap().unwrap().get("cnt").unwrap();
    assert_eq!(count, 2, "Portland OR and Portland ME should remain separate");

    cleanup(&client, tag).await;
    println!("  ✓ Portland OR and Portland ME remain as separate Location nodes");
}

/// When a signal has multiple locations, each gets its own LocationGeocoded event
/// and each should merge correctly to its canonical address.
#[tokio::test]
#[ignore]
async fn signal_with_multiple_locations_each_converge_independently() {
    let client = connect().await;
    let tag = "conv3";
    cleanup(&client, tag).await;

    let sig = Uuid::new_v4();

    let loc_mpls = test_loc(tag, "minneapolis");
    let loc_stp = test_loc(tag, "st paul");

    // Signal references two different places
    let q = query(
        "CREATE (g:Gathering {id: $id, title: $title, sensitivity: 'general', confidence: 0.5, _test_tag: $tag})
         MERGE (l1:Location {normalized_name: $n1})
         ON CREATE SET l1.name = $n1
         MERGE (l2:Location {normalized_name: $n2})
         ON CREATE SET l2.name = $n2
         CREATE (g)-[:HELD_AT]->(l1)
         CREATE (g)-[:HELD_AT]->(l2)",
    )
    .param("id", sig.to_string())
    .param("title", "Twin Cities summit")
    .param("n1", loc_mpls.as_str())
    .param("n2", loc_stp.as_str())
    .param("tag", tag);
    client.run(q).await.unwrap();

    let projector = GraphProjector::new(client.clone());

    // Geocode Minneapolis
    let e1 = Event::System(SystemEvent::LocationGeocoded {
        signal_id: sig,
        location_name: loc_mpls.clone(),
        lat: 44.9778,
        lng: -93.2650,
        address: Some(test_loc(tag, "Minneapolis, Minnesota, United States")),
        precision: "approximate".to_string(),
        timezone: Some("America/Chicago".to_string()),
        city: None, state: None, country_code: None,
    });
    let plan = projector.plan(&persisted(&e1, 300));
    projector.execute(plan).await.unwrap();

    // Geocode St Paul
    let e2 = Event::System(SystemEvent::LocationGeocoded {
        signal_id: sig,
        location_name: loc_stp.clone(),
        lat: 44.9537,
        lng: -93.0900,
        address: Some(test_loc(tag, "Saint Paul, Minnesota, United States")),
        precision: "approximate".to_string(),
        timezone: Some("America/Chicago".to_string()),
        city: None, state: None, country_code: None,
    });
    let plan = projector.plan(&persisted(&e2, 301));
    projector.execute(plan).await.unwrap();

    // Assert: 2 distinct Location nodes (different cities)
    let mut r = client.execute(
        query(
            "MATCH (g)-[]->(l:Location)
             WHERE g._test_tag = $tag
             WITH DISTINCT l
             RETURN count(l) AS cnt"
        ).param("tag", tag),
    ).await.unwrap();
    let count: i64 = r.next().await.unwrap().unwrap().get("cnt").unwrap();
    assert_eq!(count, 2, "Minneapolis and St Paul should remain separate");

    // Both should have geocoded coordinates
    let mut r = client.execute(
        query(
            "MATCH (g)-[]->(l:Location)
             WHERE g._test_tag = $tag AND l.geocoded = true
             WITH DISTINCT l
             RETURN count(l) AS cnt"
        ).param("tag", tag),
    ).await.unwrap();
    let geocoded_count: i64 = r.next().await.unwrap().unwrap().get("cnt").unwrap();
    assert_eq!(geocoded_count, 2, "Both locations should be geocoded");

    cleanup(&client, tag).await;
    println!("  ✓ Two locations on same signal stay separate with independent geocoding");
}
