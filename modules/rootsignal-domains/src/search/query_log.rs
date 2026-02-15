use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct QueryLog {
    pub id: Uuid,
    pub member_id: Option<Uuid>,
    pub query_text: String,
    pub query_type: String,
    pub filters: Value,
    pub result_count: Option<i32>,
    pub clicked_listing_id: Option<Uuid>,
    pub session_id: Option<Uuid>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub created_at: DateTime<Utc>,
}

impl QueryLog {
    pub async fn create(
        query_text: &str,
        query_type: &str,
        filters: Value,
        result_count: Option<i32>,
        member_id: Option<Uuid>,
        session_id: Option<Uuid>,
        latitude: Option<f64>,
        longitude: Option<f64>,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO query_logs (query_text, query_type, filters, result_count, member_id, session_id, latitude, longitude)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING *
            "#,
        )
        .bind(query_text)
        .bind(query_type)
        .bind(filters)
        .bind(result_count)
        .bind(member_id)
        .bind(session_id)
        .bind(latitude)
        .bind(longitude)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn record_click(id: Uuid, listing_id: Uuid, pool: &PgPool) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            "UPDATE query_logs SET clicked_listing_id = $1 WHERE id = $2 RETURNING *",
        )
        .bind(listing_id)
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_session(session_id: Uuid, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM query_logs WHERE session_id = $1 ORDER BY created_at ASC",
        )
        .bind(session_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_member(member_id: Uuid, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM query_logs WHERE member_id = $1 ORDER BY created_at DESC",
        )
        .bind(member_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn popular_queries(limit: i64, pool: &PgPool) -> Result<Vec<(String, i64)>> {
        sqlx::query_as::<_, (String, i64)>(
            r#"
            SELECT query_text, COUNT(*) as count
            FROM query_logs
            GROUP BY query_text
            ORDER BY count DESC
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }
}
