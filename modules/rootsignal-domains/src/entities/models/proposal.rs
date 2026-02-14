use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Proposal {
    pub id: Uuid,
    pub subject_type: String,
    pub subject_id: Uuid,
    pub action: String,
    pub payload: Value,
    pub reasoning: String,
    pub confidence: f32,
    pub evidence: Value,
    pub investigation_id: Option<Uuid>,
    pub status: String,
    pub reviewed_by: Option<String>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub rejection_reason: Option<String>,
    pub executed_at: Option<DateTime<Utc>>,
    pub execution_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl Proposal {
    pub async fn create(
        subject_type: &str,
        subject_id: Uuid,
        action: &str,
        payload: Value,
        reasoning: &str,
        confidence: f32,
        evidence: Value,
        investigation_id: Option<Uuid>,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO proposals (subject_type, subject_id, action, payload, reasoning, confidence, evidence, investigation_id)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING *
            "#,
        )
        .bind(subject_type)
        .bind(subject_id)
        .bind(action)
        .bind(payload)
        .bind(reasoning)
        .bind(confidence)
        .bind(evidence)
        .bind(investigation_id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_id(id: Uuid, pool: &PgPool) -> Result<Self> {
        sqlx::query_as::<_, Self>("SELECT * FROM proposals WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(Into::into)
    }

    pub async fn find_pending(pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM proposals
            WHERE status = 'pending'
              AND (expires_at IS NULL OR expires_at > NOW())
            ORDER BY confidence DESC, created_at ASC
            "#,
        )
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_pending_by_action(action: &str, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM proposals
            WHERE status = 'pending' AND action = $1
              AND (expires_at IS NULL OR expires_at > NOW())
            ORDER BY confidence DESC, created_at ASC
            "#,
        )
        .bind(action)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_subject(
        subject_type: &str,
        subject_id: Uuid,
        pool: &PgPool,
    ) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM proposals WHERE subject_type = $1 AND subject_id = $2 ORDER BY created_at DESC",
        )
        .bind(subject_type)
        .bind(subject_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn approve(id: Uuid, reviewed_by: &str, pool: &PgPool) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            UPDATE proposals
            SET status = 'approved', reviewed_by = $1, reviewed_at = NOW()
            WHERE id = $2
            RETURNING *
            "#,
        )
        .bind(reviewed_by)
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn reject(id: Uuid, reviewed_by: &str, reason: &str, pool: &PgPool) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            UPDATE proposals
            SET status = 'rejected', reviewed_by = $1, reviewed_at = NOW(), rejection_reason = $2
            WHERE id = $3
            RETURNING *
            "#,
        )
        .bind(reviewed_by)
        .bind(reason)
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn mark_executed(id: Uuid, pool: &PgPool) -> Result<()> {
        sqlx::query("UPDATE proposals SET executed_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn mark_execution_failed(id: Uuid, error: &str, pool: &PgPool) -> Result<()> {
        sqlx::query("UPDATE proposals SET execution_error = $1 WHERE id = $2")
            .bind(error)
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn auto_approve_above_threshold(
        threshold: f32,
        pool: &PgPool,
    ) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            r#"
            UPDATE proposals
            SET status = 'auto_approved', reviewed_by = 'system', reviewed_at = NOW()
            WHERE status = 'pending' AND confidence >= $1
              AND (expires_at IS NULL OR expires_at > NOW())
            RETURNING *
            "#,
        )
        .bind(threshold)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn expire_stale(pool: &PgPool) -> Result<u64> {
        let result = sqlx::query(
            "UPDATE proposals SET status = 'expired' WHERE status = 'pending' AND expires_at < NOW()",
        )
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }
}
