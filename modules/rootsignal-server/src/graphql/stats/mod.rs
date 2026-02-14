pub mod types;

use async_graphql::*;
use types::GqlListingStats;

#[derive(Default)]
pub struct StatsQuery;

#[Object]
impl StatsQuery {
    async fn listing_stats(&self, ctx: &Context<'_>) -> Result<GqlListingStats> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let stats = rootsignal_domains::listings::ListingStats::compute(pool)
            .await
            .map_err(|e| async_graphql::Error::new(format!("failed to compute stats: {e}")))?;
        Ok(GqlListingStats::from(stats))
    }
}
