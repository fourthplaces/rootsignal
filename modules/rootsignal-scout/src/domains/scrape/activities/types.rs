//! Type definitions for scrape pipeline output.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::core::extractor::ResourceTag;
use crate::domains::enrichment::activities::link_promoter::CollectedLink;
use rootsignal_common::Node;
use rootsignal_common::telemetry_events::TelemetryEvent;
use seesaw_core::Events;

pub(crate) use crate::core::embedding_cache::EmbeddingCache;
pub(crate) use crate::domains::signals::activities::dedup_utils::{
    batch_title_dedup, dedup_verdict, is_owned_source, normalize_title, score_and_filter,
    DedupVerdict,
};

// ---------------------------------------------------------------------------
// ScrapeOutput — accumulated output from a scrape phase
// ---------------------------------------------------------------------------

/// Accumulated output from a scrape phase (web, social, or topic discovery).
/// Replaces direct mutations to PipelineState during scraping.
pub struct ScrapeOutput {
    /// Events to emit (SignalsExtracted, FreshnessConfirmed, etc.)
    pub events: Events,
    /// New URL→canonical_key mappings discovered during this scrape.
    pub url_mappings: HashMap<String, String>,
    /// Per-source signal counts (canonical_key → count).
    pub source_signal_counts: HashMap<String, u32>,
    /// Canonical keys where the query API errored.
    pub query_api_errors: HashSet<String>,
    /// RSS/Atom pub_dates keyed by article URL.
    pub pub_dates: HashMap<String, DateTime<Utc>>,
    /// Links collected during scraping for promotion.
    pub collected_links: Vec<CollectedLink>,
    /// Expansion queries extracted from signals.
    pub expansion_queries: Vec<String>,
    /// Direct stat mutations not tracked through events.
    pub stats_delta: StatsDelta,
}

/// Direct stat mutations accumulated during scraping.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct StatsDelta {
    pub social_media_posts: u32,
    pub discovery_posts_found: u32,
    pub discovery_accounts_found: u32,
}

impl ScrapeOutput {
    pub fn new() -> Self {
        Self {
            events: Events::new(),
            url_mappings: HashMap::new(),
            source_signal_counts: HashMap::new(),
            query_api_errors: HashSet::new(),
            pub_dates: HashMap::new(),
            collected_links: Vec::new(),
            expansion_queries: Vec::new(),
            stats_delta: StatsDelta::default(),
        }
    }

    /// Take events out, leaving the state-update portion.
    pub fn take_events(&mut self) -> Events {
        std::mem::take(&mut self.events)
    }

    /// Merge another ScrapeOutput into this one.
    pub fn merge(&mut self, other: ScrapeOutput) {
        self.events.extend(other.events);
        self.url_mappings.extend(other.url_mappings);
        for (k, v) in other.source_signal_counts {
            *self.source_signal_counts.entry(k).or_default() += v;
        }
        self.query_api_errors.extend(other.query_api_errors);
        self.pub_dates.extend(other.pub_dates);
        self.collected_links.extend(other.collected_links);
        self.expansion_queries.extend(other.expansion_queries);
        self.stats_delta.social_media_posts += other.stats_delta.social_media_posts;
        self.stats_delta.discovery_posts_found += other.stats_delta.discovery_posts_found;
        self.stats_delta.discovery_accounts_found += other.stats_delta.discovery_accounts_found;
    }
}

/// Output from URL resolution phase (query resolution, page URL collection, blocked URL filtering).
pub struct UrlResolution {
    pub urls: Vec<String>,
    pub url_mappings: HashMap<String, String>,
    pub pub_dates: HashMap<String, DateTime<Utc>>,
    pub query_api_errors: HashSet<String>,
    pub source_count: u32,
}

/// Output from fetch+extract phase (parallel fetch, extract, signal processing).
pub struct FetchExtractResult {
    pub events: Events,
    pub source_signal_counts: HashMap<String, u32>,
    pub collected_links: Vec<CollectedLink>,
    pub expansion_queries: Vec<String>,
    pub stats: FetchExtractStats,
}

/// Per-URL fetch+extract statistics.
#[derive(Debug, Default)]
pub struct FetchExtractStats {
    pub urls_scraped: u32,
    pub urls_unchanged: u32,
    pub urls_failed: u32,
    pub signals_extracted: u32,
}

/// Result from fetching and extracting a single URL.
pub struct SingleUrlResult {
    pub events: Events,
    pub source_signal_counts: HashMap<String, u32>,
    pub collected_links: Vec<CollectedLink>,
    pub expansion_queries: Vec<String>,
    pub scraped: bool,
    pub unchanged: bool,
    pub failed: bool,
    pub signals_extracted: u32,
}

/// Result from scraping a single social source.
pub struct SingleSocialResult {
    pub events: Events,
    pub source_signal_counts: HashMap<String, u32>,
    pub collected_links: Vec<CollectedLink>,
    pub expansion_queries: Vec<String>,
    pub posts_fetched: u32,
    pub signals_extracted: u32,
    pub stats_delta: StatsDelta,
}

pub(crate) enum ScrapeOutcome {
    New {
        content: String,
        nodes: Vec<Node>,
        resource_tags: Vec<(Uuid, Vec<ResourceTag>)>,
        signal_tags: Vec<(Uuid, Vec<String>)>,
        author_actors: HashMap<Uuid, String>,
        logs: Vec<TelemetryEvent>,
    },
    Unchanged,
    Failed,
}
