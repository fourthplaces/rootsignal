use sqlx::PgPool;
use tracing::info;

use crate::types::ValidationIssue;

/// Manages validation issues in Postgres.
pub struct IssueStore {
    pool: PgPool,
}

impl IssueStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Create a validation issue, but only if no open issue already exists
    /// for the same target_id and issue_type. Returns true if a new issue was created.
    pub async fn create_if_new(&self, issue: &ValidationIssue) -> Result<bool, sqlx::Error> {
        // Check for existing open issue with same target + type
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                SELECT 1 FROM validation_issues
                WHERE target_id = $1 AND issue_type = $2 AND status = 'open'
            )",
        )
        .bind(issue.target_id)
        .bind(issue.issue_type.to_string())
        .fetch_one(&self.pool)
        .await?;

        if exists {
            return Ok(false);
        }

        sqlx::query(
            "INSERT INTO validation_issues
                (id, region, issue_type, severity, target_id, target_label,
                 description, suggested_action, status, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(issue.id)
        .bind(&issue.region)
        .bind(issue.issue_type.to_string())
        .bind(issue.severity.to_string())
        .bind(issue.target_id)
        .bind(&issue.target_label)
        .bind(&issue.description)
        .bind(&issue.suggested_action)
        .bind(issue.status.to_string())
        .bind(issue.created_at)
        .execute(&self.pool)
        .await?;

        Ok(true)
    }

    /// Auto-expire open issues older than 30 days.
    /// Returns the number of issues expired.
    pub async fn expire_stale_issues(&self) -> Result<u64, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE validation_issues
             SET status = 'resolved',
                 resolved_at = now(),
                 resolution = 'auto-expired after 30 days'
             WHERE status = 'open'
               AND created_at < now() - interval '30 days'",
        )
        .execute(&self.pool)
        .await?;

        let expired = result.rows_affected();
        if expired > 0 {
            info!(expired, "Auto-expired stale validation issues");
        }
        Ok(expired)
    }
}
