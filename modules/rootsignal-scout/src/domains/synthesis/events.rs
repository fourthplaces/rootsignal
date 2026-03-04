//! Synthesis domain events: trigger + per-role completion tracking.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A role within the synthesis phase — each runs as an independent handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SynthesisRole {
    Similarity,
    ResponseMapping,
    ConcernLinker,
    ResponseFinder,
    GatheringFinder,
    Investigation,
}

/// All synthesis roles — used for superset completion check.
pub fn all_synthesis_roles() -> std::collections::HashSet<SynthesisRole> {
    std::collections::HashSet::from([
        SynthesisRole::Similarity,
        SynthesisRole::ResponseMapping,
        SynthesisRole::ConcernLinker,
        SynthesisRole::ResponseFinder,
        SynthesisRole::GatheringFinder,
        SynthesisRole::Investigation,
    ])
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SynthesisEvent {
    SynthesisTriggered { run_id: Uuid },
    SynthesisRoleCompleted { run_id: Uuid, role: SynthesisRole },

    // Fan-out bookkeeping
    SynthesisTargetsDispatched { run_id: Uuid, role: SynthesisRole, count: u32 },

    // ConcernLinker per-target
    ConcernLinkerTargetRequested { run_id: Uuid, signal_id: Uuid, signal_title: String, signal_type: String, source_url: String },
    ConcernLinkerTargetCompleted { run_id: Uuid, signal_id: Uuid, outcome: String, tensions_discovered: u32, edges_created: u32 },

    // ResponseFinder per-target
    ResponseFinderTargetRequested { run_id: Uuid, concern_id: Uuid, concern_title: String },
    ResponseFinderTargetCompleted { run_id: Uuid, concern_id: Uuid, responses_discovered: u32, edges_created: u32 },

    // GatheringFinder per-target
    GatheringFinderTargetRequested { run_id: Uuid, concern_id: Uuid, concern_title: String },
    GatheringFinderTargetCompleted { run_id: Uuid, concern_id: Uuid, gatherings_discovered: u32, no_gravity: bool, edges_created: u32 },

    // Investigation per-target
    InvestigationTargetRequested { run_id: Uuid, signal_id: Uuid, signal_title: String, signal_type: String },
    InvestigationTargetCompleted { run_id: Uuid, signal_id: Uuid, evidence_created: u32, confidence_adjusted: bool },

    // ResponseMapping per-target
    ResponseMappingTargetRequested { run_id: Uuid, concern_id: Uuid, concern_title: String },
    ResponseMappingTargetCompleted { run_id: Uuid, concern_id: Uuid, edges_created: u32 },
}

impl SynthesisEvent {
    pub fn run_id(&self) -> Uuid {
        match self {
            Self::SynthesisTriggered { run_id }
            | Self::SynthesisRoleCompleted { run_id, .. }
            | Self::SynthesisTargetsDispatched { run_id, .. }
            | Self::ConcernLinkerTargetRequested { run_id, .. }
            | Self::ConcernLinkerTargetCompleted { run_id, .. }
            | Self::ResponseFinderTargetRequested { run_id, .. }
            | Self::ResponseFinderTargetCompleted { run_id, .. }
            | Self::GatheringFinderTargetRequested { run_id, .. }
            | Self::GatheringFinderTargetCompleted { run_id, .. }
            | Self::InvestigationTargetRequested { run_id, .. }
            | Self::InvestigationTargetCompleted { run_id, .. }
            | Self::ResponseMappingTargetRequested { run_id, .. }
            | Self::ResponseMappingTargetCompleted { run_id, .. } => {
                *run_id
            }
        }
    }
}
