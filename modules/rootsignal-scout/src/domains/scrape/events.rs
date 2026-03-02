//! Scrape domain events: content fetching, extraction, social scraping.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::events::FreshnessBucket;

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
    /// Web URLs resolved — triggers fetch+extract handler.
    WebUrlsResolved {
        run_id: Uuid,
        role: ScrapeRole,
        urls: Vec<String>,
        /// canonical_key → source_id, populated by the resolve handler so
        /// downstream fetch handlers don't re-derive it from sources.
        source_keys: HashMap<String, Uuid>,
        source_count: u32,
    },
    /// Social scrape triggered — handler reads sources from scheduled state.
    SocialScrapeTriggered {
        run_id: Uuid,
        role: ScrapeRole,
    },
    /// Topic discovery triggered — handler reads topics from PipelineState.
    TopicDiscoveryTriggered {
        run_id: Uuid,
    },
    /// A scrape sub-phase completed fetch+extract.
    ScrapeRoleCompleted {
        run_id: Uuid,
        role: ScrapeRole,
        urls_scraped: u32,
        urls_unchanged: u32,
        urls_failed: u32,
        signals_extracted: u32,
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
            | Self::WebUrlsResolved { run_id, .. }
            | Self::SocialScrapeTriggered { run_id, .. }
            | Self::TopicDiscoveryTriggered { run_id, .. }
            | Self::ScrapeRoleCompleted { run_id, .. } => *run_id,
        }
    }
}
