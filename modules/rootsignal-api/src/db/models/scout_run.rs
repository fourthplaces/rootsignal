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
    pub handler_failures: Option<u32>,
    pub spent_cents: Option<u64>,
}

/// Row from the `events` table (unified event store).
pub struct EventRow {
    pub id: Option<Uuid>,
    pub parent_id: Option<Uuid>,
    pub seq: i64,
    pub ts: DateTime<Utc>,
    pub event_type: String,
    pub data: serde_json::Value,
}

/// Extended event row with run_id, correlation_id, parent_seq, handler_id.
pub struct EventRowFull {
    pub id: Option<Uuid>,
    pub parent_id: Option<Uuid>,
    pub seq: i64,
    pub ts: DateTime<Utc>,
    pub event_type: String,
    pub data: serde_json::Value,
    pub run_id: Option<String>,
    pub correlation_id: Option<Uuid>,
    pub parent_seq: Option<i64>,
    pub handler_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Domain row returned by queries
// ---------------------------------------------------------------------------

pub struct ScoutRunRow {
    pub run_id: String,
    pub region: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub stats: StatsJson,
    pub region_id: Option<String>,
    pub flow_type: Option<String>,
    pub source_ids: Option<serde_json::Value>,
    pub scope: Option<serde_json::Value>,
    pub parent_run_id: Option<String>,
    pub schedule_id: Option<String>,
    pub run_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
    pub cancelled_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

pub async fn list_by_region(pool: &PgPool, region: &str, limit: u32) -> Result<Vec<ScoutRunRow>> {
    let limit = limit.min(100) as i64;

    let rows = sqlx::query(
        r#"
        SELECT run_id, region, started_at, finished_at, stats,
               region_id, flow_type, source_ids, scope,
               parent_run_id, schedule_id, run_at, error, cancelled_at
        FROM runs
        WHERE region = $1
        ORDER BY started_at DESC
        LIMIT $2
        "#,
    )
    .bind(region)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows.iter().map(row_to_scout_run).collect())
}

pub async fn list_by_source_id(pool: &PgPool, source_id: &str, limit: u32) -> Result<Vec<ScoutRunRow>> {
    let limit = limit.min(100) as i64;

    let rows = sqlx::query(
        r#"
        SELECT run_id, region, started_at, finished_at, stats,
               region_id, flow_type, source_ids, scope,
               parent_run_id, schedule_id, run_at, error, cancelled_at
        FROM runs
        WHERE source_ids @> $1::jsonb
        ORDER BY started_at DESC
        LIMIT $2
        "#,
    )
    .bind(serde_json::json!([source_id]))
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows.iter().map(row_to_scout_run).collect())
}

/// Scrape stats for a single source, derived from runs.
pub struct SourceScrapeStats {
    pub last_scraped: Option<DateTime<Utc>>,
    pub scrape_count: u32,
    pub consecutive_empty_runs: u32,
}

pub async fn source_scrape_stats(pool: &PgPool, source_id: &str) -> Result<SourceScrapeStats> {
    let source_json = serde_json::json!([source_id]);

    let row = sqlx::query(
        r#"
        SELECT
            MAX(finished_at) AS last_scraped,
            COUNT(*)::int     AS scrape_count
        FROM runs
        WHERE source_ids @> $1::jsonb
          AND finished_at IS NOT NULL
        "#,
    )
    .bind(&source_json)
    .fetch_one(pool)
    .await?;

    let last_scraped: Option<DateTime<Utc>> = row.try_get("last_scraped").ok().flatten();
    let scrape_count: i32 = row.try_get("scrape_count").unwrap_or(0);

    // Fetch recent signal counts to count trailing empty runs
    let recent_counts: Vec<i32> = sqlx::query_scalar(
        r#"
        SELECT COALESCE((stats->>'signals_extracted')::int, 0)
        FROM runs
        WHERE source_ids @> $1::jsonb
          AND finished_at IS NOT NULL
        ORDER BY finished_at DESC
        LIMIT 20
        "#,
    )
    .bind(&source_json)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let consecutive_empty_runs = recent_counts.iter().take_while(|&&c| c == 0).count() as u32;

    Ok(SourceScrapeStats {
        last_scraped,
        scrape_count: scrape_count as u32,
        consecutive_empty_runs,
    })
}

pub async fn list_recent(pool: &PgPool, limit: u32) -> Result<Vec<ScoutRunRow>> {
    let limit = limit.min(100) as i64;

    let rows = sqlx::query(
        r#"
        SELECT run_id, region, started_at, finished_at, stats,
               region_id, flow_type, source_ids, scope,
               parent_run_id, schedule_id, run_at, error, cancelled_at
        FROM runs
        ORDER BY started_at DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows.iter().map(row_to_scout_run).collect())
}

pub async fn find_by_id(pool: &PgPool, run_id: &str) -> Result<Option<ScoutRunRow>> {
    let row = sqlx::query(
        r#"
        SELECT run_id, region, started_at, finished_at, stats,
               region_id, flow_type, source_ids, scope,
               parent_run_id, schedule_id, run_at, error, cancelled_at
        FROM runs
        WHERE run_id = $1
        "#,
    )
    .bind(run_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.as_ref().map(row_to_scout_run))
}

/// List child runs for a given parent run.
pub async fn list_children(pool: &PgPool, parent_run_id: &str) -> Result<Vec<ScoutRunRow>> {
    let rows = sqlx::query(
        r#"
        SELECT run_id, region, started_at, finished_at, stats,
               region_id, flow_type, source_ids, scope,
               parent_run_id, schedule_id, run_at, error, cancelled_at
        FROM runs
        WHERE parent_run_id = $1
        ORDER BY started_at ASC
        "#,
    )
    .bind(parent_run_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.iter().map(row_to_scout_run).collect())
}

