//! Restate durable workflow for signal lint.
//!
//! Verifies all staged signals from a run against their source content,
//! auto-correcting fixable issues and rejecting the rest.
//! Promotes passing signals to `live`.

use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::info;

use rootsignal_graph::GraphWriter;

use crate::infra::run_log::RunLogger;
use crate::pipeline::signal_lint::SignalLinter;
use crate::pipeline::traits::{ContentFetcher, SignalStore};

use super::types::{EmptyRequest, SignalLintResult, TaskRequest};
use super::ScoutDeps;

#[restate_sdk::workflow]
#[name = "SignalLintWorkflow"]
pub trait SignalLintWorkflow {
    async fn run(req: TaskRequest) -> Result<SignalLintResult, HandlerError>;
    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct SignalLintWorkflowImpl {
    deps: Arc<ScoutDeps>,
}

impl SignalLintWorkflowImpl {
    pub fn with_deps(deps: Arc<ScoutDeps>) -> Self {
        Self { deps }
    }
}

impl SignalLintWorkflow for SignalLintWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        req: TaskRequest,
    ) -> Result<SignalLintResult, HandlerError> {
        let task_id = req.task_id.clone();

        // Status transition guard
        let tid = task_id.clone();
        let graph_client = self.deps.graph_client.clone();
        ctx.run(|| async move {
            let writer = GraphWriter::new(graph_client);
            let transitioned = writer
                .transition_task_phase_status(
                    &tid,
                    &["situation_weaver_complete", "lint_complete", "complete"],
                    "running_lint",
                )
                .await
                .map_err(|e| TerminalError::new(format!("Status check failed: {e}")))?;
            if !transitioned {
                return Err(TerminalError::new("Prerequisites not met or another phase is running").into());
            }
            Ok(())
        })
        .await?;

        ctx.set("status", "Linting signals...".to_string());

        let deps = self.deps.clone();
        let scope = req.scope.clone();
        let tid_for_lint = task_id.clone();

        let result = match ctx
            .run(|| async {
                run_signal_lint_from_deps(&deps, &scope, &tid_for_lint)
                    .await
                    .map_err(|e| -> HandlerError { e.into() })
            })
            .retry_policy(super::phase_retry_policy())
            .await
        {
            Ok(v) => v,
            Err(e) => {
                let _ = super::journaled_write_task_phase_status(&ctx, &self.deps, &task_id, "idle").await;
                return Err(e.into());
            }
        };

        super::journaled_write_task_phase_status(&ctx, &self.deps, &task_id, "lint_complete").await?;

        ctx.set("status", "Signal lint complete".to_string());
        info!("SignalLintWorkflow complete");

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

pub async fn run_signal_lint_from_deps(
    deps: &ScoutDeps,
    scope: &rootsignal_common::ScoutScope,
    task_id: &str,
) -> anyhow::Result<SignalLintResult> {
    let store: Arc<dyn SignalStore> = Arc::new(GraphWriter::new(deps.graph_client.clone()));
    let archive = super::create_archive(deps);
    let fetcher: Arc<dyn ContentFetcher> = archive;
    let logger = RunLogger::new(
        task_id.to_string(), scope.name.clone(), deps.pg_pool.clone(),
    ).await;

    let linter = SignalLinter::new(
        store,
        fetcher,
        deps.anthropic_api_key.clone(),
        scope.clone(),
        logger,
    );

    let result = linter.run().await?;

    Ok(SignalLintResult {
        passed: result.passed,
        corrected: result.corrected,
        rejected: result.rejected,
        situations_promoted: result.situations_promoted,
        stories_promoted: result.stories_promoted,
    })
}
