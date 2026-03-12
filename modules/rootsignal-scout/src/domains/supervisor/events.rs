//! Supervisor domain events.

use serde::{Deserialize, Serialize};

#[causal::event(prefix = "supervisor", ephemeral)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SupervisorEvent {
    /// Supervision completed — region supervised.
    SupervisionCompleted,
    /// Supervision skipped — nothing to supervise (missing deps, no data).
    NothingToSupervise { reason: String },
}