/// List events for a run from the unified event store, ordered by sequence number.
pub async fn list_events_by_run_id(
    pool: &PgPool,
    run_id: &str,
    event_type_filter: Option<&str>,
) -> Result<Vec<EventRow>> {
    let rows = if let Some(et) = event_type_filter {
        sqlx::query(
            r#"
            SELECT seq, ts, event_type, payload AS data, id, parent_id
            FROM events
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
            SELECT seq, ts, event_type, payload AS data, id, parent_id
            FROM events
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
/// Searches by node_id, matched_id, and existing_id within the JSONB payload column.
pub async fn list_events_by_node_id(
    pool: &PgPool,
    node_id: &str,
    limit: u32,
) -> Result<Vec<EventRow>> {
    let limit = limit.min(200) as i64;

    let rows = sqlx::query(
        r#"
        SELECT seq, ts, event_type, payload AS data, id, parent_id
        FROM events
        WHERE payload->>'node_id' = $1
           OR payload->>'matched_id' = $1
           OR payload->>'existing_id' = $1
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

/// Load a single event by its exact sequence number.
pub async fn get_event_by_seq(pool: &PgPool, seq: i64) -> Result<Option<EventRowFull>> {
    let row = sqlx::query(
        r#"
        SELECT seq, ts, event_type, payload AS data, id, parent_id,
               run_id, correlation_id, parent_seq, handler_id
        FROM events
        WHERE seq = $1
        "#,
    )
    .bind(seq)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(row_to_event_full))
}

/// Fetch events starting from a given sequence number (inclusive), ordered ascending.
/// Used for subscription catch-up: replay events the client missed between initial
/// query and subscription connect.
pub async fn get_events_from_seq(
    pool: &PgPool,
    start_seq: i64,
    limit: i64,
) -> Result<Vec<EventRowFull>> {
    let limit = limit.min(500);

    let rows = sqlx::query(
        r#"
        SELECT seq, ts, event_type, payload AS data, id, parent_id,
               run_id, correlation_id, parent_seq, handler_id
        FROM events
        WHERE seq >= $1
        ORDER BY seq ASC
        LIMIT $2
        "#,
    )
    .bind(start_seq)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(row_to_event_full).collect())
}

/// Paginated reverse-chronological event listing with optional filters.
pub async fn list_events_paginated(
    pool: &PgPool,
    search: Option<&str>,
    cursor: Option<i64>,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    run_id: Option<&str>,
    limit: i64,
) -> Result<Vec<EventRowFull>> {
    let limit = limit.min(200);

    let rows = sqlx::query(
        r#"
        SELECT seq, ts, event_type, payload AS data, id, parent_id,
               run_id, correlation_id, parent_seq, handler_id
        FROM events
        WHERE ($1::bigint IS NULL OR seq < $1)
          AND ($2::timestamptz IS NULL OR ts >= $2)
          AND ($3::timestamptz IS NULL OR ts <= $3)
          AND ($4::text IS NULL
               OR payload::text ILIKE '%' || $4 || '%'
               OR event_type ILIKE '%' || $4 || '%'
               OR run_id ILIKE '%' || $4 || '%'
               OR correlation_id::text ILIKE '%' || $4 || '%')
          AND ($6::text IS NULL OR run_id = $6)
        ORDER BY seq DESC
        LIMIT $5
        "#,
    )
    .bind(cursor)
    .bind(from)
    .bind(to)
    .bind(search)
    .bind(limit)
    .bind(run_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(row_to_event_full).collect())
}

/// Fetch all events sharing the same correlation_id as the given event.
/// Returns events ordered by seq, plus the root event's seq (the one with no parent_id).
pub async fn causal_tree(pool: &PgPool, seq: i64) -> Result<(Vec<EventRowFull>, i64)> {
    let rows = sqlx::query(
        r#"
        SELECT e.seq, e.ts, e.event_type, e.payload AS data, e.id, e.parent_id,
               e.run_id, e.correlation_id, e.parent_seq, e.handler_id
        FROM events e
        WHERE e.correlation_id = (SELECT correlation_id FROM events WHERE seq = $1)
          AND e.correlation_id IS NOT NULL
        ORDER BY e.seq
        "#,
    )
    .bind(seq)
    .fetch_all(pool)
    .await?;

    // Root = event with no parent_id (UUID)
    let root_seq = rows
        .iter()
        .find(|r| r.get::<Option<Uuid>, _>("parent_id").is_none())
        .map(|r| r.get::<i64, _>("seq"))
        .unwrap_or(seq);

    Ok((rows.into_iter().map(row_to_event_full).collect(), root_seq))
}

/// Fetch all events for a run_id with handler_id, ordered by seq.
/// Used by the causal flow viewer to build a DAG client-side.
pub async fn causal_flow(pool: &PgPool, run_id: &str) -> Result<Vec<EventRowFull>> {
    let rows = sqlx::query(
        r#"
        SELECT seq, ts, event_type, payload AS data, id, parent_id,
               run_id, correlation_id, parent_seq, handler_id
        FROM events
        WHERE run_id = $1
        ORDER BY seq ASC
        "#,
    )
    .bind(run_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(row_to_event_full).collect())
}

// ---------------------------------------------------------------------------
// Variant-filtered event queries (for outcome pages)
// ---------------------------------------------------------------------------

/// Query events for a run filtered by payload variant name, with LIMIT.
pub async fn list_events_by_variant(
    pool: &PgPool,
    run_id: &str,
    variant: &str,
    limit: i64,
) -> Result<Vec<EventRow>> {
    let rows = sqlx::query(
        r#"
        SELECT seq, ts, event_type, payload AS data, id, parent_id
        FROM events
        WHERE run_id = $1 AND payload->>'type' = $2
        ORDER BY seq
        LIMIT $3
        "#,
    )
    .bind(run_id)
    .bind(variant)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(row_to_event).collect())
}

/// Count events for a run by variant name (for "showing N of M" UI).
pub async fn count_events_by_variant(
    pool: &PgPool,
    run_id: &str,
    variant: &str,
) -> Result<i64> {
    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM events WHERE run_id = $1 AND payload->>'type' = $2",
    )
    .bind(run_id)
    .bind(variant)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

// ---------------------------------------------------------------------------
// Busy checks (uses runs as implicit lock)
// ---------------------------------------------------------------------------

/// Check if a region has a running (non-stale) run of the given flow types.
pub async fn is_region_busy(pool: &PgPool, region_id: &str, flow_types: &[&str]) -> Result<bool> {
    let (busy,): (bool,) = sqlx::query_as(
        "SELECT EXISTS(
             SELECT 1 FROM runs
             WHERE region_id = $1
               AND flow_type = ANY($2)
               AND finished_at IS NULL
               AND cancelled_at IS NULL
               AND started_at >= now() - interval '30 minutes'
         )",
    )
    .bind(region_id)
    .bind(flow_types)
    .fetch_one(pool)
    .await?;
    Ok(busy)
}

/// Check if a specific source is being scouted in a running (non-stale) run.
pub async fn is_source_busy(pool: &PgPool, source_id: &str) -> Result<bool> {
    let (busy,): (bool,) = sqlx::query_as(
        "SELECT EXISTS(
             SELECT 1 FROM runs
             WHERE source_ids @> $1::jsonb
               AND finished_at IS NULL
               AND started_at >= now() - interval '30 minutes'
         )",
    )
    .bind(serde_json::json!([source_id]))
    .fetch_one(pool)
    .await?;
    Ok(busy)
}

