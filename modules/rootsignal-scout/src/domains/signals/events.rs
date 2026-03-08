//! Signal domain events: dedup verdicts.

use rootsignal_common::types::NodeType;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::extractor::ResourceTag;

/// Resolved actor from dedup handler's inline lookup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedActor {
    pub actor_id: Uuid,
    pub is_new: bool,
    pub name: String,
    pub canonical_key: String,
    pub source_id: Option<Uuid>,
}

/// Per-signal verdict from the dedup handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum DedupOutcome {
    Created {
        node_id: Uuid,
        node_type: NodeType,
        content_hash: String,
        source_url: String,
        canonical_key: String,
        resource_tags: Vec<ResourceTag>,
        signal_tags: Vec<String>,
        source_id: Option<Uuid>,
        actor: Option<ResolvedActor>,
    },
    Corroborated {
        existing_id: Uuid,
        node_type: NodeType,
        similarity: f64,
        source_url: String,
        new_corroboration_count: u32,
    },
    Refreshed {
        existing_id: Uuid,
        node_type: NodeType,
        source_url: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SignalEvent {
    DedupCompleted {
        url: String,
        canonical_key: String,
        verdicts: Vec<DedupOutcome>,
    },
    NoNewSignals,
}

impl SignalEvent {
    pub fn event_type_str(&self) -> String {
        let variant = match self {
            Self::DedupCompleted { .. } => "dedup_completed",
            Self::NoNewSignals => "no_new_signals",
        };
        format!("signal:{variant}")
    }

    pub fn to_persist_payload(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("SignalEvent serialization should never fail")
    }
}
