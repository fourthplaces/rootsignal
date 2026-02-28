//! Discovery domain events: source discovery, link promotion, expansion queries.

use rootsignal_common::types::SourceNode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DiscoveryEvent {
    SourceDiscovered {
        source: SourceNode,
        discovered_by: String,
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
}

impl DiscoveryEvent {
    pub fn is_projectable(&self) -> bool {
        matches!(self, DiscoveryEvent::SourceDiscovered { .. })
    }

    pub fn event_type_str(&self) -> String {
        let variant = match self {
            Self::SourceDiscovered { .. } => "source_discovered",
            Self::LinksPromoted { .. } => "links_promoted",
            Self::ExpansionQueryCollected { .. } => "expansion_query_collected",
            Self::SocialTopicCollected { .. } => "social_topic_collected",
        };
        format!("discovery:{variant}")
    }

    pub fn to_persist_payload(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("DiscoveryEvent serialization should never fail")
    }
}
