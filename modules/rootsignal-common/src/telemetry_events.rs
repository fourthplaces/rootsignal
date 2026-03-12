//! Layer 3: Operational Telemetry — the system log.
//!
//! Every variant describes something the infrastructure did: scrapes, searches,
//! budget tracking, housekeeping. These events are useful for debugging and
//! monitoring but irrelevant to the world record or editorial decisions.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// An operational telemetry event — infrastructure observations and housekeeping.
#[causal_core_macros::event(prefix = "telemetry")]
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

    SystemLog {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context: Option<serde_json::Value>,
    },
}

impl TelemetryEvent {
    /// Deserialize a telemetry event from a JSON payload.
    pub fn from_payload(payload: &serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(payload.clone())
    }
}
