//! Enrichment domain events: facts emitted by enrichment handlers.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EnrichmentEvent {
    /// Trampoline: review gate passed, enrichment handlers should fire.
    EnrichmentReady,
    ActorsExtracted,
    DiversityScored,
    ActorStatsComputed,
    ActorsLocated,
}

impl EnrichmentEvent {
    pub fn event_type_str(&self) -> String {
        match self {
            Self::EnrichmentReady => "enrichment_ready",
            Self::ActorsExtracted => "actors_extracted",
            Self::DiversityScored => "diversity_scored",
            Self::ActorStatsComputed => "actor_stats_computed",
            Self::ActorsLocated => "actors_located",
        }
        .to_string()
    }
}
