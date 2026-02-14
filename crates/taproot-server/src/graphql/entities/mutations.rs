use async_graphql::*;
use uuid::Uuid;

use crate::graphql::auth::middleware::require_admin;
use crate::graphql::error;
use super::types::{GqlEntity, GqlService};

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
    pub phone: Option<String>,
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
    pub phone: Option<String>,
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
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let entity = taproot_domains::entities::Entity::create(
            &input.name,
            &input.entity_type,
            input.description.as_deref(),
            input.website.as_deref(),
            pool,
        )
        .await
        .map_err(|e| error::internal(e))?;

        Ok(GqlEntity::from(entity))
    }

    async fn update_entity(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
        input: UpdateEntityInput,
    ) -> Result<GqlEntity> {
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let entity = taproot_domains::entities::Entity::update(
            id,
            input.name.as_deref(),
            input.description.as_deref(),
            input.website.as_deref(),
            input.phone.as_deref(),
            input.email.as_deref(),
            pool,
        )
        .await
        .map_err(|e| error::internal(e))?;

        Ok(GqlEntity::from(entity))
    }

    async fn archive_entity(&self, ctx: &Context<'_>, id: Uuid) -> Result<GqlEntity> {
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let entity = taproot_domains::entities::Entity::archive(id, pool)
            .await
            .map_err(|e| Error::new(format!("{e}")))?;

        Ok(GqlEntity::from(entity))
    }

    async fn create_service(
        &self,
        ctx: &Context<'_>,
        input: CreateServiceInput,
    ) -> Result<GqlService> {
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let service = taproot_domains::entities::Service::create(
            input.entity_id,
            &input.name,
            input.description.as_deref(),
            pool,
        )
        .await
        .map_err(|e| error::internal(e))?;

        Ok(GqlService::from(service))
    }

    async fn update_service(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
        input: UpdateServiceInput,
    ) -> Result<GqlService> {
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let service = taproot_domains::entities::Service::update(
            id,
            input.name.as_deref(),
            input.description.as_deref(),
            input.url.as_deref(),
            input.email.as_deref(),
            input.phone.as_deref(),
            pool,
        )
        .await
        .map_err(|e| error::internal(e))?;

        Ok(GqlService::from(service))
    }
}
