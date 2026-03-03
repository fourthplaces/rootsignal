use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Row types
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize, Default)]
pub struct StatsJson {
    pub urls_scraped: Option<u32>,
    pub urls_unchanged: Option<u32>,
    pub urls_failed: Option<u32>,
    pub signals_extracted: Option<u32>,
    pub signals_deduplicated: Option<u32>,
    pub signals_stored: Option<u32>,
    pub social_media_posts: Option<u32>,
    pub expansion_queries_collected: Option<u32>,
    pub expansion_sources_created: Option<u32>,
}

/// Row from the `scout_run_events` table.
pub struct EventRow {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub seq: i32,
    pub ts: DateTime<Utc>,
    pub event_type: String,
    pub data: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Domain row returned by queries
// ---------------------------------------------------------------------------

pub struct ScoutRunRow {
    pub run_id: String,
    pub region: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub stats: StatsJson,
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

pub async fn list_by_region(pool: &PgPool, region: &str, limit: u32) -> Result<Vec<ScoutRunRow>> {
    let limit = limit.min(100) as i64;

    let rows = sqlx::query_as::<
        _,
        (
            String,
            String,
            DateTime<Utc>,
            DateTime<Utc>,
            serde_json::Value,
        ),
    >(
        r#"
        SELECT run_id, region, started_at, finished_at, stats
        FROM scout_runs
        WHERE region = $1
        ORDER BY finished_at DESC
        LIMIT $2
        "#,
    )
    .bind(region)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(row_to_scout_run).collect())
}

pub async fn find_by_id(pool: &PgPool, run_id: &str) -> Result<Option<ScoutRunRow>> {
    let row = sqlx::query_as::<
        _,
        (
            String,
            String,
            DateTime<Utc>,
            DateTime<Utc>,
            serde_json::Value,
        ),
    >(
        r#"
        SELECT run_id, region, started_at, finished_at, stats
        FROM scout_runs
        WHERE run_id = $1
        "#,
    )
    .bind(run_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(row_to_scout_run))
}

/// List events for a run, ordered by sequence number.
pub async fn list_events_by_run_id(
    pool: &PgPool,
    run_id: &str,
    event_type_filter: Option<&str>,
) -> Result<Vec<EventRow>> {
    let rows = if let Some(et) = event_type_filter {
        sqlx::query(
            r#"
            SELECT id, parent_id, seq, ts, event_type, data
            FROM scout_run_events
            WHERE run_id = $1 AND event_type = $2
            ORDER BY seq
            "#,
        )
        .bind(run_id)
        .bind(et)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            r#"
            SELECT id, parent_id, seq, ts, event_type, data
            FROM scout_run_events
            WHERE run_id = $1
            ORDER BY seq
            "#,
        )
        .bind(run_id)
        .fetch_all(pool)
        .await?
    };

    Ok(rows.into_iter().map(row_to_event).collect())
}

/// List events that touched a specific graph node (signal/source).
/// Searches by node_id, matched_id, and existing_id within the JSONB data column.
pub async fn list_events_by_node_id(
    pool: &PgPool,
    node_id: &str,
    limit: u32,
) -> Result<Vec<EventRow>> {
    let limit = limit.min(200) as i64;

    let rows = sqlx::query(
        r#"
        SELECT id, parent_id, seq, ts, event_type, data
        FROM scout_run_events
        WHERE data->>'node_id' = $1
           OR data->>'matched_id' = $1
           OR data->>'existing_id' = $1
        ORDER BY ts DESC
        LIMIT $2
        "#,
    )
    .bind(node_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(row_to_event).collect())
}

// ---------------------------------------------------------------------------
// Internal
// ---------------------------------------------------------------------------

fn row_to_scout_run(
    r: (
        String,
        String,
        DateTime<Utc>,
        DateTime<Utc>,
        serde_json::Value,
    ),
) -> ScoutRunRow {
    ScoutRunRow {
        run_id: r.0,
        region: r.1,
        started_at: r.2,
        finished_at: r.3,
        stats: serde_json::from_value(r.4).unwrap_or_default(),
    }
}

fn row_to_event(r: sqlx::postgres::PgRow) -> EventRow {
    EventRow {
        id: r.get("id"),
        parent_id: r.get("parent_id"),
        seq: r.get("seq"),
        ts: r.get("ts"),
        event_type: r.get("event_type"),
        data: r.get::<serde_json::Value, _>("data"),
    }
}
