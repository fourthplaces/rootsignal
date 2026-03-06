//! Situation weaving domain events.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SituationWeavingEvent {
    /// Situation weaving completed — signals assigned to situations.
    SituationsWeaved,
    /// Situation weaving skipped — nothing to weave (missing deps, no data).
    NothingToWeave { reason: String },
}

impl SituationWeavingEvent {
    pub fn event_type_str(&self) -> String {
        let variant = match self {
            Self::SituationsWeaved => "situations_weaved",
            Self::NothingToWeave { .. } => "nothing_to_weave",
        };
        format!("situation_weaving:{variant}")
    }

    pub fn to_persist_payload(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("SituationWeavingEvent serialization should never fail")
    }
}
