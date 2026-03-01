//! In-memory embedding cache for cross-batch dedup within a scout run.
//!
//! Catches duplicates that haven't been indexed in the graph yet (e.g. Instagram
//! and Facebook posts from the same org processed in the same batch).

use rootsignal_common::types::NodeType;
use uuid::Uuid;

pub(crate) struct EmbeddingCache {
    entries: std::sync::RwLock<Vec<CacheEntry>>,
}

struct CacheEntry {
    embedding: Vec<f32>,
    node_id: Uuid,
    node_type: NodeType,
    source_url: String,
}

impl EmbeddingCache {
    pub fn new() -> Self {
        Self {
            entries: std::sync::RwLock::new(Vec::new()),
        }
    }

    /// Find the best match above threshold. Returns (node_id, node_type, source_url, similarity).
    pub(crate) fn find_match(
        &self,
        embedding: &[f32],
        threshold: f64,
    ) -> Option<(Uuid, NodeType, String, f64)> {
        let entries = self.entries.read().expect("embed_cache lock poisoned");
        let mut best: Option<(Uuid, NodeType, String, f64)> = None;
        for entry in entries.iter() {
            let sim = cosine_similarity_f32(embedding, &entry.embedding);
            if sim >= threshold && best.as_ref().is_none_or(|b| sim > b.3) {
                best = Some((
                    entry.node_id,
                    entry.node_type,
                    entry.source_url.clone(),
                    sim,
                ));
            }
        }
        best
    }

    pub(crate) fn add(
        &self,
        embedding: Vec<f32>,
        node_id: Uuid,
        node_type: NodeType,
        source_url: String,
    ) {
        self.entries
            .write()
            .expect("embed_cache lock poisoned")
            .push(CacheEntry {
                embedding,
                node_id,
                node_type,
                source_url,
            });
    }
}

/// Cosine similarity for f32 embedding vectors (Voyage AI).
fn cosine_similarity_f32(a: &[f32], b: &[f32]) -> f64 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    (dot / (norm_a * norm_b)) as f64
}
