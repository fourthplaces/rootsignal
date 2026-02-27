//! Restate durable workflow for cold-start bootstrapping.
//!
//! Dispatches `EngineStarted` — the handler checks whether the region has
//! sources and seeds them if empty.

use std::collections::HashMap;
use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::info;

use rootsignal_graph::GraphWriter;

use crate::pipeline::events::{PipelineEvent, ScoutEvent};
use crate::pipeline::state::PipelineState;

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
        let scope = req.scope.clone();

        let deps = self.deps.clone();
        let sources_created = match ctx
            .run(|| async {
                let run_id = uuid::Uuid::new_v4().to_string();
                let engine = deps.build_engine(&run_id);
                let store = deps.build_store();
                let embedder: Arc<dyn crate::infra::embedder::TextEmbedder> =
                    Arc::new(crate::infra::embedder::Embedder::new(&deps.voyage_api_key));
                let pipe_deps = deps.build_pipeline_deps(
                    Arc::new(store) as Arc<dyn crate::traits::SignalReader>,
                    embedder,
                    Some(archive as Arc<dyn crate::traits::ContentFetcher>),
                    scope,
                    &run_id,
                );
                let mut state = PipelineState::new(HashMap::new());

                engine
                    .dispatch(
                        ScoutEvent::Pipeline(PipelineEvent::EngineStarted {
                            run_id: run_id.clone(),
                        }),
                        &mut state,
                        &pipe_deps,
                    )
                    .await
                    .map_err(|e| -> HandlerError { TerminalError::new(e.to_string()).into() })?;

                Ok(state.stats.sources_discovered)
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