// ---------------------------------------------------------------------------
// Chain orchestration helpers
// ---------------------------------------------------------------------------

/// Check if a run completed successfully (has finished_at, no error, not cancelled).
pub async fn run_succeeded(pool: &PgPool, run_id: &str) -> Result<bool> {
    let (ok,): (bool,) = sqlx::query_as(
        "SELECT EXISTS(
             SELECT 1 FROM runs
             WHERE run_id = $1
               AND finished_at IS NOT NULL
               AND error IS NULL
               AND cancelled_at IS NULL
         )",
    )
    .bind(run_id)
    .fetch_one(pool)
    .await?;
    Ok(ok)
}

/// Check if a parent run already has a child of the given flow_type.
pub async fn has_child_run(pool: &PgPool, parent_run_id: &str, flow_type: &str) -> Result<bool> {
    let (exists,): (bool,) = sqlx::query_as(
        "SELECT EXISTS(
             SELECT 1 FROM runs
             WHERE parent_run_id = $1
               AND flow_type = $2
         )",
    )
    .bind(parent_run_id)
    .bind(flow_type)
    .fetch_one(pool)
    .await?;
    Ok(exists)
}

// ---------------------------------------------------------------------------
// Internal
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// JSON helpers (shared by graphql::schema and investigate)
// ---------------------------------------------------------------------------

pub(crate) fn json_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|v| v.as_str()).map(String::from)
}

pub(crate) fn json_u32(v: &serde_json::Value, key: &str) -> Option<u32> {
    v.get(key).and_then(|v| v.as_u64()).map(|n| n as u32)
}

pub(crate) fn json_u64(v: &serde_json::Value, key: &str) -> Option<u64> {
    v.get(key).and_then(|v| v.as_u64())
}

pub(crate) fn json_f64(v: &serde_json::Value, key: &str) -> Option<f64> {
    v.get(key).and_then(|v| v.as_f64())
}

/// Classify an event's layer from its durable name prefix (e.g. "world:gathering_announced" → "world").
pub(crate) fn event_layer(event_type: &str) -> &'static str {
    let prefix = event_type.split_once(':').map(|(p, _)| p).unwrap_or(event_type);
    match prefix {
        "world" => "world",
        "system" | "enrichment" | "signal" | "synthesis" | "discovery" => "system",
        _ => "telemetry",
    }
}

/// Domain prefix extracted from the durable event name (e.g. "signal:dedup_completed" → "signal").
pub(crate) fn event_domain_prefix(event_type: &str) -> &'static str {
    let prefix = event_type.split_once(':').map(|(p, _)| p).unwrap_or("");
    match prefix {
        "world" => "world",
        "system" => "system",
        "telemetry" => "telemetry",
        "enrichment" => "enrichment",
        "expansion" => "expansion",
        "synthesis" => "synthesis",
        "signal" => "signal",
        "discovery" => "discovery",
        "lifecycle" => "lifecycle",
        "scrape" => "scrape",
        "pipeline" => "pipeline",
        "supervisor" => "supervisor",
        "situation_weaving" => "situation_weaving",
        "scheduling" => "scheduling",
        "curiosity" => "curiosity",
        _ => "unknown",
    }
}

