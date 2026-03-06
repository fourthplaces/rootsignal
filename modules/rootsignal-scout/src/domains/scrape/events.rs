//! Scrape domain events: content fetching, extraction, social scraping.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domains::enrichment::activities::link_promoter::CollectedLink;
use crate::domains::scrape::activities::{StatsDelta, UrlExtraction};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScrapeEvent {
    /// Sources resolved for response phase — triggers response scrape handlers.
    SourcesResolved {
        run_id: Uuid,
        is_response_phase: bool,
        web_urls: Vec<String>,
        /// canonical_key → source_id, populated by the resolve handler so
        /// downstream fetch handlers don't re-derive it from sources.
        web_source_keys: HashMap<String, Uuid>,
        web_source_count: u32,
        url_mappings: HashMap<String, String>,
        pub_dates: HashMap<String, DateTime<Utc>>,
        query_api_errors: std::collections::HashSet<String>,
    },
    /// Web scrape handler completed fetch+extract for a batch of URLs.
    WebScrapeCompleted {
        run_id: Uuid,
        is_tension: bool,
        urls_scraped: u32,
        urls_unchanged: u32,
        urls_failed: u32,
        signals_extracted: u32,
        #[serde(default)]
        source_signal_counts: HashMap<String, u32>,
        #[serde(default)]
        collected_links: Vec<CollectedLink>,
        #[serde(default)]
        expansion_queries: Vec<String>,
        #[serde(default)]
        page_previews: HashMap<String, String>,
        /// Extracted batches per URL — in-memory only, skipped during serialization.
        /// On replay, deserializes as empty (correct: replay rebuilds from downstream facts).
        #[serde(skip)]
        extracted_batches: Vec<UrlExtraction>,
    },
    /// Social scrape handler completed fetch+extract for social sources.
    SocialScrapeCompleted {
        run_id: Uuid,
        is_tension: bool,
        sources_scraped: u32,
        signals_extracted: u32,
        #[serde(default)]
        source_signal_counts: HashMap<String, u32>,
        #[serde(default)]
        collected_links: Vec<CollectedLink>,
        #[serde(default)]
        expansion_queries: Vec<String>,
        #[serde(default)]
        stats_delta: StatsDelta,
        #[serde(skip)]
        extracted_batches: Vec<UrlExtraction>,
    },
    /// Topic discovery handler completed discovery from social topics.
    TopicDiscoveryCompleted {
        run_id: Uuid,
        #[serde(default)]
        source_signal_counts: HashMap<String, u32>,
        #[serde(default)]
        collected_links: Vec<CollectedLink>,
        #[serde(default)]
        expansion_queries: Vec<String>,
        #[serde(default)]
        stats_delta: StatsDelta,
        #[serde(skip)]
        extracted_batches: Vec<UrlExtraction>,
    },
    /// Response scrape skipped entirely (missing region or graph).
    /// Reducer marks all response completion flags so downstream gates pass.
    ResponseScrapeSkipped {
        reason: String,
    },
}

impl ScrapeEvent {
    /// Is this a scrape completion event?
    pub fn is_completion(&self) -> bool {
        matches!(
            self,
            Self::WebScrapeCompleted { .. }
                | Self::SocialScrapeCompleted { .. }
                | Self::TopicDiscoveryCompleted { .. }
        )
    }

    /// Is this a tension-phase completion?
    pub fn is_tension_completion(&self) -> bool {
        match self {
            Self::WebScrapeCompleted { is_tension, .. } => *is_tension,
            Self::SocialScrapeCompleted { is_tension, .. } => *is_tension,
            _ => false,
        }
    }

    /// Is this a response-phase completion?
    pub fn is_response_completion(&self) -> bool {
        match self {
            Self::WebScrapeCompleted { is_tension, .. } => !*is_tension,
            Self::SocialScrapeCompleted { is_tension, .. } => !*is_tension,
            Self::TopicDiscoveryCompleted { .. } => true,
            _ => false,
        }
    }

    /// Extract the in-memory batches from a completion event (consumes self).
    pub fn into_extracted_batches(self) -> Vec<UrlExtraction> {
        match self {
            Self::WebScrapeCompleted { extracted_batches, .. } => extracted_batches,
            Self::SocialScrapeCompleted { extracted_batches, .. } => extracted_batches,
            Self::TopicDiscoveryCompleted { extracted_batches, .. } => extracted_batches,
            _ => Vec::new(),
        }
    }
}
