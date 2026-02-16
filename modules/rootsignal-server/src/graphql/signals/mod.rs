pub mod mutations;
pub mod types;

use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::graphql::error;
use rootsignal_domains::signals::Signal;
use types::{GqlSignal, GqlSignalConnection, SignalType};

#[derive(Default)]
pub struct SignalQuery;

#[Object]
impl SignalQuery {
    /// Fetch a single signal by ID.
    async fn signal(&self, ctx: &Context<'_>, id: Uuid) -> Result<GqlSignal> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let signal = Signal::find_by_id(id, pool)
            .await
            .map_err(|_| error::not_found(format!("signal {id}")))?;
        Ok(GqlSignal::from(signal))
    }

    /// Query signals with optional filters.
    async fn signals(
        &self,
        ctx: &Context<'_>,
        #[graphql(name = "type")] signal_type: Option<SignalType>,
        entity_id: Option<Uuid>,
        source_id: Option<Uuid>,
        search: Option<String>,
        lat: Option<f64>,
        lng: Option<f64>,
        radius_km: Option<f64>,
        #[graphql(name = "since")] _since: Option<DateTime<Utc>>,
        #[graphql(default = 50)] limit: i32,
        #[graphql(default = 0)] offset: i32,
    ) -> Result<GqlSignalConnection> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let limit = limit.min(100) as i64;
        let offset = offset.max(0) as i64;

        // Source filter
        if let Some(source_id) = source_id {
            let nodes: Vec<GqlSignal> =
                Signal::find_by_source(source_id, limit, offset, pool)
                    .await?
                    .into_iter()
                    .map(GqlSignal::from)
                    .collect();
            let total = nodes.len() as i64;
            return Ok(GqlSignalConnection {
                nodes,
                total_count: total,
            });
        }

        // Geo search
        if let (Some(lat), Some(lng), Some(radius)) = (lat, lng, radius_km) {
            let type_str = signal_type.map(|t| t.as_str());
            let results = Signal::find_near(lat, lng, radius, type_str, limit, pool).await?;
            let nodes: Vec<GqlSignal> = results
                .into_iter()
                .map(|r| GqlSignal::from(r.signal))
                .collect();
            let total = nodes.len() as i64;
            return Ok(GqlSignalConnection {
                nodes,
                total_count: total,
            });
        }

        // Full-text search
        if let Some(ref query) = search {
            let type_str = signal_type.map(|t| t.as_str());
            let nodes: Vec<GqlSignal> = Signal::search(query, type_str, limit, offset, pool)
                .await?
                .into_iter()
                .map(GqlSignal::from)
                .collect();
            let total = nodes.len() as i64;
            return Ok(GqlSignalConnection {
                nodes,
                total_count: total,
            });
        }

        // Entity filter
        if let Some(entity_id) = entity_id {
            let nodes: Vec<GqlSignal> =
                Signal::find_by_entity(entity_id, limit, offset, pool)
                    .await?
                    .into_iter()
                    .map(GqlSignal::from)
                    .collect();
            let total = nodes.len() as i64;
            return Ok(GqlSignalConnection {
                nodes,
                total_count: total,
            });
        }

        // Type filter
        if let Some(signal_type) = signal_type {
            let nodes: Vec<GqlSignal> =
                Signal::find_by_type(signal_type.as_str(), limit, offset, pool)
                    .await?
                    .into_iter()
                    .map(GqlSignal::from)
                    .collect();
            let total = Signal::count_by_type(signal_type.as_str(), pool).await?;
            return Ok(GqlSignalConnection {
                nodes,
                total_count: total,
            });
        }

        // All signals
        let nodes: Vec<GqlSignal> = Signal::find_all(limit, offset, pool)
            .await?
            .into_iter()
            .map(GqlSignal::from)
            .collect();
        let total = Signal::count(pool).await?;
        Ok(GqlSignalConnection {
            nodes,
            total_count: total,
        })
    }
}
