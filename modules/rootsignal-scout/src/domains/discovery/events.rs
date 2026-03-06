//! Discovery domain events: source discovery, link promotion, expansion queries.

use rootsignal_common::types::SourceNode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DiscoveryEvent {
    /// Batch of proposed sources — not projected directly.
    /// The domain_filter handler decides which become SourcesRegistered.
    SourcesDiscovered {
        sources: Vec<SourceNode>,
        discovered_by: String,
    },
    /// Audit trail: a proposed source was rejected by the domain filter.
    SourceRejected {
        source: SourceNode,
        reason: String,
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
        false // SourcesDiscovered is a proposal; SourceRejected is audit-only
    }

    pub fn event_type_str(&self) -> String {
        let variant = match self {
            Self::SourcesDiscovered { .. } => "sources_discovered",
            Self::SourceRejected { .. } => "source_rejected",
            Self::ExpansionQueryCollected { .. } => "expansion_query_collected",
            Self::SocialTopicCollected { .. } => "social_topic_collected",
            Self::SocialTopicsDiscovered { .. } => "social_topics_discovered",
            Self::SourceExpansionCompleted => "source_expansion_completed",
            Self::SourceExpansionSkipped { .. } => "source_expansion_skipped",
        };
        format!("discovery:{variant}")
    }

    pub fn to_persist_payload(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("DiscoveryEvent serialization should never fail")
    }
}
