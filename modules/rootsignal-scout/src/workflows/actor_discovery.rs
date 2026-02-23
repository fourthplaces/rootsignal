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
        ctx.set("status", "Discovering actors...".to_string());

        let writer = GraphWriter::new(self.deps.graph_client.clone());
        let archive = create_archive(&self.deps);
        let api_key = self.deps.anthropic_api_key.clone();
        let scope = req.scope.clone();

        let actors_discovered = ctx
            .run(|| async {
                let bootstrapper = crate::discovery::bootstrap::Bootstrapper::new(
                    &writer,
                    archive,
                    &api_key,
                    scope,
                );
                let discovered = bootstrapper.discover_actor_pages().await;
                Ok::<_, HandlerError>(discovered.len() as u32)
            })
            .await?;

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
