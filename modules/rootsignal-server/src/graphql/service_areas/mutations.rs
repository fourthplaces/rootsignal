use async_graphql::*;
use uuid::Uuid;

use crate::graphql::auth::middleware::require_admin;
use crate::graphql::error;
use super::types::GqlServiceArea;

#[derive(InputObject)]
pub struct CreateServiceAreaInput {
    pub city: String,
    pub state: String,
}

#[derive(Default)]
pub struct ServiceAreaMutation;

#[Object]
impl ServiceAreaMutation {
    async fn create_service_area(
        &self,
        ctx: &Context<'_>,
        input: CreateServiceAreaInput,
    ) -> Result<GqlServiceArea> {
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let sa = rootsignal_domains::config::ServiceArea::create(&input.city, &input.state, pool)
            .await
            .map_err(|e| error::internal(e))?;

        Ok(GqlServiceArea::from(sa))
    }

    async fn delete_service_area(&self, ctx: &Context<'_>, id: Uuid) -> Result<bool> {
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        rootsignal_domains::config::ServiceArea::delete(id, pool)
            .await
            .map_err(|e| error::internal(e))?;

        Ok(true)
    }
}
