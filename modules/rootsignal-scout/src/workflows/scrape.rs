//! Restate durable workflow for the scrape pipeline.
//!
//! Encapsulates the core scrape cycle from `Scout::run_inner()`:
//! reap → load/schedule → Phase A → mid-run discovery → Phase B →
//! topic discovery → expansion → metrics → end-of-run discovery.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::info;

use rootsignal_graph::GraphStore;

use super::types::{EmptyRequest, ScrapeResult, TaskRequest};
use super::{create_archive, ScoutDeps};

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
                scrape_region(&deps, &scope)
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

async fn scrape_region(
    deps: &ScoutDeps,
    scope: &rootsignal_common::ScoutScope,
) -> anyhow::Result<ScrapeResult> {
    let graph = GraphStore::new(deps.graph_client.clone());
    let event_store = rootsignal_events::EventStore::new(deps.pg_pool.clone());
    let extractor: Arc<dyn crate::core::extractor::SignalExtractor> =
        Arc::new(crate::core::extractor::Extractor::new(
            &deps.anthropic_api_key,
            scope.name.as_str(),
            scope.center_lat,
            scope.center_lng,
        ));
    let embedder: Arc<dyn crate::infra::embedder::TextEmbedder> =
        Arc::new(crate::infra::embedder::Embedder::new(&deps.voyage_api_key));
    let archive = create_archive(deps);
    let budget = Arc::new(crate::domains::scheduling::activities::budget::BudgetTracker::new(deps.daily_budget_cents));
    let run_id = uuid::Uuid::new_v4().to_string();

    let pipeline = crate::workflows::scrape_pipeline::ScrapePipeline::new(
        graph,
        deps.graph_client.clone(),
        event_store,
        extractor,
        embedder,
        archive,
        deps.anthropic_api_key.clone(),
        scope.clone(),
        budget.clone(),
        Arc::new(AtomicBool::new(false)),
        run_id.clone(),
        deps.pg_pool.clone(),
    );

    let stats = pipeline.dispatch_pipeline().await?;

    Ok(ScrapeResult {
        urls_scraped: stats.urls_scraped,
        signals_stored: stats.signals_stored,
        spent_cents: budget.total_spent(),
    })
}
