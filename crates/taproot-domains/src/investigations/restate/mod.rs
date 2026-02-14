use restate_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use taproot_core::ServerDeps;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestigateRequest {
    pub subject_type: String,
    pub subject_id: String,
    pub trigger: String,
}
impl_restate_serde!(InvestigateRequest);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestigateResult {
    pub investigation_id: String,
    pub status: String,
    pub summary: Option<String>,
    pub confidence: Option<f32>,
}
impl_restate_serde!(InvestigateResult);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmptyRequest {}
impl_restate_serde!(EmptyRequest);

#[restate_sdk::workflow]
#[name = "InvestigateWorkflow"]
pub trait InvestigateWorkflow {
    async fn run(req: InvestigateRequest) -> Result<InvestigateResult, HandlerError>;

    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct InvestigateWorkflowImpl {
    deps: Arc<ServerDeps>,
}

impl InvestigateWorkflowImpl {
    pub fn with_deps(deps: Arc<ServerDeps>) -> Self {
        Self { deps }
    }
}

impl InvestigateWorkflow for InvestigateWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        req: InvestigateRequest,
    ) -> Result<InvestigateResult, HandlerError> {
        ctx.set("status", "investigating".to_string());

        let subject_id: Uuid = req.subject_id.parse().map_err(|e: uuid::Error| {
            TerminalError::new(format!("Invalid subject_id UUID: {}", e))
        })?;

        let deps = self.deps.clone();
        let subject_type = req.subject_type.clone();
        let trigger = req.trigger.clone();

        let result_json: String = ctx
            .run(|| async move {
                let investigation = crate::investigations::run_investigation(
                    &subject_type,
                    subject_id,
                    &trigger,
                    &deps,
                )
                .await
                .map_err(|e| {
                    TerminalError::new(format!("Investigation failed: {}", e))
                })?;

                let result = InvestigateResult {
                    investigation_id: investigation.id.to_string(),
                    status: investigation.status,
                    summary: investigation.summary,
                    confidence: investigation.summary_confidence,
                };

                serde_json::to_string(&result)
                    .map_err(|e| TerminalError::new(format!("Serialize failed: {}", e)).into())
            })
            .await?;

        let result: InvestigateResult = serde_json::from_str(&result_json)
            .map_err(|e| TerminalError::new(format!("Deserialize: {}", e)))?;

        ctx.set("status", result.status.clone());

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
