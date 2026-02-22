//! Restate durable workflow for synthesis.
//!
//! Wraps step 7 from `Scout::run_inner()`: similarity edges + parallel
//! finders (response mapping, tension linker, response finder,
//! gathering finder, investigation).
//!
//! TODO(Phase 4): The synthesis pipeline calls ResponseFinder which holds
//! a `MutexGuard` across an `.await` point, making the future `!Send`.
//! Fix requires changing ResponseFinder to drop the guard before awaiting.

use std::sync::Arc;

use restate_sdk::prelude::*;

use super::types::{BudgetedRegionRequest, EmptyRequest, SynthesisResult};
use super::ScoutDeps;

#[restate_sdk::workflow]
#[name = "SynthesisWorkflow"]
pub trait SynthesisWorkflow {
    async fn run(req: BudgetedRegionRequest) -> Result<SynthesisResult, HandlerError>;
    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct SynthesisWorkflowImpl {
    deps: Arc<ScoutDeps>,
}

impl SynthesisWorkflowImpl {
    pub fn with_deps(deps: Arc<ScoutDeps>) -> Self {
        Self { deps }
    }
}

impl SynthesisWorkflow for SynthesisWorkflowImpl {
    async fn run(
        &self,
        _ctx: WorkflowContext<'_>,
        _req: BudgetedRegionRequest,
    ) -> Result<SynthesisResult, HandlerError> {
        // TODO(Phase 4): Wire up the full synthesis pipeline once ResponseFinder's
        // Send constraint is resolved. See module doc comment.
        Err(TerminalError::new("SynthesisWorkflow not yet implemented — use Scout::run() directly").into())
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
