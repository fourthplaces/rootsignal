use serde::{Deserialize, Serialize};

#[causal::event(prefix = "cluster_weaving", ephemeral)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClusterWeavingEvent {
    ClusterWeaveCompleted,
    ClusterWeaveSkipped { reason: String },
}
