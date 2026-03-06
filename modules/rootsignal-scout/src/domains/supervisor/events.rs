//! Supervisor domain events.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SupervisorEvent {
    /// Supervision completed — region supervised.
    SupervisionCompleted,
    /// Supervision skipped — nothing to supervise (missing deps, no data).
    NothingToSupervise { reason: String },
}

impl SupervisorEvent {
    pub fn event_type_str(&self) -> String {
        let variant = match self {
            Self::SupervisionCompleted => "supervision_completed",
            Self::NothingToSupervise { .. } => "nothing_to_supervise",
        };
        format!("supervisor:{variant}")
    }

    pub fn to_persist_payload(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("SupervisorEvent serialization should never fail")
    }
}
