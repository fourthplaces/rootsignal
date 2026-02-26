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
    pub source_url: Option<String>,
    pub query: Option<String>,
    pub url: Option<String>,
    pub provider: Option<String>,
    pub platform: Option<String>,
    pub identifier: Option<String>,
    pub signal_type: Option<String>,
    pub title: Option<String>,
    pub result_count: Option<i32>,
    pub post_count: Option<i32>,
    pub items: Option<i32>,
    pub content_bytes: Option<i64>,
    pub content_chars: Option<i64>,
    pub signals_extracted: Option<i32>,
    pub implied_queries: Option<i32>,
    pub similarity: Option<f64>,
    pub confidence: Option<f64>,
    pub success: Option<bool>,
    pub action: Option<String>,
    pub node_id: Option<String>,
    pub matched_id: Option<String>,
    pub existing_id: Option<String>,
    pub new_source_url: Option<String>,
    pub canonical_key: Option<String>,
    pub gatherings: Option<i64>,
    pub needs: Option<i64>,
    pub stale: Option<i64>,
    pub sources_created: Option<i64>,
    pub spent_cents: Option<i64>,
    pub remaining_cents: Option<i64>,
    pub topics: Option<Vec<String>>,
    pub posts_found: Option<i32>,
    pub reason: Option<String>,
    pub strategy: Option<String>,
    pub field: Option<String>,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub signal_count: Option<i32>,
    pub summary: Option<String>,
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

    let rows = sqlx::query_as::<_, (String, String, DateTime<Utc>, DateTime<Utc>, serde_json::Value)>(
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
    let row = sqlx::query_as::<_, (String, String, DateTime<Utc>, DateTime<Utc>, serde_json::Value)>(
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
            SELECT id, parent_id, seq, ts, event_type, source_url,
                   query, url, provider, platform, identifier,
                   signal_type, title, result_count, post_count, items,
                   content_bytes, content_chars, signals_extracted, implied_queries,
                   similarity, confidence, success, action, node_id,
                   matched_id, existing_id, new_source_url, canonical_key,
                   gatherings, needs, stale, sources_created,
                   spent_cents, remaining_cents, topics, posts_found, reason, strategy,
                   field, old_value, new_value, signal_count, summary
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
            SELECT id, parent_id, seq, ts, event_type, source_url,
                   query, url, provider, platform, identifier,
                   signal_type, title, result_count, post_count, items,
                   content_bytes, content_chars, signals_extracted, implied_queries,
                   similarity, confidence, success, action, node_id,
                   matched_id, existing_id, new_source_url, canonical_key,
                   gatherings, needs, stale, sources_created,
                   spent_cents, remaining_cents, topics, posts_found, reason, strategy,
                   field, old_value, new_value, signal_count, summary
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
/// Searches by node_id, matched_id, and existing_id columns.
pub async fn list_events_by_node_id(
    pool: &PgPool,
    node_id: &str,
    limit: u32,
) -> Result<Vec<EventRow>> {
    let limit = limit.min(200) as i64;

    let rows = sqlx::query(
        r#"
        SELECT id, parent_id, seq, ts, event_type, source_url,
               query, url, provider, platform, identifier,
               signal_type, title, result_count, post_count, items,
               content_bytes, content_chars, signals_extracted, implied_queries,
               similarity, confidence, success, action, node_id,
               matched_id, existing_id, new_source_url, canonical_key,
               gatherings, needs, stale, sources_created,
               spent_cents, remaining_cents, topics, posts_found, reason, strategy,
               field, old_value, new_value, signal_count, summary
        FROM scout_run_events
        WHERE node_id = $1 OR matched_id = $1 OR existing_id = $1
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
    r: (String, String, DateTime<Utc>, DateTime<Utc>, serde_json::Value),
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
        source_url: r.get("source_url"),
        query: r.get("query"),
        url: r.get("url"),
        provider: r.get("provider"),
        platform: r.get("platform"),
        identifier: r.get("identifier"),
        signal_type: r.get("signal_type"),
        title: r.get("title"),
        result_count: r.get("result_count"),
        post_count: r.get("post_count"),
        items: r.get("items"),
        content_bytes: r.get("content_bytes"),
        content_chars: r.get("content_chars"),
        signals_extracted: r.get("signals_extracted"),
        implied_queries: r.get("implied_queries"),
        similarity: r.get("similarity"),
        confidence: r.get("confidence"),
        success: r.get("success"),
        action: r.get("action"),
        node_id: r.get("node_id"),
        matched_id: r.get("matched_id"),
        existing_id: r.get("existing_id"),
        new_source_url: r.get("new_source_url"),
        canonical_key: r.get("canonical_key"),
        gatherings: r.get("gatherings"),
        needs: r.get("needs"),
        stale: r.get("stale"),
        sources_created: r.get("sources_created"),
        spent_cents: r.get("spent_cents"),
        remaining_cents: r.get("remaining_cents"),
        topics: r.get("topics"),
        posts_found: r.get("posts_found"),
        reason: r.get("reason"),
        strategy: r.get("strategy"),
        field: r.get("field"),
        old_value: r.get("old_value"),
        new_value: r.get("new_value"),
        signal_count: r.get("signal_count"),
        summary: r.get("summary"),
    }
}
