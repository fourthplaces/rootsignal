use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::extractor::ResourceTag;

#[causal::event(prefix = "curiosity")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CuriosityEvent {
    // -- Lifecycle events (update per-entity aggregates) --
    SignalInvestigated { signal_id: Uuid },
    SignalConcernLinked { signal_id: Uuid },
    ConcernResponsesScouted { concern_id: Uuid },
    ConcernGatheringsScouted { concern_id: Uuid },

    // -- Discovery events (trigger materializer) --
    TensionDiscovered {
        tension_id: Uuid,
        title: String,
        summary: String,
        severity: String,
        category: String,
        opposing: String,
        #[serde(alias = "source_url")]
        url: String,
        parent_signal_id: Uuid,
        match_strength: f64,
        explanation: String,
    },
    SignalDiscovered {
        signal_id: Uuid,
        title: String,
        summary: String,
        /// "resource", "gathering", or "help_request"
        signal_type: String,
        url: String,
        parent_concern_id: Uuid,
        match_strength: f64,
        explanation: String,
        /// If true, materializer wires ConcernLinked (drawn-to/gravity edge).
        /// If false, materializer wires ResponseLinked (responds-to edge).
        #[serde(default)]
        is_gravity: bool,
        #[serde(default)]
        event_date: Option<String>,
        #[serde(default)]
        is_recurring: bool,
        #[serde(default)]
        venue: Option<String>,
        #[serde(default)]
        organizer: Option<String>,
        #[serde(default)]
        gathering_type: Option<String>,
        #[serde(default)]
        what_needed: Option<String>,
        #[serde(default)]
        stated_goal: Option<String>,
        #[serde(default)]
        availability: Option<String>,
        #[serde(default)]
        eligibility: Option<String>,
        #[serde(default)]
        also_addresses: Vec<ResolvedEdge>,
        #[serde(default)]
        resources: Vec<ResourceTag>,
        #[serde(default)]
        diffusion_mechanism: Option<String>,
    },
    EmergentTensionDiscovered {
        tension_id: Uuid,
        title: String,
        summary: String,
        severity: String,
        opposing: String,
        #[serde(alias = "source_url")]
        url: String,
        parent_concern_id: Uuid,
    },
}

/// A pre-resolved also_addresses edge (title already matched to concern_id).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedEdge {
    pub concern_id: Uuid,
    pub similarity: f64,
}

impl CuriosityEvent {
    /// ID of the entity whose SignalLifecycle this event updates.
    pub fn lifecycle_signal_id(&self) -> Uuid {
        match self {
            Self::SignalInvestigated { signal_id }
            | Self::SignalConcernLinked { signal_id } => *signal_id,
            Self::TensionDiscovered { tension_id, .. }
            | Self::EmergentTensionDiscovered { tension_id, .. } => *tension_id,
            Self::SignalDiscovered { signal_id, .. } => *signal_id,
            _ => Uuid::nil(),
        }
    }

    /// ID of the entity whose ConcernLifecycle this event updates.
    pub fn lifecycle_concern_id(&self) -> Uuid {
        match self {
            Self::ConcernResponsesScouted { concern_id }
            | Self::ConcernGatheringsScouted { concern_id } => *concern_id,
            Self::EmergentTensionDiscovered { tension_id, .. } => *tension_id,
            _ => Uuid::nil(),
        }
    }

    pub fn is_discovery(&self) -> bool {
        matches!(
            self,
            Self::TensionDiscovered { .. }
                | Self::SignalDiscovered { .. }
                | Self::EmergentTensionDiscovered { .. }
        )
    }
}
