//! Signal domain events: dedup verdicts, creation, storage.

use rootsignal_common::types::NodeType;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::aggregate::{ExtractedBatch, PendingNode};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SignalEvent {
    /// Extracted batch ready for dedup (triggers dedup handler).
    SignalsExtracted {
        url: String,
        canonical_key: String,
        count: u32,
        /// The extracted batch, carried as event payload for the dedup handler.
        batch: Box<ExtractedBatch>,
    },
    NewSignalAccepted {
        node_id: Uuid,
        node_type: NodeType,
        title: String,
        source_url: String,
        pending_node: Box<PendingNode>,
    },
    CrossSourceMatchDetected {
        existing_id: Uuid,
        node_type: NodeType,
        source_url: String,
        similarity: f64,
    },
    SameSourceReencountered {
        existing_id: Uuid,
        node_type: NodeType,
        source_url: String,
        similarity: f64,
    },
    DedupCompleted {
        url: String,
    },
    SignalCreated {
        node_id: Uuid,
        node_type: NodeType,
        source_url: String,
        canonical_key: String,
    },
    UrlProcessed {
        url: String,
        canonical_key: String,
        signals_created: u32,
        signals_deduplicated: u32,
    },
}

impl SignalEvent {
    pub fn event_type_str(&self) -> String {
        let variant = match self {
            Self::SignalsExtracted { .. } => "signals_extracted",
            Self::NewSignalAccepted { .. } => "new_signal_accepted",
            Self::CrossSourceMatchDetected { .. } => "cross_source_match_detected",
            Self::SameSourceReencountered { .. } => "same_source_reencountered",
            Self::DedupCompleted { .. } => "dedup_completed",
            Self::SignalCreated { .. } => "signal_created",
            Self::UrlProcessed { .. } => "url_processed",
        };
        format!("signal:{variant}")
    }

    pub fn to_persist_payload(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("SignalEvent serialization should never fail")
    }
}
