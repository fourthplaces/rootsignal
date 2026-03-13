use chrono::{DateTime, Utc};
use sqlx::PgPool;

#[derive(Debug, sqlx::FromRow)]
pub struct ScheduleRow {
    pub schedule_id: String,
    pub flow_type: String,
    pub scope: serde_json::Value,
    pub timeout: i32,
    pub base_timeout: i32,
    pub recurring: bool,
    pub enabled: bool,
    pub last_run_id: Option<String>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub region_id: Option<String>,
}

const COLS: &str = "schedule_id, flow_type, scope, timeout, base_timeout, recurring, enabled, \
                last_run_id, next_run_at, deleted_at, created_at, region_id";

pub async fn list_active(pool: &PgPool, limit: u32) -> Result<Vec<ScheduleRow>, sqlx::Error> {
    sqlx::query_as::<_, ScheduleRow>(&format!(
        "SELECT {COLS} FROM schedules \
         WHERE deleted_at IS NULL \
         ORDER BY created_at DESC \
         LIMIT $1",
    ))
    .bind(limit as i64)
    .fetch_all(pool)
    .await
}

pub async fn list_all(pool: &PgPool, limit: u32) -> Result<Vec<ScheduleRow>, sqlx::Error> {
    sqlx::query_as::<_, ScheduleRow>(&format!(
        "SELECT {COLS} FROM schedules \
         ORDER BY created_at DESC \
         LIMIT $1",
    ))
    .bind(limit as i64)
    .fetch_all(pool)
    .await
}

pub async fn list_for_entity(
    pool: &PgPool,
    entity_type: &str,
    entity_id: &str,
) -> Result<Vec<ScheduleRow>, sqlx::Error> {
    match entity_type {
        "region" => {
            sqlx::query_as::<_, ScheduleRow>(&format!(
                "SELECT {COLS} FROM schedules \
                 WHERE region_id = $1 AND deleted_at IS NULL \
                 ORDER BY created_at DESC"
            ))
            .bind(entity_id)
            .fetch_all(pool)
            .await
        }
        "source" => {
            sqlx::query_as::<_, ScheduleRow>(&format!(
                "SELECT {COLS} FROM schedules \
                 WHERE scope->'source_ids' @> $1::jsonb AND deleted_at IS NULL \
                 ORDER BY created_at DESC"
            ))
            .bind(format!("[\"{entity_id}\"]"))
            .fetch_all(pool)
            .await
        }
        "cluster" => {
            sqlx::query_as::<_, ScheduleRow>(&format!(
                "SELECT {COLS} FROM schedules \
                 WHERE scope->>'group_id' = $1 AND deleted_at IS NULL \
                 ORDER BY created_at DESC"
            ))
            .bind(entity_id)
            .fetch_all(pool)
            .await
        }
        _ => Ok(vec![]),
    }
}

pub async fn find_by_id(pool: &PgPool, schedule_id: &str) -> Result<Option<ScheduleRow>, sqlx::Error> {
    sqlx::query_as::<_, ScheduleRow>(&format!(
        "SELECT {COLS} FROM schedules \
         WHERE schedule_id = $1",
    ))
    .bind(schedule_id)
    .fetch_optional(pool)
    .await
}
