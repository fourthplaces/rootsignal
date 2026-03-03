//! Restate durable workflow for cold-start bootstrapping.
//!
//! Dispatches `EngineStarted` — the handler checks whether the region has
//! sources and seeds them if empty.

use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::info;

use rootsignal_graph::GraphStore;

use crate::core::aggregate::PipelineState;
use crate::domains::lifecycle::events::LifecycleEvent;

use super::restate_runtime::RestateRuntime;
use super::types::{BootstrapResult, EmptyRequest, TaskRequest};
use super::{journaled_emit_task_phase_status, ScoutDeps};

#[restate_sdk::workflow]
#[name = "BootstrapWorkflow"]
pub trait BootstrapWorkflow {
    async fn run(req: TaskRequest) -> Result<BootstrapResult, HandlerError>;
    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct BootstrapWorkflowImpl {
    deps: Arc<ScoutDeps>,
}

impl BootstrapWorkflowImpl {
    pub fn with_deps(deps: Arc<ScoutDeps>) -> Self {
        Self { deps }
    }
}

impl BootstrapWorkflow for BootstrapWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        req: TaskRequest,
    ) -> Result<BootstrapResult, HandlerError> {
        let task_id = req.task_id.clone();
        let run_id = req.run_id.clone();

        // Status transition guard (journaled so it's skipped on replay)
        let tid = task_id.clone();
        let graph_client = self.deps.graph_client.clone();
        ctx.run(|| async move {
            let graph = GraphStore::new(graph_client);
            let transitioned = graph
                .transition_task_phase_status(
                    &tid,
                    &[
                        "idle",
                        "bootstrap_complete",
                        "scrape_complete",
                        "synthesis_complete",
                        "situation_weaver_complete",
                        "complete",
                    ],
                    "running_bootstrap",
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

        ctx.set("status", "Starting bootstrap...".to_string());

        let engine = self.deps.build_scrape_engine(
            &req.scope,
            &run_id,
            Some(&task_id),
            Some("bootstrap_complete"),
        );
        let runtime = RestateRuntime::new(&ctx);
        if let Err(e) = engine
            .emit(LifecycleEvent::EngineStarted {
                run_id: run_id.clone(),
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

        let state = engine.singleton::<PipelineState>();
        let sources_created = state.stats.sources_discovered;

        ctx.set(
            "status",
            format!("Bootstrap complete: {sources_created} sources"),
        );
        info!(sources_created, "BootstrapWorkflow complete");

        Ok(BootstrapResult { sources_created })
    }

    async fn get_status(
        &self,
        ctx: SharedWorkflowContext<'_>,
        _req: EmptyRequest,
    ) -> Result<String, HandlerError> {
        super::read_workflow_status(&ctx).await
    }
}
