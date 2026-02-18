//! Integration tests for triangulation scoring.
//!
//! These tests verify that story status, energy, and signal ranking
//! correctly incorporate type_diversity (triangulation).
//!
//! Requirements: Docker (for Memgraph via testcontainers)
//!
//! Run with: cargo test -p rootsignal-graph --features test-utils --test triangulation_test

#![cfg(feature = "test-utils")]

use chrono::Utc;
use uuid::Uuid;

use rootsignal_graph::{query, GraphClient, PublicGraphReader};

/// Spin up a fresh Memgraph container for each test.
async fn setup() -> (impl std::any::Any, GraphClient) {
    rootsignal_graph::testutil::memgraph_container().await
}

fn memgraph_dt(dt: &chrono::DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
}

/// Helper: create a signal node of a given type with minimal required properties.
async fn create_signal(
    client: &GraphClient,
    label: &str,
    id: Uuid,
    title: &str,
    source_url: &str,
    confidence: f64,
    cause_heat: f64,
) {
    let now = memgraph_dt(&Utc::now());
    let cypher = format!(
        "CREATE (n:{label} {{
            id: $id,
            title: $title,
            summary: $summary,
            sensitivity: 'general',
            confidence: $confidence,
            freshness_score: 0.8,
            corroboration_count: 0,
            source_diversity: 1,
            external_ratio: 0.0,
            cause_heat: $cause_heat,
            source_url: $source_url,
            extracted_at: datetime($now),
            last_confirmed_active: datetime($now),
            location_name: '',
            lat: 44.9778,
            lng: -93.2650,
            embedding: [0.1, 0.2, 0.3]
        }})"
    );

    let q = query(&cypher)
        .param("id", id.to_string())
        .param("title", title)
        .param("summary", format!("Test signal: {title}"))
        .param("confidence", confidence)
        .param("cause_heat", cause_heat)
        .param("source_url", source_url)
        .param("now", now);

    client.inner().run(q).await.expect("Failed to create signal");
}

/// Helper: create a Story node and link it to signals via CONTAINS edges.
async fn create_story_with_signals(
    client: &GraphClient,
    story_id: Uuid,
    headline: &str,
    status: &str,
    type_diversity: u32,
    entity_count: u32,
    energy: f64,
    signal_ids: &[(Uuid, &str)], // (id, label)
) {
    let now = memgraph_dt(&Utc::now());
    let q = query(
        "CREATE (s:Story {
            id: $id,
            headline: $headline,
            summary: $summary,
            signal_count: $signal_count,
            first_seen: datetime($now),
            last_updated: datetime($now),
            velocity: 0.0,
            energy: $energy,
            dominant_type: 'notice',
            sensitivity: 'general',
            source_count: $entity_count,
            entity_count: $entity_count,
            type_diversity: $type_diversity,
            source_domains: [],
            corroboration_depth: 0,
            status: $status
        })"
    )
    .param("id", story_id.to_string())
    .param("headline", headline)
    .param("summary", format!("Test story: {headline}"))
    .param("signal_count", signal_ids.len() as i64)
    .param("now", now)
    .param("energy", energy)
    .param("entity_count", entity_count as i64)
    .param("type_diversity", type_diversity as i64)
    .param("status", status);

    client.inner().run(q).await.expect("Failed to create story");

    // Link signals to story
    for (sig_id, label) in signal_ids {
        let cypher = format!(
            "MATCH (s:Story {{id: $story_id}}), (n:{label} {{id: $signal_id}})
             CREATE (s)-[:CONTAINS]->(n)"
        );
        let q = query(&cypher)
            .param("story_id", story_id.to_string())
            .param("signal_id", sig_id.to_string());
        client.inner().run(q).await.expect("Failed to link signal to story");
    }
}

// ---------------------------------------------------------------------------
// Test 1: Triangulated signals rank above echo signals in list_recent
// ---------------------------------------------------------------------------

