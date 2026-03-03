//! Restate durable workflow for a full scout run.
//!
//! One `engine.emit(EngineStarted).settled_with(&runtime)` drives the entire run:
//! reap → schedule → scrape → enrichment → synthesis → situation weaving →
//! supervisor → finalize → RunCompleted.
//!
//! Each handler invocation is individually journaled through Restate via
//! `RestateRuntime`, so mid-settle crashes resume from the last completed handler.

use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::info;

use crate::core::aggregate::PipelineState;
use crate::domains::lifecycle::events::LifecycleEvent;

use super::restate_runtime::RestateRuntime;
use super::types::*;
use super::{journaled_emit_task_phase_status, ScoutDeps};

#[restate_sdk::workflow]
#[name = "FullScoutRunWorkflow"]
pub trait FullScoutRunWorkflow {
    async fn run(req: TaskRequest) -> Result<FullRunResult, HandlerError>;
    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct FullScoutRunWorkflowImpl {
    deps: Arc<ScoutDeps>,
}

impl FullScoutRunWorkflowImpl {
    pub fn with_deps(deps: Arc<ScoutDeps>) -> Self {
        Self { deps }
    }
}

impl FullScoutRunWorkflow for FullScoutRunWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        req: TaskRequest,
    ) -> Result<FullRunResult, HandlerError> {
        let task_id = req.task_id.clone();
        let scope = req.scope.clone();
        let run_id = req.run_id.clone();

        ctx.set("status", "Running full scout...".to_string());

        let engine = self.deps.build_full_engine(
            &scope,
            &run_id,
            0,
            Some(&task_id),
            Some("complete"),
        );
        let runtime = RestateRuntime::new(&ctx);
        if let Err(e) = engine
            .emit(LifecycleEvent::EngineStarted {
                run_id: run_id.clone(),
            })
            .settled_with(&runtime)
            .await
        {
            let _ = journaled_emit_task_phase_status(
                &ctx, self.deps.pg_pool.clone(), self.deps.graph_client.clone(),
                &task_id, "idle",
            ).await;
            return Err(TerminalError::new(e.to_string()).into());
        }

        let state = engine.singleton::<PipelineState>();
        let result = FullRunResult {
            sources_created: state.stats.sources_discovered,
            urls_scraped: state.stats.urls_scraped,
            signals_stored: state.stats.signals_stored,
            issues_found: 0,
        };

        ctx.set("status", WorkflowPhase::Complete.to_string());
        info!("FullScoutRunWorkflow complete");

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
