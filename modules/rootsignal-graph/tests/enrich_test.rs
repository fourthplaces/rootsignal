use chrono::Utc;
use uuid::Uuid;
use rootsignal_graph::{enrich, query, GraphClient};
//! Integration tests for enrichment passes.
//!
//! These tests verify that compute_diversity and compute_actor_stats
//! write correct derived properties to Neo4j nodes.
//!
//! Requirements: Docker (for Neo4j via testcontainers)
//!
//! Run with: cargo test -p rootsignal-graph --features test-utils --test enrich_test

#![cfg(feature = "test-utils")]


async fn setup() -> (impl std::any::Any, GraphClient) {
    rootsignal_graph::testutil::neo4j_container().await
}

fn neo4j_dt(dt: &chrono::DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
}

/// Create a minimal signal node.
async fn create_signal(client: &GraphClient, label: &str, id: Uuid, source_url: &str) {
    let now = neo4j_dt(&Utc::now());
    let cypher = format!(
        "CREATE (n:{label} {{
            id: $id,
            title: 'Test signal',
            summary: 'Test',
            source_url: $source_url,
            extracted_at: datetime($now),
            lat: 44.9778,
            lng: -93.2650
        }})"
    );
    let q = query(&cypher)
        .param("id", id.to_string())
        .param("source_url", source_url)
        .param("now", now);
    client
        .inner()
        .run(q)
        .await
        .expect("Failed to create signal");
}

/// Create an Evidence node and link it to a signal via SOURCED_FROM.
async fn create_evidence(
    client: &GraphClient,
    label: &str,
    signal_id: Uuid,
    evidence_url: &str,
    channel_type: &str,
) {
    let ev_id = Uuid::new_v4();
    let cypher = format!(
        "MATCH (n:{label} {{id: $signal_id}})
         CREATE (ev:Citation {{
             id: $ev_id,
             source_url: $url,
             channel_type: $channel
         }})
         CREATE (n)-[:SOURCED_FROM]->(ev)"
    );
    let q = query(&cypher)
        .param("signal_id", signal_id.to_string())
        .param("ev_id", ev_id.to_string())
        .param("url", evidence_url)
        .param("channel", channel_type);
    client
        .inner()
        .run(q)
        .await
        .expect("Failed to create evidence");
}

/// Create an Actor node and ACTED_IN edge to a signal.
async fn create_actor_with_edge(
    client: &GraphClient,
    label: &str,
    actor_id: Uuid,
    actor_name: &str,
    signal_id: Uuid,
) {
    let now = neo4j_dt(&Utc::now());
    let cypher = format!(
        "MERGE (a:Actor {{id: $actor_id}})
         ON CREATE SET a.name = $name, a.signal_count = 0
         WITH a
         MATCH (n:{label} {{id: $signal_id}})
         CREATE (a)-[:ACTED_IN {{ts: datetime($now)}}]->(n)"
    );
    let q = query(&cypher)
        .param("actor_id", actor_id.to_string())
        .param("name", actor_name)
        .param("signal_id", signal_id.to_string())
        .param("now", now);
    client
        .inner()
        .run(q)
        .await
        .expect("Failed to create actor edge");
}

/// Read a property from a signal node.
async fn read_prop<T: for<'a> serde::Deserialize<'a> + Default>(
    client: &GraphClient,
    label: &str,
    id: Uuid,
    prop: &str,
) -> T {
    let cypher = format!("MATCH (n:{label} {{id: $id}}) RETURN n.{prop} AS val");
    let q = query(&cypher).param("id", id.to_string());
    let mut stream = client.inner().execute(q).await.expect("query failed");
    if let Some(row) = stream.next().await.expect("stream failed") {
        row.get::<T>("val").unwrap_or_default()
    } else {
        T::default()
    }
}

// ---------------------------------------------------------------------------
// Diversity tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn signal_with_no_evidence_gets_zero_diversity() {
    let (_c, client) = setup().await;
    let id = Uuid::new_v4();
    create_signal(&client, "Tension", id, "https://example.com/article").await;

    enrich(&client, &[], 0.3, 40.0, 50.0, -100.0, -80.0)
        .await
        .expect("enrich failed");

    let src_div: i64 = read_prop(&client, "Tension", id, "source_diversity").await;
    let ch_div: i64 = read_prop(&client, "Tension", id, "channel_diversity").await;
    let ext_ratio: f64 = read_prop(&client, "Tension", id, "external_ratio").await;

    assert_eq!(src_div, 0);
    assert_eq!(ch_div, 0);
    assert_eq!(ext_ratio, 0.0);
}

