//! Graph query helpers for test assertions.
//! These bypass display filters and sensitivity fuzzing — tests need raw truth.

use rootsignal_graph::{query, GraphClient};
use uuid::Uuid;

#[derive(Debug)]
pub struct SignalRow {
    pub id: Uuid,
    pub title: String,
    pub node_type: String,
    pub confidence: f32,
    pub source_url: String,
    pub source_diversity: u32,
}

#[derive(Debug)]
pub struct EvidenceRow {
    pub id: Uuid,
    pub source_url: String,
    pub relevance: Option<String>,
    pub confidence: Option<f32>,
}

#[derive(Debug)]
pub struct StoryRow {
    pub id: Uuid,
    pub headline: String,
    pub energy: f32,
    pub signal_count: u32,
}

/// All signals in the graph, unfiltered.
pub async fn all_signals(client: &GraphClient) -> Vec<SignalRow> {
    let mut results = Vec::new();

    for label in &["Event", "Give", "Ask", "Notice", "Tension"] {
        let cypher = format!(
            "MATCH (n:{label}) RETURN n.id AS id, n.title AS title, '{label}' AS node_type, \
             n.confidence AS confidence, n.source_url AS source_url, \
             COALESCE(n.source_diversity, 1) AS source_diversity \
             ORDER BY n.confidence DESC"
        );

        let q = query(&cypher);
        let mut stream = client.inner().execute(q).await.expect("query failed");

        while let Some(row) = stream.next().await.expect("row failed") {
            let id_str: String = row.get("id").unwrap_or_default();
            let id = Uuid::parse_str(&id_str).unwrap_or_default();
            results.push(SignalRow {
                id,
                title: row.get("title").unwrap_or_default(),
                node_type: row.get("node_type").unwrap_or_default(),
                confidence: row.get::<f64>("confidence").unwrap_or_default() as f32,
                source_url: row.get("source_url").unwrap_or_default(),
                source_diversity: row.get::<i64>("source_diversity").unwrap_or(1) as u32,
            });
        }
    }

    results
}

/// All signals ordered by confidence DESC.
pub async fn signals_by_confidence(client: &GraphClient) -> Vec<SignalRow> {
    let mut signals = all_signals(client).await;
    signals.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
    signals
}

/// Evidence nodes linked to a specific signal.
pub async fn evidence_for_signal(client: &GraphClient, signal_id: Uuid) -> Vec<EvidenceRow> {
    let mut results = Vec::new();

    for label in &["Event", "Give", "Ask", "Notice", "Tension"] {
        let cypher = format!(
            "MATCH (n:{label} {{id: $id}})-[:SOURCED_FROM]->(ev:Evidence) \
             RETURN ev.id AS id, ev.source_url AS source_url, \
             ev.relevance AS relevance, ev.evidence_confidence AS confidence"
        );

        let q = query(&cypher).param("id", signal_id.to_string().as_str());
        let mut stream = client.inner().execute(q).await.expect("query failed");

        while let Some(row) = stream.next().await.expect("row failed") {
            let id_str: String = row.get("id").unwrap_or_default();
            let id = Uuid::parse_str(&id_str).unwrap_or_default();
            results.push(EvidenceRow {
                id,
                source_url: row.get("source_url").unwrap_or_default(),
                relevance: row.get("relevance").ok(),
                confidence: row.get::<f64>("confidence").ok().map(|v| v as f32),
            });
        }
    }

    results
}

/// Stories ordered by energy DESC.
pub async fn stories_by_energy(client: &GraphClient) -> Vec<StoryRow> {
    let cypher = "MATCH (s:Story) \
                  OPTIONAL MATCH (s)<-[:PART_OF]-(sig) \
                  RETURN s.id AS id, s.headline AS headline, s.energy AS energy, \
                  count(sig) AS signal_count \
                  ORDER BY s.energy DESC";

    let q = query(cypher);
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let mut results = Vec::new();

    while let Some(row) = stream.next().await.expect("row failed") {
        let id_str: String = row.get("id").unwrap_or_default();
        let id = Uuid::parse_str(&id_str).unwrap_or_default();
        results.push(StoryRow {
            id,
            headline: row.get("headline").unwrap_or_default(),
            energy: row.get::<f64>("energy").unwrap_or_default() as f32,
            signal_count: row.get::<i64>("signal_count").unwrap_or_default() as u32,
        });
    }

    results
}
