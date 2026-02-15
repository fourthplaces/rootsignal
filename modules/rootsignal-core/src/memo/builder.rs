use anyhow::Result;
use chrono::{Duration, Utc};
use serde::{de::DeserializeOwned, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::future::Future;

use super::MemoCache;

pub struct MemoBuilder<'a, K> {
    function_name: &'a str,
    key: K,
    ttl_ms: Option<i64>,
    pool: &'a PgPool,
}

impl<'a, K: Serialize> MemoBuilder<'a, K> {
    pub fn new(function_name: &'a str, key: K, pool: &'a PgPool) -> Self {
        Self {
            function_name,
            key,
            ttl_ms: None,
            pool,
        }
    }

    /// Set time-to-live in milliseconds.
    pub fn ttl(mut self, ms: i64) -> Self {
        self.ttl_ms = Some(ms);
        self
    }

    /// Get cached result or compute via the provided closure.
    pub async fn get_or<T, F, Fut>(self, f: F) -> Result<T>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let input_bytes = serde_json::to_vec(&self.key)?;
        let input_hash = hex::encode(Sha256::digest(&input_bytes));

        // Check cache
        if let Some(cached) = MemoCache::get(self.function_name, &input_hash, self.pool).await? {
            return Ok(serde_json::from_slice(&cached.output)?);
        }

        // Cache miss — compute
        let result = f().await?;

        // Store
        let output_bytes = serde_json::to_vec(&result)?;
        let expires_at = self
            .ttl_ms
            .map(|ms| Utc::now() + Duration::milliseconds(ms));
        let input_summary = String::from_utf8(input_bytes).ok();

        MemoCache::set(
            self.function_name,
            &input_hash,
            input_summary.as_deref(),
            &output_bytes,
            expires_at,
            self.pool,
        )
        .await?;

        Ok(result)
    }
}
