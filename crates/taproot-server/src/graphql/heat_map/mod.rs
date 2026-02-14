pub mod types;

use async_graphql::*;
use types::GqlHeatMapPoint;

#[derive(Default)]
pub struct HeatMapQuery;

#[Object]
impl HeatMapQuery {
    async fn heat_map_points(
        &self,
        ctx: &Context<'_>,
        zip_code: Option<String>,
        radius_miles: Option<f64>,
        entity_type: Option<String>,
    ) -> Result<Vec<GqlHeatMapPoint>> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let points = if let Some(zip) = zip_code {
            let radius = radius_miles.unwrap_or(25.0);
            taproot_domains::heat_map::HeatMapPoint::find_near_zip(&zip, radius, pool)
                .await
                .unwrap_or_default()
        } else if let Some(et) = entity_type {
            taproot_domains::heat_map::HeatMapPoint::find_latest_by_type(&et, pool)
                .await
                .unwrap_or_default()
        } else {
            taproot_domains::heat_map::HeatMapPoint::find_latest(pool)
                .await
                .unwrap_or_default()
        };

        Ok(points.into_iter().map(GqlHeatMapPoint::from).collect())
    }
}