#[tokio::test]
async fn triangulated_story_signals_rank_above_echo() {
    let (_container, client) = setup().await;
    let reader = PublicGraphReader::new(client.clone());

    // Create echo story: 5 Notice signals from different sources, type_diversity=1
    let echo_story_id = Uuid::new_v4();
    let mut echo_signals = Vec::new();
    for i in 0..5 {
        let id = Uuid::new_v4();
        create_signal(
            &client,
            "Notice",
            id,
            &format!("Echo notice {i}"),
            &format!("https://outlet{i}.com/story"),
            0.8,
            0.5, // moderate cause_heat
        )
        .await;
        echo_signals.push((id, "Notice"));
    }
    let echo_refs: Vec<(Uuid, &str)> = echo_signals.iter().map(|(id, l)| (*id, *l)).collect();
    create_story_with_signals(
        &client,
        echo_story_id,
        "Media echo story",
        "echo",
        1, // single type
        5,
        0.8,
        &echo_refs,
    )
    .await;

    // Create triangulated story: 4 signals of different types, type_diversity=4
    let tri_story_id = Uuid::new_v4();
    let tri_ids: Vec<(Uuid, &str)> = vec![
        (Uuid::new_v4(), "Tension"),
        (Uuid::new_v4(), "Ask"),
        (Uuid::new_v4(), "Give"),
        (Uuid::new_v4(), "Event"),
    ];

    for (i, (id, label)) in tri_ids.iter().enumerate() {
        create_signal(
            &client,
            label,
            *id,
            &format!("Triangulated {label} signal"),
            &format!("https://org{i}.org/page"),
            0.7, // slightly lower confidence than echo
            0.3, // lower cause_heat than echo
        )
        .await;
    }
    create_story_with_signals(
        &client,
        tri_story_id,
        "Triangulated crisis story",
        "confirmed",
        4, // four different types
        4,
        0.9,
        &tri_ids,
    )
    .await;

    // Query via reader — triangulated story signals should appear first
    let results = reader.list_recent(20, None).await.expect("list_recent failed");

    assert!(
        results.len() >= 4,
        "should return at least the triangulated signals, got {}",
        results.len(),
    );

    // The first 4 results should be from the triangulated story (type_diversity=4)
    // because list_recent sorts by story_triangulation DESC first
    let first_four_titles: Vec<String> = results
        .iter()
        .take(4)
        .map(|n| n.meta().unwrap().title.clone())
        .collect();

    let triangulated_first = first_four_titles
        .iter()
        .all(|t| t.contains("Triangulated"));

    assert!(
        triangulated_first,
        "triangulated signals should rank above echo; first 4 titles: {:?}",
        first_four_titles,
    );
}

// ---------------------------------------------------------------------------
// Test 2: find_nodes_near ranks by story type_diversity
// ---------------------------------------------------------------------------

#[tokio::test]
async fn find_nodes_near_prefers_triangulated() {
    let (_container, client) = setup().await;
    let reader = PublicGraphReader::new(client.clone());

    // Echo signal: high cause_heat but type_diversity=1 story
    let echo_id = Uuid::new_v4();
    create_signal(
        &client,
        "Notice",
        echo_id,
        "Echo signal near",
        "https://echo.com/page",
        0.9,
        0.9, // high cause_heat
    )
    .await;

    let echo_story = Uuid::new_v4();
    create_story_with_signals(
        &client,
        echo_story,
        "Echo story",
        "echo",
        1,
        5,
        0.5,
        &[(echo_id, "Notice")],
    )
    .await;

    // Triangulated signal: lower cause_heat but type_diversity=3 story
    let tri_id = Uuid::new_v4();
    create_signal(
        &client,
        "Event",
        tri_id,
        "Triangulated signal near",
        "https://tri.org/page",
        0.7,
        0.2, // low cause_heat
    )
    .await;

    let tri_story = Uuid::new_v4();
    create_story_with_signals(
        &client,
        tri_story,
        "Tri story",
        "confirmed",
        3,
        3,
        0.8,
        &[(tri_id, "Event")],
    )
    .await;

    // Both signals are at lat=44.9778, lng=-93.2650
    let results = reader
        .find_nodes_near(44.9778, -93.2650, 10.0, None)
        .await
        .expect("find_nodes_near failed");

    assert!(results.len() >= 2, "should find both signals, got {}", results.len());

    // Triangulated signal (type_diversity=3) should rank above echo (type_diversity=1)
    let first_title = &results[0].meta().unwrap().title;
    assert!(
        first_title.contains("Triangulated"),
        "triangulated signal should rank first; got '{first_title}'",
    );
}

