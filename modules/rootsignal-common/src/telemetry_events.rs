//! Layer 3: Operational Telemetry — the system log.
//!
//! Every variant describes something the infrastructure did: scrapes, searches,
//! budget tracking, housekeeping. These events are useful for debugging and
//! monitoring but irrelevant to the world record or editorial decisions.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use rootsignal_world::Eventlike;

/// An operational telemetry event — infrastructure observations and housekeeping.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TelemetryEvent {
    UrlScraped {
        url: String,
        strategy: String,
        success: bool,
        content_bytes: usize,
    },

    FeedScraped {
        url: String,
        items: u32,
    },

    SocialScraped {
        platform: String,
        identifier: String,
        post_count: u32,
    },

    SocialTopicsSearched {
        platform: String,
        topics: Vec<String>,
        posts_found: u32,
    },

    SearchPerformed {
        query: String,
        provider: String,
        result_count: u32,
        canonical_key: String,
    },

    LlmExtractionCompleted {
        source_url: String,
        content_chars: usize,
        entities_extracted: u32,
        implied_queries: u32,
    },

    BudgetCheckpoint {
        spent_cents: u64,
        remaining_cents: u64,
    },

    BootstrapCompleted {
        sources_created: u64,
    },

    AgentWebSearched {
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

    PinsRemoved {
        pin_ids: Vec<Uuid>,
    },

    DemandAggregated {
        created_task_ids: Vec<Uuid>,
        consumed_demand_ids: Vec<Uuid>,
    },
}

impl Eventlike for TelemetryEvent {
    fn event_type(&self) -> &'static str {
        match self {
            TelemetryEvent::UrlScraped { .. } => "url_scraped",
            TelemetryEvent::FeedScraped { .. } => "feed_scraped",
            TelemetryEvent::SocialScraped { .. } => "social_scraped",
            TelemetryEvent::SocialTopicsSearched { .. } => "social_topics_searched",
            TelemetryEvent::SearchPerformed { .. } => "search_performed",
            TelemetryEvent::LlmExtractionCompleted { .. } => "llm_extraction_completed",
            TelemetryEvent::BudgetCheckpoint { .. } => "budget_checkpoint",
            TelemetryEvent::BootstrapCompleted { .. } => "bootstrap_completed",
            TelemetryEvent::AgentWebSearched { .. } => "agent_web_searched",
            TelemetryEvent::AgentPageRead { .. } => "agent_page_read",
            TelemetryEvent::AgentFutureQuery { .. } => "agent_future_query",
            TelemetryEvent::PinsRemoved { .. } => "pins_removed",
            TelemetryEvent::DemandAggregated { .. } => "demand_aggregated",
        }
    }

    fn to_payload(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("TelemetryEvent serialization should never fail")
    }
}

impl TelemetryEvent {
    /// Deserialize a telemetry event from a JSON payload.
    pub fn from_payload(payload: &serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(payload.clone())
    }
}
