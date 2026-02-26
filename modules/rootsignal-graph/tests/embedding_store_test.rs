//! Integration tests for EmbeddingStore.
//! Requires a Postgres instance. Set DATABASE_TEST_URL or these tests are skipped.

use std::sync::Arc;

use anyhow::Result;
use sqlx::PgPool;

use rootsignal_common::{EmbeddingLookup, TextEmbedder};
use rootsignal_graph::EmbeddingStore;

// --- Mock embedder ---

struct MockEmbedder;

#[async_trait::async_trait]
impl TextEmbedder for MockEmbedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        // Deterministic fake: first 4 bytes of text as f32 values, padded to 8 dims
        let mut v = vec![0.0f32; 8];
        for (i, b) in text.bytes().take(8).enumerate() {
            v[i] = b as f32 / 255.0;
        }
        Ok(v)
    }

    async fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        let mut results = Vec::new();
        for text in &texts {
            results.push(self.embed(text).await?);
        }
        Ok(results)
    }
}

// --- Test pool setup ---

async fn test_pool() -> Option<PgPool> {
    let url = std::env::var("DATABASE_TEST_URL").ok()?;
    let pool = PgPool::connect(&url).await.ok()?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS embedding_cache (
            input_hash    TEXT         PRIMARY KEY,
            model_version TEXT         NOT NULL,
            embedding     FLOAT4[]     NOT NULL,
            created_at    TIMESTAMPTZ  NOT NULL DEFAULT now()
        )
        "#,
    )
    .execute(&pool)
    .await
    .ok()?;

    sqlx::query("TRUNCATE embedding_cache")
        .execute(&pool)
        .await
        .ok()?;

    Some(pool)
}

fn store(pool: PgPool) -> EmbeddingStore {
    EmbeddingStore::new(pool, Arc::new(MockEmbedder), "test-v1".to_string())
}

// =========================================================================
// Behavior tests
// =========================================================================

#[tokio::test]
async fn cache_miss_computes_stores_and_returns() {
    let Some(pool) = test_pool().await else { return };
    let s = store(pool.clone());

    let embedding = s.get("hello world").await.unwrap();
    assert_eq!(embedding.len(), 8);

    // Verify it was stored in Postgres
    let row: (i64,) = sqlx::query_as("SELECT count(*) FROM embedding_cache")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.0, 1);
}

#[tokio::test]
async fn cache_hit_returns_stored_embedding() {
    let Some(pool) = test_pool().await else { return };
    let s = store(pool);

    let first = s.get("hello world").await.unwrap();
    let second = s.get("hello world").await.unwrap();

    assert_eq!(first, second);
}

#[tokio::test]
async fn different_text_produces_different_embedding() {
    let Some(pool) = test_pool().await else { return };
    let s = store(pool);

    let a = s.get("hello").await.unwrap();
    let b = s.get("world").await.unwrap();

    assert_ne!(a, b);
}

#[tokio::test]
async fn model_version_change_causes_cache_miss() {
    let Some(pool) = test_pool().await else { return };

    let s_v1 = EmbeddingStore::new(pool.clone(), Arc::new(MockEmbedder), "v1".to_string());
    let s_v2 = EmbeddingStore::new(pool.clone(), Arc::new(MockEmbedder), "v2".to_string());

    // Store with v1
    s_v1.get("hello").await.unwrap();

    // v2 should miss (different hash) and store a second row
    s_v2.get("hello").await.unwrap();

    let row: (i64,) = sqlx::query_as("SELECT count(*) FROM embedding_cache")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.0, 2, "Two entries: one per model version");
}

#[tokio::test]
async fn warm_batch_computes_and_stores() {
    let Some(pool) = test_pool().await else { return };
    let s = store(pool.clone());

    let computed = s.warm(&["alpha", "beta", "gamma"]).await.unwrap();
    assert_eq!(computed, 3);

    // All three should now be cached
    let row: (i64,) = sqlx::query_as("SELECT count(*) FROM embedding_cache")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.0, 3);
}

#[tokio::test]
async fn warm_skips_already_cached_texts() {
    let Some(pool) = test_pool().await else { return };
    let s = store(pool);

    // Pre-cache one
    s.get("alpha").await.unwrap();

    // Warm all three — should only compute 2
    let computed = s.warm(&["alpha", "beta", "gamma"]).await.unwrap();
    assert_eq!(computed, 2);
}