pub(crate) fn event_summary(variant_name: &str, data: &serde_json::Value) -> Option<String> {
    match variant_name {
        // ── Telemetry ──────────────────────────────────────────────
        "system_log" => json_str(data, "message"),
        "url_scraped" => {
            let url = json_str(data, "url")?;
            let success = data.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
            if success {
                let bytes = json_u64(data, "content_bytes").unwrap_or(0);
                Some(format!("\"{url}\" {bytes}B"))
            } else {
                Some(format!("FAIL \"{url}\""))
            }
        }
        "feed_scraped" => {
            let url = json_str(data, "url")?;
            let items = json_u32(data, "items").unwrap_or(0);
            Some(format!("\"{url}\" {items} items"))
        }
        "social_scraped" => {
            let platform = json_str(data, "platform")?;
            let id = json_str(data, "identifier").unwrap_or_default();
            let count = json_u32(data, "post_count").unwrap_or(0);
            Some(format!("{platform}:{id} {count} posts"))
        }
        "social_topics_searched" => {
            let platform = json_str(data, "platform")?;
            let found = json_u32(data, "posts_found").unwrap_or(0);
            Some(format!("{platform} {found} posts"))
        }
        "search_performed" => {
            let query = json_str(data, "query")?;
            let count = json_u32(data, "result_count").unwrap_or(0);
            let provider = json_str(data, "provider").unwrap_or_default();
            Some(format!("\"{query}\" → {count} results ({provider})"))
        }
        "llm_extraction_completed" => {
            let url = json_str(data, "source_url")?;
            let n = json_u32(data, "entities_extracted").unwrap_or(0);
            Some(format!("\"{url}\" {n} signals"))
        }
        "budget_checkpoint" => {
            let spent = json_u64(data, "spent_cents").unwrap_or(0);
            let remaining = json_u64(data, "remaining_cents").unwrap_or(0);
            Some(format!("spent {spent}¢, {remaining}¢ left"))
        }
        "bootstrap_completed" => {
            let n = json_u64(data, "sources_created").unwrap_or(0);
            Some(format!("{n} sources created"))
        }
        "agent_web_searched" => {
            let query = json_str(data, "query")?;
            let count = json_u32(data, "result_count").unwrap_or(0);
            Some(format!("\"{query}\" → {count} results"))
        }
        "agent_page_read" => {
            let url = json_str(data, "url")?;
            let chars = json_u64(data, "content_chars").unwrap_or(0);
            Some(format!("\"{url}\" {chars} chars"))
        }
        "agent_future_query" => json_str(data, "query").map(|q| format!("\"{q}\"")),
        "pins_removed" => {
            let ids = data.get("pin_ids").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
            Some(format!("{ids} pins"))
        }
        "demand_aggregated" => {
            let tasks = data.get("created_task_ids").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
            Some(format!("{tasks} tasks created"))
        }

        // ── Lifecycle ──────────────────────────────────────────────
        "engine_started" => json_str(data, "run_id").map(|id| format!("run {id}")),
        "run_completed" => {
            let stats = data.get("stats").unwrap_or(data);
            let scraped = json_u32(stats, "urls_scraped").unwrap_or(0);
            let stored = json_u32(stats, "signals_stored").unwrap_or(0);
            let dedup = json_u32(stats, "signals_deduplicated").unwrap_or(0);
            Some(format!("{scraped} scraped, {stored} stored, {dedup} deduped"))
        }
        "sources_scheduled" => {
            let t = json_u32(data, "tension_count").unwrap_or(0);
            let r = json_u32(data, "response_count").unwrap_or(0);
            Some(format!("{t} tension, {r} response"))
        }
        "metrics_completed" | "news_scan_requested" => None,

        // ── Scrape domain ──────────────────────────────────────────
        "content_fetched" => json_str(data, "url").map(|u| format!("\"{u}\"")),
        "content_unchanged" => json_str(data, "url").map(|u| format!("\"{u}\" (unchanged)")),
        "content_fetch_failed" => {
            let url = json_str(data, "url").unwrap_or_default();
            let err = json_str(data, "error").unwrap_or_default();
            Some(format!("FAIL \"{url}\": {err}"))
        }
        "extraction_failed" => {
            let url = json_str(data, "url").unwrap_or_default();
            let err = json_str(data, "error").unwrap_or_default();
            Some(format!("FAIL \"{url}\": {err}"))
        }
        "social_posts_fetched" => {
            let platform = json_str(data, "platform").unwrap_or_default();
            let count = json_u32(data, "count").unwrap_or(0);
            Some(format!("{platform} {count} posts"))
        }
        "web_urls_resolved" => {
            let role = json_str(data, "role").unwrap_or_default();
            let count = data.get("urls").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
            Some(format!("{role}: {count} urls"))
        }
        "url_fetch_requested" => json_str(data, "url").map(|u| format!("\"{u}\"")),
        "url_scrape_completed" => {
            let url = json_str(data, "url").unwrap_or_default();
            let scraped = data.get("scraped").and_then(|v| v.as_bool()).unwrap_or(false);
            let unchanged = data.get("unchanged").and_then(|v| v.as_bool()).unwrap_or(false);
            let failed = data.get("failed").and_then(|v| v.as_bool()).unwrap_or(false);
            let signals = json_u32(data, "signals_extracted").unwrap_or(0);
            let outcome = if scraped { format!("{signals} signals") }
                else if unchanged { "unchanged".to_string() }
                else if failed { "FAILED".to_string() }
                else { "unknown".to_string() };
            Some(format!("\"{url}\" {outcome}"))
        }
        "social_source_requested" => {
            let platform = json_str(data, "platform").unwrap_or_default();
            let id = json_str(data, "identifier").unwrap_or_default();
            Some(format!("{platform}:{id}"))
        }
        "social_source_completed" => {
            let ck = json_str(data, "canonical_key").unwrap_or_default();
            let posts = json_u32(data, "posts_fetched").unwrap_or(0);
            let signals = json_u32(data, "signals_extracted").unwrap_or(0);
            Some(format!("{ck} {posts} posts, {signals} signals"))
        }
        // Legacy: persisted events from before the split into WebScrapeCompleted etc.
        "scrape_role_completed" => {
            let role = json_str(data, "role").unwrap_or_default();
            let scraped = json_u32(data, "urls_scraped").unwrap_or(0);
            let unchanged = json_u32(data, "urls_unchanged").unwrap_or(0);
            let failed = json_u32(data, "urls_failed").unwrap_or(0);
            let signals = json_u32(data, "signals_extracted").unwrap_or(0);
            Some(format!("{role}: {scraped} scraped, {unchanged} unchanged, {failed} failed, {signals} signals"))
        }
        "web_scrape_completed" => {
            let role = json_str(data, "role").unwrap_or_default();
            let scraped = json_u32(data, "urls_scraped").unwrap_or(0);
            let unchanged = json_u32(data, "urls_unchanged").unwrap_or(0);
            let failed = json_u32(data, "urls_failed").unwrap_or(0);
            let signals = json_u32(data, "signals_extracted").unwrap_or(0);
            Some(format!("{role}: {scraped} scraped, {unchanged} unchanged, {failed} failed, {signals} signals"))
        }
        "social_scrape_completed" => {
            let role = json_str(data, "role").unwrap_or_default();
            let scraped = json_u32(data, "sources_scraped").unwrap_or(0);
            let signals = json_u32(data, "signals_extracted").unwrap_or(0);
            Some(format!("{role}: {scraped} sources, {signals} signals"))
        }
        "topic_discovery_completed" => {
            let signals: u32 = data.get("source_signal_counts")
                .and_then(|v| v.as_object())
                .map(|m| m.values().filter_map(|v| v.as_u64()).sum::<u64>() as u32)
                .unwrap_or(0);
            Some(format!("{signals} signals from topics"))
        }
        "response_scrape_skipped" => {
            let reason = json_str(data, "reason").unwrap_or_default();
            Some(format!("skipped: {reason}"))
        }

        // ── Signal domain ──────────────────────────────────────────
        // Note: both ScrapeEvent::SignalsExtracted and SignalEvent::SignalsExtracted
        // serialize as "signals_extracted" — same formatter works for both.
        "signals_extracted" => {
            let url = json_str(data, "url")?;
            let count = json_u32(data, "count").unwrap_or(0);
            Some(format!("\"{url}\" {count} signals"))
        }
        "new_signal_accepted" => {
            let title = json_str(data, "title")?;
            let nt = json_str(data, "node_type").unwrap_or_default();
            Some(format!("\"{title}\" ({nt})"))
        }
        "cross_source_match_detected" | "same_source_reencountered" => {
            let nt = json_str(data, "node_type").unwrap_or_default();
            let sim = json_f64(data, "similarity").map(|s| format!("{:.0}%", s * 100.0)).unwrap_or_default();
            Some(format!("{nt} ~{sim}"))
        }
        "dedup_completed" => json_str(data, "url").map(|u| format!("\"{u}\"")),
        "signal_created" => {
            let nt = json_str(data, "node_type").unwrap_or_default();
            let key = json_str(data, "canonical_key").unwrap_or_default();
            Some(format!("{nt} from {key}"))
        }
        "url_processed" => {
            let url = json_str(data, "url")?;
            let created = json_u32(data, "signals_created").unwrap_or(0);
            let dedup = json_u32(data, "signals_deduplicated").unwrap_or(0);
            Some(format!("\"{url}\" {created} created, {dedup} deduped"))
        }

        // ── Discovery domain ───────────────────────────────────────
        "source_discovered" => {
            let src = data.get("source");
            let key = src.and_then(|s| s.get("canonical_key")).and_then(|v| v.as_str()).unwrap_or("?");
            let url = src.and_then(|s| s.get("url")).and_then(|v| v.as_str());
            let method = src.and_then(|s| s.get("discovery_method")).and_then(|v| v.as_str()).unwrap_or("");
            let gap = src.and_then(|s| s.get("gap_context")).and_then(|v| v.as_str());
            let link = url.unwrap_or(key);
            match gap {
                Some(g) => Some(format!("{link} via {method} ({g})")),
                None => Some(format!("{link} via {method}")),
            }
        }
        "sources_discovered" => {
            let count = data.get("sources")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            Some(format!("{count} sources proposed"))
        }
        "source_rejected" => {
            let src = data.get("source");
            let key = src.and_then(|s| s.get("canonical_key")).and_then(|v| v.as_str()).unwrap_or("?");
            let reason = json_str(data, "reason").unwrap_or_default();
            Some(format!("{key} rejected: {reason}"))
        }
        "links_promoted" => json_u32(data, "count").map(|n| format!("{n} links")),
        "expansion_query_collected" => json_str(data, "query").map(|q| format!("\"{q}\"")),
        "social_topic_collected" => json_str(data, "topic").map(|t| format!("\"{t}\"")),

        // ── Enrichment domain ──────────────────────────────────────
        "enrichment_role_completed" => json_str(data, "role"),

        // ── World events (signal creation) ─────────────────────────
        "gathering_announced" | "resource_offered" | "help_requested"
        | "announcement_shared" | "concern_raised" | "condition_observed" => {
            json_str(data, "title")
        }
        "citation_published" => {
            let url = json_str(data, "url")?;
            let sid = json_str(data, "signal_id").unwrap_or_default();
            Some(format!("\"{url}\" for signal {sid}"))
        }
        "gathering_cancelled" | "resource_depleted" | "announcement_retracted" => {
            let sid = json_str(data, "signal_id").unwrap_or_default();
            let reason = json_str(data, "reason").unwrap_or_default();
            Some(format!("signal {sid}: {reason}"))
        }
        "citation_retracted" => {
            let cid = json_str(data, "citation_id").unwrap_or_default();
            let reason = json_str(data, "reason").unwrap_or_default();
            Some(format!("citation {cid}: {reason}"))
        }
        "details_changed" => json_str(data, "signal_id").map(|sid| format!("signal {sid}")),
        "resource_identified" => json_str(data, "name"),
        "resource_linked" => {
            let sid = json_str(data, "signal_id").unwrap_or_default();
            let slug = json_str(data, "resource_slug").unwrap_or_default();
            Some(format!("signal {sid} → {slug}"))
        }

        // ── System events ──────────────────────────────────────────
        "duplicate_detected" => {
            let title = json_str(data, "title")?;
            let sim = json_f64(data, "similarity").map(|s| format!("~{:.0}%", s * 100.0)).unwrap_or_default();
            let action = json_str(data, "action").unwrap_or_default();
            Some(format!("\"{title}\" {sim} → {action}"))
        }
        "observation_rejected" => {
            let title = json_str(data, "title")?;
            let reason = json_str(data, "reason").unwrap_or_default();
            Some(format!("\"{title}\" — {reason}"))
        }
        "observation_corroborated" => {
            let sid = json_str(data, "signal_id").unwrap_or_default();
            let url = json_str(data, "new_source_url").unwrap_or_default();
            Some(format!("signal {sid} from {url}"))
        }
        "extraction_dropped_no_date" => json_str(data, "title").map(|t| format!("\"{t}\"")),
        "actor_identified" => {
            let name = json_str(data, "name")?;
            let at = json_str(data, "actor_type").unwrap_or_default();
            Some(format!("\"{name}\" ({at})"))
        }
        "actor_linked_to_signal" => {
            let aid = json_str(data, "actor_id").unwrap_or_default();
            let sid = json_str(data, "signal_id").unwrap_or_default();
            let role = json_str(data, "role").unwrap_or_default();
            Some(format!("actor {aid} → signal {sid} ({role})"))
        }
        "actor_location_identified" => {
            let aid = json_str(data, "actor_id").unwrap_or_default();
            let loc = json_str(data, "location_name").unwrap_or_else(|| "unknown".to_string());
            Some(format!("actor {aid}: {loc}"))
        }
        "source_registered" => {
            let key = json_str(data, "canonical_key")?;
            let method = json_str(data, "discovery_method").unwrap_or_default();
            Some(format!("\"{key}\" via {method}"))
        }
        "sources_registered" => {
            let count = data.get("sources").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
            Some(format!("{count} sources registered"))
        }
        "source_changed" | "source_system_changed" => json_str(data, "canonical_key").map(|k| format!("\"{k}\"")),
        "source_deactivated" => {
            let ids = data.get("source_ids").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
            let reason = json_str(data, "reason").unwrap_or_default();
            Some(format!("{ids} sources: {reason}"))
        }
        "sensitivity_classified" | "tone_classified" | "severity_classified"
        | "urgency_classified" | "category_classified" => {
            let sid = json_str(data, "signal_id").unwrap_or_default();
            let val = json_str(data, "level")
                .or_else(|| json_str(data, "tone"))
                .or_else(|| json_str(data, "severity"))
                .or_else(|| json_str(data, "urgency"))
                .or_else(|| json_str(data, "category"))
                .unwrap_or_default();
            Some(format!("signal {sid}: {val}"))
        }
        "confidence_scored" => {
            let sid = json_str(data, "signal_id").unwrap_or_default();
            let old = json_f64(data, "old_confidence").map(|v| format!("{v:.2}")).unwrap_or_default();
            let new = json_f64(data, "new_confidence").map(|v| format!("{v:.2}")).unwrap_or_default();
            Some(format!("signal {sid}: {old} → {new}"))
        }
        "corroboration_scored" => {
            let sid = json_str(data, "signal_id").unwrap_or_default();
            let count = json_u32(data, "new_corroboration_count").unwrap_or(0);
            Some(format!("signal {sid}: {count} corroborations"))
        }
        "situation_identified" => json_str(data, "headline").map(|h| format!("\"{h}\"")),
        "situation_changed" => json_str(data, "situation_id").map(|id| format!("situation {id}")),
        "situation_promoted" => {
            let ids = data.get("situation_ids").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
            Some(format!("{ids} situations"))
        }
        "dispatch_created" => {
            let dt = json_str(data, "dispatch_type").unwrap_or_default();
            let sid = json_str(data, "situation_id").unwrap_or_default();
            Some(format!("\"{dt}\" for situation {sid}"))
        }
        "dispatch_flagged_for_review" => json_str(data, "reason"),
        "signal_assigned_to_situation" => {
            let sid = json_str(data, "signal_id").unwrap_or_default();
            let sit = json_str(data, "situation_id").unwrap_or_default();
            Some(format!("signal {sid} → situation {sit}"))
        }
        "review_verdict_reached" => {
            let sid = json_str(data, "signal_id").unwrap_or_default();
            let old = json_str(data, "old_status").unwrap_or_default();
            let new = json_str(data, "new_status").unwrap_or_default();
            Some(format!("signal {sid}: {old} → {new}"))
        }
        "response_linked" | "concern_linked" => {
            let sid = json_str(data, "signal_id").unwrap_or_default();
            let cid = json_str(data, "concern_id").unwrap_or_default();
            Some(format!("signal {sid} → concern {cid}"))
        }
        "gathering_corrected" | "resource_corrected" | "help_request_corrected"
        | "announcement_corrected" | "concern_corrected" => {
            let sid = json_str(data, "signal_id").unwrap_or_default();
            let reason = json_str(data, "reason").unwrap_or_default();
            Some(format!("signal {sid}: {reason}"))
        }
        "implied_queries_extracted" => {
            let sid = json_str(data, "signal_id").unwrap_or_default();
            let n = data.get("queries").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
            Some(format!("signal {sid}: {n} queries"))
        }
        "implied_queries_consumed" => {
            let n = data.get("signal_ids").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
            Some(format!("{n} signals"))
        }
        "signals_expired" => {
            let n = data.get("signals").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
            Some(format!("{n} signals expired"))
        }
        "entity_purged" => {
            let sid = json_str(data, "signal_id").unwrap_or_default();
            let reason = json_str(data, "reason").unwrap_or_default();
            Some(format!("signal {sid}: {reason}"))
        }
        "duplicate_actors_merged" => {
            let n = data.get("merged_ids").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
            Some(format!("{n} actors merged"))
        }
        "signal_tagged" => {
            let sid = json_str(data, "signal_id").unwrap_or_default();
            Some(format!("signal {sid}"))
        }
        "situation_tags_aggregated" => json_str(data, "situation_id").map(|id| format!("situation {id}")),
        "pin_created" => json_str(data, "source_id").map(|id| format!("source {id}")),
        "demand_received" => json_str(data, "query").map(|q| format!("\"{q}\"")),
        "submission_received" => json_str(data, "url").map(|u| format!("\"{u}\"")),
        "scout_task_created" => json_str(data, "context"),
        "scout_task_cancelled" => json_str(data, "task_id").map(|id| format!("task {id}")),
        "curiosity_triggered" => json_str(data, "situation_id").map(|id| format!("situation {id}")),
        "expansion_query_collected" => json_str(data, "query").map(|q| format!("\"{q}\"")),
        "source_scraped" => {
            let key = json_str(data, "canonical_key").unwrap_or_default();
            let n = json_u32(data, "signals_produced").unwrap_or(0);
            Some(format!("\"{key}\" {n} signals"))
        }
        "place_discovered" => json_str(data, "name"),
        "beacon_detected" => Some("new task".to_string()),
        "concern_linker_outcome_recorded" => {
            let sid = json_str(data, "signal_id").unwrap_or_default();
            let outcome = json_str(data, "outcome").unwrap_or_default();
            Some(format!("signal {sid}: {outcome}"))
        }

        // ── Synthesis per-target events ────────────────────────────
        "synthesis_targets_dispatched" => {
            let role = json_str(data, "role").unwrap_or_default();
            let count = json_u32(data, "count").unwrap_or(0);
            Some(format!("{role}: {count} targets"))
        }
        "concern_linker_target_requested" => {
            let title = json_str(data, "signal_title").unwrap_or_default();
            Some(format!("investigating: {title}"))
        }
        "concern_linker_target_completed" => {
            let outcome = json_str(data, "outcome").unwrap_or_default();
            let tensions = json_u32(data, "tensions_discovered").unwrap_or(0);
            let edges = json_u32(data, "edges_created").unwrap_or(0);
            Some(format!("{outcome}: {tensions} tensions, {edges} edges"))
        }
        "response_finder_target_requested" => {
            let title = json_str(data, "concern_title").unwrap_or_default();
            Some(format!("scouting: {title}"))
        }
        "response_finder_target_completed" => {
            let responses = json_u32(data, "responses_discovered").unwrap_or(0);
            let edges = json_u32(data, "edges_created").unwrap_or(0);
            Some(format!("{responses} responses, {edges} edges"))
        }
        "gathering_finder_target_requested" => {
            let title = json_str(data, "concern_title").unwrap_or_default();
            Some(format!("finding gravity: {title}"))
        }
        "gathering_finder_target_completed" => {
            let gatherings = json_u32(data, "gatherings_discovered").unwrap_or(0);
            let no_gravity = data.get("no_gravity").and_then(|v| v.as_bool()).unwrap_or(false);
            let edges = json_u32(data, "edges_created").unwrap_or(0);
            if no_gravity {
                Some("no gravity found".to_string())
            } else {
                Some(format!("{gatherings} gatherings, {edges} edges"))
            }
        }
        "investigation_target_requested" => {
            let title = json_str(data, "signal_title").unwrap_or_default();
            Some(format!("investigating: {title}"))
        }
        "investigation_target_completed" => {
            let evidence = json_u32(data, "evidence_created").unwrap_or(0);
            let adjusted = data.get("confidence_adjusted").and_then(|v| v.as_bool()).unwrap_or(false);
            let suffix = if adjusted { " (confidence revised)" } else { "" };
            Some(format!("{evidence} evidence{suffix}"))
        }
        "response_mapping_target_requested" => {
            let title = json_str(data, "concern_title").unwrap_or_default();
            Some(format!("mapping: {title}"))
        }
        "response_mapping_target_completed" => {
            let edges = json_u32(data, "edges_created").unwrap_or(0);
            Some(format!("{edges} edges created"))
        }

        // ── Pipeline bookkeeping ─────────────────────────────────
        "handler_skipped" => {
            let handler = json_str(data, "handler_id").unwrap_or_default();
            let reason = json_str(data, "reason").unwrap_or_default();
            Some(format!("{handler}: {reason}"))
        }
        "handler_failed" => {
            let handler = json_str(data, "handler_id").unwrap_or_default();
            let error = json_str(data, "error").unwrap_or_default();
            let attempts = data.get("attempts").and_then(|v| v.as_i64()).unwrap_or(0);
            Some(format!("{handler} failed after {attempts} attempts: {error}"))
        }

        // ── Fallback: generic field sniffing ───────────────────────
        _ => json_str(data, "message")
            .or_else(|| json_str(data, "title"))
            .or_else(|| json_str(data, "summary"))
            .or_else(|| json_str(data, "headline"))
            .or_else(|| json_str(data, "reason"))
            .or_else(|| json_str(data, "canonical_key"))
            .or_else(|| json_str(data, "url"))
            .or_else(|| json_str(data, "query"))
            .or_else(|| json_str(data, "source_url"))
            .or_else(|| json_str(data, "signal_id").map(|id| format!("signal {id}")))
            .or_else(|| json_str(data, "node_id").map(|id| format!("node {id}")))
            .or_else(|| json_str(data, "phase")),
    }
}

