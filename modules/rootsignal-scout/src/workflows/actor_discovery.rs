//! Restate durable workflow for actor discovery.
//!
//! Wraps `Bootstrapper::discover_actor_pages()` — searches the web for
//! organization pages, extracts actor identity via LLM, and creates Actor nodes.

use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::info;

use rootsignal_graph::GraphWriter;

use super::types::{ActorDiscoveryResult, EmptyRequest, RegionRequest};
use super::{create_archive, ScoutDeps};

#[restate_sdk::workflow]
#[name = "ActorDiscoveryWorkflow"]
pub trait ActorDiscoveryWorkflow {
    async fn run(req: RegionRequest) -> Result<ActorDiscoveryResult, HandlerError>;
    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct ActorDiscoveryWorkflowImpl {
    deps: Arc<ScoutDeps>,
}

impl ActorDiscoveryWorkflowImpl {
    pub fn with_deps(deps: Arc<ScoutDeps>) -> Self {
        Self { deps }
    }
}

impl ActorDiscoveryWorkflow for ActorDiscoveryWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        req: RegionRequest,
    ) -> Result<ActorDiscoveryResult, HandlerError> {
        // Status transition guard (journaled so it's skipped on replay)
        let slug = rootsignal_common::slugify(&req.scope.name);
        let graph_client = self.deps.graph_client.clone();
        ctx.run(|| async move {
            let writer = GraphWriter::new(graph_client);
            let transitioned = writer
                .transition_region_status(
                    &slug,
                    &[
                        "bootstrap_complete", "actor_discovery_complete", "scrape_complete",
                        "synthesis_complete", "situation_weaver_complete", "complete",
                    ],
                    "running_actor_discovery",
                )
                .await
                .map_err(|e| TerminalError::new(format!("Status check failed: {e}")))?;
            if !transitioned {
                return Err(TerminalError::new("Prerequisites not met or another phase is running").into());
            }
            Ok(())
        })
        .await?;
        let slug = rootsignal_common::slugify(&req.scope.name);

        ctx.set("status", "Discovering actors...".to_string());
        let archive = create_archive(&self.deps);
        let api_key = self.deps.anthropic_api_key.clone();
        let scope = req.scope.clone();
        let graph_client = self.deps.graph_client.clone();

        let actors_discovered = match ctx
            .run(|| async {
                let writer = GraphWriter::new(graph_client);
                let bootstrapper = crate::discovery::bootstrap::Bootstrapper::new(
                    &writer,
                    archive,
                    &api_key,
                    scope,
                );
                let discovered = bootstrapper.discover_actor_pages().await;
                Ok::<_, HandlerError>(discovered.len() as u32)
            })
            .await
        {
            Ok(v) => v,
            Err(e) => {
                super::write_phase_status(&self.deps, &slug, "idle").await;
                return Err(e.into());
            }
        };

        let region_key = rootsignal_common::slugify(&req.scope.name);
        super::write_phase_status(&self.deps, &region_key, "actor_discovery_complete").await;

        ctx.set(
            "status",
            format!("Discovery complete: {actors_discovered} actors"),
        );
        info!(actors_discovered, "ActorDiscoveryWorkflow complete");

        Ok(ActorDiscoveryResult { actors_discovered })
    }

    async fn get_status(
        &self,
        ctx: SharedWorkflowContext<'_>,
        _req: EmptyRequest,
    ) -> Result<String, HandlerError> {
        super::read_workflow_status(&ctx).await
    }
}
