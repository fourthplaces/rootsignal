use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Observation {
    pub id: Uuid,
    pub subject_type: String,
    pub subject_id: Uuid,
    pub observation_type: String,
    pub value: Value,
    pub source: String,
    pub confidence: f32,
    pub investigation_id: Option<Uuid>,
    pub observed_at: DateTime<Utc>,
    pub review_status: String,
}

impl Observation {
    pub async fn create(
        subject_type: &str,
        subject_id: Uuid,
        observation_type: &str,
        value: Value,
        source: &str,
        confidence: f32,
        investigation_id: Option<Uuid>,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO observations (subject_type, subject_id, observation_type, value, source, confidence, investigation_id)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING *
            "#,
        )
        .bind(subject_type)
        .bind(subject_id)
        .bind(observation_type)
        .bind(value)
        .bind(source)
        .bind(confidence)
        .bind(investigation_id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_subject(
        subject_type: &str,
        subject_id: Uuid,
        pool: &PgPool,
    ) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM observations WHERE subject_type = $1 AND subject_id = $2 ORDER BY observed_at DESC",
        )
        .bind(subject_type)
        .bind(subject_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_investigation(investigation_id: Uuid, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM observations WHERE investigation_id = $1 ORDER BY observed_at ASC",
        )
        .bind(investigation_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_pending(limit: i64, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM observations WHERE review_status = 'pending' ORDER BY observed_at DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_id(id: Uuid, pool: &PgPool) -> Result<Option<Self>> {
        sqlx::query_as::<_, Self>("SELECT * FROM observations WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(Into::into)
    }

    pub async fn set_review_status(id: Uuid, status: &str, pool: &PgPool) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            "UPDATE observations SET review_status = $1 WHERE id = $2 RETURNING *",
        )
        .bind(status)
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }
}
