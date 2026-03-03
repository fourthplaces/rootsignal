//! Restate durable workflow for synthesis.
//!
//! Emits PhaseCompleted(Expansion) into a full engine — triggers synthesis
//! and all downstream phases (situation weaving, supervisor, finalize).

use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::info;

use crate::core::events::PipelinePhase;
use crate::domains::lifecycle::events::LifecycleEvent;

use super::restate_runtime::RestateRuntime;
use super::types::{BudgetedTaskRequest, EmptyRequest, SynthesisResult};
use super::{journaled_emit_task_phase_status, ScoutDeps};

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

        let engine = self.deps.build_full_engine(
            &req.scope,
            &run_id,
            req.spent_cents,
            Some(&task_id),
            Some("synthesis_complete"),
        );
        let runtime = RestateRuntime::new(&ctx);
        if let Err(e) = engine
            .emit(LifecycleEvent::PhaseCompleted {
                phase: PipelinePhase::SignalExpansion,
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
        let result = SynthesisResult {
            spent_cents: budget,
        };

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
