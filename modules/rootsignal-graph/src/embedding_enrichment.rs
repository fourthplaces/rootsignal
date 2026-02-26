//! Embedding enrichment pass — post-projection batch embedding of signal nodes.
//!
//! Runs after all projection is complete. Queries nodes missing embeddings,
//! batch-embeds their text, writes embeddings back to Neo4j.
//!
//! This decouples embedding generation from the event sourcing path:
//! events stay lean (no 4KB vectors), and embeddings are recomputable.

use anyhow::Result;
use neo4rs::query;
use tracing::{info, warn};

use crate::GraphClient;

/// Stats from an embedding enrichment run.
#[derive(Debug, Default)]
pub struct EmbeddingEnrichStats {
    pub nodes_enriched: u32,
    pub nodes_skipped: u32,
}

impl std::fmt::Display for EmbeddingEnrichStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Embedding enrichment: {} nodes enriched, {} skipped",
            self.nodes_enriched, self.nodes_skipped
        )
    }
}

/// Enrich nodes missing embeddings. For each signal type, query nodes where
/// `embedding IS NULL`, build embed text from title + summary, batch-embed,
/// and write embeddings back.
pub async fn enrich_embeddings(
    client: &GraphClient,
    embedder: &dyn rootsignal_common::TextEmbedder,
    batch_size: usize,
) -> Result<EmbeddingEnrichStats> {
    let mut stats = EmbeddingEnrichStats::default();

    for label in &["Gathering", "Aid", "Need", "Notice", "Tension"] {
        let enriched = enrich_label(client, embedder, label, batch_size).await?;
        stats.nodes_enriched += enriched;
    }

    info!("{stats}");
    Ok(stats)
}

async fn enrich_label(
    client: &GraphClient,
    embedder: &dyn rootsignal_common::TextEmbedder,
    label: &str,
    batch_size: usize,
) -> Result<u32> {
    let mut total_enriched = 0u32;

    loop {
        // Fetch nodes missing embeddings
        let q = query(&format!(
            "MATCH (n:{label}) WHERE n.embedding IS NULL
             RETURN n.id AS id, n.title AS title, n.summary AS summary
             LIMIT $limit"
        ))
        .param("limit", batch_size as i64);

        let mut rows: Vec<(String, String, String)> = Vec::new();
        let mut stream = client.inner().execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id: String = row.get("id").unwrap_or_default();
            let title: String = row.get("title").unwrap_or_default();
            let summary: String = row.get("summary").unwrap_or_default();
            if !id.is_empty() {
                rows.push((id, title, summary));
            }
        }

        if rows.is_empty() {
            break;
        }

        // Build embed texts: "{title} {summary}" truncated to 500 chars
        let texts: Vec<String> = rows
            .iter()
            .map(|(_, title, summary)| {
                let text = format!("{title} {summary}");
                if text.len() > 500 {
                    text[..500].to_string()
                } else {
                    text
                }
            })
            .collect();

        // Batch embed
        let embeddings = match embedder.embed_batch(texts).await {
            Ok(e) => e,
            Err(e) => {
                warn!(label, error = %e, "Embedding batch failed, skipping remaining {label} nodes");
                break;
            }
        };

        // Write embeddings back one at a time (MERGE with vector property)
        for ((id, _, _), embedding) in rows.iter().zip(embeddings.iter()) {
            let embedding_f64: Vec<f64> = embedding.iter().map(|v| *v as f64).collect();
            let q = query(&format!(
                "MATCH (n:{label} {{id: $id}}) SET n.embedding = $embedding"
            ))
            .param("id", id.as_str())
            .param("embedding", embedding_f64);

            if let Err(e) = client.inner().run(q).await {
                warn!(label, id, error = %e, "Failed to write embedding");
            }
        }

        total_enriched += rows.len() as u32;

        // If we got fewer than batch_size, we've processed all missing embeddings
        if rows.len() < batch_size {
            break;
        }
    }

    if total_enriched > 0 {
        info!(label, enriched = total_enriched, "Embedding enrichment for {label}");
    }
    Ok(total_enriched)
}
