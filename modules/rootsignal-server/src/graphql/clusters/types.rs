use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(SimpleObject, Clone)]
pub struct GqlSignalTypeCounts {
    pub ask: i32,
    pub give: i32,
    pub event: i32,
    pub informative: i32,
}

#[derive(SimpleObject, Clone)]
pub struct GqlMapCluster {
    pub id: Uuid,
    pub latitude: f64,
    pub longitude: f64,
    pub member_count: i32,
    pub dominant_signal_type: String,
    pub representative_content: String,
    pub representative_about: Option<String>,
    pub signal_counts: GqlSignalTypeCounts,
    pub entity_names: Vec<String>,
}

impl From<rootsignal_domains::clustering::MapCluster> for GqlMapCluster {
    fn from(mc: rootsignal_domains::clustering::MapCluster) -> Self {
        let entity_names: Vec<String> = serde_json::from_value(mc.entity_names).unwrap_or_default();
        Self {
            id: mc.id,
            latitude: mc.latitude,
            longitude: mc.longitude,
            member_count: mc.member_count as i32,
            dominant_signal_type: mc.dominant_signal_type,
            representative_content: mc.representative_content,
            representative_about: mc.representative_about,
            signal_counts: GqlSignalTypeCounts {
                ask: mc.ask_count as i32,
                give: mc.give_count as i32,
                event: mc.event_count as i32,
                informative: mc.informative_count as i32,
            },
            entity_names,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct GqlClusterSignal {
    pub id: Uuid,
    pub signal_type: String,
    pub content: String,
    pub confidence: f64,
    pub broadcasted_at: Option<DateTime<Utc>>,
}

impl From<rootsignal_domains::clustering::ClusterSignal> for GqlClusterSignal {
    fn from(s: rootsignal_domains::clustering::ClusterSignal) -> Self {
        Self {
            id: s.id,
            signal_type: s.signal_type,
            content: s.content,
            confidence: s.confidence,
            broadcasted_at: s.broadcasted_at,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct GqlClusterEntity {
    pub id: Uuid,
    pub name: String,
    pub entity_type: String,
}

impl From<rootsignal_domains::clustering::ClusterEntity> for GqlClusterEntity {
    fn from(e: rootsignal_domains::clustering::ClusterEntity) -> Self {
        Self {
            id: e.id,
            name: e.name,
            entity_type: e.entity_type,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct GqlClusterDetail {
    pub id: Uuid,
    pub cluster_type: String,
    pub representative_content: String,
    pub representative_about: Option<String>,
    pub representative_signal_type: String,
    pub representative_confidence: f64,
    pub representative_broadcasted_at: Option<DateTime<Utc>>,
    pub signals: Vec<GqlClusterSignal>,
    pub entities: Vec<GqlClusterEntity>,
}
