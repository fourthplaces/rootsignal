//! Graph query helpers for test assertions.
//! These bypass display filters and sensitivity fuzzing — tests need raw truth.

use rootsignal_graph::{query, GraphClient};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct SignalRow {
    pub id: Uuid,
    pub title: String,
    pub node_type: String,
    pub confidence: f32,
    pub source_url: String,
    pub source_diversity: u32,
}

#[derive(Debug)]
pub struct SignalGeoRow {
    pub title: String,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub geo_precision: Option<String>,
}

/// All signals with their geo coordinates, for verifying city-center backfill.
pub async fn all_signals_with_geo(client: &GraphClient) -> Vec<SignalGeoRow> {
    let mut results = Vec::new();

    for label in &["Event", "Give", "Ask", "Notice", "Tension"] {
        let cypher = format!(
            "MATCH (n:{label}) RETURN n.title AS title, n.lat AS lat, n.lng AS lng, \
             n.geo_precision AS geo_precision"
        );

        let q = query(&cypher);
        let mut stream = client.inner().execute(q).await.expect("query failed");

        while let Some(row) = stream.next().await.expect("row failed") {
            results.push(SignalGeoRow {
                title: row.get("title").unwrap_or_default(),
                lat: row.get::<f64>("lat").ok(),
                lng: row.get::<f64>("lng").ok(),
                geo_precision: row.get("geo_precision").ok(),
            });
        }
    }

    results
}

#[derive(Debug, Serialize)]
pub struct EvidenceRow {
    pub id: Uuid,
    pub source_url: String,
    pub relevance: Option<String>,
    pub confidence: Option<f32>,
}

#[derive(Debug, Serialize)]
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
    signals.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
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

#[derive(Debug, Serialize)]
pub struct TensionRow {
    pub id: Uuid,
    pub title: String,
    pub confidence: f32,
    pub category: Option<String>,
    pub what_would_help: Option<String>,
}

/// Tension signals with category and what_would_help fields.
pub async fn tension_signals(client: &GraphClient) -> Vec<TensionRow> {
    let cypher = "MATCH (n:Tension) \
                  RETURN n.id AS id, n.title AS title, n.confidence AS confidence, \
                  n.category AS category, n.what_would_help AS what_would_help \
                  ORDER BY n.confidence DESC";

    let q = query(cypher);
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let mut results = Vec::new();

    while let Some(row) = stream.next().await.expect("row failed") {
        let id_str: String = row.get("id").unwrap_or_default();
        let id = Uuid::parse_str(&id_str).unwrap_or_default();
        let category: String = row.get("category").unwrap_or_default();
        let what_would_help: String = row.get("what_would_help").unwrap_or_default();
        results.push(TensionRow {
            id,
            title: row.get("title").unwrap_or_default(),
            confidence: row.get::<f64>("confidence").unwrap_or_default() as f32,
            category: if category.is_empty() {
                None
            } else {
                Some(category)
            },
            what_would_help: if what_would_help.is_empty() {
                None
            } else {
                Some(what_would_help)
            },
        });
    }

    results
}

/// Serialize the full graph state (signals, tensions, stories, evidence) to JSON
/// for passing to the judge. When `city_slug` is provided, only returns signals
/// connected to sources for that city.
pub async fn serialize_graph_state(client: &GraphClient) -> String {
    serialize_graph_state_for_city(client, None).await
}

