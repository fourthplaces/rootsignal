//! Restate durable workflow for synthesis.
//!
//! Runs similarity edges + parallel finders (response mapping, tension linker,
//! response finder, gathering finder, investigation).

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::{info, warn};

use rootsignal_graph::{GraphWriter, SimilarityBuilder};

use crate::scheduling::budget::{BudgetTracker, OperationCost};

use super::types::{BudgetedTaskRequest, EmptyRequest, SynthesisResult};
use super::{create_archive, ScoutDeps};

#[restate_sdk::workflow]
#[name = "SynthesisWorkflow"]
pub trait SynthesisWorkflow {
    async fn run(req: BudgetedTaskRequest) -> Result<SynthesisResult, HandlerError>;
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
        ctx: WorkflowContext<'_>,
        req: BudgetedTaskRequest,
    ) -> Result<SynthesisResult, HandlerError> {
        let task_id = req.task_id.clone();

        // Status transition guard (journaled so it's skipped on replay)
        let tid = task_id.clone();
        let graph_client = self.deps.graph_client.clone();
        ctx.run(|| async move {
            let writer = rootsignal_graph::GraphWriter::new(graph_client);
            let transitioned = writer
                .transition_task_phase_status(
                    &tid,
                    &[
                        "scrape_complete", "synthesis_complete",
                        "situation_weaver_complete", "complete",
                    ],
                    "running_synthesis",
                )
                .await
                .map_err(|e| TerminalError::new(format!("Status check failed: {e}")))?;
            if !transitioned {
                return Err(TerminalError::new("Prerequisites not met or another phase is running").into());
            }
            Ok(())
        })
        .await?;

        ctx.set("status", "Starting synthesis...".to_string());

        let deps = self.deps.clone();
        let scope = req.scope.clone();
        let spent_cents = req.spent_cents;

        let result = match ctx
            .run(|| async {
                run_synthesis_from_deps(&deps, &scope, spent_cents)
                    .await
                    .map_err(|e| -> HandlerError { TerminalError::new(e.to_string()).into() })
            })
            .await
        {
            Ok(v) => v,
            Err(e) => {
                super::write_task_phase_status(&self.deps, &task_id, "idle").await;
                return Err(e.into());
            }
        };

        super::write_task_phase_status(&self.deps, &task_id, "synthesis_complete").await;

        ctx.set("status", "Synthesis complete".to_string());
        info!("SynthesisWorkflow complete");

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

pub async fn run_synthesis_from_deps(
    deps: &ScoutDeps,
    scope: &rootsignal_common::ScoutScope,
    spent_cents: u64,
) -> anyhow::Result<SynthesisResult> {
    let writer = GraphWriter::new(deps.graph_client.clone());
    let embedder: Arc<dyn crate::infra::embedder::TextEmbedder> =
        Arc::new(crate::infra::embedder::Embedder::new(&deps.voyage_api_key));
    let archive = create_archive(deps);
    let budget = BudgetTracker::new_with_spent(deps.daily_budget_cents, spent_cents);
    let cancelled = Arc::new(AtomicBool::new(false));
    let run_id = uuid::Uuid::new_v4().to_string();
    let store = deps.build_store(run_id.clone());

    // Parallel synthesis — similarity edges + finders run concurrently.
    // Finders don't read SIMILAR_TO edges; only StoryWeaver does (runs after).
    info!("Starting parallel synthesis (similarity edges, response mapping, tension linker, response finder, gathering finder, investigation)...");

    let run_response_mapping = budget
        .has_budget(OperationCost::CLAUDE_HAIKU_SYNTHESIS * 10);
    let run_tension_linker = budget.has_budget(
        OperationCost::CLAUDE_HAIKU_TENSION_LINKER + OperationCost::SEARCH_TENSION_LINKER,
    );
    let run_response_finder = budget.has_budget(
        OperationCost::CLAUDE_HAIKU_RESPONSE_FINDER + OperationCost::SEARCH_RESPONSE_FINDER,
    );
    let run_gathering_finder = budget.has_budget(
        OperationCost::CLAUDE_HAIKU_GATHERING_FINDER + OperationCost::SEARCH_GATHERING_FINDER,
    );
    let run_investigation = budget.has_budget(
        OperationCost::CLAUDE_HAIKU_INVESTIGATION + OperationCost::SEARCH_INVESTIGATION,
    );

    let run_id_owned = run_id.to_string();

    let (sim_result, rm_result, tl_result, rf_result, gf_result, inv_result) = tokio::join!(
        async {
            info!("Building similarity edges...");
            let similarity = SimilarityBuilder::new(deps.graph_client.clone());
            similarity.clear_edges().await.unwrap_or_else(|e| {
                warn!(error = %e, "Failed to clear similarity edges");
                0
            });
            match similarity.build_edges().await {
                Ok(edges) => info!(edges, "Similarity edges built"),
                Err(e) => warn!(error = %e, "Similarity edge building failed (non-fatal)"),
            }
        },
        async {
            if run_response_mapping {
                info!("Starting response mapping...");
                let response_mapper = crate::discovery::response_mapper::ResponseMapper::new(
                    &writer,
                    &store as &dyn crate::pipeline::traits::SignalStore,
                    &deps.anthropic_api_key,
                    scope.center_lat,
                    scope.center_lng,
                    scope.radius_km,
                );
                match response_mapper.map_responses().await {
                    Ok(rm_stats) => info!("{rm_stats}"),
                    Err(e) => warn!(error = %e, "Response mapping failed (non-fatal)"),
                }
            } else if budget.is_active() {
                info!("Skipping response mapping (budget exhausted)");
            }
        },
        async {
            if run_tension_linker {
                info!("Starting tension linker...");
                let tension_linker = crate::discovery::tension_linker::TensionLinker::new(
                    &writer,
                    &store as &dyn crate::pipeline::traits::SignalStore,
                    archive.clone(),
                    &*embedder,
                    &deps.anthropic_api_key,
                    scope.clone(),
                    cancelled.clone(),
                    run_id_owned.clone(),
                );
                let tl_stats = tension_linker.run().await;
                info!("{tl_stats}");
            } else if budget.is_active() {
                info!("Skipping tension linker (budget exhausted)");
            }
        },
        async {
            if run_response_finder {
                info!("Starting response finder...");
                let response_finder = crate::discovery::response_finder::ResponseFinder::new(
                    &writer,
                    &store as &dyn crate::pipeline::traits::SignalStore,
                    archive.clone(),
                    &*embedder,
                    &deps.anthropic_api_key,
                    scope.clone(),
                    cancelled.clone(),
                    run_id_owned.clone(),
                );
                let rf_stats = response_finder.run().await;
                info!("{rf_stats}");
            } else if budget.is_active() {
                info!("Skipping response finder (budget exhausted)");
            }
        },
        async {
            if run_gathering_finder {
                info!("Starting gathering finder...");
                let gathering_finder = crate::discovery::gathering_finder::GatheringFinder::new(
                    &writer,
                    &store as &dyn crate::pipeline::traits::SignalStore,
                    archive.clone(),
                    &*embedder,
                    &deps.anthropic_api_key,
                    scope.clone(),
                    cancelled.clone(),
                    run_id_owned.clone(),
                );
                let gf_stats = gathering_finder.run().await;
                info!("{gf_stats}");
            } else if budget.is_active() {
                info!("Skipping gathering finder (budget exhausted)");
            }
        },
        async {
            if run_investigation {
                info!("Starting investigation phase...");
                let investigator = crate::discovery::investigator::Investigator::new(
                    &writer,
                    &store as &dyn crate::pipeline::traits::SignalStore,
                    archive.clone(),
                    &deps.anthropic_api_key,
                    scope,
                    cancelled.clone(),
                );
                let investigation_stats = investigator.run().await;
                info!("{investigation_stats}");
            } else if budget.is_active() {
                info!("Skipping investigation (budget exhausted)");
            }
        },
    );

    let _ = (sim_result, rm_result, tl_result, rf_result, gf_result, inv_result);

    info!("Parallel synthesis complete");

    Ok(SynthesisResult {
        spent_cents: budget.total_spent(),
    })
}
