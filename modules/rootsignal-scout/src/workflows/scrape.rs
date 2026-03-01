//! Restate durable workflow for the scrape pipeline.
//!
//! Builds a scrape-chain engine and emits `EngineStarted` — the handler chain
//! runs reap → schedule → scrape → enrichment → expansion → synthesis → finalize.
//! Does NOT include situation weaving or supervisor.

use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::info;

use crate::domains::lifecycle::events::LifecycleEvent;

use super::types::{EmptyRequest, ScrapeResult, TaskRequest};
use super::ScoutDeps;

#[restate_sdk::workflow]
#[name = "ScrapeWorkflow"]
pub trait ScrapeWorkflow {
    async fn run(req: TaskRequest) -> Result<ScrapeResult, HandlerError>;
    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct ScrapeWorkflowImpl {
    deps: Arc<ScoutDeps>,
}

impl ScrapeWorkflowImpl {
    pub fn with_deps(deps: Arc<ScoutDeps>) -> Self {
        Self { deps }
    }
}

impl ScrapeWorkflow for ScrapeWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        req: TaskRequest,
    ) -> Result<ScrapeResult, HandlerError> {
        let task_id = req.task_id.clone();

        // Status transition guard (journaled so it's skipped on replay)
        let tid = task_id.clone();
        let graph_client = self.deps.graph_client.clone();
        ctx.run(|| async move {
            let graph = rootsignal_graph::GraphStore::new(graph_client);
            let transitioned = graph
                .transition_task_phase_status(
                    &tid,
                    &[
                        "bootstrap_complete",
                        "scrape_complete",
                        "synthesis_complete",
                        "situation_weaver_complete",
                        "complete",
                    ],
                    "running_scrape",
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

        ctx.set("status", "Starting scrape pipeline...".to_string());

        let deps = self.deps.clone();
        let scope = req.scope.clone();

        let result = match ctx
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

                let state = engine.deps().state.read().await;
                let budget = engine
                    .deps()
                    .budget
                    .as_ref()
                    .map(|b| b.total_spent())
                    .unwrap_or(0);
                Ok(ScrapeResult {
                    urls_scraped: state.stats.urls_scraped,
                    signals_stored: state.stats.signals_stored,
                    spent_cents: budget,
                })
            })
            .await
        {
            Ok(v) => v,
            Err(e) => {
                super::write_task_phase_status(&self.deps, &task_id, "idle").await;
                return Err(e.into());
            }
        };

        super::write_task_phase_status(&self.deps, &task_id, "scrape_complete").await;

        ctx.set(
            "status",
            format!(
                "Scrape complete: {} URLs, {} signals",
                result.urls_scraped, result.signals_stored
            ),
        );
        info!(
            urls_scraped = result.urls_scraped,
            signals_stored = result.signals_stored,
            "ScrapeWorkflow complete"
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
