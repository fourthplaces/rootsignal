//! Restate durable workflow for the scrape pipeline.
//!
//! Encapsulates the core scrape cycle from `Scout::run_inner()`:
//! reap → load/schedule → Phase A → mid-run discovery → Phase B →
//! topic discovery → expansion → metrics → end-of-run discovery.
//!
//! RunContext (embedding cache, URL maps, expansion queries) lives
//! entirely within this workflow — never serialized across boundaries.
//!
//! TODO(Phase 3): The scrape pipeline uses `ScrapePhase::run_web()` which
//! contains async closures with higher-ranked lifetime bounds that are
//! incompatible with Restate's `#[workflow]` macro. Fix requires changing
//! `ScrapePhase` to use `FuturesUnordered` or boxed futures instead of
//! `for_each_concurrent` with borrowed closures.

use std::sync::Arc;

use restate_sdk::prelude::*;

use super::types::{EmptyRequest, RegionRequest, ScrapeResult};
use super::ScoutDeps;

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
        _ctx: WorkflowContext<'_>,
        _req: RegionRequest,
    ) -> Result<ScrapeResult, HandlerError> {
        // TODO(Phase 3): Wire up the full scrape pipeline once ScrapePhase's
        // HRTB lifetime constraints are resolved. See module doc comment.
        Err(TerminalError::new("ScrapeWorkflow not yet implemented — use Scout::run() directly").into())
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
