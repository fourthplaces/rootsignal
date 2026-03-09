//! Discovery domain events: source discovery, link promotion, expansion queries.

use rootsignal_common::types::SourceNode;
use serde::{Deserialize, Serialize};

#[seesaw_core::event(prefix = "discovery")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DiscoveryEvent {
    /// Batch of proposed sources — not projected directly.
    /// The domain_filter handler decides which become SourcesRegistered.
    SourcesDiscovered {
        sources: Vec<SourceNode>,
        discovered_by: String,
    },
    ExpansionQueryCollected {
        query: String,
        source_url: String,
    },
    SocialTopicCollected {
        topic: String,
    },
    /// Bulk social topics discovered during mid-run source expansion.
    SocialTopicsDiscovered {
        topics: Vec<String>,
    },
    /// Source expansion completed — handler finished its work.
    SourceExpansionCompleted,
    /// Source expansion skipped — missing deps or no data.
    SourceExpansionSkipped {
        reason: String,
    },
}

impl DiscoveryEvent {
    pub fn is_projectable(&self) -> bool {
        false
    }

}
