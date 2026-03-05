//! Signal domain events: dedup verdicts, creation, storage.

use rootsignal_common::types::NodeType;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::aggregate::WiringContext;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SignalEvent {
    DedupCompleted {
        url: String,
        canonical_key: String,
        signals_created: u32,
        signals_deduplicated: u32,
    },
    SignalCreated {
        node_id: Uuid,
        node_type: NodeType,
        source_url: String,
        canonical_key: String,
        /// Wiring data for edge creation — serialized for replay correctness.
        wiring: Option<WiringContext>,
    },
}

impl SignalEvent {
    pub fn event_type_str(&self) -> String {
        let variant = match self {
            Self::DedupCompleted { .. } => "dedup_completed",
            Self::SignalCreated { .. } => "signal_created",
        };
        format!("signal:{variant}")
    }

    pub fn to_persist_payload(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("SignalEvent serialization should never fail")
    }
}
