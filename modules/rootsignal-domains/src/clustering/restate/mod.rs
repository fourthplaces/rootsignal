use restate_sdk::prelude::*;
use rootsignal_core::ServerDeps;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::clustering::activities::ClusterStats;

// ─── Request / Response types ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterRequest {
    pub cluster_type: String,
}
impl_restate_serde!(ClusterRequest);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterResult {
    pub embedding_count: u32,
    pub stats: ClusterStats,
    pub status: String,
}
impl_restate_serde!(ClusterResult);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmptyRequest {}
impl_restate_serde!(EmptyRequest);

// ─── ClusteringJob (virtual object — prevents concurrent clustering) ─────────

#[restate_sdk::object]
#[name = "ClusteringJob"]
pub trait ClusteringJob {
    async fn run(req: ClusterRequest) -> Result<ClusterResult, HandlerError>;

    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct ClusteringJobImpl {
    deps: Arc<ServerDeps>,
}

impl ClusteringJobImpl {
    pub fn with_deps(deps: Arc<ServerDeps>) -> Self {
        Self { deps }
    }
}

impl ClusteringJob for ClusteringJobImpl {
    async fn run(
        &self,
        ctx: ObjectContext<'_>,
        req: ClusterRequest,
    ) -> Result<ClusterResult, HandlerError> {
        tracing::info!("ClusteringJob.start");
        ctx.set("status", "clustering".to_string());

        let embedding_count: u32 = 0;

        // Step 2: Run clustering algorithm
        let deps = self.deps.clone();
        let cluster_type = req.cluster_type.clone();
        let stats_json: String = ctx
            .run(|| async move {
                let stats = crate::clustering::activities::cluster_signals(&deps, &cluster_type)
                    .await
                    .map_err(|e| TerminalError::new(format!("Clustering failed: {}", e)))?;
                serde_json::to_string(&stats)
                    .map_err(|e| TerminalError::new(format!("Serialize: {}", e)).into())
            })
            .await?;

        let stats: ClusterStats = serde_json::from_str(&stats_json)
            .map_err(|e| TerminalError::new(format!("Deserialize: {}", e)))?;

        ctx.set("status", "completed".to_string());

        tracing::info!(
            embedding_count,
            items_processed = stats.items_processed,
            clusters_created = stats.clusters_created,
            items_assigned = stats.items_assigned,
            "Clustering job completed"
        );

        Ok(ClusterResult {
            embedding_count,
            stats,
            status: "completed".to_string(),
        })
    }

    async fn get_status(
        &self,
        ctx: SharedObjectContext<'_>,
        _req: EmptyRequest,
    ) -> Result<String, HandlerError> {
        Ok(ctx
            .get::<String>("status")
            .await?
            .unwrap_or_else(|| "idle".to_string()))
    }
}