fn row_to_scout_run(r: &sqlx::postgres::PgRow) -> ScoutRunRow {
    ScoutRunRow {
        run_id: r.get("run_id"),
        region: r.get("region"),
        started_at: r.get("started_at"),
        finished_at: r.get("finished_at"),
        stats: serde_json::from_value(r.get::<serde_json::Value, _>("stats")).unwrap_or_default(),
        region_id: r.get("region_id"),
        flow_type: r.get("flow_type"),
        source_ids: r.get("source_ids"),
        scope: r.get("scope"),
        parent_run_id: r.get("parent_run_id"),
        schedule_id: r.get("schedule_id"),
        run_at: r.get("run_at"),
        error: r.get("error"),
        cancelled_at: r.get("cancelled_at"),
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

// ---------------------------------------------------------------------------
// Handler logs
// ---------------------------------------------------------------------------

pub struct HandlerLogRow {
    pub level: String,
    pub message: String,
    pub data: Option<serde_json::Value>,
    pub logged_at: DateTime<Utc>,
}

pub struct HandlerLogRowFull {
    pub event_id: Uuid,
    pub handler_id: String,
    pub level: String,
    pub message: String,
    pub data: Option<serde_json::Value>,
    pub logged_at: DateTime<Utc>,
}

pub async fn handler_logs(
    pool: &PgPool,
    event_id: &Uuid,
    handler_id: &str,
) -> Result<Vec<HandlerLogRow>> {
    let rows = sqlx::query_as::<_, (String, String, Option<serde_json::Value>, DateTime<Utc>)>(
        "SELECT level, message, data, logged_at \
         FROM seesaw_handler_logs \
         WHERE event_id = $1 AND handler_id = $2 \
         ORDER BY id",
    )
    .bind(event_id)
    .bind(handler_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(level, message, data, logged_at)| HandlerLogRow {
            level,
            message,
            data,
            logged_at,
        })
        .collect())
}

/// Fetch all handler logs for a run, identified by run_id (which equals correlation_id as UUID).
pub async fn handler_logs_by_run(
    pool: &PgPool,
    run_id: &str,
) -> Result<Vec<HandlerLogRowFull>> {
    let correlation_id = Uuid::parse_str(run_id)
        .map_err(|e| anyhow::anyhow!("Invalid run_id as UUID: {e}"))?;

    let rows = sqlx::query_as::<_, (Uuid, String, String, String, Option<serde_json::Value>, DateTime<Utc>)>(
        "SELECT event_id, handler_id, level, message, data, logged_at \
         FROM seesaw_handler_logs \
         WHERE correlation_id = $1 \
         ORDER BY id",
    )
    .bind(correlation_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(event_id, handler_id, level, message, data, logged_at)| HandlerLogRowFull {
            event_id,
            handler_id,
            level,
            message,
            data,
            logged_at,
        })
        .collect())
}

