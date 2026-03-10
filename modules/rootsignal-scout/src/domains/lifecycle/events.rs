//! Lifecycle domain events: engine start, source preparation.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use uuid::Uuid;

use crate::core::aggregate::SourcePlan;
use crate::core::run_scope::RunScope;
use rootsignal_common::types::ActorContext;

#[seesaw_core::event(prefix = "lifecycle")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LifecycleEvent {
    ScoutRunRequested {
        run_id: Uuid,
        #[serde(default)]
        scope: RunScope,
        #[serde(default)]
        budget_cents: u64,
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
    NewsScanRequested,
}

