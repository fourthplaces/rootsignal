use neo4rs::query;
use tracing::info;

use crate::GraphClient;

/// Cosine similarity threshold for creating SIMILAR_TO edges.
/// Single-city deployments use cosine only (geo/temporal add noise).
const SIMILARITY_THRESHOLD: f64 = 0.65;

/// Batch size for UNWIND edge creation.
const EDGE_BATCH_SIZE: usize = 500;

/// Builds SIMILAR_TO weighted edges between signal nodes based on cosine similarity.
/// For single-city deployments, uses cosine similarity only.
pub struct SimilarityBuilder {
    client: GraphClient,
}

/// A signal with its embedding and confidence, fetched from the graph.
struct SignalEmbedding {
    id: String,
    embedding: Vec<f64>,
    confidence: f64,
}

impl SimilarityBuilder {
    pub fn new(client: GraphClient) -> Self {
        Self { client }
    }

    /// Build SIMILAR_TO edges for all signals. Compares every pair and creates
    /// edges for pairs with cosine similarity >= threshold.
    /// Returns the number of edges created.
    pub async fn build_edges(&self) -> Result<u64, neo4rs::Error> {
        // Fetch all signal embeddings
        let signals = self.fetch_all_embeddings().await?;
        let count = signals.len();
        info!(
            signals = count,
            "Fetched signal embeddings for similarity computation"
        );

        if count < 2 {
            info!("Too few signals for similarity edges");
            return Ok(0);
        }

        // Compute pairwise cosine similarity, weighted by confidence.
        // Weight = cosine_sim * geometric_mean(conf_a, conf_b)
        // Low-confidence signals form weaker edges, resisting garbage clustering.
        let mut edges: Vec<(String, String, f64)> = Vec::new();

        for i in 0..count {
            for j in (i + 1)..count {
                let sim = cosine_similarity(&signals[i].embedding, &signals[j].embedding);
                if sim >= SIMILARITY_THRESHOLD {
                    let conf_weight = (signals[i].confidence * signals[j].confidence).sqrt();
                    let weight = sim * conf_weight;
                    edges.push((signals[i].id.clone(), signals[j].id.clone(), weight));
                }
            }
        }

        info!(
            edges = edges.len(),
            "Computed similarity edges above threshold {}", SIMILARITY_THRESHOLD
        );

        if edges.is_empty() {
            return Ok(0);
        }

        // Write edges in batches using UNWIND
        let mut total_created = 0u64;
        for batch in edges.chunks(EDGE_BATCH_SIZE) {
            let created = self.write_edge_batch(batch).await?;
            total_created += created;
        }

        info!(total_created, "SIMILAR_TO edges written");
        Ok(total_created)
    }

    /// Fetch all active signal embeddings from the graph.
    async fn fetch_all_embeddings(&self) -> Result<Vec<SignalEmbedding>, neo4rs::Error> {
        let mut signals = Vec::new();

        for label in &["Event", "Give", "Ask", "Notice", "Tension"] {
            let q = query(&format!(
                "MATCH (n:{label}) WHERE n.embedding IS NOT NULL \
                 RETURN n.id AS id, n.embedding AS embedding, n.confidence AS confidence"
            ));

            let mut stream = self.client.graph.execute(q).await?;
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

    /// Write a batch of edges using UNWIND for efficiency.
    /// Uses MERGE to avoid duplicates.
    async fn write_edge_batch(
        &self,
        batch: &[(String, String, f64)],
    ) -> Result<u64, neo4rs::Error> {
        // Build the edge data as a list of maps
        let edge_data: Vec<neo4rs::BoltType> = batch
            .iter()
            .map(|(from, to, weight)| {
                neo4rs::BoltType::Map(neo4rs::BoltMap::from_iter(vec![
                    (
                        neo4rs::BoltString::from("from"),
                        neo4rs::BoltType::String(neo4rs::BoltString::from(from.as_str())),
                    ),
                    (
                        neo4rs::BoltString::from("to"),
                        neo4rs::BoltType::String(neo4rs::BoltString::from(to.as_str())),
                    ),
                    (
                        neo4rs::BoltString::from("weight"),
                        neo4rs::BoltType::Float(neo4rs::BoltFloat::new(*weight)),
                    ),
                ]))
            })
            .collect();

        // Use UNWIND + MERGE for idempotent edge creation
        let q = query(
            "UNWIND $edges AS edge
             MATCH (a) WHERE a.id = edge.from AND (a:Event OR a:Give OR a:Ask OR a:Notice OR a:Tension)
             MATCH (b) WHERE b.id = edge.to AND (b:Event OR b:Give OR b:Ask OR b:Notice OR b:Tension)
             MERGE (a)-[r:SIMILAR_TO]->(b)
             SET r.weight = edge.weight
             RETURN count(*) AS created"
        )
        .param("edges", edge_data);

        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let created: i64 = row.get("created").unwrap_or(0);
            return Ok(created as u64);
        }

        Ok(0)
    }

    /// Remove all existing SIMILAR_TO edges (full rebuild).
    /// Called before build_edges() for a clean rebuild.
    pub async fn clear_edges(&self) -> Result<u64, neo4rs::Error> {
        let q = query(
            "MATCH ()-[e:SIMILAR_TO]->()
             DELETE e
             RETURN count(e) AS deleted",
        );

        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let deleted: i64 = row.get("deleted").unwrap_or(0);
            info!(deleted, "Cleared existing SIMILAR_TO edges");
            return Ok(deleted as u64);
        }

        Ok(0)
    }
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
