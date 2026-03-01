//! Restate durable workflow for synthesis.
//!
//! Emits PhaseCompleted(Expansion) into a full engine — triggers synthesis
//! and all downstream phases (situation weaving, supervisor, finalize).

use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::info;

use crate::core::events::PipelinePhase;
use crate::domains::lifecycle::events::LifecycleEvent;

use super::types::{BudgetedTaskRequest, EmptyRequest, SynthesisResult};
use super::ScoutDeps;

#[restate_sdk::workflow]
#[name = "SynthesisWorkflow"]
pub trait SynthesisWorkflow {
    async fn run(req: BudgetedTaskRequest) -> Result<SynthesisResult, HandlerError>;
    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct SynthesisWorkflowImpl {
    deps: Arc<ScoutDeps>,
}

impl SynthesisWorkflowImpl {
    pub fn with_deps(deps: Arc<ScoutDeps>) -> Self {
        Self { deps }
    }
}

impl SynthesisWorkflow for SynthesisWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        req: BudgetedTaskRequest,
    ) -> Result<SynthesisResult, HandlerError> {
        let task_id = req.task_id.clone();

        // Status transition guard (journaled so it's skipped on replay)
        let tid = task_id.clone();
        let graph_client = self.deps.graph_client.clone();
        ctx.run(|| async move {
            let graph = rootsignal_graph::GraphStore::new(graph_client);
            let transitioned = graph
                .transition_task_phase_status(
                    &tid,
                    &[
                        "scrape_complete",
                        "synthesis_complete",
                        "situation_weaver_complete",
                        "complete",
                    ],
                    "running_synthesis",
                )
                .await
                .map_err(|e| TerminalError::new(format!("Status check failed: {e}")))?;
            if !transitioned {
                return Err(TerminalError::new(
                    "Prerequisites not met or another phase is running",
                )
                .into());
            }
            Ok(())
        })
        .await?;

        ctx.set("status", "Starting synthesis...".to_string());

        let deps = self.deps.clone();
        let scope = req.scope.clone();
        let spent_cents = req.spent_cents;

        let result = match ctx
            .run(|| async {
                let run_id = uuid::Uuid::new_v4().to_string();
                let engine = deps.build_full_engine(&scope, &run_id, spent_cents);

                // Emit PhaseCompleted(Expansion) — triggers synthesis + all downstream
                engine
                    .emit(LifecycleEvent::PhaseCompleted {
                        phase: PipelinePhase::Expansion,
                    })
                    .settled()
                    .await
                    .map_err(|e| -> HandlerError { TerminalError::new(e.to_string()).into() })?;

                let budget = engine
                    .deps()
                    .budget
                    .as_ref()
                    .map(|b| b.total_spent())
                    .unwrap_or(0);
                Ok(SynthesisResult {
                    spent_cents: budget,
                })
            })
            .await
        {
            Ok(v) => v,
            Err(e) => {
                super::write_task_phase_status(&self.deps, &task_id, "idle").await;
                return Err(e.into());
            }
        };

        super::write_task_phase_status(&self.deps, &task_id, "synthesis_complete").await;

        ctx.set("status", "Synthesis complete".to_string());
        info!("SynthesisWorkflow complete");

        Ok(result)
    }

    async fn get_status(
        &self,
        ctx: SharedWorkflowContext<'_>,
        _req: EmptyRequest,
    ) -> Result<String, HandlerError> {
        super::read_workflow_status(&ctx).await
    }
}
