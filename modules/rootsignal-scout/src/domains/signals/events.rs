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
    pub content_changed: Vec<ContentChangedSignal>,
    pub actor_actions: Vec<ActorAction>,
    pub verdicts: Vec<DedupOutcome>,
}

/// A newly created signal with its associated citation.
pub struct CreatedSignal {
    pub node: Node,
    pub citation: NewCitation,
}

/// A signal whose content has changed since last encounter.
pub struct ContentChangedSignal {
    pub existing_id: Uuid,
    pub node: Node,
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
        actor_type: rootsignal_common::ActorType,
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
    Refreshed {
        existing_id: Uuid,
        node_type: NodeType,
        #[serde(alias = "source_url")]
        url: String,
        #[serde(default)]
        source_id: Option<Uuid>,
    },
    ContentChanged {
        existing_id: Uuid,
        node_type: NodeType,
        url: String,
        similarity: f64,
        #[serde(default)]
        source_id: Option<Uuid>,
    },
}

#[seesaw_core::event(prefix = "signal")]
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

