use async_graphql::*;

#[derive(Default)]
pub struct HeatMapMutation;

#[Object]
impl HeatMapMutation {
    /// Recompute heat map points from current locationables, notes, and tags.
    async fn recompute_heat_map(&self, ctx: &Context<'_>) -> Result<i32> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let count = rootsignal_domains::heat_map::HeatMapPoint::compute_and_store(pool)
            .await
            .map_err(|e| Error::new(format!("Failed to recompute heat map: {}", e)))?;
        Ok(count as i32)
    }
}
