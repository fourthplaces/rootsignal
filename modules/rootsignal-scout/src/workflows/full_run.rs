//! Restate durable workflow for a full scout run.
//!
//! Orchestrator that calls all phase workflows in sequence:
//! Bootstrap → Scrape → Synthesis → SituationWeaver → Supervisor
//!
//! Budget flows as `spent_cents` between workflows.

use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::info;

use super::types::*;
use super::ScoutDeps;

#[restate_sdk::workflow]
#[name = "FullScoutRunWorkflow"]
pub trait FullScoutRunWorkflow {
    async fn run(req: TaskRequest) -> Result<FullRunResult, HandlerError>;
    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct FullScoutRunWorkflowImpl {
    _deps: Arc<ScoutDeps>,
}

impl FullScoutRunWorkflowImpl {
    pub fn with_deps(deps: Arc<ScoutDeps>) -> Self {
        Self { _deps: deps }
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
        // Sub-workflow keys must be unique per full run — Restate workflows are
        // one-shot, so reusing the bare slug would 409 on subsequent runs.
        // ctx.key() = task_id (UUID), so sub_key = "{slug}-{task_id}" — unique per task.
        let slug = rootsignal_common::slugify(&scope.name);
        let sub_key = format!("{slug}-{}", ctx.key());

        // 1. Bootstrap
        ctx.set("status", WorkflowPhase::Bootstrap.to_string());
        let bootstrap_result: BootstrapResult = ctx
            .workflow_client::<super::bootstrap::BootstrapWorkflowClient>(&sub_key)
            .run(TaskRequest {
                task_id: task_id.clone(),
                scope: scope.clone(),
            })
            .call()
            .await?;
        info!(
            sources_created = bootstrap_result.sources_created,
            "Bootstrap phase complete"
        );

        // 2. Scrape
        ctx.set("status", WorkflowPhase::Scraping.to_string());
        let scrape_result: ScrapeResult = ctx
            .workflow_client::<super::scrape::ScrapeWorkflowClient>(&sub_key)
            .run(TaskRequest {
                task_id: task_id.clone(),
                scope: scope.clone(),
            })
            .call()
            .await?;
        info!(
            urls_scraped = scrape_result.urls_scraped,
            signals_stored = scrape_result.signals_stored,
            "Scrape phase complete"
        );

        let mut spent_cents = scrape_result.spent_cents;

        // 3. Synthesis
        ctx.set("status", WorkflowPhase::Synthesis.to_string());
        let synthesis_result: SynthesisResult = ctx
            .workflow_client::<super::synthesis::SynthesisWorkflowClient>(&sub_key)
            .run(BudgetedTaskRequest {
                task_id: task_id.clone(),
                scope: scope.clone(),
                spent_cents,
            })
            .call()
            .await?;
        spent_cents = synthesis_result.spent_cents;
        info!("Synthesis phase complete");

        // 4. Situation Weaving
        ctx.set("status", WorkflowPhase::SituationWeaving.to_string());
        let _weaver_result: SituationWeaverResult = ctx
            .workflow_client::<super::situation_weaver::SituationWeaverWorkflowClient>(&sub_key)
            .run(BudgetedTaskRequest {
                task_id: task_id.clone(),
                scope: scope.clone(),
                spent_cents,
            })
            .call()
            .await?;
        info!("Situation weaving phase complete");

        // 5. Signal Lint
        ctx.set("status", WorkflowPhase::SignalLint.to_string());
        let lint_result: SignalLintResult = ctx
            .workflow_client::<super::lint::SignalLintWorkflowClient>(&sub_key)
            .run(TaskRequest {
                task_id: task_id.clone(),
                scope: scope.clone(),
            })
            .call()
            .await?;
        info!(
            passed = lint_result.passed,
            corrected = lint_result.corrected,
            rejected = lint_result.rejected,
            "Signal lint phase complete"
        );

        // 6. Supervisor
        ctx.set("status", WorkflowPhase::Supervisor.to_string());
        let supervisor_result: SupervisorResult = ctx
            .workflow_client::<super::supervisor::SupervisorWorkflowClient>(&sub_key)
            .run(TaskRequest {
                task_id: task_id.clone(),
                scope: scope.clone(),
            })
            .call()
            .await?;
        info!(
            issues_found = supervisor_result.issues_found,
            "Supervisor phase complete"
        );

        ctx.set("status", WorkflowPhase::Complete.to_string());

        Ok(FullRunResult {
            sources_created: bootstrap_result.sources_created,
            urls_scraped: scrape_result.urls_scraped,
            signals_stored: scrape_result.signals_stored,
            issues_found: supervisor_result.issues_found,
        })
    }

    async fn get_status(
        &self,
        ctx: SharedWorkflowContext<'_>,
        _req: EmptyRequest,
    ) -> Result<String, HandlerError> {
        super::read_workflow_status(&ctx).await
    }
}