/// Serialize graph state scoped to a specific city's sources.
pub async fn serialize_graph_state_for_city(
    client: &GraphClient,
    city_slug: Option<&str>,
) -> String {
    let mut signals = all_signals(client).await;
    let mut tensions = tension_signals(client).await;
    let stories = stories_by_energy(client).await;
    let responds_to = responds_to_edges(client).await;

    // If city_slug is given, filter to only signals connected to that city's sources
    if let Some(slug) = city_slug {
        let city_signal_ids = city_signal_ids(client, slug).await;
        signals.retain(|s| city_signal_ids.contains(&s.id));
        tensions.retain(|t| city_signal_ids.contains(&t.id));
    }

    // Collect evidence for each signal
    let mut evidence_map: std::collections::HashMap<String, Vec<EvidenceRow>> =
        std::collections::HashMap::new();
    for signal in &signals {
        let evidence = evidence_for_signal(client, signal.id).await;
        if !evidence.is_empty() {
            evidence_map.insert(signal.id.to_string(), evidence);
        }
    }

    #[derive(Serialize)]
    struct GraphState<'a> {
        signals: &'a [SignalRow],
        tensions: &'a [TensionRow],
        stories: &'a [StoryRow],
        evidence: &'a std::collections::HashMap<String, Vec<EvidenceRow>>,
        responds_to: &'a [RespondsToEdge],
    }

    let state = GraphState {
        signals: &signals,
        tensions: &tensions,
        stories: &stories,
        evidence: &evidence_map,
        responds_to: &responds_to,
    };

    serde_json::to_string_pretty(&state).unwrap_or_else(|_| "{}".to_string())
}

/// RESPONDS_TO edges linking response signals to tensions.
#[derive(Debug, Serialize)]
pub struct RespondsToEdge {
    pub response_id: Uuid,
    pub response_type: String,
    pub response_title: String,
    pub tension_id: Uuid,
    pub tension_title: String,
    pub match_strength: f32,
    pub explanation: String,
}

/// All RESPONDS_TO edges in the graph.
pub async fn responds_to_edges(client: &GraphClient) -> Vec<RespondsToEdge> {
    let cypher = "MATCH (resp)-[rel:RESPONDS_TO]->(t:Tension) \
                  WHERE resp:Give OR resp:Event OR resp:Ask \
                  RETURN resp.id AS response_id, labels(resp)[0] AS response_type, \
                  resp.title AS response_title, t.id AS tension_id, t.title AS tension_title, \
                  rel.match_strength AS match_strength, \
                  COALESCE(rel.explanation, '') AS explanation \
                  ORDER BY rel.match_strength DESC";

    let q = query(cypher);
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let mut results = Vec::new();

    while let Some(row) = stream.next().await.expect("row failed") {
        let resp_id_str: String = row.get("response_id").unwrap_or_default();
        let tension_id_str: String = row.get("tension_id").unwrap_or_default();
        results.push(RespondsToEdge {
            response_id: Uuid::parse_str(&resp_id_str).unwrap_or_default(),
            response_type: row.get("response_type").unwrap_or_default(),
            response_title: row.get("response_title").unwrap_or_default(),
            tension_id: Uuid::parse_str(&tension_id_str).unwrap_or_default(),
            tension_title: row.get("tension_title").unwrap_or_default(),
            match_strength: row.get::<f64>("match_strength").unwrap_or_default() as f32,
            explanation: row.get("explanation").unwrap_or_default(),
        });
    }

    results
}

/// Get IDs of all signals connected via Evidence to sources for a given city.
/// Evidence nodes have a `source_url` property that matches Source node URLs.
async fn city_signal_ids(client: &GraphClient, city_slug: &str) -> std::collections::HashSet<Uuid> {
    let cypher = "MATCH (s:Source {city: $slug}) WHERE s.url IS NOT NULL \
                  WITH collect(s.url) AS urls \
                  MATCH (n)-[:SOURCED_FROM]->(ev:Evidence) \
                  WHERE (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension) \
                  AND ev.source_url IN urls \
                  RETURN DISTINCT n.id AS id";
    let q = query(cypher).param("slug", city_slug);
    let mut stream = client.inner().execute(q).await.expect("query failed");
    let mut ids = std::collections::HashSet::new();
    while let Some(row) = stream.next().await.expect("row failed") {
        let id_str: String = row.get("id").unwrap_or_default();
        if let Ok(id) = Uuid::parse_str(&id_str) {
            ids.insert(id);
        }
    }
    ids
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
