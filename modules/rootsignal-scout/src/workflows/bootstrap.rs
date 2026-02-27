//! Restate durable workflow for cold-start bootstrapping.
//!
//! Wraps `Bootstrapper::run()` — generates seed queries, platform sources,
//! and discovers actor pages for a new region.

use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::info;

use rootsignal_graph::GraphWriter;

use super::types::{BootstrapResult, EmptyRequest, TaskRequest};
use super::{create_archive, ScoutDeps};

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
            let writer = GraphWriter::new(graph_client);
            let transitioned = writer
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
        let archive = create_archive(&self.deps);
        let api_key = self.deps.anthropic_api_key.clone();
        let scope = req.scope.clone();

        let deps = self.deps.clone();
        let sources_created = match ctx
            .run(|| async {
                let writer = GraphWriter::new(deps.graph_client.clone());
                let store = deps.build_store(uuid::Uuid::new_v4().to_string());
                let bootstrapper = crate::discovery::bootstrap::Bootstrapper::new(
                    &writer,
                    &store as &dyn crate::traits::SignalStore,
                    archive,
                    &api_key,
                    scope,
                );
                bootstrapper
                    .run()
                    .await
                    .map_err(|e| -> HandlerError { TerminalError::new(e.to_string()).into() })
            })
            .await
        {
            Ok(v) => v,
            Err(e) => {
                super::write_task_phase_status(&self.deps, &task_id, "idle").await;
                return Err(e.into());
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
