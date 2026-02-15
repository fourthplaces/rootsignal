use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct FindingEvidence {
    pub id: Uuid,
    pub finding_id: Uuid,
    pub evidence_type: String,
    pub quote: String,
    pub attribution: Option<String>,
    pub url: Option<String>,
    pub page_snapshot_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

impl FindingEvidence {
    pub async fn create(
        finding_id: Uuid,
        evidence_type: &str,
        quote: &str,
        attribution: Option<&str>,
        url: Option<&str>,
        page_snapshot_id: Option<Uuid>,
        pool: &PgPool,
    ) -> Result<Self> {
        sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO finding_evidence (finding_id, evidence_type, quote, attribution, url, page_snapshot_id)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING *
            "#,
        )
        .bind(finding_id)
        .bind(evidence_type)
        .bind(quote)
        .bind(attribution)
        .bind(url)
        .bind(page_snapshot_id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn find_by_finding(finding_id: Uuid, pool: &PgPool) -> Result<Vec<Self>> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM finding_evidence WHERE finding_id = $1 ORDER BY created_at ASC",
        )
        .bind(finding_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    pub async fn count_by_finding(finding_id: Uuid, pool: &PgPool) -> Result<i64> {
        let row = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM finding_evidence WHERE finding_id = $1",
        )
        .bind(finding_id)
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }

    /// Count distinct evidence types for a finding.
    pub async fn distinct_type_count(finding_id: Uuid, pool: &PgPool) -> Result<i64> {
        let row = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(DISTINCT evidence_type) FROM finding_evidence WHERE finding_id = $1",
        )
        .bind(finding_id)
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }
}
