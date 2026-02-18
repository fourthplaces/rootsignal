use std::sync::Arc;

use async_graphql::dataloader::DataLoader;
use async_graphql::{Context, EmptyMutation, EmptySubscription, Object, Result, Schema};
use uuid::Uuid;

use rootsignal_graph::PublicGraphReader;

use super::loaders::{ActorsBySignalLoader, EvidenceBySignalLoader, StoryBySignalLoader};
use super::types::*;

pub type ApiSchema = Schema<QueryRoot, EmptyMutation, EmptySubscription>;

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    /// Find signals near a geographic point.
    async fn signals_near(
        &self,
        ctx: &Context<'_>,
        lat: f64,
        lng: f64,
        radius_km: f64,
        types: Option<Vec<SignalType>>,
    ) -> Result<Vec<GqlSignal>> {
        let reader = ctx.data_unchecked::<Arc<PublicGraphReader>>();
        let node_types: Option<Vec<rootsignal_common::NodeType>> =
            types.map(|t| t.into_iter().map(|st| st.to_node_type()).collect());
        let radius = radius_km.min(50.0);
        let nodes = reader
            .find_nodes_near(lat, lng, radius, node_types.as_deref())
            .await?;
        Ok(nodes.into_iter().map(GqlSignal::from).collect())
    }

    /// List recent signals, ordered by triangulation quality.
    async fn signals_recent(
        &self,
        ctx: &Context<'_>,
        limit: Option<u32>,
        types: Option<Vec<SignalType>>,
    ) -> Result<Vec<GqlSignal>> {
        let reader = ctx.data_unchecked::<Arc<PublicGraphReader>>();
        let node_types: Option<Vec<rootsignal_common::NodeType>> =
            types.map(|t| t.into_iter().map(|st| st.to_node_type()).collect());
        let limit = limit.unwrap_or(50).min(200);
        let nodes = reader.list_recent(limit, node_types.as_deref()).await?;
        Ok(nodes.into_iter().map(GqlSignal::from).collect())
    }

    /// Get a single signal by ID.
    async fn signal(&self, ctx: &Context<'_>, id: Uuid) -> Result<Option<GqlSignal>> {
        let reader = ctx.data_unchecked::<Arc<PublicGraphReader>>();
        let node = reader.get_signal_by_id(id).await?;
        Ok(node.map(GqlSignal::from))
    }

    /// List stories ordered by energy.
    async fn stories(
        &self,
        ctx: &Context<'_>,
        limit: Option<u32>,
        status: Option<String>,
    ) -> Result<Vec<GqlStory>> {
        let reader = ctx.data_unchecked::<Arc<PublicGraphReader>>();
        let limit = limit.unwrap_or(20).min(100);
        let stories = reader
            .top_stories_by_energy(limit, status.as_deref())
            .await?;
        Ok(stories.into_iter().map(GqlStory).collect())
    }

    /// Get a single story by ID.
    async fn story(&self, ctx: &Context<'_>, id: Uuid) -> Result<Option<GqlStory>> {
        let reader = ctx.data_unchecked::<Arc<PublicGraphReader>>();
        let story = reader.get_story_by_id(id).await?;
        Ok(story.map(GqlStory))
    }

    /// List stories by category.
    async fn stories_by_category(
        &self,
        ctx: &Context<'_>,
        category: String,
        limit: Option<u32>,
    ) -> Result<Vec<GqlStory>> {
        let reader = ctx.data_unchecked::<Arc<PublicGraphReader>>();
        let limit = limit.unwrap_or(20).min(100);
        let stories = reader.stories_by_category(&category, limit).await?;
        Ok(stories.into_iter().map(GqlStory).collect())
    }

    /// List actors in a city.
    async fn actors(
        &self,
        ctx: &Context<'_>,
        city: String,
        limit: Option<u32>,
    ) -> Result<Vec<GqlActor>> {
        let reader = ctx.data_unchecked::<Arc<PublicGraphReader>>();
        let limit = limit.unwrap_or(50).min(200);
        let actors = reader.actors_active_in_area(&city, limit).await?;
        Ok(actors.into_iter().map(GqlActor).collect())
    }

    /// Get a single actor by ID.
    async fn actor(&self, ctx: &Context<'_>, id: Uuid) -> Result<Option<GqlActor>> {
        let reader = ctx.data_unchecked::<Arc<PublicGraphReader>>();
        let actor = reader.actor_detail(id).await?;
        Ok(actor.map(GqlActor))
    }

    /// List editions for a city.
    async fn editions(
        &self,
        ctx: &Context<'_>,
        city: String,
        limit: Option<u32>,
    ) -> Result<Vec<GqlEdition>> {
        let reader = ctx.data_unchecked::<Arc<PublicGraphReader>>();
        let limit = limit.unwrap_or(10).min(50);
        let editions = reader.list_editions(&city, limit).await?;
        Ok(editions.into_iter().map(GqlEdition).collect())
    }

    /// Get the latest edition for a city.
    async fn latest_edition(
        &self,
        ctx: &Context<'_>,
        city: String,
    ) -> Result<Option<GqlEdition>> {
        let reader = ctx.data_unchecked::<Arc<PublicGraphReader>>();
        let edition = reader.latest_edition(&city).await?;
        Ok(edition.map(GqlEdition))
    }
}

pub fn build_schema(reader: Arc<PublicGraphReader>) -> ApiSchema {
    let evidence_loader = DataLoader::new(
        EvidenceBySignalLoader {
            reader: reader.clone(),
        },
        tokio::spawn,
    );
    let actors_loader = DataLoader::new(
        ActorsBySignalLoader {
            reader: reader.clone(),
        },
        tokio::spawn,
    );
    let story_loader = DataLoader::new(
        StoryBySignalLoader {
            reader: reader.clone(),
        },
        tokio::spawn,
    );

    Schema::build(QueryRoot, EmptyMutation, EmptySubscription)
        .data(reader)
        .data(evidence_loader)
        .data(actors_loader)
        .data(story_loader)
        .finish()
}
