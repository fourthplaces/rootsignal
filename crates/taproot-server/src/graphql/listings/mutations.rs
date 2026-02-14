use async_graphql::*;
use uuid::Uuid;

use crate::graphql::auth::middleware::require_admin;
use crate::graphql::error;
use super::types::GqlListing;

#[derive(InputObject)]
pub struct CreateListingInput {
    pub title: String,
    pub description: Option<String>,
    pub source_locale: String,
    pub entity_id: Option<Uuid>,
    pub service_id: Option<Uuid>,
}

#[derive(InputObject)]
pub struct UpdateListingInput {
    pub title: Option<String>,
    pub description: Option<String>,
    pub source_locale: Option<String>,
    pub entity_id: Option<Uuid>,
    pub service_id: Option<Uuid>,
}

#[derive(Default)]
pub struct ListingMutation;

#[Object]
impl ListingMutation {
    async fn create_listing(
        &self,
        ctx: &Context<'_>,
        input: CreateListingInput,
    ) -> Result<GqlListing> {
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let listing = taproot_domains::listings::Listing::create(
            &input.title,
            input.description.as_deref(),
            &input.source_locale,
            input.entity_id,
            input.service_id,
            pool,
        )
        .await
        .map_err(|e| error::internal(e))?;

        Ok(GqlListing::from(listing))
    }

    async fn update_listing(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
        input: UpdateListingInput,
    ) -> Result<GqlListing> {
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let listing = taproot_domains::listings::Listing::update(
            id,
            input.title.as_deref(),
            input.description.as_deref(),
            input.source_locale.as_deref(),
            input.entity_id.map(Some),
            input.service_id.map(Some),
            pool,
        )
        .await
        .map_err(|e| error::internal(e))?;

        Ok(GqlListing::from(listing))
    }

    async fn archive_listing(&self, ctx: &Context<'_>, id: Uuid) -> Result<GqlListing> {
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let listing = taproot_domains::listings::Listing::archive(id, pool)
            .await
            .map_err(|e| error::internal(e))?;

        Ok(GqlListing::from(listing))
    }
}
