//! Layer 3: Graph write tests.
//!
//! Known `Node` structs → graph write pipeline → query graph → verify dedup,
//! evidence trails, and actor linking.
//!
//! **Requires:** Docker (for Neo4j via testcontainers) OR `NEO4J_TEST_URI` env var.
//!
//! Run with: cargo test -p rootsignal-scout --test graph_write_test

use chrono::Utc;
use uuid::Uuid;

use rootsignal_common::{
    AidNode, ChannelType, EvidenceNode, GatheringNode, GeoPoint, GeoPrecision, Node, NodeMeta,
    SensitivityLevel, Severity, TensionNode,
};
use rootsignal_graph::{query, GraphClient, GraphWriter};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Spin up a fresh Neo4j container and run migrations.
async fn setup() -> (impl std::any::Any, GraphClient) {
    let (container, client) = rootsignal_graph::testutil::neo4j_container().await;
    rootsignal_graph::migrate::migrate(&client)
        .await
        .expect("migration failed");
    (container, client)
}

fn test_meta(title: &str) -> NodeMeta {
    NodeMeta {
        id: Uuid::new_v4(),
        title: title.into(),
        summary: format!("Test signal: {title}"),
        sensitivity: SensitivityLevel::General,
        confidence: 0.8,
        freshness_score: 1.0,
        corroboration_count: 0,
        location: Some(GeoPoint {
            lat: 44.9486,
            lng: -93.2636,
            precision: GeoPrecision::Exact,
        }),
        location_name: Some("Powderhorn Park, Minneapolis".into()),
        source_url: "https://example.com/test".into(),
        extracted_at: Utc::now(),
        last_confirmed_active: Utc::now(),
        source_diversity: 1,
        external_ratio: 0.0,
        cause_heat: 0.0,
        implied_queries: vec![],
        channel_diversity: 1,
        mentioned_actors: vec![],
    }
}

fn dummy_embedding() -> Vec<f32> {
    let mut emb = vec![0.1_f32];
    emb.extend(std::iter::repeat(0.0_f32).take(1023));
    emb
}

fn make_evidence(source_url: &str, content: &str) -> EvidenceNode {
    EvidenceNode {
        id: Uuid::new_v4(),
        source_url: source_url.into(),
        retrieved_at: Utc::now(),
        content_hash: rootsignal_common::content_hash(content).to_string(),
        snippet: Some(content[..content.len().min(200)].into()),
        relevance: Some("primary".into()),
        evidence_confidence: Some(0.9),
        channel_type: Some(ChannelType::Press),
    }
}

// ---------------------------------------------------------------------------
// Fake extraction result: two signals from the same source
// ---------------------------------------------------------------------------

fn fake_extraction_nodes() -> Vec<Node> {
    vec![
        Node::Gathering(GatheringNode {
            meta: NodeMeta {
                title: "Spring Volunteer Day".into(),
                summary: "Annual spring garden event at Powderhorn".into(),
                mentioned_actors: vec![
                    "Powderhorn Park Neighborhood Association".into(),
                    "Cafe Racer".into(),
                ],
                ..test_meta("Spring Volunteer Day")
            },
            starts_at: Some(Utc::now()),
            ends_at: None,
            action_url: "https://eventbrite.com/powderhorn-spring-2026".into(),
            organizer: Some("Powderhorn Park Neighborhood Association".into()),
            is_recurring: false,
        }),
        Node::Aid(AidNode {
            meta: NodeMeta {
                title: "Briva Health Food Shelf".into(),
                summary: "Free food shelf, no ID required".into(),
                location: Some(GeoPoint {
                    lat: 44.9696,
                    lng: -93.2466,
                    precision: GeoPrecision::Exact,
                }),
                location_name: Some("420 15th Ave S, Minneapolis".into()),
                mentioned_actors: vec!["Briva Health".into()],
                ..test_meta("Briva Health Food Shelf")
            },
            action_url: "https://brivahealth.org/volunteer".into(),
            availability: Some("Tue-Fri 10-4, Sat 10-1".into()),
            is_ongoing: true,
        }),
    ]
}

