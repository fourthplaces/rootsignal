use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Investigation {
    pub id: Uuid,
    pub subject_type: String,
    pub subject_id: Uuid,
    pub trigger: String,
    pub status: String,
    pub summary_confidence: Option<f32>,
    pub summary: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl Investigation {
    pub async fn create(
        subject_type: &str,
        subject_id: Uuid,
        trigger: &str,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO investigations (subject_type, subject_id, trigger)
            VALUES ($1, $2, $3)
            RETURNING *
            "#,
        )
        .bind(subject_type)
        .bind(subject_id)
        .bind(trigger)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_id(id: Uuid, pool: &PgPool) -> Result<Self> {
        sqlx::query_as::<_, Self>("SELECT * FROM investigations WHERE id = $1")
            .bind(id)
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
            "SELECT * FROM investigations WHERE subject_type = $1 AND subject_id = $2 ORDER BY created_at DESC",
        )
        .bind(subject_type)
        .bind(subject_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn update_status(id: Uuid, status: &str, pool: &PgPool) -> Result<()> {
        let mut query = String::from("UPDATE investigations SET status = $1");
        if status == "running" {
            query.push_str(", started_at = NOW()");
        }
        query.push_str(" WHERE id = $2");

        sqlx::query(&query)
            .bind(status)
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn complete(id: Uuid, summary: &str, confidence: f32, pool: &PgPool) -> Result<()> {
        sqlx::query(
            "UPDATE investigations SET status = 'completed', summary = $1, summary_confidence = $2, completed_at = NOW() WHERE id = $3",
        )
        .bind(summary)
        .bind(confidence)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }
}
