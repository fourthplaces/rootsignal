use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct InvestigationStep {
    pub id: Uuid,
    pub investigation_id: Uuid,
    pub step_number: i32,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub output: serde_json::Value,
    pub page_snapshot_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

impl InvestigationStep {
    pub async fn create(
        investigation_id: Uuid,
        step_number: i32,
        tool_name: &str,
        input: serde_json::Value,
        output: serde_json::Value,
        page_snapshot_id: Option<Uuid>,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO investigation_steps (investigation_id, step_number, tool_name, input, output, page_snapshot_id)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING *
            "#,
        )
        .bind(investigation_id)
        .bind(step_number)
        .bind(tool_name)
        .bind(&input)
        .bind(&output)
        .bind(page_snapshot_id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_investigation(
        investigation_id: Uuid,
        pool: &PgPool,
    ) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM investigation_steps WHERE investigation_id = $1 ORDER BY step_number ASC",
        )
        .bind(investigation_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    /// Get the next step number for an investigation.
    pub async fn next_step_number(investigation_id: Uuid, pool: &PgPool) -> Result<i32> {
        let row = sqlx::query_as::<_, (Option<i32>,)>(
            "SELECT MAX(step_number) FROM investigation_steps WHERE investigation_id = $1",
        )
        .bind(investigation_id)
        .fetch_one(pool)
        .await?;
        Ok(row.0.unwrap_or(0) + 1)
    }
}
