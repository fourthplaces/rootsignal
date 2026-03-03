//! Restate durable workflow for the supervisor.
//!
//! Emits PhaseCompleted(SituationWeaving) into a full engine — triggers
//! supervisor and finalize.

use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::info;

use crate::core::events::PipelinePhase;
use crate::domains::lifecycle::events::LifecycleEvent;

use super::types::{EmptyRequest, SupervisorResult, TaskRequest};
use super::{journaled_emit_task_phase_status, ScoutDeps};

#[restate_sdk::workflow]
#[name = "SupervisorWorkflow"]
pub trait SupervisorWorkflow {
    async fn run(req: TaskRequest) -> Result<SupervisorResult, HandlerError>;
    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct SupervisorWorkflowImpl {
    deps: Arc<ScoutDeps>,
}

impl SupervisorWorkflowImpl {
    pub fn with_deps(deps: Arc<ScoutDeps>) -> Self {
        Self { deps }
    }
}

impl SupervisorWorkflow for SupervisorWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        req: TaskRequest,
    ) -> Result<SupervisorResult, HandlerError> {
        let task_id = req.task_id.clone();

        // Status transition guard (journaled so it's skipped on replay)
        let tid = task_id.clone();
        let graph_client = self.deps.graph_client.clone();
        ctx.run(|| async move {
            let graph = rootsignal_graph::GraphStore::new(graph_client);
            let transitioned = graph
                .transition_task_phase_status(
                    &tid,
                    &["lint_complete", "situation_weaver_complete", "complete"],
                    "running_supervisor",
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

        ctx.set("status", "Starting supervisor...".to_string());

        let deps = self.deps.clone();
        let scope = req.scope.clone();
        let tid = task_id.clone();

        let result = match ctx
            .run(|| async {
                let run_id = uuid::Uuid::new_v4().to_string();
                let engine = deps.build_full_engine(
                    &scope,
                    &run_id,
                    0,
                    Some(&tid),
                    Some("complete"),
                );

                // Emit PhaseCompleted(SituationWeaving) — triggers supervisor + finalize
                engine
                    .emit(LifecycleEvent::PhaseCompleted {
                        phase: PipelinePhase::SituationWeaving,
                    })
                    .settled()
                    .await
                    .map_err(|e| -> HandlerError { TerminalError::new(e.to_string()).into() })?;

                Ok(SupervisorResult { issues_found: 0 })
            })
            .retry_policy(super::phase_retry_policy())
            .await
        {
            Ok(v) => v,
            Err(e) => {
                let _ = journaled_emit_task_phase_status(
                    &ctx, self.deps.pg_pool.clone(), self.deps.graph_client.clone(),
                    &task_id, "idle",
                ).await;
                return Err(e.into());
            }
        };

        ctx.set(
            "status",
            format!("Supervisor complete: {} issues", result.issues_found),
        );
        info!(
            issues_found = result.issues_found,
            "SupervisorWorkflow complete"
        );

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
