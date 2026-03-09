//! Enrichment domain events: facts emitted by enrichment handlers.

use serde::{Deserialize, Serialize};

#[seesaw_core::event(prefix = "enrichment")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EnrichmentEvent {
    /// Trampoline: review gate passed, enrichment handlers should fire.
    EnrichmentReady,
}

