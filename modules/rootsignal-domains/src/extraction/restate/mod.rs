use restate_sdk::prelude::*;
use rootsignal_core::ServerDeps;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractRequest {
    pub snapshot_ids: Vec<String>,
    pub source_id: Option<String>,
}
impl_restate_serde!(ExtractRequest);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractResult {
    pub signal_ids: Vec<String>,
    pub extraction_count: u32,
    pub status: String,
}
impl_restate_serde!(ExtractResult);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmptyRequest {}
impl_restate_serde!(EmptyRequest);

#[restate_sdk::workflow]
#[name = "ExtractWorkflow"]
pub trait ExtractWorkflow {
    async fn run(req: ExtractRequest) -> Result<ExtractResult, HandlerError>;

    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct ExtractWorkflowImpl {
    deps: Arc<ServerDeps>,
}

impl ExtractWorkflowImpl {
    pub fn with_deps(deps: Arc<ServerDeps>) -> Self {
        Self { deps }
    }
}

impl ExtractWorkflow for ExtractWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        req: ExtractRequest,
    ) -> Result<ExtractResult, HandlerError> {
        tracing::info!(snapshots = req.snapshot_ids.len(), source_id = ?req.source_id, "ExtractWorkflow.start");
        ctx.set("status", "extracting".to_string());

        let mut all_signal_ids = Vec::new();

        for snapshot_id_str in &req.snapshot_ids {
            let snapshot_id: Uuid = snapshot_id_str
                .parse()
                .map_err(|e: uuid::Error| TerminalError::new(format!("Invalid UUID: {}", e)))?;

            // Signal extraction (includes entity resolution, location, schedule, embeddings)
            let deps = self.deps.clone();
            let signal_ids_json: String = ctx
                .run(|| async move {
                    let ids =
                        crate::signals::activities::extract_signals::extract_signals_from_snapshot(
                            snapshot_id,
                            &deps,
                        )
                        .await
                        .map_err(|e| {
                            TerminalError::new(format!("Signal extraction failed: {}", e))
                        })?;
                    serde_json::to_string(&ids.iter().map(|id| id.to_string()).collect::<Vec<_>>())
                        .map_err(|e| TerminalError::new(format!("Serialize failed: {}", e)).into())
                })
                .await?;

            let signal_ids: Vec<String> = serde_json::from_str(&signal_ids_json)
                .map_err(|e| TerminalError::new(format!("Deserialize: {}", e)))?;

            all_signal_ids.extend(signal_ids);
        }

        ctx.set("status", "completed".to_string());

        let signal_count = all_signal_ids.len() as u32;
        tracing::info!(signals = signal_count, "ExtractWorkflow.completed");
        Ok(ExtractResult {
            signal_ids: all_signal_ids,
            extraction_count: signal_count,
            status: "completed".to_string(),
        })
    }

    async fn get_status(
        &self,
        ctx: SharedWorkflowContext<'_>,
        _req: EmptyRequest,
    ) -> Result<String, HandlerError> {
        Ok(ctx
            .get::<String>("status")
            .await?
            .unwrap_or_else(|| "unknown".to_string()))
    }
}
