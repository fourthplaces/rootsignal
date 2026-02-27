//! Restate durable workflow for the scrape pipeline.
//!
//! Encapsulates the core scrape cycle from `Scout::run_inner()`:
//! reap → load/schedule → Phase A → mid-run discovery → Phase B →
//! topic discovery → expansion → metrics → end-of-run discovery.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::info;

use rootsignal_graph::GraphWriter;

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
            let writer = rootsignal_graph::GraphWriter::new(graph_client);
            let transitioned = writer
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
                run_scrape_from_deps(&deps, &scope)
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

async fn run_scrape_from_deps(
    deps: &ScoutDeps,
    scope: &rootsignal_common::ScoutScope,
) -> anyhow::Result<ScrapeResult> {
    let writer = GraphWriter::new(deps.graph_client.clone());
    let event_store = rootsignal_events::EventStore::new(deps.pg_pool.clone());
    let extractor: Arc<dyn crate::pipeline::extractor::SignalExtractor> =
        Arc::new(crate::pipeline::extractor::Extractor::new(
            &deps.anthropic_api_key,
            scope.name.as_str(),
            scope.center_lat,
            scope.center_lng,
        ));
    let embedder: Arc<dyn crate::infra::embedder::TextEmbedder> =
        Arc::new(crate::infra::embedder::Embedder::new(&deps.voyage_api_key));
    let archive = create_archive(deps);
    let budget = crate::scheduling::budget::BudgetTracker::new(deps.daily_budget_cents);
    let run_id = uuid::Uuid::new_v4().to_string();

    let pipeline = crate::pipeline::scrape_pipeline::ScrapePipeline::new(
        writer,
        deps.graph_client.clone(),
        event_store,
        extractor,
        embedder,
        archive,
        deps.anthropic_api_key.clone(),
        scope.clone(),
        &budget,
        Arc::new(AtomicBool::new(false)),
        run_id.clone(),
        deps.pg_pool.clone(),
    );

    let run_log =
        crate::infra::run_log::RunLogger::new(run_id, scope.name.clone(), deps.pg_pool.clone())
            .await;

    pipeline.reap_expired_signals(&run_log).await;
    let (run, mut ctx) = pipeline.load_and_schedule_sources(&run_log).await?;
    pipeline
        .scrape_tension_sources(&run, &mut ctx, &run_log)
        .await;
    let (_, social_topics, mid_run_sources) = pipeline.discover_mid_run_sources().await;
    if !mid_run_sources.is_empty() {
        run.phase
            .register_sources(mid_run_sources, "source_finder", &mut ctx)
            .await?;
    }
    pipeline
        .scrape_response_sources(&run, social_topics, &mut ctx, &run_log)
        .await?;
    pipeline.update_source_metrics(&run, &ctx).await;
    pipeline
        .expand_and_discover(&run, &mut ctx, &run_log)
        .await?;

    let stats = pipeline.finalize(ctx, run_log).await;

    Ok(ScrapeResult {
        urls_scraped: stats.urls_scraped,
        signals_stored: stats.signals_stored,
        spent_cents: budget.total_spent(),
    })
}
