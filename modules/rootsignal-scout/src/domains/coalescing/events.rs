use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[causal::event(prefix = "coalescing", ephemeral)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoalescingEvent {
    CoalescingCompleted {
        new_groups: u32,
        fed_signals: u32,
        refined_groups: u32,
    },
    CoalescingSkipped {
        reason: String,
    },
    /// A single-group feed completed.
    GroupFeedCompleted {
        group_id: Uuid,
        signals_added: u32,
        queries_refined: bool,
    },
}