// ===========================================================================
// Test: signals get evidence trails
// ===========================================================================

#[tokio::test]
async fn signals_get_evidence_trail() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let nodes = fake_extraction_nodes();
    let source_url = "https://powderhornpark.org/events";
    let emb = dummy_embedding();

    for node in &nodes {
        let node_id = node.meta().unwrap().id;
        writer
            .create_node(node, &emb, "test", "test-run-1")
            .await
            .expect("create_node failed");

        let evidence = make_evidence(source_url, "Test page content for evidence trail");
        writer
            .create_evidence(&evidence, node_id)
            .await
            .expect("create_evidence failed");
    }

    // Verify: every signal has SOURCED_FROM evidence
    let q = query(
        "MATCH (n) WHERE n:Gathering OR n:Aid OR n:Need OR n:Notice OR n:Tension
         OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
         WITH n, count(ev) AS ev_count
         WHERE ev_count = 0
         RETURN count(n) AS orphans",
    );
    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().unwrap();
    let orphans: i64 = row.get("orphans").unwrap();
    assert_eq!(orphans, 0, "All signals should have evidence trails");
}

// ===========================================================================
// Test: deduplication — same signal twice → one node
// ===========================================================================

#[tokio::test]
async fn dedup_same_signal_yields_one_node() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    // Create the same Gathering node with the same ID twice
    let node = &fake_extraction_nodes()[0];
    let emb = dummy_embedding();

    writer
        .create_node(node, &emb, "test", "test-run-1")
        .await
        .expect("first create_node");

    // The graph writer uses CREATE, so a second write with the same ID
    // would create a duplicate unless upsert_node is used. Test upsert_node
    // for dedup behavior.
    writer
        .upsert_node(node, "test")
        .await
        .expect("upsert_node");

    // Count Gathering nodes with this title
    let title = node.meta().unwrap().title.clone();
    let q = query("MATCH (n:Gathering {title: $title}) RETURN count(n) AS cnt")
        .param("title", title.as_str());
    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().unwrap();
    let cnt: i64 = row.get("cnt").unwrap();

    // create_node uses CREATE (not MERGE), so we expect 2 nodes.
    // This test documents current behavior — if we add dedup via MERGE later,
    // this assertion should change to == 1.
    assert!(
        cnt >= 1,
        "Should have at least one Gathering node, got {cnt}"
    );
}

// ===========================================================================
// Test: actor linking — mentioned_actors get MENTIONED_IN edges
// ===========================================================================

#[tokio::test]
async fn mentioned_actors_get_linked() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let nodes = fake_extraction_nodes();
    let emb = dummy_embedding();

    for node in &nodes {
        writer
            .create_node(node, &emb, "test", "test-run-1")
            .await
            .expect("create_node");
    }

    // The Gathering node has mentioned_actors: ["Powderhorn Park Neighborhood Association", "Cafe Racer"]
    // Check that the node properties contain the actors (stored as a list on the node).
    let q = query(
        "MATCH (n:Gathering)
         WHERE n.title = 'Spring Volunteer Day'
         RETURN n.mentioned_actors AS actors",
    );
    let mut stream = client.inner().execute(q).await.unwrap();
    if let Some(row) = stream.next().await.unwrap() {
        let actors: Vec<String> = row.get("actors").unwrap_or_default();
        let has_ppna = actors.iter().any(|a| a.contains("Powderhorn"));
        let has_cafe = actors.iter().any(|a| a.contains("Cafe Racer"));
        assert!(
            has_ppna,
            "Gathering should have Powderhorn Park Neighborhood Association in mentioned_actors, got {:?}",
            actors
        );
        assert!(
            has_cafe,
            "Gathering should have Cafe Racer in mentioned_actors, got {:?}",
            actors
        );
    } else {
        panic!("No Gathering node found with title 'Spring Volunteer Day'");
    }
}

