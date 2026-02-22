//! Restate durable workflow for synthesis.
//!
//! Wraps step 7 from `Scout::run_inner()`: similarity edges + parallel
//! finders (response mapping, tension linker, response finder,
//! gathering finder, investigation).

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::info;

use rootsignal_graph::GraphWriter;

use super::types::{BudgetedRegionRequest, EmptyRequest, SynthesisResult};
use super::{create_archive, ScoutDeps};

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
        ctx: WorkflowContext<'_>,
        req: BudgetedRegionRequest,
    ) -> Result<SynthesisResult, HandlerError> {
        ctx.set("status", "Starting synthesis...".to_string());

        let deps = self.deps.clone();
        let scope = req.scope.clone();
        let spent_cents = req.spent_cents;

        let result = tokio::spawn(async move {
            run_synthesis_from_deps(&deps, &scope, spent_cents).await
        })
        .await
        .map_err(|e| -> HandlerError {
            TerminalError::new(format!("Synthesis task panicked: {e}")).into()
        })?
        .map_err(|e| -> HandlerError {
            TerminalError::new(e.to_string()).into()
        })?;

        ctx.set("status", "Synthesis complete".to_string());
        info!("SynthesisWorkflow complete");

        Ok(result)
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

async fn run_synthesis_from_deps(
    deps: &ScoutDeps,
    scope: &rootsignal_common::ScoutScope,
    spent_cents: u64,
) -> anyhow::Result<SynthesisResult> {
    let writer = GraphWriter::new(deps.graph_client.clone());
    let embedder: Arc<dyn crate::embedder::TextEmbedder> =
        Arc::new(crate::embedder::Embedder::new(&deps.voyage_api_key));
    let region_slug = rootsignal_common::slugify(&scope.name);
    let archive = create_archive(deps, &region_slug);
    let budget = crate::budget::BudgetTracker::new_with_spent(deps.daily_budget_cents, spent_cents);
    let run_id = uuid::Uuid::new_v4().to_string();

    crate::scout::run_synthesis(
        &deps.graph_client,
        &writer,
        &*embedder,
        archive,
        &deps.anthropic_api_key,
        scope,
        &budget,
        Arc::new(AtomicBool::new(false)),
        &run_id,
    )
    .await?;

    Ok(SynthesisResult {
        spent_cents: budget.total_spent(),
    })
}
