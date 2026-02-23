//! Restate durable workflow for the scrape pipeline.
//!
//! Encapsulates the core scrape cycle from `Scout::run_inner()`:
//! reap → load/schedule → Phase A → mid-run discovery → Phase B →
//! topic discovery → expansion → metrics → end-of-run discovery.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use restate_sdk::prelude::*;
use tokio::sync::watch;
use tracing::info;

use rootsignal_graph::GraphWriter;

use super::types::{EmptyRequest, RegionRequest, ScrapeResult};
use super::{create_archive, ScoutDeps};

#[restate_sdk::workflow]
#[name = "ScrapeWorkflow"]
pub trait ScrapeWorkflow {
    async fn run(req: RegionRequest) -> Result<ScrapeResult, HandlerError>;
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
        req: RegionRequest,
    ) -> Result<ScrapeResult, HandlerError> {
        ctx.set("status", "Starting scrape pipeline...".to_string());

        let deps = self.deps.clone();
        let scope = req.scope.clone();

        let (status_tx, mut status_rx) = watch::channel("Starting...".to_string());

        let mut handle = tokio::spawn(async move {
            run_scrape_from_deps(&deps, &scope, status_tx).await
        });

        let result = loop {
            tokio::select! {
                result = &mut handle => {
                    break result
                        .map_err(|e| -> HandlerError {
                            TerminalError::new(format!("Scrape task panicked: {e}")).into()
                        })?
                        .map_err(|e| -> HandlerError {
                            TerminalError::new(e.to_string()).into()
                        })?;
                }
                Ok(()) = status_rx.changed() => {
                    ctx.set("status", status_rx.borrow_and_update().clone());
                }
            }
        };

        let region_key = rootsignal_common::slugify(&req.scope.name);
        super::write_phase_status(&self.deps, &region_key, "scrape_complete").await;

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
    status: watch::Sender<String>,
) -> anyhow::Result<ScrapeResult> {
    let writer = GraphWriter::new(deps.graph_client.clone());
    let extractor: Arc<dyn crate::pipeline::extractor::SignalExtractor> =
        Arc::new(crate::pipeline::extractor::Extractor::new(
            &deps.anthropic_api_key,
            scope.name.as_str(),
            scope.center_lat,
            scope.center_lng,
        ));
    let embedder: Arc<dyn crate::infra::embedder::TextEmbedder> =
        Arc::new(crate::infra::embedder::Embedder::new(&deps.voyage_api_key));
    let region_slug = rootsignal_common::slugify(&scope.name);
    let archive = create_archive(deps);
    let budget = crate::scheduling::budget::BudgetTracker::new(deps.daily_budget_cents);
    let run_id = uuid::Uuid::new_v4().to_string();

    let pipeline = crate::pipeline::scrape_pipeline::ScrapePipeline::new(
        writer,
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

    let mut run_log = crate::infra::run_log::RunLog::new(run_id, scope.name.clone());

    let _ = status.send("Reaping expired signals...".into());
    pipeline.reap_expired_signals(&mut run_log).await;

    let _ = status.send("Loading and scheduling sources...".into());
    let (run, mut ctx) = pipeline.load_and_schedule_sources(&mut run_log).await?;

    let _ = status.send("Scraping tension sources...".into());
    pipeline.scrape_tension_sources(&run, &mut ctx, &mut run_log).await;

    let _ = status.send("Discovering new sources...".into());
    let (_, social_topics) = pipeline.discover_mid_run_sources().await;

    let _ = status.send("Scraping response sources...".into());
    pipeline.scrape_response_sources(&run, social_topics, &mut ctx, &mut run_log).await?;

    let _ = status.send("Updating metrics and expanding...".into());
    pipeline.update_source_metrics(&run, &ctx).await;
    pipeline.expand_and_discover(&run, &mut ctx, &mut run_log).await?;

    let stats = pipeline.finalize(ctx, run_log).await;

    Ok(ScrapeResult {
        urls_scraped: stats.urls_scraped,
        signals_stored: stats.signals_stored,
        spent_cents: budget.total_spent(),
    })
}
