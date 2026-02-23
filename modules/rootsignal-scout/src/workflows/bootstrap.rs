//! Restate durable workflow for cold-start bootstrapping.
//!
//! Wraps `Bootstrapper::run()` — generates seed queries, platform sources,
//! and discovers actor pages for a new region.

use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::info;

use rootsignal_graph::GraphWriter;

use super::types::{BootstrapResult, EmptyRequest, RegionRequest};
use super::{create_archive, ScoutDeps};

#[restate_sdk::workflow]
#[name = "BootstrapWorkflow"]
pub trait BootstrapWorkflow {
    async fn run(req: RegionRequest) -> Result<BootstrapResult, HandlerError>;
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
        req: RegionRequest,
    ) -> Result<BootstrapResult, HandlerError> {
        // Status transition guard — reject if prerequisites not met or another phase is running
        let slug = rootsignal_common::slugify(&req.scope.name);
        let writer = GraphWriter::new(self.deps.graph_client.clone());
        let transitioned = writer
            .transition_region_status(
                &slug,
                &[
                    "idle", "bootstrap_complete", "actor_discovery_complete",
                    "scrape_complete", "synthesis_complete", "situation_weaver_complete", "complete",
                ],
                "running_bootstrap",
            )
            .await
            .map_err(|e| TerminalError::new(format!("Status check failed: {e}")))?;
        if !transitioned {
            return Err(TerminalError::new("Prerequisites not met or another phase is running").into());
        }

        ctx.set("status", "Starting bootstrap...".to_string());
        let archive = create_archive(&self.deps);
        let api_key = self.deps.anthropic_api_key.clone();
        let scope = req.scope.clone();

        let sources_created = ctx
            .run(|| async {
                let bootstrapper = crate::discovery::bootstrap::Bootstrapper::new(
                    &writer,
                    archive,
                    &api_key,
                    scope,
                );
                bootstrapper
                    .run()
                    .await
                    .map_err(|e| -> HandlerError {
                        TerminalError::new(e.to_string()).into()
                    })
            })
            .await?;

        let region_key = rootsignal_common::slugify(&req.scope.name);
        super::write_phase_status(&self.deps, &region_key, "bootstrap_complete").await;

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
