//! Pipeline-level events for PipelineState aggregate mutations.
//!
//! These replace direct handler writes to PipelineState. Each variant
//! carries accumulated output from a pipeline phase, applied to state
//! via the `apply_pipeline` method on PipelineState.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::core::aggregate::ScheduledData;
use crate::domains::enrichment::activities::link_promoter::CollectedLink;
use crate::domains::scrape::activities::StatsDelta;
use rootsignal_common::types::ActorContext;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PipelineEvent {
    /// Schedule phase resolved — stash sources, actor contexts, URL mappings.
    ScheduleResolved {
        scheduled_data: ScheduledData,
        actor_contexts: HashMap<String, ActorContext>,
        url_mappings: HashMap<String, String>,
    },
    /// Expansion phase accumulated output — social topics, stats.
    ExpansionAccumulated {
        social_expansion_topics: Vec<String>,
        expansion_deferred_expanded: u32,
        expansion_queries_collected: u32,
        expansion_sources_created: u32,
        expansion_social_topics_queued: u32,
    },
    /// Social topics collected during mid-run discovery.
    SocialTopicsCollected {
        topics: Vec<String>,
    },
    /// Social topics consumed by response scrape.
    SocialTopicsConsumed,
    /// URL resolution state — mappings, pub_dates, API errors.
    UrlsResolvedAccumulated {
        url_mappings: HashMap<String, String>,
        pub_dates: HashMap<String, DateTime<Utc>>,
        query_api_errors: HashSet<String>,
    },
    /// Handler saw an event but chose not to act — pipeline bookkeeping.
    HandlerSkipped {
        handler_id: String,
        reason: String,
    },
    /// Handler exhausted retries and was dead-lettered.
    HandlerFailed {
        handler_id: String,
        source_event_type: String,
        error: String,
        attempts: i32,
    },
    /// Fetch+extract state — signal counts, links, expansion queries, stats.
    ScrapeResultAccumulated {
        source_signal_counts: HashMap<String, u32>,
        collected_links: Vec<CollectedLink>,
        expansion_queries: Vec<String>,
        stats_delta: StatsDelta,
    },
}

impl PipelineEvent {
    pub fn event_type_str(&self) -> String {
        let variant = match self {
            Self::ScheduleResolved { .. } => "schedule_resolved",
            Self::ExpansionAccumulated { .. } => "expansion_accumulated",
            Self::SocialTopicsCollected { .. } => "social_topics_collected",
            Self::SocialTopicsConsumed => "social_topics_consumed",
            Self::HandlerSkipped { .. } => "handler_skipped",
            Self::HandlerFailed { .. } => "handler_failed",
            Self::UrlsResolvedAccumulated { .. } => "urls_resolved_accumulated",
            Self::ScrapeResultAccumulated { .. } => "scrape_result_accumulated",
        };
        format!("pipeline:{variant}")
    }

    pub fn to_persist_payload(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("PipelineEvent serialization should never fail")
    }
}
