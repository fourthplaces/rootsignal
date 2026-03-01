//! Restate durable workflow for situation weaving.
//!
//! Emits PhaseCompleted(Synthesis) into a full engine — triggers situation
//! weaving and all downstream phases (supervisor, finalize).

use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::info;

use crate::core::events::PipelinePhase;
use crate::domains::lifecycle::events::LifecycleEvent;

use super::types::{BudgetedTaskRequest, EmptyRequest, SituationWeaverResult};
use super::ScoutDeps;

#[restate_sdk::workflow]
#[name = "SituationWeaverWorkflow"]
pub trait SituationWeaverWorkflow {
    async fn run(req: BudgetedTaskRequest) -> Result<SituationWeaverResult, HandlerError>;
    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct SituationWeaverWorkflowImpl {
    deps: Arc<ScoutDeps>,
}

impl SituationWeaverWorkflowImpl {
    pub fn with_deps(deps: Arc<ScoutDeps>) -> Self {
        Self { deps }
    }
}

impl SituationWeaverWorkflow for SituationWeaverWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        req: BudgetedTaskRequest,
    ) -> Result<SituationWeaverResult, HandlerError> {
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
                        "synthesis_complete",
                        "situation_weaver_complete",
                        "complete",
                    ],
                    "running_situation_weaver",
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

        ctx.set("status", "Starting situation weaving...".to_string());

        let deps = self.deps.clone();
        let scope = req.scope.clone();
        let spent_cents = req.spent_cents;

        let result = match ctx
            .run(|| async {
                let run_id = uuid::Uuid::new_v4().to_string();
                let engine = deps.build_full_engine(&scope, &run_id, spent_cents);

                // Emit PhaseCompleted(Synthesis) — triggers situation weaving + downstream
                engine
                    .emit(LifecycleEvent::PhaseCompleted {
                        phase: PipelinePhase::Synthesis,
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
                Ok(SituationWeaverResult {
                    situations_woven: 0,
                    spent_cents: budget,
                })
            })
            .retry_policy(super::phase_retry_policy())
            .await
        {
            Ok(v) => v,
            Err(e) => {
                let _ =
                    super::journaled_write_task_phase_status(&ctx, &self.deps, &task_id, "idle")
                        .await;
                return Err(e.into());
            }
        };

        super::journaled_write_task_phase_status(
            &ctx,
            &self.deps,
            &task_id,
            "situation_weaver_complete",
        )
        .await?;

        ctx.set("status", "Situation weaving complete".to_string());
        info!("SituationWeaverWorkflow complete");

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
