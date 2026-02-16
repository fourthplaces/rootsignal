pub mod types;

use async_graphql::*;
use types::GqlSignalStats;

#[derive(Default)]
pub struct StatsQuery;

#[Object]
impl StatsQuery {
    async fn signal_stats(&self, ctx: &Context<'_>) -> Result<GqlSignalStats> {
        tracing::info!("graphql.signal_stats");
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let stats = rootsignal_domains::signals::SignalStats::compute(pool)
            .await
            .map_err(|e| async_graphql::Error::new(format!("failed to compute stats: {e}")))?;
        Ok(GqlSignalStats::from(stats))
    }
}
