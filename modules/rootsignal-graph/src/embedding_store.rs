use std::sync::Arc;

use anyhow::Result;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tracing::debug;

use rootsignal_common::{EmbeddingLookup, TextEmbedder};

/// Get-or-compute embedding cache backed by Postgres.
///
/// Keyed by SHA-256 of (model_version + input_text). On cache hit, returns
/// the stored embedding instantly. On cache miss, computes via the underlying
/// TextEmbedder, stores the result, and returns it.
pub struct EmbeddingStore {
    pool: PgPool,
    embedder: Arc<dyn TextEmbedder>,
    model_version: String,
}

impl EmbeddingStore {
    pub fn new(pool: PgPool, embedder: Arc<dyn TextEmbedder>, model_version: String) -> Self {
        Self {
            pool,
            embedder,
            model_version,
        }
    }

    /// Pre-warm the cache for a batch of texts in a single API call.
    /// Skips texts that are already cached. Returns the number of new embeddings computed.
    pub async fn warm(&self, texts: &[&str]) -> Result<usize> {
        // Check which texts are missing from cache
        let mut missing: Vec<(String, String)> = Vec::new(); // (hash, text)
        for &text in texts {
            let hash = self.hash_key(text);
            let cached: Option<(String,)> =
                sqlx::query_as("SELECT input_hash FROM embedding_cache WHERE input_hash = $1")
                    .bind(&hash)
                    .fetch_optional(&self.pool)
                    .await?;

            if cached.is_none() {
                missing.push((hash, text.to_string()));
            }
        }

        if missing.is_empty() {
            return Ok(0);
        }

        let count = missing.len();
        let texts_to_embed: Vec<String> = missing.iter().map(|(_, t)| t.clone()).collect();

        let embeddings = self.embedder.embed_batch(texts_to_embed).await?;

        for ((hash, _), embedding) in missing.iter().zip(embeddings.iter()) {
            sqlx::query(
                "INSERT INTO embedding_cache (input_hash, model_version, embedding)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (input_hash) DO NOTHING",
            )
            .bind(hash)
            .bind(&self.model_version)
            .bind(embedding)
            .execute(&self.pool)
            .await?;
        }

        debug!(count, "Warmed embedding cache");
        Ok(count)
    }

    fn hash_key(&self, text: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.model_version.as_bytes());
        hasher.update(text.as_bytes());
        hex::encode(hasher.finalize())
    }
}

#[async_trait::async_trait]
impl EmbeddingLookup for EmbeddingStore {
    async fn get(&self, text: &str) -> Result<Vec<f32>> {
        let hash = self.hash_key(text);

        // Check cache
        let cached: Option<(Vec<f32>,)> =
            sqlx::query_as("SELECT embedding FROM embedding_cache WHERE input_hash = $1")
                .bind(&hash)
                .fetch_optional(&self.pool)
                .await?;

        if let Some((embedding,)) = cached {
            return Ok(embedding);
        }

        // Cache miss: compute, store, return
        let embedding = self.embedder.embed(text).await?;

        sqlx::query(
            "INSERT INTO embedding_cache (input_hash, model_version, embedding)
             VALUES ($1, $2, $3)
             ON CONFLICT (input_hash) DO NOTHING",
        )
        .bind(&hash)
        .bind(&self.model_version)
        .bind(&embedding)
        .execute(&self.pool)
        .await?;

        Ok(embedding)
    }
}
