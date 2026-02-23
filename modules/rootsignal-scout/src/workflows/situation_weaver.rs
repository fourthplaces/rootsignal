//! Restate durable workflow for situation weaving.
//!
//! Runs situation weaving (assigns signals to living situations),
//! source boost for hot situations, and curiosity-triggered re-investigation.

use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::{info, warn};

use rootsignal_graph::GraphWriter;

use crate::scheduling::budget::{BudgetTracker, OperationCost};

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
        // Status transition guard (journaled so it's skipped on replay)
        let slug = rootsignal_common::slugify(&req.scope.name);
        let graph_client = self.deps.graph_client.clone();
        ctx.run(|| async move {
            let writer = rootsignal_graph::GraphWriter::new(graph_client);
            let transitioned = writer
                .transition_region_status(
                    &slug,
                    &["synthesis_complete", "situation_weaver_complete", "complete"],
                    "running_situation_weaver",
                )
                .await
                .map_err(|e| TerminalError::new(format!("Status check failed: {e}")))?;
            if !transitioned {
                return Err(TerminalError::new("Prerequisites not met or another phase is running").into());
            }
            Ok(())
        })
        .await?;
        let slug = rootsignal_common::slugify(&req.scope.name);

        ctx.set("status", "Starting situation weaving...".to_string());

        let deps = self.deps.clone();
        let scope = req.scope.clone();
        let spent_cents = req.spent_cents;

        let result = match ctx
            .run(|| async {
                run_situation_weaving_from_deps(&deps, &scope, spent_cents)
                    .await
                    .map_err(|e| -> HandlerError { TerminalError::new(e.to_string()).into() })
            })
            .await
        {
            Ok(v) => v,
            Err(e) => {
                super::write_phase_status(&self.deps, &slug, "idle").await;
                return Err(e.into());
            }
        };

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

pub async fn run_situation_weaving_from_deps(
    deps: &ScoutDeps,
    scope: &rootsignal_common::ScoutScope,
    spent_cents: u64,
) -> anyhow::Result<SituationWeaverResult> {
    let writer = GraphWriter::new(deps.graph_client.clone());
    let embedder: Arc<dyn crate::infra::embedder::TextEmbedder> =
        Arc::new(crate::infra::embedder::Embedder::new(&deps.voyage_api_key));
    let budget = BudgetTracker::new_with_spent(deps.daily_budget_cents, spent_cents);
    let run_id = uuid::Uuid::new_v4().to_string();

    // ================================================================
    // Situation Weaving (assigns signals to living situations)
    // ================================================================
    info!("Starting situation weaving...");
    let situation_weaver = rootsignal_graph::SituationWeaver::new(
        deps.graph_client.clone(),
        &deps.anthropic_api_key,
        Arc::clone(&embedder),
        scope.clone(),
    );
    let has_situation_budget = budget
        .has_budget(OperationCost::CLAUDE_HAIKU_STORY_WEAVE);
    let weaver_stats = match situation_weaver.run(&run_id, has_situation_budget).await {
        Ok(sit_stats) => {
            info!("{sit_stats}");
            sit_stats
        }
        Err(e) => {
            warn!(error = %e, "Situation weaving failed (non-fatal)");
            Default::default()
        }
    };

    // ================================================================
    // Situation-driven source boost
    // ================================================================
    match writer.get_situation_landscape(20).await {
        Ok(situations) => {
            let hot: Vec<_> = situations
                .iter()
                .filter(|s| s.temperature >= 0.6 && s.sensitivity != "SENSITIVE" && s.sensitivity != "RESTRICTED")
                .collect();
            if !hot.is_empty() {
                info!(count = hot.len(), "Hot situations boosting source cadence");
                for sit in &hot {
                    if let Err(e) = writer
                        .boost_sources_for_situation_headline(&sit.headline, 1.2)
                        .await
                    {
                        warn!(error = %e, headline = sit.headline.as_str(), "Failed to boost sources for hot situation");
                    }
                }
            }

            let fuzzy: Vec<_> = situations
                .iter()
                .filter(|s| s.clarity == "Fuzzy" && s.temperature >= 0.3)
                .collect();
            if !fuzzy.is_empty() {
                info!(
                    count = fuzzy.len(),
                    "Fuzzy situations identified for investigation: {}",
                    fuzzy.iter().map(|s| s.headline.as_str()).collect::<Vec<_>>().join(", ")
                );
            }
        }
        Err(e) => warn!(error = %e, "Failed to fetch situation landscape for feedback"),
    }

    // ================================================================
    // Situation-triggered curiosity re-investigation
    // ================================================================
    match writer.trigger_situation_curiosity().await {
        Ok(0) => {}
        Ok(n) => info!(count = n, "Situations triggered curiosity re-investigation"),
        Err(e) => warn!(error = %e, "Failed to trigger situation curiosity"),
    }

    Ok(SituationWeaverResult {
        situations_woven: weaver_stats.situations_created + weaver_stats.situations_updated,
        spent_cents: budget.total_spent(),
    })
}
