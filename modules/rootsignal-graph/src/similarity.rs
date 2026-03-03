use neo4rs::{query, Graph};
use tracing::info;
use uuid::Uuid;

use rootsignal_common::events::SimilarityEdge;

use crate::GraphClient;

/// Cosine similarity threshold for creating SIMILAR_TO edges.
/// Single-region deployments use cosine only (geo/temporal add noise).
const SIMILARITY_THRESHOLD: f64 = 0.65;

/// A signal with its embedding and confidence, fetched from the graph.
struct SignalEmbedding {
    id: String,
    embedding: Vec<f64>,
    confidence: f64,
}

/// Compute similarity edges without writing to Neo4j.
/// Returns edges as `SimilarityEdge` values for event-sourcing through the projector.
pub async fn compute_edges(client: &GraphClient) -> Result<Vec<SimilarityEdge>, neo4rs::Error> {
    let graph = client;
    let signals = fetch_all_embeddings(graph).await?;
    let count = signals.len();
    info!(
        signals = count,
        "Fetched signal embeddings for similarity computation"
    );

    if count < 2 {
        info!("Too few signals for similarity edges");
        return Ok(Vec::new());
    }

    let mut edges = Vec::new();
    for i in 0..count {
        for j in (i + 1)..count {
            let sim = cosine_similarity(&signals[i].embedding, &signals[j].embedding);
            if sim >= SIMILARITY_THRESHOLD {
                let conf_weight = (signals[i].confidence * signals[j].confidence).sqrt();
                let weight = sim * conf_weight;
                let from_id = signals[i].id.parse::<Uuid>().unwrap_or_default();
                let to_id = signals[j].id.parse::<Uuid>().unwrap_or_default();
                edges.push(SimilarityEdge {
                    from_id,
                    to_id,
                    weight,
                });
            }
        }
    }

    info!(
        edges = edges.len(),
        "Computed similarity edges above threshold {}", SIMILARITY_THRESHOLD
    );

    Ok(edges)
}

/// Fetch all active signal embeddings from the graph.
async fn fetch_all_embeddings(graph: &Graph) -> Result<Vec<SignalEmbedding>, neo4rs::Error> {
    let mut signals = Vec::new();

    for label in &["Gathering", "Resource", "HelpRequest", "Announcement", "Concern", "Condition"] {
        let q = query(&format!(
            "MATCH (n:{label}) WHERE n.embedding IS NOT NULL \
             RETURN n.id AS id, n.embedding AS embedding, n.confidence AS confidence"
        ));

        let mut stream = graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id: String = row.get("id").unwrap_or_default();
            let embedding: Vec<f64> = row.get("embedding").unwrap_or_default();
            let confidence: f64 = row.get("confidence").unwrap_or(0.5);
            if !id.is_empty() && !embedding.is_empty() {
                signals.push(SignalEmbedding {
                    id,
                    embedding,
                    confidence,
                });
            }
        }
    }

    Ok(signals)
}

fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}
