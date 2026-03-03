//! Restate durable workflow for a full scout run.
//!
//! One `engine.emit(EngineStarted).settled()` drives the entire run:
//! reap → schedule → scrape → enrichment → synthesis → situation weaving →
//! supervisor → finalize → RunCompleted.

use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::info;

use crate::core::aggregate::PipelineState;
use crate::domains::lifecycle::events::LifecycleEvent;

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
        let run_id = uuid::Uuid::new_v4().to_string();

        ctx.set("status", "Running full scout...".to_string());

        let deps = self.deps.clone();
        let tid = task_id.clone();
        let result = match ctx
            .run(|| async {
                let engine = deps.build_full_engine(
                    &scope,
                    &run_id,
                    0,
                    Some(&tid),
                    Some("complete"),
                );
                engine
                    .emit(LifecycleEvent::EngineStarted {
                        run_id: run_id.clone(),
                    })
                    .settled()
                    .await
                    .map_err(|e| -> HandlerError { TerminalError::new(e.to_string()).into() })?;

                let state = engine.singleton::<PipelineState>();
                Ok(FullRunResult {
                    sources_created: state.stats.sources_discovered,
                    urls_scraped: state.stats.urls_scraped,
                    signals_stored: state.stats.signals_stored,
                    issues_found: 0,
                })
            })
            .await
        {
            Ok(v) => v,
            Err(e) => {
                let _ = journaled_emit_task_phase_status(
                    &ctx, self.deps.pg_pool.clone(), self.deps.graph_client.clone(),
                    &task_id, "idle",
                ).await;
                return Err(e.into());
            }
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
