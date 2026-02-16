use restate_sdk::prelude::*;
use rootsignal_core::ServerDeps;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::findings::activities::detect_cluster::detect_signal_clusters;
use crate::findings::activities::investigate::{run_why_investigation, InvestigationTrigger};

// ─── Why Investigation Workflow ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhyInvestigateRequest {
    pub signal_id: String,
}
impl_restate_serde!(WhyInvestigateRequest);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhyInvestigateResult {
    pub investigation_id: Option<String>,
    pub status: String,
    pub finding_id: Option<String>,
}
impl_restate_serde!(WhyInvestigateResult);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmptyRequest {}
impl_restate_serde!(EmptyRequest);

#[restate_sdk::workflow]
#[name = "WhyInvestigationWorkflow"]
pub trait WhyInvestigationWorkflow {
    async fn run(req: WhyInvestigateRequest) -> Result<WhyInvestigateResult, HandlerError>;

    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct WhyInvestigationWorkflowImpl {
    deps: Arc<ServerDeps>,
}

impl WhyInvestigationWorkflowImpl {
    pub fn with_deps(deps: Arc<ServerDeps>) -> Self {
        Self { deps }
    }
}

impl WhyInvestigationWorkflow for WhyInvestigationWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        req: WhyInvestigateRequest,
    ) -> Result<WhyInvestigateResult, HandlerError> {
        tracing::info!(signal_id = %req.signal_id, "WhyInvestigationWorkflow.start");
        ctx.set("status", "investigating".to_string());

        let signal_id: Uuid = req.signal_id.parse().map_err(|e: uuid::Error| {
            TerminalError::new(format!("Invalid signal_id UUID: {}", e))
        })?;

        let deps = self.deps.clone();

        let result_json: String = ctx
            .run(|| async move {
                let trigger = InvestigationTrigger::FlaggedSignal { signal_id };
                let finding = run_why_investigation(trigger, &deps)
                    .await
                    .map_err(|e| TerminalError::new(format!("Investigation failed: {}", e)))?;

                let result = WhyInvestigateResult {
                    investigation_id: None, // Investigation ID is internal
                    status: if finding.is_some() {
                        "completed".to_string()
                    } else {
                        "completed_no_finding".to_string()
                    },
                    finding_id: finding.map(|f| f.id.to_string()),
                };

                serde_json::to_string(&result)
                    .map_err(|e| TerminalError::new(format!("Serialize failed: {}", e)).into())
            })
            .await?;

        let result: WhyInvestigateResult = serde_json::from_str(&result_json)
            .map_err(|e| TerminalError::new(format!("Deserialize: {}", e)))?;

        ctx.set("status", result.status.clone());

        tracing::info!(status = %result.status, finding_id = ?result.finding_id, "WhyInvestigationWorkflow.completed");
        Ok(result)
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

// ─── Cluster Detection Workflow ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterDetectionResult {
    pub triggered_signal_ids: Vec<String>,
    pub status: String,
}
impl_restate_serde!(ClusterDetectionResult);

#[restate_sdk::workflow]
#[name = "ClusterDetectionWorkflow"]
pub trait ClusterDetectionWorkflow {
    async fn run(req: EmptyRequest) -> Result<ClusterDetectionResult, HandlerError>;

    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct ClusterDetectionWorkflowImpl {
    deps: Arc<ServerDeps>,
}

impl ClusterDetectionWorkflowImpl {
    pub fn with_deps(deps: Arc<ServerDeps>) -> Self {
        Self { deps }
    }
}

impl ClusterDetectionWorkflow for ClusterDetectionWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        _req: EmptyRequest,
    ) -> Result<ClusterDetectionResult, HandlerError> {
        tracing::info!("ClusterDetectionWorkflow.start");
        ctx.set("status", "detecting".to_string());

        let deps = self.deps.clone();

        let result_json: String = ctx
            .run(|| async move {
                let triggered = detect_signal_clusters(&deps)
                    .await
                    .map_err(|e| TerminalError::new(format!("Cluster detection failed: {}", e)))?;

                let result = ClusterDetectionResult {
                    triggered_signal_ids: triggered.iter().map(|id| id.to_string()).collect(),
                    status: "completed".to_string(),
                };

                serde_json::to_string(&result)
                    .map_err(|e| TerminalError::new(format!("Serialize failed: {}", e)).into())
            })
            .await?;

        let result: ClusterDetectionResult = serde_json::from_str(&result_json)
            .map_err(|e| TerminalError::new(format!("Deserialize: {}", e)))?;

        ctx.set("status", result.status.clone());

        tracing::info!(
            triggered_count = result.triggered_signal_ids.len(),
            "ClusterDetectionWorkflow.completed"
        );
        Ok(result)
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
