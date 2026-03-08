//! Signal domain events and domain types for dedup results.

use rootsignal_common::types::{ChannelType, NodeType};
use rootsignal_common::Node;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::extractor::ResourceTag;

// ---------------------------------------------------------------------------
// Domain types returned by dedup activity
// ---------------------------------------------------------------------------

/// Result of deduplicating an extracted batch.
pub struct DedupBatchResult {
    pub created: Vec<CreatedSignal>,
    pub corroborations: Vec<Corroboration>,
    pub actor_actions: Vec<ActorAction>,
    pub verdicts: Vec<DedupOutcome>,
}

/// A newly created signal with its associated citation.
pub struct CreatedSignal {
    pub node: Node,
    pub citation: NewCitation,
}

/// A cross-source corroboration of an existing signal.
pub struct Corroboration {
    pub signal_id: Uuid,
    pub node_type: NodeType,
    pub url: String,
    pub similarity: f64,
    pub new_corroboration_count: u32,
    pub citation: NewCitation,
}

/// Citation evidence linking a signal to a source URL.
pub struct NewCitation {
    pub citation_id: Uuid,
    pub signal_id: Uuid,
    pub url: String,
    pub content_hash: String,
    pub snippet: Option<String>,
    pub channel_type: Option<ChannelType>,
}

/// Actor-related action from inline actor resolution during dedup.
pub enum ActorAction {
    Identified {
        actor_id: Uuid,
        name: String,
        canonical_key: String,
    },
    LinkedToSource {
        actor_id: Uuid,
        source_id: Uuid,
    },
    LinkedToSignal {
        actor_id: Uuid,
        signal_id: Uuid,
    },
}

// ---------------------------------------------------------------------------
// Persisted event types (serialized to event store)
// ---------------------------------------------------------------------------

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
        #[serde(alias = "source_url")]
        url: String,
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
        #[serde(alias = "source_url")]
        url: String,
        new_corroboration_count: u32,
    },
    Refreshed {
        existing_id: Uuid,
        node_type: NodeType,
        #[serde(alias = "source_url")]
        url: String,
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
