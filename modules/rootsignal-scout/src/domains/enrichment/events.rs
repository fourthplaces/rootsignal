//! Enrichment domain events: role-based parallel enrichment.

use serde::{Deserialize, Serialize};

/// A role within the enrichment phase — each runs as an independent handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnrichmentRole {
    ActorExtraction,
    Diversity,
    ActorStats,
    ActorLocation,
}

/// All enrichment roles — used for superset completion check.
pub fn all_enrichment_roles() -> std::collections::HashSet<EnrichmentRole> {
    std::collections::HashSet::from([
        EnrichmentRole::ActorExtraction,
        EnrichmentRole::Diversity,
        EnrichmentRole::ActorStats,
        EnrichmentRole::ActorLocation,
    ])
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EnrichmentEvent {
    EnrichmentRoleCompleted { role: EnrichmentRole },
}

impl EnrichmentEvent {
    pub fn event_type_str(&self) -> String {
        let variant = match self {
            Self::EnrichmentRoleCompleted { .. } => "enrichment_role_completed",
        };
        format!("enrichment:{variant}")
    }

    pub fn to_persist_payload(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("EnrichmentEvent serialization should never fail")
    }
}
