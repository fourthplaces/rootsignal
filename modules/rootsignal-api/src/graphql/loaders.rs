use std::collections::HashMap;
use std::sync::Arc;

use async_graphql::dataloader::Loader;
use uuid::Uuid;

use rootsignal_common::{ActorNode, EvidenceNode, StoryNode};
use rootsignal_graph::PublicGraphReader;

// --- EvidenceBySignalLoader ---

pub struct EvidenceBySignalLoader {
    pub reader: Arc<PublicGraphReader>,
}

impl Loader<Uuid> for EvidenceBySignalLoader {
    type Value = Vec<EvidenceNode>;
    type Error = Arc<anyhow::Error>;

    async fn load(&self, keys: &[Uuid]) -> Result<HashMap<Uuid, Self::Value>, Self::Error> {
        self.reader
            .batch_evidence_by_signal_ids(keys)
            .await
            .map_err(|e| Arc::new(anyhow::anyhow!(e)))
    }
}

// --- ActorsBySignalLoader ---

pub struct ActorsBySignalLoader {
    pub reader: Arc<PublicGraphReader>,
}

impl Loader<Uuid> for ActorsBySignalLoader {
    type Value = Vec<ActorNode>;
    type Error = Arc<anyhow::Error>;

    async fn load(&self, keys: &[Uuid]) -> Result<HashMap<Uuid, Self::Value>, Self::Error> {
        self.reader
            .batch_actors_by_signal_ids(keys)
            .await
            .map_err(|e| Arc::new(anyhow::anyhow!(e)))
    }
}

// --- StoryBySignalLoader ---

pub struct StoryBySignalLoader {
    pub reader: Arc<PublicGraphReader>,
}

impl Loader<Uuid> for StoryBySignalLoader {
    type Value = StoryNode;
    type Error = Arc<anyhow::Error>;

    async fn load(&self, keys: &[Uuid]) -> Result<HashMap<Uuid, Self::Value>, Self::Error> {
        self.reader
            .batch_story_by_signal_ids(keys)
            .await
            .map_err(|e| Arc::new(anyhow::anyhow!(e)))
    }
}