pub struct HandlerOutcomeRow {
    pub handler_id: String,
    pub status: String,
    pub error: Option<String>,
    pub attempts: i64,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub triggering_event_ids: Vec<String>,
}

pub async fn handler_outcomes(
    pool: &PgPool,
    run_id: &str,
) -> Result<Vec<HandlerOutcomeRow>> {
    let correlation_id = Uuid::parse_str(run_id)
        .map_err(|e| anyhow::anyhow!("Invalid run_id as UUID: {e}"))?;

    let rows = sqlx::query_as::<_, (String, String, Option<String>, i64, Option<DateTime<Utc>>, Option<DateTime<Utc>>, Option<Vec<String>>)>(
        "SELECT handler_id, \
                CASE WHEN bool_or(status = 'error') THEN 'error' \
                     WHEN bool_or(status = 'running') THEN 'running' \
                     WHEN bool_or(status = 'pending') AND bool_or(status = 'completed') THEN 'running' \
                     WHEN bool_or(status = 'pending') THEN 'pending' \
                     ELSE 'completed' END AS status, \
                string_agg(DISTINCT error, '; ') FILTER (WHERE error IS NOT NULL) AS error, \
                COALESCE(SUM(attempts), 0) AS attempts, \
                MIN(created_at) AS started_at, \
                MAX(updated_at) FILTER (WHERE status = 'completed') AS completed_at, \
                array_agg(DISTINCT event_id::text) AS triggering_event_ids \
         FROM seesaw_effect_executions \
         WHERE correlation_id = $1 \
         GROUP BY handler_id",
    )
    .bind(correlation_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(handler_id, status, error, attempts, started_at, completed_at, triggering_event_ids)| HandlerOutcomeRow {
            handler_id,
            status,
            error,
            attempts,
            started_at,
            completed_at,
            triggering_event_ids: triggering_event_ids.unwrap_or_default(),
        })
        .collect())
}

