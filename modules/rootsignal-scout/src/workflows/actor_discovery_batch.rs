//! Restate durable workflow for batch actor discovery via web search.
//!
//! Replaces the `discover_actors` GraphQL mutation's inline work:
//! web search → per-URL actor creation (each individually durable/retriable).

use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::{info, warn};

use rootsignal_graph::GraphWriter;

use super::types::{
    CreateFromPageResult, DiscoverActorsBatchRequest, DiscoverActorsBatchResult, EmptyRequest,
    MaybeActor, UrlList,
};
use super::{create_archive, ScoutDeps};

#[restate_sdk::workflow]
#[name = "ActorDiscoveryBatchWorkflow"]
pub trait ActorDiscoveryBatchWorkflow {
    async fn run(
        req: DiscoverActorsBatchRequest,
    ) -> Result<DiscoverActorsBatchResult, HandlerError>;
    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct ActorDiscoveryBatchWorkflowImpl {
    deps: Arc<ScoutDeps>,
}

impl ActorDiscoveryBatchWorkflowImpl {
    pub fn with_deps(deps: Arc<ScoutDeps>) -> Self {
        Self { deps }
    }
}

impl ActorDiscoveryBatchWorkflow for ActorDiscoveryBatchWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        req: DiscoverActorsBatchRequest,
    ) -> Result<DiscoverActorsBatchResult, HandlerError> {
        ctx.set("status", format!("Searching for '{}'...", req.query));

        let deps = self.deps.clone();
        let query = req.query.clone();
        let max_results = req.max_results;

        // 1. Web search — collect URLs
        let UrlList(urls) = ctx
            .run(|| async move {
                let archive = create_archive(&deps);
                let handle = archive.source(&query).await.map_err(|e| -> HandlerError {
                    TerminalError::new(e.to_string()).into()
                })?;
                let search = handle.search(&query).max_results(max_results).await.map_err(|e| -> HandlerError {
                    TerminalError::new(e.to_string()).into()
                })?;
                Ok(UrlList(search.results.into_iter().map(|r| r.url).collect()))
            })
            .await?;

        ctx.set(
            "status",
            format!("Processing {} URLs...", urls.len()),
        );

        // 2. Process each URL individually (durable per-URL)
        let mut actors: Vec<CreateFromPageResult> = Vec::new();
        for url in &urls {
            let deps = self.deps.clone();
            let url = url.clone();
            let region = req.region.clone();

            let result = ctx
                .run(|| async move {
                    let archive = create_archive(&deps);
                    let writer = GraphWriter::new(deps.graph_client.clone());

                    match crate::discovery::actor_discovery::create_actor_from_page(
                        &archive,
                        &writer,
                        &deps.anthropic_api_key,
                        &url,
                        &region,
                        true, // discover mode: require social links
                        0.0,
                        0.0,
                    )
                    .await
                    {
                        Ok(Some(r)) => Ok(MaybeActor(Some(CreateFromPageResult {
                            actor_id: Some(r.actor_id.to_string()),
                            location_name: Some(r.location_name),
                        }))),
                        Ok(None) => Ok(MaybeActor(None)),
                        Err(e) => {
                            warn!(url = url.as_str(), error = %e, "Failed to process search result");
                            Ok(MaybeActor(None))
                        }
                    }
                })
                .await;

            match result {
                Ok(MaybeActor(Some(actor))) => actors.push(actor),
                Ok(MaybeActor(None)) => {}
                Err(e) => {
                    warn!(error = ?e, "Actor creation failed for URL");
                }
            }
        }

        let discovered = actors.len() as u32;
        info!(query = req.query.as_str(), region = req.region.as_str(), discovered, "ActorDiscoveryBatch complete");

        ctx.set(
            "status",
            format!("Complete: {discovered} actors discovered"),
        );

        Ok(DiscoverActorsBatchResult { discovered, actors })
    }

    async fn get_status(
        &self,
        ctx: SharedWorkflowContext<'_>,
        _req: EmptyRequest,
    ) -> Result<String, HandlerError> {
        super::read_workflow_status(&ctx).await
    }
}
