//! Scrape domain events: content fetching, extraction, social scraping.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::events::FreshnessBucket;

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
            | Self::LinkCollected { run_id, .. } => *run_id,
        }
    }
}
