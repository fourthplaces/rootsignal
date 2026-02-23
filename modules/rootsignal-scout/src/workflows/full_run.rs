//! Restate durable workflow for a full scout run.
//!
//! Orchestrator that calls all phase workflows in sequence:
//! Bootstrap → ActorDiscovery → Scrape → Synthesis → SituationWeaver → Supervisor
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
    async fn run(req: RegionRequest) -> Result<FullRunResult, HandlerError>;
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
        req: RegionRequest,
    ) -> Result<FullRunResult, HandlerError> {
        let scope = req.scope.clone();
        // Sub-workflow keys must be unique per full run — Restate workflows are
        // one-shot, so reusing the bare slug would 409 on subsequent runs.
        // Derive from the parent key (which already has a timestamp).
        let slug = rootsignal_common::slugify(&scope.name);
        let sub_key = format!("{slug}-{}", ctx.key());

        // 1. Bootstrap
        ctx.set("status", WorkflowPhase::Bootstrap.to_string());
        let bootstrap_result: BootstrapResult = ctx
            .workflow_client::<super::bootstrap::BootstrapWorkflowClient>(&sub_key)
            .run(RegionRequest {
                scope: scope.clone(),
            })
            .call()
            .await?;
        info!(
            sources_created = bootstrap_result.sources_created,
            "Bootstrap phase complete"
        );

        // 2. Actor Discovery
        ctx.set("status", WorkflowPhase::ActorDiscovery.to_string());
        let discovery_result: ActorDiscoveryResult = ctx
            .workflow_client::<super::actor_discovery::ActorDiscoveryWorkflowClient>(&sub_key)
            .run(RegionRequest {
                scope: scope.clone(),
            })
            .call()
            .await?;
        info!(
            actors_discovered = discovery_result.actors_discovered,
            "Actor discovery phase complete"
        );

        // 3. Scrape
        ctx.set("status", WorkflowPhase::Scraping.to_string());
        let scrape_result: ScrapeResult = ctx
            .workflow_client::<super::scrape::ScrapeWorkflowClient>(&sub_key)
            .run(RegionRequest {
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

        // 4. Synthesis
        ctx.set("status", WorkflowPhase::Synthesis.to_string());
        let synthesis_result: SynthesisResult = ctx
            .workflow_client::<super::synthesis::SynthesisWorkflowClient>(&sub_key)
            .run(BudgetedRegionRequest {
                scope: scope.clone(),
                spent_cents,
            })
            .call()
            .await?;
        spent_cents = synthesis_result.spent_cents;
        info!("Synthesis phase complete");

        // 5. Situation Weaving
        ctx.set("status", WorkflowPhase::SituationWeaving.to_string());
        let _weaver_result: SituationWeaverResult = ctx
            .workflow_client::<super::situation_weaver::SituationWeaverWorkflowClient>(&sub_key)
            .run(BudgetedRegionRequest {
                scope: scope.clone(),
                spent_cents,
            })
            .call()
            .await?;
        info!("Situation weaving phase complete");

        // 6. Supervisor
        ctx.set("status", WorkflowPhase::Supervisor.to_string());
        let supervisor_result: SupervisorResult = ctx
            .workflow_client::<super::supervisor::SupervisorWorkflowClient>(&sub_key)
            .run(RegionRequest {
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
            actors_discovered: discovery_result.actors_discovered,
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
