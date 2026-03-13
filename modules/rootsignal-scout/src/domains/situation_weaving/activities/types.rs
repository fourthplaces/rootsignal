//! Types for situation weaving activities.
//!
//! Re-exports graph types and adds weaving-specific types.

pub use rootsignal_graph::{WeaveCandidate, WeaveSignal};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// --- LLM response schemas ---

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WeavingResponse {
    pub assignments: Vec<SignalAssignment>,
    #[serde(default)]
    pub new_situations: Vec<NewSituation>,
    #[serde(default)]
    pub dispatches: Vec<DispatchInput>,
    #[serde(default)]
    pub state_updates: Vec<StateUpdate>,
    #[serde(default)]
    pub splits: Vec<SplitMerge>,
    #[serde(default)]
    pub merges: Vec<SplitMerge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SignalAssignment {
    pub signal_id: String,
    pub situation_id: String,
    pub confidence: f64,
    pub reasoning: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NewSituation {
    pub temp_id: String,
    pub headline: String,
    pub lede: String,
    pub location_name: String,
    #[serde(default)]
    pub initial_structured_state: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DispatchInput {
    pub situation_id: String,
    pub body: String,
    pub signal_ids: Vec<String>,
    pub dispatch_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StateUpdate {
    pub situation_id: String,
    pub structured_state_patch: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SplitMerge {
    pub from_situation_id: String,
    pub to_situation_ids: Vec<String>,
    pub reasoning: String,
}

/// Stats tracking for situation weaving.
#[derive(Debug, Default)]
pub struct SituationWeaverStats {
    pub signals_discovered: u32,
    pub signals_assigned: u32,
    pub situations_created: u32,
    pub situations_updated: u32,
    pub dispatches_written: u32,
    pub dispatches_flagged: u32,
    pub splits: u32,
    pub merges: u32,
}

impl std::fmt::Display for SituationWeaverStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SituationWeaver: {} discovered, {} assigned, {} created, {} updated, {} dispatches ({} flagged)",
            self.signals_discovered, self.signals_assigned,
            self.situations_created, self.situations_updated,
            self.dispatches_written, self.dispatches_flagged,
        )
    }
}
