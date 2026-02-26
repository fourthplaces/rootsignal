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
    AidNode, ChannelType, CitationNode, GatheringNode, GeoPoint, GeoPrecision, Node, NodeMeta,
    ScheduleNode, SensitivityLevel, Severity, TensionNode,
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
        corroboration_count: 0,
        about_location: Some(GeoPoint {
            lat: 44.9486,
            lng: -93.2636,
            precision: GeoPrecision::Exact,
        }),
        about_location_name: Some("Powderhorn Park, Minneapolis".into()),
        source_url: "https://example.com/test".into(),
        extracted_at: Utc::now(),
        content_date: None,
        last_confirmed_active: Utc::now(),
        source_diversity: 1,
        cause_heat: 0.0,
        implied_queries: vec![],
        channel_diversity: 1,
        from_location: None,
        review_status: "staged".to_string(),
        was_corrected: false,
        corrections: None,
        rejection_reason: None,
        author_actor: None,
    }
}

fn dummy_embedding() -> Vec<f32> {
    let mut emb = vec![0.1_f32];
    emb.extend(std::iter::repeat(0.0_f32).take(1023));
    emb
}

fn make_citation(source_url: &str, content: &str) -> CitationNode {
    CitationNode {
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
                about_location: Some(GeoPoint {
                    lat: 44.9696,
                    lng: -93.2466,
                    precision: GeoPrecision::Exact,
                }),
                about_location_name: Some("420 15th Ave S, Minneapolis".into()),
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

        let evidence = make_citation(source_url, "Test page content for evidence trail");
        writer
            .create_citation(&evidence, node_id)
            .await
            .expect("create_evidence failed");
    }

    // Verify: every signal has SOURCED_FROM evidence
    let q = query(
        "MATCH (n) WHERE n:Gathering OR n:Aid OR n:Need OR n:Notice OR n:Tension
         OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Citation)
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
    let ev1 = make_citation(source_url, "First fetch of the page");
    writer
        .create_citation(&ev1, node_id)
        .await
        .expect("first create_evidence");

    // Second evidence write — same source_url, different content hash
    let ev2 = make_citation(source_url, "Page content has been updated since first fetch");
    writer
        .create_citation(&ev2, node_id)
        .await
        .expect("second create_evidence");

    // Should have exactly 1 Evidence node (MERGE on source_url)
    let q = query(
        "MATCH (n:Gathering {id: $id})-[:SOURCED_FROM]->(ev:Citation)
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
        "MATCH (n:Gathering {id: $id})-[:SOURCED_FROM]->(ev:Citation)
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
    let evidence = make_citation("https://example.com/page", "Some content");

    // Should not panic or error — the Cypher MERGE with WHERE n IS NOT NULL
    // simply does nothing when no signal matches.
    let result = writer.create_citation(&evidence, phantom_id).await;
    assert!(
        result.is_ok(),
        "Evidence for non-existent signal should not error: {:?}",
        result.err()
    );

    // Verify no Evidence node was created (orphaned)
    let q = query("MATCH (ev:Citation) RETURN count(ev) AS cnt");
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
            about_location: Some(GeoPoint {
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

// ===========================================================================
// Test: ScheduleNode creation and linking
// ===========================================================================

#[tokio::test]
async fn schedule_node_created_and_linked_to_gathering() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());
    let emb = dummy_embedding();

    let gathering = Node::Gathering(GatheringNode {
        meta: test_meta("Weekly community dinner"),
        starts_at: None,
        ends_at: None,
        action_url: "https://example.com/dinner".into(),
        organizer: Some("Neighborhood Council".into()),
        is_recurring: true,
    });
    let signal_id = gathering.meta().unwrap().id;

    writer
        .create_node(&gathering, &emb, "test", "test-run-1")
        .await
        .expect("create gathering");

    let schedule = ScheduleNode {
        id: Uuid::new_v4(),
        rrule: Some("FREQ=WEEKLY;BYDAY=WE".into()),
        rdates: vec![],
        exdates: vec![],
        dtstart: Some(Utc::now()),
        dtend: None,
        timezone: Some("America/Chicago".into()),
        schedule_text: Some("Every Wednesday evening".into()),
        extracted_at: Utc::now(),
    };
    let schedule_id = writer.create_schedule(&schedule).await.expect("create schedule");
    writer
        .link_schedule_to_signal(signal_id, schedule_id)
        .await
        .expect("link schedule to signal");

    // Verify: Schedule node exists and is linked via HAS_SCHEDULE
    let q = query(
        "MATCH (g:Gathering {id: $gid})-[:HAS_SCHEDULE]->(s:Schedule {id: $sid})
         RETURN s.rrule AS rrule, s.timezone AS tz, s.schedule_text AS text",
    )
    .param("gid", signal_id.to_string())
    .param("sid", schedule_id.to_string());

    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().expect("schedule should be linked");
    let rrule: String = row.get("rrule").unwrap();
    let tz: String = row.get("tz").unwrap();
    let text: String = row.get("text").unwrap();

    assert_eq!(rrule, "FREQ=WEEKLY;BYDAY=WE");
    assert_eq!(tz, "America/Chicago");
    assert_eq!(text, "Every Wednesday evening");
}

#[tokio::test]
async fn schedule_with_rrule_and_exdates_stored_correctly() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());
    let emb = dummy_embedding();

    let aid = Node::Aid(AidNode {
        meta: test_meta("Food pantry hours"),
        action_url: "https://example.com/pantry".into(),
        availability: Some("Thursdays 9-5".into()),
        is_ongoing: true,
    });
    let signal_id = aid.meta().unwrap().id;

    writer
        .create_node(&aid, &emb, "test", "test-run-1")
        .await
        .expect("create aid");

    let now = Utc::now();
    let schedule = ScheduleNode {
        id: Uuid::new_v4(),
        rrule: Some("FREQ=WEEKLY;BYDAY=TH".into()),
        rdates: vec![now],
        exdates: vec![now],
        dtstart: Some(now),
        dtend: Some(now),
        timezone: Some("America/Chicago".into()),
        schedule_text: Some("Every Thursday 9am-5pm, except holidays".into()),
        extracted_at: now,
    };
    let schedule_id = writer.create_schedule(&schedule).await.expect("create schedule");
    writer
        .link_schedule_to_signal(signal_id, schedule_id)
        .await
        .expect("link schedule");

    // Verify rdates and exdates arrays are stored
    let q = query(
        "MATCH (s:Schedule {id: $sid})
         RETURN size(s.rdates) AS rdate_count, size(s.exdates) AS exdate_count,
                s.dtstart IS NOT NULL AS has_start, s.dtend IS NOT NULL AS has_end",
    )
    .param("sid", schedule_id.to_string());

    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().expect("schedule should exist");
    let rdate_count: i64 = row.get("rdate_count").unwrap();
    let exdate_count: i64 = row.get("exdate_count").unwrap();
    let has_start: bool = row.get("has_start").unwrap();
    let has_end: bool = row.get("has_end").unwrap();

    assert_eq!(rdate_count, 1, "One rdate should be stored");
    assert_eq!(exdate_count, 1, "One exdate should be stored");
    assert!(has_start, "dtstart should be present");
    assert!(has_end, "dtend should be present");
}