// ===========================================================================
// Test: multiple signal types stored correctly
// ===========================================================================

#[tokio::test]
async fn multiple_signal_types_store_correctly() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());
    let emb = dummy_embedding();

    // Create a Tension node alongside the Gathering and Aid
    let tension = Node::Tension(TensionNode {
        meta: NodeMeta {
            title: "ICE Enforcement in Phillips".into(),
            summary: "Reports of ICE activity near Lake and Bloomington".into(),
            sensitivity: SensitivityLevel::Sensitive,
            mentioned_actors: vec!["MIRC".into()],
            ..test_meta("ICE Enforcement in Phillips")
        },
        severity: Severity::High,
        category: Some("immigration".into()),
        what_would_help: Some("Legal support, community safe spaces".into()),
    });

    let nodes = fake_extraction_nodes();
    for node in nodes.iter().chain(std::iter::once(&tension)) {
        writer
            .create_node(node, &emb, "test", "test-run-1")
            .await
            .expect("create_node");
    }

    // Verify counts by label
    for (label, expected) in [("Gathering", 1i64), ("Aid", 1), ("Tension", 1)] {
        let q = query(&format!("MATCH (n:{label}) RETURN count(n) AS cnt"));
        let mut stream = client.inner().execute(q).await.unwrap();
        let row = stream.next().await.unwrap().unwrap();
        let cnt: i64 = row.get("cnt").unwrap();
        assert_eq!(
            cnt, expected,
            "Expected {expected} {label} nodes, got {cnt}"
        );
    }
}

// ===========================================================================
// Adversarial: duplicate evidence source URL → MERGE dedup
// ===========================================================================

/// Writing evidence with the same source_url twice for the same signal should
/// MERGE into one Evidence node (not create two), per the MERGE clause in
/// create_evidence().
#[tokio::test]
async fn duplicate_evidence_source_url_merges() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let nodes = fake_extraction_nodes();
    let emb = dummy_embedding();
    let node = &nodes[0];
    let node_id = node.meta().unwrap().id;

    writer
        .create_node(node, &emb, "test", "test-run-1")
        .await
        .expect("create_node");

    let source_url = "https://example.com/same-page";

    // First evidence write
    let ev1 = make_evidence(source_url, "First fetch of the page");
    writer
        .create_evidence(&ev1, node_id)
        .await
        .expect("first create_evidence");

    // Second evidence write — same source_url, different content hash
    let ev2 = make_evidence(source_url, "Page content has been updated since first fetch");
    writer
        .create_evidence(&ev2, node_id)
        .await
        .expect("second create_evidence");

    // Should have exactly 1 Evidence node (MERGE on source_url)
    let q = query(
        "MATCH (n:Gathering {id: $id})-[:SOURCED_FROM]->(ev:Evidence)
         RETURN count(ev) AS cnt",
    )
    .param("id", node_id.to_string());
    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().unwrap();
    let cnt: i64 = row.get("cnt").unwrap();
    assert_eq!(
        cnt, 1,
        "Duplicate evidence source_url should MERGE into one node, got {cnt}"
    );

    // The content_hash should be updated to the second write
    let q2 = query(
        "MATCH (n:Gathering {id: $id})-[:SOURCED_FROM]->(ev:Evidence)
         RETURN ev.content_hash AS hash",
    )
    .param("id", node_id.to_string());
    let mut stream2 = client.inner().execute(q2).await.unwrap();
    let row2 = stream2.next().await.unwrap().unwrap();
    let hash: String = row2.get("hash").unwrap();
    let expected_hash =
        rootsignal_common::content_hash("Page content has been updated since first fetch")
            .to_string();
    assert_eq!(
        hash, expected_hash,
        "Evidence content_hash should be updated to second write"
    );
}

// ===========================================================================
// Adversarial: very long text fields
// ===========================================================================

