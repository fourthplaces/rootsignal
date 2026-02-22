//! Restate durable workflow for the supervisor.
//!
//! Wraps post-run cleanup: `Supervisor::run()` + `merge_duplicate_tensions`
//! + `compute_cause_heat`.

use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::{info, warn};

use rootsignal_graph::GraphWriter;

use super::types::{EmptyRequest, RegionRequest, SupervisorResult};
use super::ScoutDeps;

#[restate_sdk::workflow]
#[name = "SupervisorWorkflow"]
pub trait SupervisorWorkflow {
    async fn run(req: RegionRequest) -> Result<SupervisorResult, HandlerError>;
    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct SupervisorWorkflowImpl {
    deps: Arc<ScoutDeps>,
}

impl SupervisorWorkflowImpl {
    pub fn with_deps(deps: Arc<ScoutDeps>) -> Self {
        Self { deps }
    }
}

impl SupervisorWorkflow for SupervisorWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        req: RegionRequest,
    ) -> Result<SupervisorResult, HandlerError> {
        ctx.set("status", "Starting supervisor...".to_string());

        let deps = self.deps.clone();
        let scope = req.scope.clone();

        let result = tokio::spawn(async move {
            run_supervisor_pipeline(&deps, &scope).await
        })
        .await
        .map_err(|e| -> HandlerError {
            TerminalError::new(format!("Supervisor task panicked: {e}")).into()
        })?
        .map_err(|e| -> HandlerError {
            TerminalError::new(e.to_string()).into()
        })?;

        ctx.set(
            "status",
            format!("Supervisor complete: {} issues", result.issues_found),
        );
        info!(issues_found = result.issues_found, "SupervisorWorkflow complete");

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

async fn run_supervisor_pipeline(
    deps: &ScoutDeps,
    scope: &rootsignal_common::ScoutScope,
) -> anyhow::Result<SupervisorResult> {
    let writer = GraphWriter::new(deps.graph_client.clone());
    let (min_lat, max_lat, min_lng, max_lng) = scope.bounding_box();

    // 1. Run supervisor checks
    let notifier: Box<dyn rootsignal_scout_supervisor::notify::backend::NotifyBackend> =
        Box::new(rootsignal_scout_supervisor::notify::noop::NoopBackend);

    let supervisor = rootsignal_scout_supervisor::supervisor::Supervisor::new(
        deps.graph_client.clone(),
        scope.clone(),
        deps.anthropic_api_key.clone(),
        notifier,
    );

    let issues_found = match supervisor.run().await {
        Ok(stats) => {
            info!(%stats, "Supervisor run complete");
            stats.issues_created as u32
        }
        Err(e) => {
            warn!(error = %e, "Supervisor run failed");
            0
        }
    };

    // 2. Merge duplicate tensions
    match writer
        .merge_duplicate_tensions(0.85, min_lat, max_lat, min_lng, max_lng)
        .await
    {
        Ok(merged) if merged > 0 => info!(merged, "Duplicate tensions merged"),
        Ok(_) => {}
        Err(e) => warn!(error = %e, "Failed to merge duplicate tensions"),
    }

    // 3. Compute cause heat
    match rootsignal_graph::cause_heat::compute_cause_heat(
        &deps.graph_client,
        0.7,
        min_lat,
        max_lat,
        min_lng,
        max_lng,
    )
    .await
    {
        Ok(_) => info!("Cause heat computed"),
        Err(e) => warn!(error = %e, "Failed to compute cause heat"),
    }

    Ok(SupervisorResult {
        issues_found: issues_found as u32,
    })
}
