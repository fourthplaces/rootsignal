//! Scrape domain events: content fetching, extraction, social scraping.

use std::collections::HashMap;

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
        query_api_errors: std::collections::HashSet<String>,
    },
    /// Web scrape handler completed fetch+extract for a batch of URLs.
    WebScrapeCompleted {
        run_id: Uuid,
        role: ScrapeRole,
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
        role: ScrapeRole,
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
    /// Reducer marks all 3 response roles as completed so downstream gates pass.
    ResponseScrapeSkipped {
        reason: String,
    },
}

impl ScrapeEvent {
    /// The scrape role completed by this event, if any.
    pub fn completed_role(&self) -> Option<ScrapeRole> {
        match self {
            Self::WebScrapeCompleted { role, .. } => Some(*role),
            Self::SocialScrapeCompleted { role, .. } => Some(*role),
            Self::TopicDiscoveryCompleted { .. } => Some(ScrapeRole::TopicDiscovery),
            _ => None,
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
