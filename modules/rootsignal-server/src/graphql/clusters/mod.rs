pub mod types;

use async_graphql::*;
use rootsignal_domains::clustering::Cluster;
use types::{GqlClusterDetail, GqlClusterEntity, GqlClusterSignal, GqlMapCluster};
use uuid::Uuid;

#[derive(Default)]
pub struct ClusterQuery;

#[Object]
impl ClusterQuery {
    /// Fetch clusters with location data for map display.
    async fn signal_clusters(
        &self,
        ctx: &Context<'_>,
        signal_type: Option<String>,
        since: Option<String>,
        min_confidence: Option<f64>,
        zip_code: Option<String>,
        radius_miles: Option<f64>,
        about: Option<String>,
        limit: Option<i32>,
    ) -> Result<Vec<GqlMapCluster>> {
        tracing::info!(
            signal_type = ?signal_type,
            since = ?since,
            zip_code = ?zip_code,
            "graphql.signal_clusters"
        );
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let limit = limit.unwrap_or(500).min(1000) as i64;

        let clusters = Cluster::find_for_map(
            signal_type.as_deref(),
            since.as_deref(),
            min_confidence,
            zip_code.as_deref(),
            radius_miles,
            about.as_deref(),
            limit,
            pool,
        )
        .await
        .unwrap_or_default();

        Ok(clusters.into_iter().map(GqlMapCluster::from).collect())
    }

    /// Fetch a single cluster's detail for sidebar display.
    async fn signal_cluster(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
    ) -> Result<Option<GqlClusterDetail>> {
        tracing::info!(id = %id, "graphql.signal_cluster");
        let pool = ctx.data_unchecked::<sqlx::PgPool>();

        let detail = Cluster::find_detail(id, pool).await?;
        let Some(detail) = detail else {
            return Ok(None);
        };

        let signals = Cluster::find_signals(id, 50, pool).await.unwrap_or_default();
        let entities = Cluster::find_entities(id, pool).await.unwrap_or_default();

        Ok(Some(GqlClusterDetail {
            id: detail.id,
            cluster_type: detail.cluster_type,
            representative_content: detail.representative_content,
            representative_about: detail.representative_about,
            representative_signal_type: detail.representative_signal_type,
            representative_confidence: detail.representative_confidence,
            representative_broadcasted_at: detail.representative_broadcasted_at,
            signals: signals.into_iter().map(GqlClusterSignal::from).collect(),
            entities: entities.into_iter().map(GqlClusterEntity::from).collect(),
        }))
    }
}
