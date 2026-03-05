// Integration tests for cause_heat enrichment.
//
// Diversity and actor stats are now event-sourced (pure logic tested in enrich.rs unit tests).
// These tests verify that cause_heat reads from the graph correctly.

use chrono::Utc;
use uuid::Uuid;
use rootsignal_graph::{query, GraphClient};

fn neo4j_dt(dt: &chrono::DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
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
    let mut stream = client.execute(q).await.expect("query failed");
    if let Some(row) = stream.next().await.expect("stream failed") {
        row.get::<T>("val").unwrap_or_default()
    } else {
        T::default()
    }
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
        .run(q)
        .await
        .expect("Failed to create evidence");
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
            sensitivity: 'general',
            confidence: 0.8,
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
    let (_guard, client) = super::setup().await;

    // Two tensions with similar embeddings — they should boost each other's heat
    let t1 = Uuid::new_v4();
    let t2 = Uuid::new_v4();
    let emb = make_embedding(1.0);

    create_signal_with_embedding(&client, "Concern", t1, "https://a.com/1", &emb).await;
    create_signal_with_embedding(&client, "Concern", t2, "https://b.com/2", &emb).await;

    // Add evidence so the signals have source_diversity (needed for cause_heat)
    create_evidence(&client, "Concern", t1, "https://a.com/ev", "press").await;
    create_evidence(&client, "Concern", t2, "https://b.com/ev", "press").await;

    // Manually set source_diversity since it's now event-sourced
    let q = query("MATCH (n:Concern) SET n.source_diversity = 1");
    client.run(q).await.expect("set diversity failed");

    // bbox covers the test lat/lng (44.9778, -93.2650)
    rootsignal_graph::cause_heat::compute_cause_heat(&client, 0.3, 40.0, 50.0, -100.0, -80.0)
        .await
        .expect("cause_heat failed");

    let heat1: f64 = read_prop(&client, "Concern", t1, "cause_heat").await;
    let heat2: f64 = read_prop(&client, "Concern", t2, "cause_heat").await;

    // Both tensions should have nonzero heat (they corroborate each other)
    assert!(heat1 > 0.0, "tension 1 should have heat, got {heat1}");
    assert!(heat2 > 0.0, "tension 2 should have heat, got {heat2}");
}

#[tokio::test]
async fn cause_heat_not_written_for_signals_outside_bbox() {
    let (_guard, client) = super::setup().await;

    let t1 = Uuid::new_v4();
    let emb = make_embedding(1.0);
    create_signal_with_embedding(&client, "Concern", t1, "https://a.com/1", &emb).await;

    // bbox does NOT cover lat 44.9778 (our test signal)
    rootsignal_graph::cause_heat::compute_cause_heat(&client, 0.3, 10.0, 20.0, -100.0, -80.0)
        .await
        .expect("cause_heat failed");

    // cause_heat should not be set (signal outside bbox)
    let cypher = "MATCH (n:Concern {id: $id}) RETURN n.cause_heat AS val";
    let q = query(cypher).param("id", t1.to_string());
    let mut stream = client.execute(q).await.expect("query failed");
    let row = stream.next().await.expect("stream failed").expect("no row");
    let heat: Option<f64> = row.get("val").ok();
    assert!(
        heat.is_none(),
        "signal outside bbox should have no cause_heat, got {heat:?}"
    );
}
