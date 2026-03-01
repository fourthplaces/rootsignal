//! Restate durable workflow for cold-start bootstrapping.
//!
//! Dispatches `EngineStarted` — the handler checks whether the region has
//! sources and seeds them if empty.

use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::info;

use rootsignal_graph::GraphStore;

use crate::domains::lifecycle::events::LifecycleEvent;

use super::types::{BootstrapResult, EmptyRequest, TaskRequest};
use super::ScoutDeps;

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
        let scope = req.scope.clone();

        let deps = self.deps.clone();
        let sources_created = match ctx
            .run(|| async {
                let run_id = uuid::Uuid::new_v4().to_string();
                let engine = deps.build_scrape_engine(&scope, &run_id);
                engine
                    .emit(LifecycleEvent::EngineStarted {
                        run_id: run_id.clone(),
                    })
                    .settled()
                    .await
                    .map_err(|e| -> HandlerError { TerminalError::new(e.to_string()).into() })?;

                let sources = engine.deps().state.read().await.stats.sources_discovered;
                Ok(sources)
            })
            .await
        {
            Ok(v) => v,
            Err(e) => {
                super::write_task_phase_status(&self.deps, &task_id, "idle").await;
                let err: HandlerError = e.into();
                return Err(err);
            }
        };

        super::write_task_phase_status(&self.deps, &task_id, "bootstrap_complete").await;

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