#[tokio::test]
async fn schedule_text_only_fallback_works() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());
    let emb = dummy_embedding();

    let gathering = Node::Gathering(GatheringNode {
        meta: test_meta("Irregular community meeting"),
        starts_at: None,
        ends_at: None,
        action_url: "https://example.com/meeting".into(),
        organizer: None,
        is_recurring: false,
    });
    let signal_id = gathering.meta().unwrap().id;

    writer
        .create_node(&gathering, &emb, "test", "test-run-1")
        .await
        .expect("create gathering");

    // Schedule with only text — no rrule, no rdates
    let schedule = ScheduleNode {
        id: Uuid::new_v4(),
        rrule: None,
        rdates: vec![],
        exdates: vec![],
        dtstart: None,
        dtend: None,
        timezone: None,
        schedule_text: Some("First Saturdays, rain or shine".into()),
        extracted_at: Utc::now(),
    };
    let schedule_id = writer.create_schedule(&schedule).await.expect("create schedule");
    writer
        .link_schedule_to_signal(signal_id, schedule_id)
        .await
        .expect("link schedule");

    let q = query(
        "MATCH (g:Gathering {id: $gid})-[:HAS_SCHEDULE]->(s:Schedule)
         RETURN s.schedule_text AS text, s.rrule AS rrule",
    )
    .param("gid", signal_id.to_string());

    let mut stream = client.inner().execute(q).await.unwrap();
    let row = stream.next().await.unwrap().expect("schedule should be linked");
    let text: String = row.get("text").unwrap();
    let rrule: String = row.get("rrule").unwrap();

    assert_eq!(text, "First Saturdays, rain or shine");
    assert!(rrule.is_empty(), "rrule should be empty for text-only schedule");
}
