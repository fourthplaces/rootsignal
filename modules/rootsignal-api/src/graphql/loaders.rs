use std::collections::HashMap;
use std::sync::Arc;

use async_graphql::dataloader::Loader;
use uuid::Uuid;

use rootsignal_common::{ActorNode, CitationNode, ScheduleNode, SituationNode, TagNode};
use rootsignal_graph::CachedReader;

// --- CitationBySignalLoader ---

pub struct CitationBySignalLoader {
    pub reader: Arc<CachedReader>,
}

impl Loader<Uuid> for CitationBySignalLoader {
    type Value = Vec<CitationNode>;
    type Error = Arc<anyhow::Error>;

    async fn load(&self, keys: &[Uuid]) -> Result<HashMap<Uuid, Self::Value>, Self::Error> {
        self.reader
            .batch_citation_by_signal_ids(keys)
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

// --- ScheduleBySignalLoader ---

pub struct ScheduleBySignalLoader {
    pub reader: Arc<CachedReader>,
}

impl Loader<Uuid> for ScheduleBySignalLoader {
    type Value = ScheduleNode;
    type Error = Arc<anyhow::Error>;

    async fn load(&self, keys: &[Uuid]) -> Result<HashMap<Uuid, Self::Value>, Self::Error> {
        self.reader
            .batch_schedules_by_signal_ids(keys)
            .await
            .map_err(|e| Arc::new(anyhow::anyhow!(e)))
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