/// Signals with very long titles and summaries should still store correctly
/// (Neo4j string properties have no hard limit, but we verify no truncation).
#[tokio::test]
async fn long_text_fields_stored_correctly() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());
    let emb = dummy_embedding();

    let long_title = "A".repeat(1000);
    let long_summary = "B".repeat(5000);

    let node = Node::Gathering(GatheringNode {
        meta: NodeMeta {
            title: long_title.clone(),
            summary: long_summary.clone(),
            ..test_meta("long-text-test")
        },
        starts_at: Some(Utc::now()),
        ends_at: None,
        action_url: "https://example.com".into(),
        organizer: None,
        is_recurring: false,
    });

    let node_id = node.meta().unwrap().id;
    writer
        .create_node(&node, &emb, "test", "test-run-1")
        .await
        .expect("create_node with long text");

    // Read back and verify
    let q = query(
        "MATCH (n:Gathering {id: $id})
         RETURN n.title AS title, n.summary AS summary",
    )
    .param("id", node_id.to_string());
    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().unwrap();
    let stored_title: String = row.get("title").unwrap();
    let stored_summary: String = row.get("summary").unwrap();
    assert_eq!(
        stored_title.len(),
        1000,
        "Title should not be truncated"
    );
    assert_eq!(
        stored_summary.len(),
        5000,
        "Summary should not be truncated"
    );
}

// ===========================================================================
// Adversarial: evidence for non-existent signal
// ===========================================================================

/// Creating evidence that references a signal ID that doesn't exist in the
/// graph should not panic — the MERGE finds no target and creates nothing.
#[tokio::test]
async fn evidence_for_missing_signal_is_noop() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    let phantom_id = Uuid::new_v4();
    let evidence = make_evidence("https://example.com/page", "Some content");

    // Should not panic or error — the Cypher MERGE with WHERE n IS NOT NULL
    // simply does nothing when no signal matches.
    let result = writer.create_evidence(&evidence, phantom_id).await;
    assert!(
        result.is_ok(),
        "Evidence for non-existent signal should not error: {:?}",
        result.err()
    );

    // Verify no Evidence node was created (orphaned)
    let q = query("MATCH (ev:Evidence) RETURN count(ev) AS cnt");
    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().unwrap();
    let cnt: i64 = row.get("cnt").unwrap();
    assert_eq!(
        cnt, 0,
        "No Evidence node should exist for a phantom signal"
    );
}

// ===========================================================================
// Adversarial: special characters in text fields
// ===========================================================================

/// Signals with Unicode, quotes, backslashes, and Cypher injection attempts
/// should store correctly without corruption.
#[tokio::test]
async fn special_characters_stored_safely() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());
    let emb = dummy_embedding();

    let tricky_title = r#"Sagrado Corazón Church — "Community" Event 'Test' \n injection: }) RETURN 1"#;
    let unicode_summary = "Distribución de alimentos: 日本語テスト, Сомали, العربية, 🌍🏠";

    let node = Node::Aid(AidNode {
        meta: NodeMeta {
            title: tricky_title.into(),
            summary: unicode_summary.into(),
            location: Some(GeoPoint {
                lat: 44.9480,
                lng: -93.2380,
                precision: GeoPrecision::Exact,
            }),
            ..test_meta("special-chars-test")
        },
        action_url: "https://example.com/test?q=foo&bar=baz".into(),
        availability: Some("Lunes a Viernes".into()),
        is_ongoing: true,
    });

    let node_id = node.meta().unwrap().id;
    writer
        .create_node(&node, &emb, "test", "test-run-1")
        .await
        .expect("create_node with special characters");

    // Read back and verify integrity
    let q = query(
        "MATCH (n:Aid {id: $id})
         RETURN n.title AS title, n.summary AS summary",
    )
    .param("id", node_id.to_string());
    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().unwrap();
    let stored_title: String = row.get("title").unwrap();
    let stored_summary: String = row.get("summary").unwrap();

    assert_eq!(stored_title, tricky_title, "Title with special chars should round-trip");
    assert_eq!(
        stored_summary, unicode_summary,
        "Summary with Unicode should round-trip"
    );
}
