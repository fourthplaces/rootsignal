use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Finding {
    pub id: Uuid,
    pub title: String,
    pub summary: String,
    pub status: String,
    pub validation_status: Option<String>,
    pub signal_velocity: Option<f32>,
    pub fingerprint: Vec<u8>,
    pub investigation_id: Option<Uuid>,
    pub trigger_signal_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Finding {
    pub async fn create(
        title: &str,
        summary: &str,
        fingerprint: &[u8],
        investigation_id: Option<Uuid>,
        trigger_signal_id: Option<Uuid>,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO findings (title, summary, fingerprint, investigation_id, trigger_signal_id)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (fingerprint) DO UPDATE SET
                title = EXCLUDED.title,
                summary = EXCLUDED.summary,
                updated_at = NOW()
            RETURNING *
            "#,
        )
        .bind(title)
        .bind(summary)
        .bind(fingerprint)
        .bind(investigation_id)
        .bind(trigger_signal_id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_id(id: Uuid, pool: &PgPool) -> Result<Self> {
        sqlx::query_as::<_, Self>("SELECT * FROM findings WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(Into::into)
    }

    pub async fn find_by_status(
        status: &str,
        limit: i64,
        offset: i64,
        pool: &PgPool,
    ) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM findings WHERE status = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
        )
        .bind(status)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_all(limit: i64, offset: i64, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM findings ORDER BY created_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn search(
        query: &str,
        status: Option<&str>,
        limit: i64,
        offset: i64,
        pool: &PgPool,
    ) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM findings
            WHERE search_vector @@ plainto_tsquery('english', $1)
              AND ($2::text IS NULL OR status = $2)
            ORDER BY ts_rank(search_vector, plainto_tsquery('english', $1)) DESC
            LIMIT $3 OFFSET $4
            "#,
        )
        .bind(query)
        .bind(status)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn update_status(id: Uuid, status: &str, pool: &PgPool) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            "UPDATE findings SET status = $1, updated_at = NOW() WHERE id = $2 RETURNING *",
        )
        .bind(status)
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn update_validation_status(
        id: Uuid,
        validation_status: &str,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            "UPDATE findings SET validation_status = $1, updated_at = NOW() WHERE id = $2 RETURNING *",
        )
        .bind(validation_status)
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn update_velocity(id: Uuid, velocity: f32, pool: &PgPool) -> Result<()> {
        sqlx::query("UPDATE findings SET signal_velocity = $1, updated_at = NOW() WHERE id = $2")
            .bind(velocity)
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn count(pool: &PgPool) -> Result<i64> {
        let row = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM findings")
            .fetch_one(pool)
            .await?;
        Ok(row.0)
    }

    pub async fn count_by_status(status: &str, pool: &PgPool) -> Result<i64> {
        let row =
            sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM findings WHERE status = $1")
                .bind(status)
                .fetch_one(pool)
                .await?;
        Ok(row.0)
    }

    /// Count connections pointing to this finding.
    pub async fn connection_count(id: Uuid, pool: &PgPool) -> Result<i64> {
        let row = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM connections WHERE to_type = 'finding' AND to_id = $1",
        )
        .bind(id)
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }
}
