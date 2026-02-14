use restate_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use taproot_core::ServerDeps;

use crate::clustering::activities::ClusterStats;

// ─── Request / Response types ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterRequest {}
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
        _req: ClusterRequest,
    ) -> Result<ClusterResult, HandlerError> {
        ctx.set("status", "generating_embeddings".to_string());

        // Step 1: Generate embeddings for un-embedded listings
        let deps = self.deps.clone();
        let batch_size = deps.file_config.clustering.batch_size;
        let embed_count_json: String = ctx
            .run(|| async move {
                let count = crate::extraction::activities::generate_embeddings(batch_size, &deps)
                    .await
                    .map_err(|e| {
                        TerminalError::new(format!("Embedding generation failed: {}", e))
                    })?;
                serde_json::to_string(&count)
                    .map_err(|e| TerminalError::new(format!("Serialize: {}", e)).into())
            })
            .await?;

        let embedding_count: u32 = serde_json::from_str(&embed_count_json)
            .map_err(|e| TerminalError::new(format!("Deserialize: {}", e)))?;

        ctx.set("status", "clustering".to_string());

        // Step 2: Run clustering algorithm
        let deps = self.deps.clone();
        let stats_json: String = ctx
            .run(|| async move {
                let stats = crate::clustering::activities::cluster_listings(&deps)
                    .await
                    .map_err(|e| {
                        TerminalError::new(format!("Clustering failed: {}", e))
                    })?;
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
