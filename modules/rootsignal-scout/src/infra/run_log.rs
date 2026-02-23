//! Scout run log — persisted timeline of every action taken during a run.
//!
//! Each run produces a single row in the `scout_runs` Postgres table
//! containing JSONB columns for stats and events.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::info;

use crate::pipeline::stats::ScoutStats;

// ---------------------------------------------------------------------------
// RunLog
// ---------------------------------------------------------------------------

pub struct RunLog {
    pub run_id: String,
    pub region: String,
    pub started_at: DateTime<Utc>,
    events: Vec<RunEvent>,
    seq: u32,
}

#[derive(Serialize, Deserialize)]
pub struct RunEvent {
    pub seq: u32,
    pub ts: DateTime<Utc>,
    #[serde(flatten)]
    pub kind: EventKind,
}

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
    },
    SignalCorroborated {
        existing_id: String,
        signal_type: String,
        new_source_url: String,
        similarity: f64,
    },
    ExpansionQueryCollected {
        query: String,
    },
    ExpansionSourceCreated {
        canonical_key: String,
        query: String,
    },
    BudgetCheckpoint {
        spent_cents: u64,
        remaining_cents: u64,
    },
}

impl RunLog {
    pub fn new(run_id: String, region: String) -> Self {
        Self {
            run_id,
            region,
            started_at: Utc::now(),
            events: Vec::new(),
            seq: 0,
        }
    }

    pub fn log(&mut self, kind: EventKind) {
        self.events.push(RunEvent {
            seq: self.seq,
            ts: Utc::now(),
            kind,
        });
        self.seq += 1;
    }

    /// Serialize the run log and write to Postgres.
    pub async fn save_to_db(&self, pool: &PgPool, stats: &ScoutStats) -> Result<()> {
        let stats_json = serde_json::to_value(SerializedStats::from(stats))?;
        let events_json = serde_json::to_value(&self.events)?;

        sqlx::query(
            r#"
            INSERT INTO scout_runs (run_id, region, started_at, finished_at, stats, events)
            VALUES ($1, $2, $3, now(), $4, $5)
            "#,
        )
        .bind(&self.run_id)
        .bind(&self.region)
        .bind(self.started_at)
        .bind(&stats_json)
        .bind(&events_json)
        .execute(pool)
        .await?;

        info!(run_id = %self.run_id, events = self.events.len(), "Scout run log saved to Postgres");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Serialization wrappers
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
