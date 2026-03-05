//! Discovery domain events: source discovery, link promotion, expansion queries.

use rootsignal_common::types::SourceNode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DiscoveryEvent {
    /// Batch of proposed sources — not projected directly.
    /// The domain_filter handler decides which become SourceRegistered.
    SourcesDiscovered {
        sources: Vec<SourceNode>,
        discovered_by: String,
    },
    /// Audit trail: a proposed source was rejected by the domain filter.
    SourceRejected {
        source: SourceNode,
        reason: String,
    },
    LinksPromoted {
        count: u32,
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
    /// Page triage result: whether a zero-signal page's outbound links are worth promoting.
    PageTriaged {
        url: String,
        relevant: bool,
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
            Self::LinksPromoted { .. } => "links_promoted",
            Self::ExpansionQueryCollected { .. } => "expansion_query_collected",
            Self::SocialTopicCollected { .. } => "social_topic_collected",
            Self::SocialTopicsDiscovered { .. } => "social_topics_discovered",
            Self::PageTriaged { .. } => "page_triaged",
        };
        format!("discovery:{variant}")
    }

    pub fn to_persist_payload(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("DiscoveryEvent serialization should never fail")
    }
}
