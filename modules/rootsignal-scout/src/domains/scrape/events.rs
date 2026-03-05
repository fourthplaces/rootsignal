//! Scrape domain events: content fetching, extraction, social scraping.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::events::FreshnessBucket;
use crate::domains::enrichment::activities::link_promoter::CollectedLink;
use crate::domains::scrape::activities::StatsDelta;

/// Sub-phase role within a scrape phase (tension or response).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrapeRole {
    TensionWeb,
    TensionSocial,
    ResponseWeb,
    ResponseSocial,
    TopicDiscovery,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScrapeEvent {
    ContentFetched {
        run_id: Uuid,
        url: String,
        canonical_key: String,
        content_hash: String,
        link_count: u32,
    },
    ContentUnchanged {
        run_id: Uuid,
        url: String,
        canonical_key: String,
    },
    ContentFetchFailed {
        run_id: Uuid,
        url: String,
        canonical_key: String,
        error: String,
    },
    SignalsExtracted {
        run_id: Uuid,
        url: String,
        canonical_key: String,
        count: u32,
    },
    ExtractionFailed {
        run_id: Uuid,
        url: String,
        canonical_key: String,
        error: String,
    },
    SocialPostsFetched {
        run_id: Uuid,
        canonical_key: String,
        platform: String,
        count: u32,
    },
    FreshnessRecorded {
        run_id: Uuid,
        node_id: Uuid,
        published_at: Option<DateTime<Utc>>,
        bucket: FreshnessBucket,
    },
    LinkCollected {
        run_id: Uuid,
        url: String,
        discovered_on: String,
    },
    /// Sources resolved for a scrape phase — triggers scrape_web, scrape_social, fetch_topics.
    SourcesResolved {
        run_id: Uuid,
        web_role: ScrapeRole,
        web_urls: Vec<String>,
        /// canonical_key → source_id, populated by the resolve handler so
        /// downstream fetch handlers don't re-derive it from sources.
        web_source_keys: HashMap<String, Uuid>,
        web_source_count: u32,
        url_mappings: HashMap<String, String>,
        pub_dates: HashMap<String, DateTime<Utc>>,
        query_api_errors: HashSet<String>,
    },
    /// A scrape sub-phase completed fetch+extract.
    ScrapeRoleCompleted {
        run_id: Uuid,
        role: ScrapeRole,
        urls_scraped: u32,
        urls_unchanged: u32,
        urls_failed: u32,
        signals_extracted: u32,
        /// Accumulated per-source signal counts from this role's scrape.
        #[serde(default)]
        source_signal_counts: HashMap<String, u32>,
        /// Links collected during this role's scrape for promotion.
        #[serde(default)]
        collected_links: Vec<CollectedLink>,
        /// Expansion queries extracted from signals during this role's scrape.
        #[serde(default)]
        expansion_queries: Vec<String>,
        /// Social/discovery stats delta from this role's scrape.
        #[serde(default)]
        stats_delta: StatsDelta,
        /// Content previews (first 500 chars) keyed by URL, for page triage.
        #[serde(default)]
        page_previews: HashMap<String, String>,
    },
}

impl ScrapeEvent {
    pub fn run_id(&self) -> Uuid {
        match self {
            Self::ContentFetched { run_id, .. }
            | Self::ContentUnchanged { run_id, .. }
            | Self::ContentFetchFailed { run_id, .. }
            | Self::SignalsExtracted { run_id, .. }
            | Self::ExtractionFailed { run_id, .. }
            | Self::SocialPostsFetched { run_id, .. }
            | Self::FreshnessRecorded { run_id, .. }
            | Self::LinkCollected { run_id, .. }
            | Self::SourcesResolved { run_id, .. }
            | Self::ScrapeRoleCompleted { run_id, .. } => *run_id,
        }
    }
}
