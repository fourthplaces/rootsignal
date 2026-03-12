//! Expansion domain events: signal expansion stats.

use serde::{Deserialize, Serialize};

#[causal::event(prefix = "expansion")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExpansionEvent {
    /// Trampoline: all enrichment facts set, expansion should fire.
    ExpansionReady,
    /// Signal expansion phase completed with accumulated stats.
    ExpansionCompleted {
        social_expansion_topics: Vec<String>,
        expansion_deferred_expanded: u32,
        expansion_queries_collected: u32,
        expansion_sources_created: u32,
        expansion_social_topics_queued: u32,
    },
}

