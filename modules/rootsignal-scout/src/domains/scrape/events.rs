//! Scrape domain events: content fetching, extraction, social scraping.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domains::enrichment::activities::link_promoter::CollectedLink;
use crate::domains::scrape::activities::{StatsDelta, UrlExtraction};

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
        /// Extracted batches per URL — in-memory only, skipped during serialization.
        /// On replay, deserializes as empty (correct: replay rebuilds from downstream facts).
        #[serde(skip)]
        extracted_batches: Vec<UrlExtraction>,
        /// Sources discovered during this role (e.g. topic discovery) — in-memory only.
        #[serde(skip)]
        discovered_sources: Vec<rootsignal_common::SourceNode>,
    },
}

impl ScrapeEvent {
    pub fn run_id(&self) -> Uuid {
        match self {
            Self::SourcesResolved { run_id, .. }
            | Self::ScrapeRoleCompleted { run_id, .. } => *run_id,
        }
    }
}
