use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MemoCache {
    pub id: Uuid,
    pub function_name: String,
    pub input_hash: String,
    pub input_summary: Option<String>,
    pub output: Vec<u8>,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub hit_count: i32,
}

impl MemoCache {
    /// Look up a cached result. Returns None if missing or expired.
    pub async fn get(
        function_name: &str,
        input_hash: &str,
        pool: &PgPool,
    ) -> Result<Option<Self>> {
        let row = sqlx::query_as::<_, Self>(
            "SELECT * FROM memo_cache
             WHERE function_name = $1 AND input_hash = $2
               AND (expires_at IS NULL OR expires_at > now())",
        )
        .bind(function_name)
        .bind(input_hash)
        .fetch_optional(pool)
        .await?;

        if let Some(ref row) = row {
            let id = row.id;
            let pool = pool.clone();
            tokio::spawn(async move {
                let _ = sqlx::query(
                    "UPDATE memo_cache SET hit_count = hit_count + 1 WHERE id = $1",
                )
                .bind(id)
                .execute(&pool)
                .await;
            });
        }

        Ok(row)
    }

    /// Store a result in the cache (upsert).
    pub async fn set(
        function_name: &str,
        input_hash: &str,
        input_summary: Option<&str>,
        output: &[u8],
        expires_at: Option<DateTime<Utc>>,
        pool: &PgPool,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO memo_cache (function_name, input_hash, input_summary, output, expires_at)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (function_name, input_hash)
             DO UPDATE SET output = EXCLUDED.output,
                          input_summary = EXCLUDED.input_summary,
                          expires_at = EXCLUDED.expires_at,
                          hit_count = 0,
                          created_at = now()",
        )
        .bind(function_name)
        .bind(input_hash)
        .bind(input_summary)
        .bind(output)
        .bind(expires_at)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Delete expired entries.
    pub async fn evict_expired(pool: &PgPool) -> Result<u64> {
        let result =
            sqlx::query("DELETE FROM memo_cache WHERE expires_at IS NOT NULL AND expires_at <= now()")
                .execute(pool)
                .await?;
        Ok(result.rows_affected())
    }
}
