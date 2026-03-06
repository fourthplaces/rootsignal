//! Lifecycle domain events: engine start, run completion.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use uuid::Uuid;

use crate::core::aggregate::SourcePlan;
use crate::core::run_scope::RunScope;
use crate::core::stats::ScoutStats;
use rootsignal_common::types::ActorContext;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LifecycleEvent {
    ScoutRunRequested {
        run_id: Uuid,
        #[serde(default)]
        scope: RunScope,
    },
    SourcesPrepared {
        tension_count: u32,
        response_count: u32,
        source_plan: SourcePlan,
        actor_contexts: HashMap<String, ActorContext>,
        url_mappings: HashMap<String, String>,
        web_urls: Vec<String>,
        web_source_keys: HashMap<String, Uuid>,
        web_source_count: u32,
        pub_dates: HashMap<String, DateTime<Utc>>,
        query_api_errors: HashSet<String>,
    },
    RunCompleted {
        stats: ScoutStats,
    },
    MetricsCompleted,
    NewsScanRequested,
}

impl LifecycleEvent {
    pub fn event_type_str(&self) -> String {
        let variant = match self {
            Self::ScoutRunRequested { .. } => "scout_run_requested",
            Self::SourcesPrepared { .. } => "sources_prepared",
            Self::RunCompleted { .. } => "run_completed",
            Self::MetricsCompleted => "metrics_completed",
            Self::NewsScanRequested => "news_scan_requested",
        };
        format!("lifecycle:{variant}")
    }

    pub fn to_persist_payload(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("LifecycleEvent serialization should never fail")
    }
}
