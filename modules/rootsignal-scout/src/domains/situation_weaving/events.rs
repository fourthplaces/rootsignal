//! Situation weaving domain events.

use serde::{Deserialize, Serialize};

#[causal::event(prefix = "situation_weaving", ephemeral)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SituationWeavingEvent {
    /// Situation weaving completed — signals assigned to situations.
    SituationsWeaved,
    /// Situation weaving skipped — nothing to weave (missing deps, no data).
    NothingToWeave { reason: String },
}

