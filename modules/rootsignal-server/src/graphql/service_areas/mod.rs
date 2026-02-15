pub mod mutations;
pub mod types;

use async_graphql::*;
use types::GqlServiceArea;

#[derive(Default)]
pub struct ServiceAreaQuery;

#[Object]
impl ServiceAreaQuery {
    async fn service_areas(&self, ctx: &Context<'_>) -> Result<Vec<GqlServiceArea>> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let areas = rootsignal_domains::config::ServiceArea::find_all(pool)
            .await
            .map_err(|e| async_graphql::Error::new(format!("database error: {e}")))?;
        Ok(areas.into_iter().map(GqlServiceArea::from).collect())
    }
}
