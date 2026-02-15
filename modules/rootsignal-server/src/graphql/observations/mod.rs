pub mod mutations;
pub mod types;

use async_graphql::*;
use uuid::Uuid;

use crate::graphql::auth::middleware::require_admin;
use crate::graphql::error;
use types::{GqlInvestigation, GqlObservation};

#[derive(Default)]
pub struct ObservationQuery;

#[Object]
impl ObservationQuery {
    async fn observation(&self, ctx: &Context<'_>, id: Uuid) -> Result<GqlObservation> {
        tracing::info!(id = %id, "graphql.observation");
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let obs = sqlx::query_as::<_, rootsignal_domains::investigations::Observation>(
            "SELECT * FROM observations WHERE id = $1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|_| error::not_found(format!("observation {id}")))?;
        Ok(GqlObservation::from(obs))
    }

    async fn pending_observations(
        &self,
        ctx: &Context<'_>,
        #[graphql(default = 50)] limit: i64,
    ) -> Result<Vec<GqlObservation>> {
        tracing::info!(limit = limit, "graphql.pending_observations");
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let obs =
            rootsignal_domains::investigations::Observation::find_pending(limit, pool).await?;
        Ok(obs.into_iter().map(GqlObservation::from).collect())
    }

    async fn investigation(&self, ctx: &Context<'_>, id: Uuid) -> Result<GqlInvestigation> {
        tracing::info!(id = %id, "graphql.investigation");
        require_admin(ctx)?;
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let inv = rootsignal_domains::investigations::Investigation::find_by_id(id, pool)
            .await
            .map_err(|_| error::not_found(format!("investigation {id}")))?;
        Ok(GqlInvestigation::from(inv))
    }
}
