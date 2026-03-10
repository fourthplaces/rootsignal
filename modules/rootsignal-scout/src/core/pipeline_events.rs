//! Pipeline-level observability events.
//!
//! `PipelineEvent` covers handler-failure bookkeeping.
//! Domain-specific state mutations live on domain events (ScrapeEvent,
//! LifecycleEvent, etc.) and are applied by their respective `apply_*`
//! methods on PipelineState.

use serde::{Deserialize, Serialize};

#[seesaw_core::event(prefix = "pipeline")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PipelineEvent {
    /// Handler exhausted retries and was dead-lettered.
    HandlerFailed {
        handler_id: String,
        source_event_type: String,
        error: String,
        attempts: i32,
    },
    /// Budget consumed by an operation (e.g. LLM call, search query).
    BudgetSpent {
        cents: u64,
    },
}

impl PipelineEvent {
    pub fn is_projectable(&self) -> bool {
        matches!(self, Self::HandlerFailed { .. })
    }

}