#[tokio::test]
async fn same_domain_evidence_has_one_entity_zero_external() {
    let (_c, client) = setup().await;
    let id = Uuid::new_v4();
    create_signal(&client, "Gathering", id, "https://example.com/original").await;
    create_evidence(
        &client,
        "Gathering",
        id,
        "https://example.com/page1",
        "press",
    )
    .await;
    create_evidence(
        &client,
        "Gathering",
        id,
        "https://example.com/page2",
        "press",
    )
    .await;

    enrich(&client, &[], 0.3, 40.0, 50.0, -100.0, -80.0)
        .await
        .expect("enrich failed");

    let src_div: i64 = read_prop(&client, "Gathering", id, "source_diversity").await;
    let ext_ratio: f64 = read_prop(&client, "Gathering", id, "external_ratio").await;

    assert_eq!(src_div, 1); // all same domain = one entity
    assert_eq!(ext_ratio, 0.0); // all internal
}

#[tokio::test]
async fn different_domains_increase_source_diversity() {
    let (_c, client) = setup().await;
    let id = Uuid::new_v4();
    create_signal(&client, "Aid", id, "https://example.com/original").await;
    create_evidence(&client, "Aid", id, "https://example.com/a", "press").await;
    create_evidence(&client, "Aid", id, "https://other.org/b", "press").await;
    create_evidence(&client, "Aid", id, "https://third.net/c", "social").await;

    enrich(&client, &[], 0.3, 40.0, 50.0, -100.0, -80.0)
        .await
        .expect("enrich failed");

    let src_div: i64 = read_prop(&client, "Aid", id, "source_diversity").await;
    let ext_ratio: f64 = read_prop(&client, "Aid", id, "external_ratio").await;

    assert_eq!(src_div, 3); // example.com, other.org, third.net
    assert!((ext_ratio - 2.0 / 3.0).abs() < 0.001); // 2 of 3 are external
}

#[tokio::test]
async fn channel_diversity_only_counts_channels_with_external_sources() {
    let (_c, client) = setup().await;
    let id = Uuid::new_v4();
    create_signal(&client, "Need", id, "https://example.com/original").await;
    // Same domain, press — internal
    create_evidence(&client, "Need", id, "https://example.com/a", "press").await;
    // Different domain, press — external
    create_evidence(&client, "Need", id, "https://other.org/b", "press").await;
    // Same domain, social — internal (channel doesn't count)
    create_evidence(&client, "Need", id, "https://example.com/c", "social").await;
    // Different domain, government — external
    create_evidence(
        &client,
        "Need",
        id,
        "https://gov.state.mn.us/d",
        "government",
    )
    .await;

    enrich(&client, &[], 0.3, 40.0, 50.0, -100.0, -80.0)
        .await
        .expect("enrich failed");

    let ch_div: i64 = read_prop(&client, "Need", id, "channel_diversity").await;
    // Only press (via other.org) and government (via gov.state.mn.us) have external entities
    assert_eq!(ch_div, 2);
}

// ---------------------------------------------------------------------------
// Actor stats tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn actor_signal_count_matches_acted_in_edge_count() {
    let (_c, client) = setup().await;

    let actor_id = Uuid::new_v4();
    let sig1 = Uuid::new_v4();
    let sig2 = Uuid::new_v4();
    let sig3 = Uuid::new_v4();

    create_signal(&client, "Tension", sig1, "https://a.com/1").await;
    create_signal(&client, "Gathering", sig2, "https://b.com/2").await;
    create_signal(&client, "Aid", sig3, "https://c.com/3").await;

    create_actor_with_edge(&client, "Tension", actor_id, "Test Org", sig1).await;
    create_actor_with_edge(&client, "Gathering", actor_id, "Test Org", sig2).await;
    create_actor_with_edge(&client, "Aid", actor_id, "Test Org", sig3).await;

    enrich(&client, &[], 0.3, 40.0, 50.0, -100.0, -80.0)
        .await
        .expect("enrich failed");

    let count: i64 = read_prop(&client, "Actor", actor_id, "signal_count").await;
    assert_eq!(count, 3);
}

