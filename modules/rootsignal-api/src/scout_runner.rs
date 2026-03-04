//! Spawns seesaw engines in tokio tasks. Each GraphQL mutation creates a
//! short-lived task that builds the appropriate engine variant, emits the
//! entry event, and settles.
//!
//! On failure the spawned task emits `TaskPhaseTransitioned { status: "idle" }`
//! so the UI doesn't show a permanently-stuck task.
//!
//! On startup, `resume_incomplete_runs` queries for runs that crashed mid-flight,
//! reclaims their stale queue entries, and calls `settle()` to finish them.
//!
//! Cancellation is DB-backed via `seesaw_cancellations`. Any server can cancel
//! any run — no engine handle or in-process flag needed.

use std::sync::Arc;

use rootsignal_common::ScoutScope;
use tracing::{info, warn};

use rootsignal_scout::core::engine::{self, build_infra_only_engine};
use rootsignal_scout::core::postgres_store::PostgresStore;
use seesaw_core::store::Store;
use rootsignal_scout::domains::lifecycle::events::LifecycleEvent;
use rootsignal_scout::workflows::ScoutDeps;

/// Individual scout workflow phases that can be run independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScoutPhase {
    Bootstrap,
    Scrape,
    Synthesis,
    SituationWeaver,
    Supervisor,
}

impl ScoutPhase {
    fn running_status(self) -> &'static str {
        match self {
            ScoutPhase::Bootstrap => "running_bootstrap",
            ScoutPhase::Scrape => "running_scrape",
            ScoutPhase::Synthesis => "running_synthesis",
            ScoutPhase::SituationWeaver => "running_situation_weaver",
            ScoutPhase::Supervisor => "running_supervisor",
        }
    }
}

/// Holds `Arc<ScoutDeps>`. Spawns seesaw engines in tokio tasks.
/// Cancellation goes through Postgres (seesaw_cancellations table).
#[derive(Clone)]
pub struct ScoutRunner {
    deps: Arc<ScoutDeps>,
}

impl ScoutRunner {
    pub fn new(deps: Arc<ScoutDeps>) -> Self {
        Self { deps }
    }

    /// Spawn a full scout run for the given task.
    pub async fn run_scout(&self, task_id: &str, scope: &ScoutScope) {
        let deps = self.deps.clone();
        let task_id = task_id.to_string();
        let scope = scope.clone();
        let run_id = uuid::Uuid::new_v4().to_string();

        info!(task_id = task_id.as_str(), run_id = run_id.as_str(), "Spawning full scout run");

        tokio::spawn(async move {
            emit_running_status(&deps, &task_id, ScoutPhase::Bootstrap).await;

            let engine = deps.build_full_engine(&scope, &run_id, 0, Some(&task_id), Some("complete"));

            let run_id_uuid = uuid::Uuid::parse_str(&run_id).unwrap();

            // Early INSERT so cancel() can find this run immediately
            early_insert_scout_run(&deps, &run_id, &task_id, &scope).await;

            let result = engine
                .emit(LifecycleEvent::EngineStarted { run_id: run_id.clone() })
                .correlation_id(run_id_uuid)
                .settled()
                .await;

            if let Err(e) = result {
                warn!(task_id = task_id.as_str(), error = %e, "Full scout run failed");
                emit_idle_status(&deps, &task_id).await;
            } else {
                info!(task_id = task_id.as_str(), "Full scout run completed");
            }

            // Post-settle: if cancelled, reset task to idle; always mark run finished
            post_settle_cleanup(&deps, run_id_uuid, Some(&task_id)).await;
        });
    }

    /// Spawn an individual phase for a task.
    pub async fn run_phase(&self, phase: ScoutPhase, task_id: &str, scope: &ScoutScope) {
        let deps = self.deps.clone();
        let task_id = task_id.to_string();
        let scope = scope.clone();
        let run_id = uuid::Uuid::new_v4().to_string();

        info!(task_id = task_id.as_str(), phase = ?phase, "Spawning scout phase");

        tokio::spawn(async move {
            let run_id_uuid = uuid::Uuid::parse_str(&run_id).unwrap();

            // Early INSERT so cancel() can find this run immediately
            early_insert_scout_run(&deps, &run_id, &task_id, &scope).await;

            let result = run_phase_inner(&deps, phase, &task_id, &scope, &run_id).await;

            if let Err(e) = result {
                warn!(task_id = task_id.as_str(), phase = ?phase, error = %e, "Scout phase failed");
                emit_idle_status(&deps, &task_id).await;
            } else {
                info!(task_id = task_id.as_str(), phase = ?phase, "Scout phase completed");
            }

            post_settle_cleanup(&deps, run_id_uuid, Some(&task_id)).await;
        });
    }

