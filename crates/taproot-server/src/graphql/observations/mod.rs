pub mod types;

use async_graphql::*;
use uuid::Uuid;

use crate::graphql::error;
use types::{GqlInvestigation, GqlObservation};

#[derive(Default)]
pub struct ObservationQuery;

#[Object]
impl ObservationQuery {
    async fn observation(&self, ctx: &Context<'_>, id: Uuid) -> Result<GqlObservation> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let obs = sqlx::query_as::<_, taproot_domains::entities::Observation>(
            "SELECT * FROM observations WHERE id = $1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|_| error::not_found(format!("observation {id}")))?;
        Ok(GqlObservation::from(obs))
    }

    async fn investigation(&self, ctx: &Context<'_>, id: Uuid) -> Result<GqlInvestigation> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let inv = taproot_domains::entities::Investigation::find_by_id(id, pool)
            .await
            .map_err(|_| error::not_found(format!("investigation {id}")))?;
        Ok(GqlInvestigation::from(inv))
    }
}