#[tokio::test]
async fn actor_with_no_edges_keeps_zero_signal_count() {
    let (_c, client) = setup().await;

    let actor_id = Uuid::new_v4();
    let q = query("CREATE (a:Actor {id: $id, name: 'Lonely Actor', signal_count: 0})")
        .param("id", actor_id.to_string());
    client.inner().run(q).await.expect("Failed to create actor");

    enrich(&client, &[], 0.3, 40.0, 50.0, -100.0, -80.0)
        .await
        .expect("enrich failed");

    // Actor with no ACTED_IN edges should not be touched by compute_actor_stats
    // (the Cypher MATCH pattern won't match it)
    let count: i64 = read_prop(&client, "Actor", actor_id, "signal_count").await;
    assert_eq!(count, 0);
}

// ---------------------------------------------------------------------------
// Cause heat tests
// ---------------------------------------------------------------------------

/// Create a signal with an embedding (needed for cause_heat computation).
async fn create_signal_with_embedding(
    client: &GraphClient,
    label: &str,
    id: Uuid,
    source_url: &str,
    embedding: &[f64],
) {
    let now = neo4j_dt(&Utc::now());
    let emb_str = format!(
        "[{}]",
        embedding
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    let cypher = format!(
        "CREATE (n:{label} {{
            id: $id,
            title: 'Test signal',
            summary: 'Test',
            source_url: $source_url,
            extracted_at: datetime($now),
            lat: 44.9778,
            lng: -93.2650,
            embedding: {emb_str}
        }})"
    );
    let q = query(&cypher)
        .param("id", id.to_string())
        .param("source_url", source_url)
        .param("now", now);
    client
        .inner()
        .run(q)
        .await
        .expect("Failed to create signal with embedding");
}

fn make_embedding(direction: f64) -> Vec<f64> {
    let mut emb = vec![0.0; 1024];
    emb[0] = direction;
    emb[1] = 0.5;
    emb
}

#[tokio::test]
async fn cause_heat_written_for_signals_with_embeddings() {
    let (_c, client) = setup().await;

    // Two tensions with similar embeddings — they should boost each other's heat
    let t1 = Uuid::new_v4();
    let t2 = Uuid::new_v4();
    let emb = make_embedding(1.0);

    create_signal_with_embedding(&client, "Tension", t1, "https://a.com/1", &emb).await;
    create_signal_with_embedding(&client, "Tension", t2, "https://b.com/2", &emb).await;

    // Add evidence so diversity enrichment gives them nonzero diversity
    create_evidence(&client, "Tension", t1, "https://a.com/ev", "press").await;
    create_evidence(&client, "Tension", t2, "https://b.com/ev", "press").await;

    // bbox covers the test lat/lng (44.9778, -93.2650)
    enrich(&client, &[], 0.3, 40.0, 50.0, -100.0, -80.0)
        .await
        .expect("enrich failed");

    let heat1: f64 = read_prop(&client, "Tension", t1, "cause_heat").await;
    let heat2: f64 = read_prop(&client, "Tension", t2, "cause_heat").await;

    // Both tensions should have nonzero heat (they corroborate each other)
    assert!(heat1 > 0.0, "tension 1 should have heat, got {heat1}");
    assert!(heat2 > 0.0, "tension 2 should have heat, got {heat2}");
}

#[tokio::test]
async fn cause_heat_not_written_for_signals_outside_bbox() {
    let (_c, client) = setup().await;

    let t1 = Uuid::new_v4();
    let emb = make_embedding(1.0);
    create_signal_with_embedding(&client, "Tension", t1, "https://a.com/1", &emb).await;

    // bbox does NOT cover lat 44.9778 (our test signal)
    enrich(&client, &[], 0.3, 10.0, 20.0, -100.0, -80.0)
        .await
        .expect("enrich failed");

    // cause_heat should not be set (signal outside bbox)
    let cypher = "MATCH (n:Tension {id: $id}) RETURN n.cause_heat AS val";
    let q = query(cypher).param("id", t1.to_string());
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream failed").expect("no row");
    let heat: Option<f64> = row.get("val").ok();
    assert!(
        heat.is_none(),
        "signal outside bbox should have no cause_heat, got {heat:?}"
    );
}

