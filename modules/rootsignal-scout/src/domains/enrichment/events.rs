//! Enrichment domain events: actor enrichment.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EnrichmentEvent {
    ActorEnrichmentCompleted { actors_updated: u32 },
}

impl EnrichmentEvent {
    pub fn event_type_str(&self) -> String {
        let variant = match self {
            Self::ActorEnrichmentCompleted { .. } => "actor_enrichment_completed",
        };
        format!("enrichment:{variant}")
    }

    pub fn to_persist_payload(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("EnrichmentEvent serialization should never fail")
    }
}
