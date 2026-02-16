use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Connection {
    pub id: Uuid,
    pub from_type: String,
    pub from_id: Uuid,
    pub to_type: String,
    pub to_id: Uuid,
    pub role: String,
    pub causal_quote: Option<String>,
    pub confidence: Option<f32>,
    pub created_at: DateTime<Utc>,
}

impl Connection {
    pub async fn create(
        from_type: &str,
        from_id: Uuid,
        to_type: &str,
        to_id: Uuid,
        role: &str,
        causal_quote: Option<&str>,
        confidence: Option<f32>,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO connections (from_type, from_id, to_type, to_id, role, causal_quote, confidence)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (from_type, from_id, to_type, to_id, role) DO UPDATE SET
                causal_quote = COALESCE(EXCLUDED.causal_quote, connections.causal_quote),
                confidence = COALESCE(EXCLUDED.confidence, connections.confidence)
            RETURNING *
            "#,
        )
        .bind(from_type)
        .bind(from_id)
        .bind(to_type)
        .bind(to_id)
        .bind(role)
        .bind(causal_quote)
        .bind(confidence)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    /// Find all connections pointing to a node.
    pub async fn find_to(to_type: &str, to_id: Uuid, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM connections WHERE to_type = $1 AND to_id = $2 ORDER BY created_at DESC",
        )
        .bind(to_type)
        .bind(to_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    /// Find all connections from a node.
    pub async fn find_from(from_type: &str, from_id: Uuid, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM connections WHERE from_type = $1 AND from_id = $2 ORDER BY created_at DESC",
        )
        .bind(from_type)
        .bind(from_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    /// Find connections to a node filtered by role.
    pub async fn find_to_by_role(
        to_type: &str,
        to_id: Uuid,
        role: &str,
        pool: &PgPool,
    ) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM connections WHERE to_type = $1 AND to_id = $2 AND role = $3 ORDER BY created_at DESC",
        )
        .bind(to_type)
        .bind(to_id)
        .bind(role)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    /// Count connections created in the last N days for a target.
    pub async fn count_recent(to_type: &str, to_id: Uuid, days: i32, pool: &PgPool) -> Result<i64> {
        let row = sqlx::query_as::<_, (i64,)>(
            r#"
            SELECT COUNT(*) FROM connections
            WHERE to_type = $1 AND to_id = $2
              AND created_at > NOW() - ($3::int || ' days')::interval
            "#,
        )
        .bind(to_type)
        .bind(to_id)
        .bind(days)
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }

    /// Most recent connection time for a target.
    pub async fn latest_connection_at(
        to_type: &str,
        to_id: Uuid,
        pool: &PgPool,
    ) -> Result<Option<DateTime<Utc>>> {
        let row = sqlx::query_as::<_, (Option<DateTime<Utc>>,)>(
            "SELECT MAX(created_at) FROM connections WHERE to_type = $1 AND to_id = $2",
        )
        .bind(to_type)
        .bind(to_id)
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }
}
