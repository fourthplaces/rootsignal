//! Restate durable workflow for situation weaving.
//!
//! Wraps steps 8, 8b, 8c from `Scout::run_inner()`:
//! - Situation weaving (assigns signals to living situations)
//! - Source boost for hot situations
//! - Curiosity-triggered re-investigation
//!
//! TODO(Phase 4): Verify Send safety of SituationWeaver::run() and
//! wire up the full pipeline.

use std::sync::Arc;

use restate_sdk::prelude::*;

use super::types::{BudgetedRegionRequest, EmptyRequest, SituationWeaverResult};
use super::ScoutDeps;

#[restate_sdk::workflow]
#[name = "SituationWeaverWorkflow"]
pub trait SituationWeaverWorkflow {
    async fn run(req: BudgetedRegionRequest) -> Result<SituationWeaverResult, HandlerError>;
    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct SituationWeaverWorkflowImpl {
    deps: Arc<ScoutDeps>,
}

impl SituationWeaverWorkflowImpl {
    pub fn with_deps(deps: Arc<ScoutDeps>) -> Self {
        Self { deps }
    }
}

impl SituationWeaverWorkflow for SituationWeaverWorkflowImpl {
    async fn run(
        &self,
        _ctx: WorkflowContext<'_>,
        _req: BudgetedRegionRequest,
    ) -> Result<SituationWeaverResult, HandlerError> {
        // TODO(Phase 4): Wire up situation weaving pipeline.
        Err(TerminalError::new("SituationWeaverWorkflow not yet implemented — use Scout::run() directly").into())
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
