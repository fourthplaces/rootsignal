//! Synthesis domain events: trigger + per-role completion tracking.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A role within the synthesis phase — each runs as an independent handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SynthesisRole {
    Similarity,
    ResponseMapping,
}

/// All synthesis roles — used for superset completion check.
pub fn all_synthesis_roles() -> std::collections::HashSet<SynthesisRole> {
    std::collections::HashSet::from([
        SynthesisRole::Similarity,
        SynthesisRole::ResponseMapping,
    ])
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SynthesisEvent {
    SynthesisRoleCompleted {
        run_id: Uuid,
        role: SynthesisRole,
    },
    SynthesisCompleted {
        run_id: Uuid,
    },
}

impl SynthesisEvent {
    pub fn run_id(&self) -> Uuid {
        match self {
            Self::SynthesisRoleCompleted { run_id, .. }
            | Self::SynthesisCompleted { run_id } => *run_id,
        }
    }
}
