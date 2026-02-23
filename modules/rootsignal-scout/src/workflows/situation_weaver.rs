//! Restate durable workflow for situation weaving.
//!
//! Wraps steps 8, 8b, 8c from `Scout::run_inner()`:
//! - Situation weaving (assigns signals to living situations)
//! - Source boost for hot situations
//! - Curiosity-triggered re-investigation

use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::info;

use rootsignal_graph::GraphWriter;

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
        ctx: WorkflowContext<'_>,
        req: BudgetedRegionRequest,
    ) -> Result<SituationWeaverResult, HandlerError> {
        ctx.set("status", "Starting situation weaving...".to_string());

        let deps = self.deps.clone();
        let scope = req.scope.clone();
        let spent_cents = req.spent_cents;

        let result = super::spawn_workflow("Situation weaver", async move {
            run_situation_weaving_from_deps(&deps, &scope, spent_cents).await
        })
        .await?;

        let region_key = rootsignal_common::slugify(&req.scope.name);
        super::write_phase_status(&self.deps, &region_key, "situation_weaver_complete").await;

        ctx.set("status", "Situation weaving complete".to_string());
        info!("SituationWeaverWorkflow complete");

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

async fn run_situation_weaving_from_deps(
    deps: &ScoutDeps,
    scope: &rootsignal_common::ScoutScope,
    spent_cents: u64,
) -> anyhow::Result<SituationWeaverResult> {
    let writer = GraphWriter::new(deps.graph_client.clone());
    let embedder: Arc<dyn crate::infra::embedder::TextEmbedder> =
        Arc::new(crate::infra::embedder::Embedder::new(&deps.voyage_api_key));
    let budget = crate::scheduling::budget::BudgetTracker::new_with_spent(deps.daily_budget_cents, spent_cents);
    let run_id = uuid::Uuid::new_v4().to_string();

    let weaver_stats = crate::scout::run_situation_weaving(
        &deps.graph_client,
        &writer,
        embedder,
        &deps.anthropic_api_key,
        scope,
        &budget,
        &run_id,
    )
    .await?;

    Ok(SituationWeaverResult {
        situations_woven: weaver_stats.situations_created + weaver_stats.situations_updated,
        spent_cents: budget.total_spent(),
    })
}
