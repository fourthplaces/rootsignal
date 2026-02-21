//! Scout run log — persisted JSON timeline of every action taken during a run.
//!
//! Each run produces a single `{DATA_DIR}/scout-runs/{region}/{run_id}.json` file
//! containing an ordered list of events with timestamps.

use std::path::PathBuf;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use tracing::info;

use crate::scout::ScoutStats;

// ---------------------------------------------------------------------------
// data_dir helper
// ---------------------------------------------------------------------------

/// Root data directory, controlled by `DATA_DIR` env var (default: `"data"`).
/// On Railway, set `DATA_DIR=/data` and mount a persistent volume there.
pub fn data_dir() -> PathBuf {
    PathBuf::from(std::env::var("DATA_DIR").unwrap_or_else(|_| "data".to_string()))
}

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

#[derive(Serialize)]
struct RunEvent {
    seq: u32,
    ts: DateTime<Utc>,
    #[serde(flatten)]
    kind: EventKind,
}

#[derive(Serialize)]
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

    /// Serialize the run log to JSON and write to disk.
    /// Returns the file path on success.
    pub fn save(&self, stats: &ScoutStats) -> Result<PathBuf> {
        let dir = data_dir()
            .join("scout-runs")
            .join(&self.region);
        std::fs::create_dir_all(&dir)?;

        let path = dir.join(format!("{}.json", self.run_id));

        let output = SerializedRunLog {
            run_id: &self.run_id,
            region: &self.region,
            started_at: self.started_at,
            finished_at: Utc::now(),
            stats: SerializedStats::from(stats),
            events: &self.events,
        };

        std::fs::write(&path, serde_json::to_string_pretty(&output)?)?;
        info!(path = %path.display(), events = self.events.len(), "Scout run log saved");

        Ok(path)
    }
}

// ---------------------------------------------------------------------------
// Serialization wrappers
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct SerializedRunLog<'a> {
    run_id: &'a str,
    region: &'a str,
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
    stats: SerializedStats,
    events: &'a [RunEvent],
}

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
