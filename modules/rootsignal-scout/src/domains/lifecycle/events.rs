//! Lifecycle domain events: engine start, phase transitions, run completion.

use serde::{Deserialize, Serialize};

use crate::core::events::PipelinePhase;
use crate::core::stats::ScoutStats;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LifecycleEvent {
    EngineStarted {
        run_id: String,
    },
    PhaseStarted {
        phase: PipelinePhase,
    },
    PhaseCompleted {
        phase: PipelinePhase,
    },
    SourcesScheduled {
        tension_count: u32,
        response_count: u32,
    },
    RunCompleted {
        stats: ScoutStats,
    },
    MetricsCompleted,
}

impl LifecycleEvent {
    pub fn event_type_str(&self) -> String {
        let variant = match self {
            Self::EngineStarted { .. } => "engine_started",
            Self::PhaseStarted { .. } => "phase_started",
            Self::PhaseCompleted { .. } => "phase_completed",
            Self::SourcesScheduled { .. } => "sources_scheduled",
            Self::RunCompleted { .. } => "run_completed",
            Self::MetricsCompleted => "metrics_completed",
        };
        format!("lifecycle:{variant}")
    }

    pub fn to_persist_payload(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("LifecycleEvent serialization should never fail")
    }
}
