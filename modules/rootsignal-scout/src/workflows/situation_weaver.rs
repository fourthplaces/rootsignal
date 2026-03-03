//! Restate durable workflow for situation weaving.
//!
//! Emits PhaseCompleted(Synthesis) into a full engine — triggers situation
//! weaving and all downstream phases (supervisor, finalize).

use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::info;

use crate::core::events::PipelinePhase;
use crate::domains::lifecycle::events::LifecycleEvent;

use super::restate_runtime::RestateRuntime;
use super::types::{BudgetedTaskRequest, EmptyRequest, SituationWeaverResult};
use super::{journaled_emit_task_phase_status, ScoutDeps};

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
        let run_id = req.run_id.clone();

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

        let engine = self.deps.build_full_engine(
            &req.scope,
            &run_id,
            req.spent_cents,
            Some(&task_id),
            Some("situation_weaver_complete"),
        );
        let runtime = RestateRuntime::new(&ctx);
        if let Err(e) = engine
            .emit(LifecycleEvent::PhaseCompleted {
                phase: PipelinePhase::Synthesis,
            })
            .settled_with(&runtime)
            .await
        {
            let _ = journaled_emit_task_phase_status(
                &ctx, self.deps.pg_pool.clone(), self.deps.graph_client.clone(),
                &task_id, "idle",
            ).await;
            return Err(TerminalError::new(e.to_string()).into());
        }

        let budget = engine
            .deps()
            .budget
            .as_ref()
            .map(|b| b.total_spent())
            .unwrap_or(0);
        let result = SituationWeaverResult {
            situations_woven: 0,
            spent_cents: budget,
        };

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