// ---------------------------------------------------------------------------
// Test 3: Story status correctly stored and queryable
// ---------------------------------------------------------------------------

#[tokio::test]
async fn story_status_reflects_triangulation() {
    let (_container, client) = setup().await;

    // Create an "echo" story (type_diversity=1, 5 signals)
    let echo_id = Uuid::new_v4();
    let mut echo_sigs = Vec::new();
    for i in 0..5 {
        let id = Uuid::new_v4();
        create_signal(&client, "Notice", id, &format!("Notice {i}"), &format!("https://n{i}.com"), 0.7, 0.0).await;
        echo_sigs.push((id, "Notice"));
    }
    let refs: Vec<(Uuid, &str)> = echo_sigs.iter().map(|(id, l)| (*id, *l)).collect();
    create_story_with_signals(&client, echo_id, "Echo cluster", "echo", 1, 5, 0.5, &refs).await;

    // Create a "confirmed" story (type_diversity=3, entity_count=3)
    let confirmed_id = Uuid::new_v4();
    let c_sigs: Vec<(Uuid, &str)> = vec![
        (Uuid::new_v4(), "Tension"),
        (Uuid::new_v4(), "Give"),
        (Uuid::new_v4(), "Event"),
    ];
    for (id, label) in &c_sigs {
        create_signal(&client, label, *id, &format!("Confirmed {label}"), &format!("https://c-{label}.org"), 0.7, 0.0).await;
    }
    create_story_with_signals(&client, confirmed_id, "Confirmed story", "confirmed", 3, 3, 0.9, &c_sigs).await;

    // Create an "emerging" story (type_diversity=1, 2 signals — below echo threshold)
    let emerging_id = Uuid::new_v4();
    let e_sigs: Vec<(Uuid, &str)> = vec![
        (Uuid::new_v4(), "Ask"),
        (Uuid::new_v4(), "Ask"),
    ];
    for (id, label) in &e_sigs {
        create_signal(&client, label, *id, "Emerging ask", "https://emerging.org", 0.5, 0.0).await;
    }
    create_story_with_signals(&client, emerging_id, "Emerging story", "emerging", 1, 1, 0.3, &e_sigs).await;

    // Query stories by status
    let reader = PublicGraphReader::new(client.clone());

    let echo_stories = reader.top_stories_by_energy(10, Some("echo")).await.unwrap();
    assert_eq!(echo_stories.len(), 1, "should have 1 echo story");
    assert_eq!(echo_stories[0].type_diversity, 1);

    let confirmed_stories = reader.top_stories_by_energy(10, Some("confirmed")).await.unwrap();
    assert_eq!(confirmed_stories.len(), 1, "should have 1 confirmed story");
    assert_eq!(confirmed_stories[0].type_diversity, 3);

    let emerging_stories = reader.top_stories_by_energy(10, Some("emerging")).await.unwrap();
    assert_eq!(emerging_stories.len(), 1, "should have 1 emerging story");

    // Energy ordering: confirmed > echo > emerging
    let all = reader.top_stories_by_energy(10, None).await.unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].status, "confirmed", "confirmed should have highest energy");
}
