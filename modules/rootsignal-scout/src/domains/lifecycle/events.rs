//! Lifecycle domain events: engine start, source preparation.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use uuid::Uuid;

use crate::core::aggregate::SourcePlan;
use crate::core::run_scope::RunScope;
use rootsignal_common::types::ActorContext;

#[causal::event(prefix = "lifecycle")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LifecycleEvent {
    ScoutRunRequested {
        run_id: Uuid,
        #[serde(default)]
        scope: RunScope,
        #[serde(default)]
        budget_cents: u64,
        #[serde(default)]
        region_id: Option<String>,
        #[serde(default)]
        flow_type: String,
        #[serde(default)]
        source_ids: Option<Vec<String>>,
        #[serde(default)]
        task_id: Option<String>,
    },
    ScoutRunCompleted {
        run_id: Uuid,
        finished_at: DateTime<Utc>,
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
    /// Kick off situation weaving for a region — independent of any scout run.
    GenerateSituationsRequested {
        run_id: Uuid,
        region: rootsignal_common::ScoutScope,
        #[serde(default)]
        budget_cents: u64,
        #[serde(default)]
        region_id: Option<String>,
        #[serde(default)]
        task_id: Option<String>,
    },
    /// Kick off coalescing only — seed from a specific signal or auto-select.
    CoalesceRequested {
        run_id: Uuid,
        region: rootsignal_common::ScoutScope,
        seed_signal_id: Option<Uuid>,
        #[serde(default)]
        budget_cents: u64,
        #[serde(default)]
        region_id: Option<String>,
        #[serde(default)]
        task_id: Option<String>,
    },
}

