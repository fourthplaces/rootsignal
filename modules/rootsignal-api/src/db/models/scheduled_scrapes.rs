use chrono::{DateTime, Utc};
use sqlx::PgPool;

#[derive(Debug, sqlx::FromRow)]
pub struct ScheduledScrapeRow {
    pub id: String,
    pub scope_type: String,
    pub scope_data: serde_json::Value,
    pub run_after: DateTime<Utc>,
    pub reason: String,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

pub async fn list_pending(pool: &PgPool, limit: u32) -> Result<Vec<ScheduledScrapeRow>, sqlx::Error> {
    sqlx::query_as::<_, ScheduledScrapeRow>(
        "SELECT id::text, scope_type, scope_data, run_after, reason, created_at, completed_at \
         FROM scheduled_scrapes \
         WHERE completed_at IS NULL \
         ORDER BY run_after ASC \
         LIMIT $1",
    )
    .bind(limit as i64)
    .fetch_all(pool)
    .await
}

pub async fn list_recent(pool: &PgPool, limit: u32) -> Result<Vec<ScheduledScrapeRow>, sqlx::Error> {
    sqlx::query_as::<_, ScheduledScrapeRow>(
        "SELECT id::text, scope_type, scope_data, run_after, reason, created_at, completed_at \
         FROM scheduled_scrapes \
         ORDER BY created_at DESC \
         LIMIT $1",
    )
    .bind(limit as i64)
    .fetch_all(pool)
    .await
}
