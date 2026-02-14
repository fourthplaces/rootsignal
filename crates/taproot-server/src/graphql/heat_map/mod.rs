pub mod types;

use async_graphql::*;
use chrono::{Duration, Utc};
use types::{GqlHeatMapPoint, GqlTemporalDelta, GqlZipDensity};

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

    /// Signal density aggregated by zip code.
    async fn signal_density(
        &self,
        ctx: &Context<'_>,
        signal_domain: Option<String>,
        category: Option<String>,
    ) -> Result<Vec<GqlZipDensity>> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let results = taproot_domains::heat_map::HeatMapPoint::signal_density_by_zip(
            signal_domain.as_deref(),
            category.as_deref(),
            pool,
        )
        .await
        .unwrap_or_default();

        Ok(results.into_iter().map(GqlZipDensity::from).collect())
    }

    /// Signal gaps: zip codes with lowest signal coverage.
    async fn signal_gaps(
        &self,
        ctx: &Context<'_>,
        signal_domain: Option<String>,
        category: Option<String>,
        limit: Option<i32>,
    ) -> Result<Vec<GqlZipDensity>> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let limit = limit.unwrap_or(10).min(100) as i64;

        let results = taproot_domains::heat_map::HeatMapPoint::signal_gaps(
            signal_domain.as_deref(),
            category.as_deref(),
            limit,
            pool,
        )
        .await
        .unwrap_or_default();

        Ok(results.into_iter().map(GqlZipDensity::from).collect())
    }

    /// Temporal comparison: signal trends over time.
    async fn signal_trends(
        &self,
        ctx: &Context<'_>,
        period: String,
        signal_domain: Option<String>,
    ) -> Result<Vec<GqlTemporalDelta>> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let now = Utc::now();

        let (current_start, current_end, previous_start, previous_end) = match period.as_str() {
            "week" => {
                let cs = now - Duration::days(7);
                let ps = cs - Duration::days(7);
                (cs, now, ps, cs)
            }
            "month" => {
                let cs = now - Duration::days(30);
                let ps = cs - Duration::days(30);
                (cs, now, ps, cs)
            }
            "quarter" => {
                let cs = now - Duration::days(90);
                let ps = cs - Duration::days(90);
                (cs, now, ps, cs)
            }
            _ => {
                return Err(Error::new("period must be 'week', 'month', or 'quarter'"));
            }
        };

        let results = taproot_domains::heat_map::HeatMapPoint::temporal_comparison(
            current_start,
            current_end,
            previous_start,
            previous_end,
            signal_domain.as_deref(),
            pool,
        )
        .await
        .unwrap_or_default();

        Ok(results.into_iter().map(GqlTemporalDelta::from).collect())
    }
}