    /// Spawn a news scan (global, no region).
    pub async fn run_news_scan(&self) {
        let deps = self.deps.clone();
        let run_id = uuid::Uuid::new_v4().to_string();

        info!("Spawning news scan");

        tokio::spawn(async move {
            let engine = deps.build_news_engine(&run_id);

            let run_id_uuid = uuid::Uuid::parse_str(&run_id).unwrap();
            if let Err(e) = engine
                .emit(LifecycleEvent::NewsScanRequested)
                .correlation_id(run_id_uuid)
                .settled()
                .await
            {
                warn!(error = %e, "News scan failed");
            } else {
                info!("News scan completed");
            }
        });
    }

    /// Cancel a running task via Postgres. Works from any server.
    ///
    /// Finds all active run_ids for the task and inserts them into
    /// `seesaw_cancellations`. The settle loop will reject their events
    /// and DLQ their effects at the next checkpoint.
    pub async fn cancel(&self, task_id: &str) -> bool {
        let rows = match sqlx::query_as::<_, (String,)>(
            "SELECT run_id FROM scout_runs WHERE task_id = $1 AND finished_at IS NULL",
        )
        .bind(task_id)
        .fetch_all(&self.deps.pg_pool)
        .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!(task_id, error = %e, "Failed to query active runs for cancel");
                return false;
            }
        };

        if rows.is_empty() {
            warn!(task_id, "No active runs found to cancel");
            return false;
        }

        let mut cancelled_any = false;
        for (run_id_str,) in &rows {
            let run_uuid = match uuid::Uuid::parse_str(run_id_str) {
                Ok(u) => u,
                Err(_) => continue,
            };
            match sqlx::query(
                "INSERT INTO seesaw_cancellations (correlation_id) VALUES ($1) ON CONFLICT DO NOTHING",
            )
            .bind(run_uuid)
            .execute(&self.deps.pg_pool)
            .await
            {
                Ok(r) => {
                    if r.rows_affected() > 0 {
                        cancelled_any = true;
                    }
                }
                Err(e) => {
                    warn!(run_id = %run_id_str, error = %e, "Failed to insert cancellation");
                }
            }
        }

        if cancelled_any {
            info!(task_id, count = rows.len(), "Cancellation inserted for active runs");
        }
        cancelled_any
    }

    /// Resume runs that were in-flight when the server crashed.
    ///
    /// Queries `scout_runs` for rows without `finished_at`, reclaims stale
    /// queue entries, rebuilds engines, and calls `settle()` to finish them.
    pub async fn resume_incomplete_runs(&self) {
        let rows = match sqlx::query_as::<_, IncompleteRun>(
            "SELECT run_id, task_id, scope FROM scout_runs WHERE finished_at IS NULL",
        )
        .fetch_all(&self.deps.pg_pool)
        .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "Failed to query incomplete runs");
                return;
            }
        };

        if rows.is_empty() {
            info!("No incomplete runs to resume");
            return;
        }

        info!(count = rows.len(), "Found incomplete runs to resume");

        cleanup_old_cancellations(&self.deps.pg_pool).await;

        for run in rows {
            let run_uuid = match uuid::Uuid::parse_str(&run.run_id) {
                Ok(u) => u,
                Err(_) => continue,
            };

            let store = PostgresStore::new(self.deps.pg_pool.clone(), run_uuid);

            if let Err(e) = store.reclaim_stale().await {
                warn!(run_id = %run.run_id, error = %e, "Failed to reclaim stale work");
                continue;
            }

            match store.has_pending_work().await {
                Ok(false) => {
                    info!(run_id = %run.run_id, "No pending work, marking finished");
                    let _ = sqlx::query(
                        "UPDATE scout_runs SET finished_at = now() WHERE run_id = $1 AND finished_at IS NULL",
                    )
                    .bind(&run.run_id)
                    .execute(&self.deps.pg_pool)
                    .await;
                    continue;
                }
                Err(e) => {
                    warn!(run_id = %run.run_id, error = %e, "Failed to check pending work");
                    continue;
                }
                Ok(true) => {}
            }

            let scope: ScoutScope = match run.scope {
                Some(v) => match serde_json::from_value(v) {
                    Ok(s) => s,
                    Err(e) => {
                        warn!(run_id = %run.run_id, error = %e, "Failed to deserialize scope");
                        continue;
                    }
                },
                None => {
                    warn!(run_id = %run.run_id, "No scope stored, cannot resume");
                    continue;
                }
            };

            let store_arc = Arc::new(store) as Arc<dyn seesaw_core::Store>;
            let deps = self.deps.clone();
            let run_id = run.run_id.clone();
            let task_id = run.task_id.clone();

            info!(run_id = %run_id, "Resuming incomplete run");

            tokio::spawn(async move {
                let run_uuid = uuid::Uuid::parse_str(&run_id).unwrap();
                let engine_deps = deps.build_engine_deps_for_resume(
                    &scope,
                    &run_id,
                    task_id.as_deref(),
                );
                let engine = engine::build_full_engine(engine_deps, Some(store_arc));

                if let Err(e) = engine.settle().await {
                    warn!(run_id = %run_id, error = %e, "Resume settle failed");
                } else {
                    info!(run_id = %run_id, "Resumed run completed");
                }

                post_settle_cleanup(&deps, run_uuid, task_id.as_deref()).await;
            });
        }
    }
}

