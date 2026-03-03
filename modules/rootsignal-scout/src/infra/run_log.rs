//! Scout run logger — writes events directly to Postgres as they happen.
//!
//! Each `.track()` call inserts a row into `scout_run_events` immediately,
//! providing live observability and crash resilience. Events form a tree
//! via `id`/`parent_id` for causal nesting (e.g. ScrapeUrl → LlmExtraction → SignalCreated).

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

use crate::core::stats::ScoutStats;

// ---------------------------------------------------------------------------
// RunLogger — root logger for a scout run
// ---------------------------------------------------------------------------

/// Root logger for a scout run. Each `.track()` inserts a row immediately.
#[derive(Clone)]
pub struct RunLogger {
    pub run_id: String,
    pub region: String,
    pub started_at: DateTime<Utc>,
    pool: Option<PgPool>,
    seq: Arc<AtomicU32>,
    /// In-memory fallback for noop loggers (used in tests / workflow free functions)
    events: Arc<std::sync::Mutex<Vec<InMemoryEvent>>>,
}

/// Handle to a logged event. Call `.track()` on it to log child events.
pub struct EventHandle {
    event_id: Uuid,
    run_id: String,
    pool: Option<PgPool>,
    seq: Arc<AtomicU32>,
    events: Arc<std::sync::Mutex<Vec<InMemoryEvent>>>,
}

/// In-memory event record for noop loggers and test assertions.
#[derive(Clone)]
struct InMemoryEvent {
    _seq: u32,
    event_type: String,
}

