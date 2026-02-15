use std::sync::Arc;

use async_graphql::*;
use uuid::Uuid;

use rootsignal_core::ServerDeps;

use super::super::sources::types::GqlSource;
use super::types::{GqlEntity, GqlService};
use crate::graphql::auth::middleware::require_admin;
use crate::graphql::error;

#[derive(InputObject)]
pub struct CreateEntityInput {
    pub name: String,
    pub entity_type: String,
    pub description: Option<String>,
    pub website: Option<String>,
}

#[derive(InputObject)]
pub struct UpdateEntityInput {
    pub name: Option<String>,
    pub description: Option<String>,
    pub website: Option<String>,
    pub telephone: Option<String>,
    pub email: Option<String>,
}

#[derive(InputObject)]
pub struct CreateServiceInput {
    pub entity_id: Uuid,
    pub name: String,
    pub description: Option<String>,
}

#[derive(InputObject)]
pub struct UpdateServiceInput {
    pub name: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    pub email: Option<String>,
    pub telephone: Option<String>,
}

#[derive(Default)]
pub struct EntityMutation;

#[Object]
impl EntityMutation {
    async fn create_entity(
        &self,
        ctx: &Context<'_>,
        input: CreateEntityInput,
    ) -> Result<GqlEntity> {
        tracing::info!(name = %input.name, entity_type = %input.entity_type, "graphql.create_entity");
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let entity = rootsignal_domains::entities::Entity::create(
            &input.name,
            &input.entity_type,
            input.description.as_deref(),
            input.website.as_deref(),
            pool,
        )
        .await
        .map_err(|e| error::internal(e))?;

        tracing::info!(id = %entity.id, "graphql.create_entity.ok");
        Ok(GqlEntity::from(entity))
    }

    async fn update_entity(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
        input: UpdateEntityInput,
    ) -> Result<GqlEntity> {
        tracing::info!(id = %id, "graphql.update_entity");
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let entity = rootsignal_domains::entities::Entity::update(
            id,
            input.name.as_deref(),
            input.description.as_deref(),
            input.website.as_deref(),
            input.telephone.as_deref(),
            input.email.as_deref(),
            pool,
        )
        .await
        .map_err(|e| error::internal(e))?;

        tracing::info!(id = %id, "graphql.update_entity.ok");
        Ok(GqlEntity::from(entity))
    }

    async fn archive_entity(&self, ctx: &Context<'_>, id: Uuid) -> Result<GqlEntity> {
        tracing::info!(id = %id, "graphql.archive_entity");
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let entity = rootsignal_domains::entities::Entity::archive(id, pool)
            .await
            .map_err(|e| Error::new(format!("{e}")))?;

        tracing::info!(id = %id, "graphql.archive_entity.ok");
        Ok(GqlEntity::from(entity))
    }

    async fn create_service(
        &self,
        ctx: &Context<'_>,
        input: CreateServiceInput,
    ) -> Result<GqlService> {
        tracing::info!(entity_id = %input.entity_id, name = %input.name, "graphql.create_service");
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let service = rootsignal_domains::shared::Service::create(
            input.entity_id,
            &input.name,
            input.description.as_deref(),
            pool,
        )
        .await
        .map_err(|e| error::internal(e))?;

        tracing::info!(id = %service.id, "graphql.create_service.ok");
        Ok(GqlService::from(service))
    }

    async fn update_service(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
        input: UpdateServiceInput,
    ) -> Result<GqlService> {
        tracing::info!(id = %id, "graphql.update_service");
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let service = rootsignal_domains::shared::Service::update(
            id,
            input.name.as_deref(),
            input.description.as_deref(),
            input.url.as_deref(),
            input.email.as_deref(),
            input.telephone.as_deref(),
            pool,
        )
        .await
        .map_err(|e| error::internal(e))?;

        tracing::info!(id = %id, "graphql.update_service.ok");
        Ok(GqlService::from(service))
    }

    async fn discover_social_links(
        &self,
        ctx: &Context<'_>,
        entity_id: Uuid,
    ) -> Result<Vec<GqlSource>> {
        tracing::info!(entity_id = %entity_id, "graphql.discover_social_links");
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let mut sources =
            rootsignal_domains::entities::activities::discover_social_for_entity(entity_id, pool)
                .await
                .map_err(|e| error::internal(e))?;

        // Fall back to fetching the entity's website URL if no snapshots yielded results
        if sources.is_empty() {
            let entity = rootsignal_domains::entities::Entity::find_by_id(entity_id, pool)
                .await
                .map_err(|e| error::internal(e))?;

            if let Some(ref website) = entity.website {
                let deps = ctx.data::<Arc<ServerDeps>>()?;
                sources = rootsignal_domains::entities::activities::discover_social_from_url(
                    website,
                    entity_id,
                    &deps.http_client,
                    pool,
                )
                .await
                .map_err(|e| error::internal(e))?;
            }
        }

        tracing::info!(entity_id = %entity_id, count = sources.len(), "graphql.discover_social_links.ok");
        Ok(sources.into_iter().map(GqlSource::from).collect())
    }
}