#[derive(sqlx::FromRow)]
struct IncompleteRun {
    run_id: String,
    task_id: Option<String>,
    scope: Option<serde_json::Value>,
}

/// Run a specific phase. Builds the right engine variant, emits the entry
/// event, and settles.
async fn run_phase_inner(
    deps: &ScoutDeps,
    phase: ScoutPhase,
    task_id: &str,
    scope: &ScoutScope,
    run_id: &str,
) -> anyhow::Result<()> {
    use rootsignal_scout::core::events::PipelinePhase;

    emit_running_status(deps, task_id, phase).await;

    let run_id_uuid = uuid::Uuid::parse_str(run_id)
        .map_err(|e| anyhow::anyhow!("invalid run_id: {e}"))?;

    match phase {
        ScoutPhase::Bootstrap => {
            let engine = deps.build_scrape_engine(scope, run_id, Some(task_id), Some("bootstrap_complete"));
            engine.emit(LifecycleEvent::EngineStarted { run_id: run_id.to_string() })
                .correlation_id(run_id_uuid)
                .settled().await.map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        ScoutPhase::Scrape => {
            let engine = deps.build_scrape_engine(scope, run_id, Some(task_id), Some("scrape_complete"));
            engine.emit(LifecycleEvent::EngineStarted { run_id: run_id.to_string() })
                .correlation_id(run_id_uuid)
                .settled().await.map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        ScoutPhase::Synthesis => {
            let engine = deps.build_full_engine(scope, run_id, 0, Some(task_id), Some("synthesis_complete"));
            engine.emit(LifecycleEvent::PhaseCompleted { phase: PipelinePhase::SignalExpansion })
                .correlation_id(run_id_uuid)
                .settled().await.map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        ScoutPhase::SituationWeaver => {
            let engine = deps.build_full_engine(scope, run_id, 0, Some(task_id), Some("situation_weaver_complete"));
            engine.emit(LifecycleEvent::PhaseCompleted { phase: PipelinePhase::Synthesis })
                .correlation_id(run_id_uuid)
                .settled().await.map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        ScoutPhase::Supervisor => {
            let engine = deps.build_full_engine(scope, run_id, 0, Some(task_id), Some("complete"));
            engine.emit(LifecycleEvent::PhaseCompleted { phase: PipelinePhase::SituationWeaving })
                .correlation_id(run_id_uuid)
                .settled().await.map_err(|e| anyhow::anyhow!("{e}"))?;
        }
    }

    Ok(())
}

/// INSERT scout_runs row before settle — ensures cancel() can find the run_id
/// immediately. The projection handler uses ON CONFLICT DO NOTHING, so the
/// later INSERT from EngineStarted is harmless.
async fn early_insert_scout_run(deps: &ScoutDeps, run_id: &str, task_id: &str, scope: &ScoutScope) {
    let scope_json = serde_json::to_value(scope).ok();
    if let Err(e) = sqlx::query(
        "INSERT INTO scout_runs (run_id, region, task_id, scope, started_at) \
         VALUES ($1, $2, $3, $4, now()) \
         ON CONFLICT (run_id) DO NOTHING",
    )
    .bind(run_id)
    .bind(&scope.name)
    .bind(task_id)
    .bind(&scope_json)
    .execute(&deps.pg_pool)
    .await
    {
        warn!(run_id, error = %e, "Failed to early-insert scout_runs row");
    }
}

/// Post-settle cleanup:
/// 1. If cancelled AND task_id is known, emit idle status via infra engine
/// 2. Always mark scout_runs.finished_at
async fn post_settle_cleanup(deps: &ScoutDeps, run_id_uuid: uuid::Uuid, task_id: Option<&str>) {
    // Check if this run was cancelled
    let was_cancelled = sqlx::query_as::<_, (bool,)>(
        "SELECT EXISTS(SELECT 1 FROM seesaw_cancellations WHERE correlation_id = $1)",
    )
    .bind(run_id_uuid)
    .fetch_one(&deps.pg_pool)
    .await
    .map(|(b,)| b)
    .unwrap_or(false);

    if was_cancelled {
        if let Some(tid) = task_id {
            // Only emit idle if the run didn't complete normally.
            // If it did, the finalize handler already emitted the completion status.
            let completed_normally = sqlx::query_as::<_, (bool,)>(
                "SELECT EXISTS(
                    SELECT 1 FROM seesaw_events
                    WHERE correlation_id = $1
                      AND event_type = 'run_completed'
                      AND status = 'completed'
                )",
            )
            .bind(run_id_uuid)
            .fetch_one(&deps.pg_pool)
            .await
            .map(|(b,)| b)
            .unwrap_or(false);

            if !completed_normally {
                info!(task_id = tid, "Run was cancelled, resetting task to idle");
                emit_idle_status(deps, tid).await;
            }
        }
    }

    // Always mark finished
    if let Err(e) = sqlx::query(
        "UPDATE scout_runs SET finished_at = now() WHERE run_id = $1 AND finished_at IS NULL",
    )
    .bind(run_id_uuid.to_string())
    .execute(&deps.pg_pool)
    .await
    {
        warn!(run_id = %run_id_uuid, error = %e, "Failed to mark scout_run finished");
    }
}

/// Remove cancellation rows older than 7 days. Called on startup during resume.
async fn cleanup_old_cancellations(pool: &sqlx::PgPool) {
    match sqlx::query(
        "DELETE FROM seesaw_cancellations WHERE cancelled_at < now() - interval '7 days'",
    )
    .execute(pool)
    .await
    {
        Ok(r) => {
            if r.rows_affected() > 0 {
                info!(deleted = r.rows_affected(), "Cleaned up old cancellation rows");
            }
        }
        Err(e) => {
            warn!(error = %e, "Failed to clean up old cancellations");
        }
    }
}

/// Emit a `running_*` status so the UI shows the task as active immediately.
async fn emit_running_status(deps: &ScoutDeps, task_id: &str, phase: ScoutPhase) {
    use rootsignal_common::events::SystemEvent;

    let engine = build_infra_only_engine(deps.pg_pool.clone(), deps.graph_client.clone());
    if let Err(e) = engine
        .emit(SystemEvent::TaskPhaseTransitioned {
            task_id: task_id.to_string(),
            phase: String::new(),
            status: phase.running_status().to_string(),
        })
        .settled()
        .await
    {
        warn!(task_id, error = %e, "Failed to emit running status");
    }
}

/// Emit an idle status via a minimal infra-only engine so the task doesn't
/// appear stuck after a failure.
async fn emit_idle_status(deps: &ScoutDeps, task_id: &str) {
    use rootsignal_common::events::SystemEvent;

    let engine = build_infra_only_engine(deps.pg_pool.clone(), deps.graph_client.clone());
    if let Err(e) = engine
        .emit(SystemEvent::TaskPhaseTransitioned {
            task_id: task_id.to_string(),
            phase: String::new(),
            status: "idle".to_string(),
        })
        .settled()
        .await
    {
        warn!(task_id, error = %e, "Failed to emit idle status after failure");
    }
}
