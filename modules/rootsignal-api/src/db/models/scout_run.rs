use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

// ---------------------------------------------------------------------------
// Row types (deserialized from JSONB columns)
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

#[derive(serde::Deserialize)]
pub struct EventJson {
    pub seq: u32,
    pub ts: DateTime<Utc>,
    #[serde(rename = "type")]
    pub event_type: String,
    pub query: Option<String>,
    pub url: Option<String>,
    pub provider: Option<String>,
    pub platform: Option<String>,
    pub identifier: Option<String>,
    pub signal_type: Option<String>,
    pub title: Option<String>,
    pub result_count: Option<u32>,
    pub post_count: Option<u32>,
    pub items: Option<u32>,
    pub content_bytes: Option<u64>,
    pub content_chars: Option<u64>,
    pub signals_extracted: Option<u32>,
    pub implied_queries: Option<u32>,
    pub similarity: Option<f64>,
    pub confidence: Option<f64>,
    pub success: Option<bool>,
    pub action: Option<String>,
    pub node_id: Option<String>,
    pub matched_id: Option<String>,
    pub existing_id: Option<String>,
    pub source_url: Option<String>,
    pub new_source_url: Option<String>,
    pub canonical_key: Option<String>,
    pub gatherings: Option<u64>,
    pub needs: Option<u64>,
    pub stale: Option<u64>,
    pub sources_created: Option<u64>,
    pub spent_cents: Option<u64>,
    pub remaining_cents: Option<u64>,
    pub topics: Option<Vec<String>>,
    pub posts_found: Option<u32>,
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
    pub events: Vec<EventJson>,
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

pub async fn list_by_region(pool: &PgPool, region: &str, limit: u32) -> Result<Vec<ScoutRunRow>> {
    let limit = limit.min(100) as i64;

    let rows = sqlx::query_as::<_, (String, String, DateTime<Utc>, DateTime<Utc>, serde_json::Value, serde_json::Value)>(
        r#"
        SELECT run_id, region, started_at, finished_at, stats, events
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
    let row = sqlx::query_as::<_, (String, String, DateTime<Utc>, DateTime<Utc>, serde_json::Value, serde_json::Value)>(
        r#"
        SELECT run_id, region, started_at, finished_at, stats, events
        FROM scout_runs
        WHERE run_id = $1
        "#,
    )
    .bind(run_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(row_to_scout_run))
}

// ---------------------------------------------------------------------------
// Internal
// ---------------------------------------------------------------------------

fn row_to_scout_run(
    r: (String, String, DateTime<Utc>, DateTime<Utc>, serde_json::Value, serde_json::Value),
) -> ScoutRunRow {
    ScoutRunRow {
        run_id: r.0,
        region: r.1,
        started_at: r.2,
        finished_at: r.3,
        stats: serde_json::from_value(r.4).unwrap_or_default(),
        events: serde_json::from_value(r.5).unwrap_or_default(),
    }
}
