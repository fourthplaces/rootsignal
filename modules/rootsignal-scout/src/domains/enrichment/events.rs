//! Enrichment domain events: facts emitted by enrichment handlers.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EnrichmentEvent {
    /// Trampoline: review gate passed, enrichment handlers should fire.
    EnrichmentReady,
}

impl EnrichmentEvent {
    pub fn event_type_str(&self) -> String {
        match self {
            Self::EnrichmentReady => "enrichment_ready",
        }
        .to_string()
    }
}
