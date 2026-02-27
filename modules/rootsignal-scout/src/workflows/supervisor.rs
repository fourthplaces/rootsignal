//! Restate durable workflow for the supervisor.
//!
//! Wraps post-run cleanup: `Supervisor::run()` + `compute_cause_heat`
//! + beacon detection.

use std::sync::Arc;

use restate_sdk::prelude::*;
use tracing::{info, warn};

use rootsignal_graph::GraphWriter;

use super::types::{EmptyRequest, SupervisorResult, TaskRequest};
use super::ScoutDeps;

#[restate_sdk::workflow]
#[name = "SupervisorWorkflow"]
pub trait SupervisorWorkflow {
    async fn run(req: TaskRequest) -> Result<SupervisorResult, HandlerError>;
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
        req: TaskRequest,
    ) -> Result<SupervisorResult, HandlerError> {
        let task_id = req.task_id.clone();

        // Status transition guard (journaled so it's skipped on replay)
        let tid = task_id.clone();
        let graph_client = self.deps.graph_client.clone();
        ctx.run(|| async move {
            let writer = rootsignal_graph::GraphWriter::new(graph_client);
            let transitioned = writer
                .transition_task_phase_status(
                    &tid,
                    &["lint_complete", "situation_weaver_complete", "complete"],
                    "running_supervisor",
                )
                .await
                .map_err(|e| TerminalError::new(format!("Status check failed: {e}")))?;
            if !transitioned {
                return Err(TerminalError::new(
                    "Prerequisites not met or another phase is running",
                )
                .into());
            }
            Ok(())
        })
        .await?;

        ctx.set("status", "Starting supervisor...".to_string());

        let deps = self.deps.clone();
        let scope = req.scope.clone();

        let result = match ctx
            .run(|| async {
                run_supervisor_pipeline(&deps, &scope)
                    .await
                    .map_err(|e| -> HandlerError { e.into() })
            })
            .retry_policy(super::phase_retry_policy())
            .await
        {
            Ok(v) => v,
            Err(e) => {
                let _ =
                    super::journaled_write_task_phase_status(&ctx, &self.deps, &task_id, "idle")
                        .await;
                return Err(e.into());
            }
        };

        super::journaled_write_task_phase_status(&ctx, &self.deps, &task_id, "complete").await?;

        ctx.set(
            "status",
            format!("Supervisor complete: {} issues", result.issues_found),
        );
        info!(
            issues_found = result.issues_found,
            "SupervisorWorkflow complete"
        );

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

pub async fn run_supervisor_pipeline(
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
        deps.pg_pool.clone(),
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

    // 2. Merge duplicate tensions (before heat computation)
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

    // 4. Detect beacons (geographic signal clusters → new ScoutTasks)
    match rootsignal_graph::beacon::detect_beacons(&deps.graph_client, &writer).await {
        Ok(tasks) if !tasks.is_empty() => info!(count = tasks.len(), "Beacon tasks created"),
        Ok(_) => {}
        Err(e) => warn!(error = %e, "Beacon detection failed"),
    }

    Ok(SupervisorResult {
        issues_found: issues_found as u32,
    })
}
