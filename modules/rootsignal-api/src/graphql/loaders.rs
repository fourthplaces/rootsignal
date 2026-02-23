use std::collections::HashMap;
use std::sync::Arc;

use async_graphql::dataloader::Loader;
use uuid::Uuid;

use rootsignal_common::{ActorNode, EvidenceNode, SituationNode, StoryNode, TagNode};
use rootsignal_graph::CachedReader;

// --- EvidenceBySignalLoader ---

pub struct EvidenceBySignalLoader {
    pub reader: Arc<CachedReader>,
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
    pub reader: Arc<CachedReader>,
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
    pub reader: Arc<CachedReader>,
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

// --- SituationsBySignalLoader ---

pub struct SituationsBySignalLoader {
    pub reader: Arc<CachedReader>,
}

impl Loader<Uuid> for SituationsBySignalLoader {
    type Value = Vec<SituationNode>;
    type Error = Arc<anyhow::Error>;

    async fn load(&self, keys: &[Uuid]) -> Result<HashMap<Uuid, Self::Value>, Self::Error> {
        self.reader
            .batch_situations_by_signal_ids(keys)
            .await
            .map_err(|e| Arc::new(anyhow::anyhow!(e)))
    }
}

// --- TagsByStoryLoader ---

pub struct TagsByStoryLoader {
    pub reader: Arc<CachedReader>,
}

impl Loader<Uuid> for TagsByStoryLoader {
    type Value = Vec<TagNode>;
    type Error = Arc<anyhow::Error>;

    async fn load(&self, keys: &[Uuid]) -> Result<HashMap<Uuid, Self::Value>, Self::Error> {
        self.reader
            .batch_tags_by_story_ids(keys)
            .await
            .map_err(Arc::new)
    }
}

// --- TagsBySituationLoader ---

pub struct TagsBySituationLoader {
    pub reader: Arc<CachedReader>,
}

impl Loader<Uuid> for TagsBySituationLoader {
    type Value = Vec<TagNode>;
    type Error = Arc<anyhow::Error>;

    async fn load(&self, keys: &[Uuid]) -> Result<HashMap<Uuid, Self::Value>, Self::Error> {
        self.reader
            .batch_tags_by_situation_ids(keys)
            .await
            .map_err(Arc::new)
    }
}