impl RunLogger {
    /// Create a RunLogger that writes events to Postgres.
    /// Inserts the `scout_runs` row immediately so the FK on `scout_run_events` is satisfied.
    pub async fn new(run_id: String, region: String, pool: PgPool) -> Self {
        let started_at = Utc::now();

        // Create the scout_runs row up front so event FKs are satisfied during the run.
        if let Err(e) = sqlx::query(
            r#"
            INSERT INTO scout_runs (run_id, region, started_at, finished_at, stats)
            VALUES ($1, $2, $3, $3, '{}')
            ON CONFLICT (run_id) DO NOTHING
            "#,
        )
        .bind(&run_id)
        .bind(&region)
        .bind(started_at)
        .execute(&pool)
        .await
        {
            warn!(run_id = %run_id, error = %e, "Failed to create scout_runs row");
        }

        Self {
            run_id,
            region,
            started_at,
            pool: Some(pool),
            seq: Arc::new(AtomicU32::new(0)),
            events: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    /// Create a no-op RunLogger that tracks events in memory but never persists.
    /// Used by free functions called from the Restate workflow path where
    /// run-level logging is handled by the workflow itself.
    pub fn noop() -> Self {
        Self {
            run_id: String::new(),
            region: String::new(),
            started_at: Utc::now(),
            pool: None,
            seq: Arc::new(AtomicU32::new(0)),
            events: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    /// Log a root-level event (no parent). Returns a handle for nesting children.
    /// Fire-and-forget: spawns a background INSERT.
    fn track_impl(&self, kind: EventKind, parent_id: Option<Uuid>) -> EventHandle {
        let id = Uuid::new_v4();
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let event_type = kind.event_type();

        if let Some(ref pool) = self.pool {
            let pool = pool.clone();
            let run_id = self.run_id.clone();
            tokio::spawn(async move {
                if let Err(e) = insert_event(&pool, &run_id, id, parent_id, seq, &kind).await {
                    warn!(error = %e, event_type, "Failed to insert scout run event");
                }
            });
        }

        // Track in memory for test assertions
        if let Ok(mut events) = self.events.lock() {
            events.push(InMemoryEvent {
                _seq: seq,
                event_type: event_type.to_string(),
            });
        }

        EventHandle {
            event_id: id,
            run_id: self.run_id.clone(),
            pool: self.pool.clone(),
            seq: self.seq.clone(),
            events: self.events.clone(),
        }
    }

    /// Check if any event matches the given type tag (for test assertions).
    pub fn has_event_type(&self, type_tag: &str) -> bool {
        if let Ok(events) = self.events.lock() {
            events.iter().any(|e| e.event_type == type_tag)
        } else {
            false
        }
    }

    /// Save run stats to Postgres (events are already persisted row-by-row).
    /// The `scout_runs` row was created in `new()` — this updates it with final stats.
    pub async fn save_stats(&self, pool: &PgPool, stats: &ScoutStats) -> Result<()> {
        let stats_json = serde_json::to_value(SerializedStats::from(stats))?;

        sqlx::query(
            r#"
            UPDATE scout_runs
            SET finished_at = now(), stats = $2
            WHERE run_id = $1
            "#,
        )
        .bind(&self.run_id)
        .bind(&stats_json)
        .execute(pool)
        .await?;

        info!(run_id = %self.run_id, "Scout run stats saved to Postgres");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// EventLogger trait — shared interface for RunLogger and EventHandle
// ---------------------------------------------------------------------------

/// Common interface for logging events at any level of the tree.
/// `RunLogger` logs root events; `EventHandle` logs children under a parent.
pub trait EventLogger: Send + Sync {
    fn track(&self, kind: EventKind) -> EventHandle;
    fn log(&self, kind: EventKind) {
        self.track(kind);
    }
}

impl EventLogger for RunLogger {
    fn track(&self, kind: EventKind) -> EventHandle {
        self.track_impl(kind, None)
    }
}

impl EventLogger for EventHandle {
    fn track(&self, kind: EventKind) -> EventHandle {
        // Reuse RunLogger's shared implementation but with our event_id as parent
        let id = Uuid::new_v4();
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let event_type = kind.event_type();

        if let Some(ref pool) = self.pool {
            let pool = pool.clone();
            let run_id = self.run_id.clone();
            let parent_id = self.event_id;
            tokio::spawn(async move {
                if let Err(e) = insert_event(&pool, &run_id, id, Some(parent_id), seq, &kind).await
                {
                    warn!(error = %e, event_type, "Failed to insert scout run event");
                }
            });
        }

        if let Ok(mut events) = self.events.lock() {
            events.push(InMemoryEvent {
                _seq: seq,
                event_type: event_type.to_string(),
            });
        }

        EventHandle {
            event_id: id,
            run_id: self.run_id.clone(),
            pool: self.pool.clone(),
            seq: self.seq.clone(),
            events: self.events.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// EventKind — all event types
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    ReapExpired {
        gatherings: u64,
        needs: u64,
        stale: u64,
    },
    Bootstrap {
        sources_created: u64,
    },
    SearchQuery {
        query: String,
        provider: String,
        result_count: u32,
        canonical_key: String,
    },
    ScrapeUrl {
        url: String,
        strategy: String,
        success: bool,
        content_bytes: usize,
    },
    ScrapeFeed {
        url: String,
        items: u32,
    },
    SocialScrape {
        platform: String,
        identifier: String,
        post_count: u32,
    },
    SocialTopicSearch {
        platform: String,
        topics: Vec<String>,
        posts_found: u32,
    },
    LlmExtraction {
        source_url: String,
        content_chars: usize,
        signals_extracted: u32,
        implied_queries: u32,
    },
    SignalCreated {
        node_id: String,
        signal_type: String,
        title: String,
        confidence: f64,
        source_url: String,
    },
    SignalDeduplicated {
        signal_type: String,
        title: String,
        matched_id: String,
        similarity: f64,
        action: String,
        source_url: String,
        summary: String,
    },
    SignalCorroborated {
        existing_id: String,
        signal_type: String,
        title: String,
        new_source_url: String,
        similarity: f64,
        summary: String,
    },
    SignalRejected {
        title: String,
        source_url: String,
        reason: String,
    },
    ExpansionQueryCollected {
        query: String,
        source_url: String,
    },
    ExpansionSourceCreated {
        canonical_key: String,
        query: String,
        source_url: String,
    },
    SignalDroppedNoDate {
        title: String,
        source_url: String,
    },
    BudgetCheckpoint {
        spent_cents: u64,
        remaining_cents: u64,
    },
    LintBatch {
        source_url: String,
        signal_count: u32,
        passed: u32,
        corrected: u32,
        rejected: u32,
    },
    LintCorrection {
        node_id: String,
        signal_type: String,
        title: String,
        field: String,
        old_value: String,
        new_value: String,
        reason: String,
    },
    LintRejection {
        node_id: String,
        signal_type: String,
        title: String,
        reason: String,
    },
    AgentWebSearch {
        provider: String,
        query: String,
        result_count: u32,
        title: String,
    },
    AgentPageRead {
        provider: String,
        url: String,
        content_chars: usize,
        title: String,
    },
    AgentFutureQuery {
        provider: String,
        query: String,
        title: String,
    },
}

impl EventKind {
    /// Return the snake_case event type string for this variant.
    pub fn event_type(&self) -> &'static str {
        match self {
            EventKind::ReapExpired { .. } => "reap_expired",
            EventKind::Bootstrap { .. } => "bootstrap",
            EventKind::SearchQuery { .. } => "search_query",
            EventKind::ScrapeUrl { .. } => "scrape_url",
            EventKind::ScrapeFeed { .. } => "scrape_feed",
            EventKind::SocialScrape { .. } => "social_scrape",
            EventKind::SocialTopicSearch { .. } => "social_topic_search",
            EventKind::LlmExtraction { .. } => "llm_extraction",
            EventKind::SignalCreated { .. } => "signal_created",
            EventKind::SignalDeduplicated { .. } => "signal_deduplicated",
            EventKind::SignalCorroborated { .. } => "signal_corroborated",
            EventKind::SignalRejected { .. } => "signal_rejected",
            EventKind::ExpansionQueryCollected { .. } => "expansion_query_collected",
            EventKind::ExpansionSourceCreated { .. } => "expansion_source_created",
            EventKind::SignalDroppedNoDate { .. } => "signal_dropped_no_date",
            EventKind::BudgetCheckpoint { .. } => "budget_checkpoint",
            EventKind::LintBatch { .. } => "lint_batch",
            EventKind::LintCorrection { .. } => "lint_correction",
            EventKind::LintRejection { .. } => "lint_rejection",
            EventKind::AgentWebSearch { .. } => "agent_web_search",
            EventKind::AgentPageRead { .. } => "agent_page_read",
            EventKind::AgentFutureQuery { .. } => "agent_future_query",
        }
    }
}

// ---------------------------------------------------------------------------
// insert_event — write a single event row to Postgres
// ---------------------------------------------------------------------------

async fn insert_event(
    pool: &PgPool,
    run_id: &str,
    id: Uuid,
    parent_id: Option<Uuid>,
    seq: u32,
    kind: &EventKind,
) -> Result<()> {
    let event_type = kind.event_type();
    let data = serde_json::to_value(kind)?;

    sqlx::query(
        r#"
        INSERT INTO scout_run_events (id, parent_id, run_id, seq, ts, event_type, data)
        VALUES ($1, $2, $3, $4, now(), $5, $6)
        "#,
    )
    .bind(id)
    .bind(parent_id)
    .bind(run_id)
    .bind(seq as i32)
    .bind(event_type)
    .bind(data)
    .execute(pool)
    .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Serialization wrappers (stats only — events are in the events table)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct SerializedStats {
    urls_scraped: u32,
    urls_unchanged: u32,
    urls_failed: u32,
    signals_extracted: u32,
    signals_deduplicated: u32,
    signals_stored: u32,
    social_media_posts: u32,
    expansion_queries_collected: u32,
    expansion_sources_created: u32,
}

impl From<&ScoutStats> for SerializedStats {
    fn from(s: &ScoutStats) -> Self {
        Self {
            urls_scraped: s.urls_scraped,
            urls_unchanged: s.urls_unchanged,
            urls_failed: s.urls_failed,
            signals_extracted: s.signals_extracted,
            signals_deduplicated: s.signals_deduplicated,
            signals_stored: s.signals_stored,
            social_media_posts: s.social_media_posts,
            expansion_queries_collected: s.expansion_queries_collected,
            expansion_sources_created: s.expansion_sources_created,
        }
    }
}
