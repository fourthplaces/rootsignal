//! Restate durable workflow for the scrape pipeline.
//!
//! Encapsulates the core scrape cycle from `Scout::run_inner()`:
//! reap → load/schedule → Phase A → mid-run discovery → Phase B →
//! topic discovery → expansion → metrics → end-of-run discovery.
//!
//! Uses `std::thread::spawn` with a dedicated tokio runtime because
//! `ScrapePhase::run_web()` uses `for_each_concurrent` with closures
//! that have higher-ranked lifetime bounds, making the future `!Send`
//! (incompatible with `tokio::spawn`).

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use restate_sdk::prelude::*;
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

        // Use std::thread::spawn + dedicated runtime because ScrapePhase's
        // for_each_concurrent closures have HRTB bounds that are !Send.
        let (tx, rx) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
            let result = rt.block_on(run_scrape_pipeline_from_deps(&deps, &scope));
            let _ = tx.send(result);
        });

        let result = rx
            .await
            .map_err(|_| -> HandlerError {
                TerminalError::new("Scrape thread dropped without sending result").into()
            })?
            .map_err(|e| -> HandlerError {
                TerminalError::new(e.to_string()).into()
            })?;

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
        Ok(ctx
            .get::<String>("status")
            .await?
            .unwrap_or_else(|| "pending".to_string()))
    }
}

async fn run_scrape_pipeline_from_deps(
    deps: &ScoutDeps,
    scope: &rootsignal_common::ScoutScope,
) -> anyhow::Result<ScrapeResult> {
    let writer = GraphWriter::new(deps.graph_client.clone());
    let extractor = Box::new(crate::extractor::Extractor::new(
        &deps.anthropic_api_key,
        scope.name.as_str(),
        scope.center_lat,
        scope.center_lng,
    ));
    let embedder: Arc<dyn crate::embedder::TextEmbedder> =
        Arc::new(crate::embedder::Embedder::new(&deps.voyage_api_key));
    let region_slug = rootsignal_common::slugify(&scope.name);
    let archive = create_archive(deps, &region_slug);
    let budget = crate::budget::BudgetTracker::new(deps.daily_budget_cents);
    let run_id = uuid::Uuid::new_v4().to_string();

    // Ensure archive tables exist
    archive.migrate().await?;

    let stats = crate::scout::run_scrape_pipeline(
        &writer,
        &*extractor,
        &*embedder,
        archive,
        &deps.anthropic_api_key,
        scope,
        &budget,
        Arc::new(AtomicBool::new(false)),
        &run_id,
    )
    .await?;

    Ok(ScrapeResult {
        urls_scraped: stats.urls_scraped,
        signals_stored: stats.signals_stored,
        spent_cents: budget.total_spent(),
    })
}