pub struct HandlerDescriptionRow {
    pub handler_id: String,
    pub description: serde_json::Value,
}

pub async fn handler_descriptions(
    pool: &PgPool,
    run_id: &str,
) -> Result<Vec<HandlerDescriptionRow>> {
    let correlation_id = Uuid::parse_str(run_id)
        .map_err(|e| anyhow::anyhow!("Invalid run_id as UUID: {e}"))?;

    let rows = sqlx::query_as::<_, (String, serde_json::Value)>(
        "SELECT handler_id, description \
         FROM seesaw_handler_descriptions \
         WHERE correlation_id = $1",
    )
    .bind(correlation_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(handler_id, description)| HandlerDescriptionRow {
            handler_id,
            description,
        })
        .collect())
}

fn row_to_event_full(r: sqlx::postgres::PgRow) -> EventRowFull {
    EventRowFull {
        id: r.get("id"),
        parent_id: r.get("parent_id"),
        seq: r.get("seq"),
        ts: r.get("ts"),
        event_type: r.get("event_type"),
        data: r.get::<serde_json::Value, _>("data"),
        run_id: r.get("run_id"),
        correlation_id: r.get("correlation_id"),
        parent_seq: r.get("parent_seq"),
        handler_id: r.get("handler_id"),
    }
}
