//! Synthesis domain events: facts emitted by synthesis handlers.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SynthesisEvent {
    SimilarityComputed,
    ResponsesMapped,
    SeverityInferred,
}
