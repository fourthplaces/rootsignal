//! Spawns seesaw engines in tokio tasks. Each GraphQL mutation creates a
//! short-lived task that builds the appropriate engine variant, emits the
//! entry event, and settles.
//!
//! On failure the spawned task emits `TaskPhaseTransitioned { status: "idle" }`
//! so the UI doesn't show a permanently-stuck task.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rootsignal_common::ScoutScope;
use tokio::sync::Mutex;
use tracing::{info, warn};

use rootsignal_scout::core::engine::build_infra_only_engine;
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

/// Holds `Arc<ScoutDeps>` and a map of active cancellation flags.
/// Spawns seesaw engines in tokio tasks.
#[derive(Clone)]
pub struct ScoutRunner {
    deps: Arc<ScoutDeps>,
    cancellations: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl ScoutRunner {
    pub fn new(deps: Arc<ScoutDeps>) -> Self {
        Self {
            deps,
            cancellations: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Spawn a full scout run for the given task.
    pub async fn run_scout(&self, task_id: &str, scope: &ScoutScope) {
        let deps = self.deps.clone();
        let task_id = task_id.to_string();
        let scope = scope.clone();
        let run_id = uuid::Uuid::new_v4().to_string();
        let cancellations = self.cancellations.clone();

        info!(task_id = task_id.as_str(), run_id = run_id.as_str(), "Spawning full scout run");

        tokio::spawn(async move {
            emit_running_status(&deps, &task_id, ScoutPhase::Bootstrap).await;

            let engine = deps.build_full_engine(&scope, &run_id, 0, Some(&task_id), Some("complete"));

            // Share the engine's own cancellation flag so `cancel()` can flip it.
            let cancel_flag = engine.deps().cancelled.clone();
            if let Some(ref flag) = cancel_flag {
                cancellations.lock().await.insert(task_id.clone(), flag.clone());
            }

            let result = engine
                .emit(LifecycleEvent::EngineStarted { run_id: run_id.clone() })
                .settled()
                .await;

            cancellations.lock().await.remove(&task_id);

            if let Err(e) = result {
                warn!(task_id = task_id.as_str(), error = %e, "Full scout run failed");
                emit_idle_status(&deps, &task_id).await;
            } else {
                info!(task_id = task_id.as_str(), "Full scout run completed");
            }
        });
    }

    /// Spawn an individual phase for a task.
    pub async fn run_phase(&self, phase: ScoutPhase, task_id: &str, scope: &ScoutScope) {
        let deps = self.deps.clone();
        let task_id = task_id.to_string();
        let scope = scope.clone();
        let run_id = uuid::Uuid::new_v4().to_string();
        let cancellations = self.cancellations.clone();

        info!(task_id = task_id.as_str(), phase = ?phase, "Spawning scout phase");

        tokio::spawn(async move {
            let result = run_phase_inner(&deps, phase, &task_id, &scope, &run_id, &cancellations).await;

            cancellations.lock().await.remove(&task_id);

            if let Err(e) = result {
                warn!(task_id = task_id.as_str(), phase = ?phase, error = %e, "Scout phase failed");
                emit_idle_status(&deps, &task_id).await;
            } else {
                info!(task_id = task_id.as_str(), phase = ?phase, "Scout phase completed");
            }
        });
    }

    /// Spawn a news scan (global, no region).
    pub async fn run_news_scan(&self) {
        let deps = self.deps.clone();
        let run_id = uuid::Uuid::new_v4().to_string();

        info!("Spawning news scan");

        tokio::spawn(async move {
            let engine = deps.build_news_engine(&run_id);

            if let Err(e) = engine
                .emit(LifecycleEvent::NewsScanRequested)
                .settled()
                .await
            {
                warn!(error = %e, "News scan failed");
            } else {
                info!("News scan completed");
            }
        });
    }

    /// Set the cancellation flag for a running task.
    pub async fn cancel(&self, task_id: &str) -> bool {
        if let Some(flag) = self.cancellations.lock().await.get(task_id) {
            flag.store(true, Ordering::Relaxed);
            info!(task_id, "Cancellation flag set");
            true
        } else {
            warn!(task_id, "No running task found to cancel");
            false
        }
    }
}

/// Run a specific phase. Builds the right engine variant, registers its
/// cancellation flag, emits the entry event, and settles.
async fn run_phase_inner(
    deps: &ScoutDeps,
    phase: ScoutPhase,
    task_id: &str,
    scope: &ScoutScope,
    run_id: &str,
    cancellations: &Mutex<HashMap<String, Arc<AtomicBool>>>,
) -> anyhow::Result<()> {
    use rootsignal_scout::core::events::PipelinePhase;

    emit_running_status(deps, task_id, phase).await;

    match phase {
        ScoutPhase::Bootstrap => {
            let engine = deps.build_scrape_engine(scope, run_id, Some(task_id), Some("bootstrap_complete"));
            register_cancel_flag(cancellations, task_id, &engine).await;
            engine.emit(LifecycleEvent::EngineStarted { run_id: run_id.to_string() })
                .settled().await.map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        ScoutPhase::Scrape => {
            let engine = deps.build_scrape_engine(scope, run_id, Some(task_id), Some("scrape_complete"));
            register_cancel_flag(cancellations, task_id, &engine).await;
            engine.emit(LifecycleEvent::EngineStarted { run_id: run_id.to_string() })
                .settled().await.map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        ScoutPhase::Synthesis => {
            let engine = deps.build_full_engine(scope, run_id, 0, Some(task_id), Some("synthesis_complete"));
            register_cancel_flag(cancellations, task_id, &engine).await;
            engine.emit(LifecycleEvent::PhaseCompleted { phase: PipelinePhase::SignalExpansion })
                .settled().await.map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        ScoutPhase::SituationWeaver => {
            let engine = deps.build_full_engine(scope, run_id, 0, Some(task_id), Some("situation_weaver_complete"));
            register_cancel_flag(cancellations, task_id, &engine).await;
            engine.emit(LifecycleEvent::PhaseCompleted { phase: PipelinePhase::Synthesis })
                .settled().await.map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        ScoutPhase::Supervisor => {
            let engine = deps.build_full_engine(scope, run_id, 0, Some(task_id), Some("complete"));
            register_cancel_flag(cancellations, task_id, &engine).await;
            engine.emit(LifecycleEvent::PhaseCompleted { phase: PipelinePhase::SituationWeaving })
                .settled().await.map_err(|e| anyhow::anyhow!("{e}"))?;
        }
    }

    Ok(())
}

/// Grab the engine's own `Arc<AtomicBool>` and register it so `cancel()` can flip it.
async fn register_cancel_flag(
    cancellations: &Mutex<HashMap<String, Arc<AtomicBool>>>,
    task_id: &str,
    engine: &rootsignal_scout::core::engine::ScoutEngine,
) {
    if let Some(flag) = &engine.deps().cancelled {
        cancellations.lock().await.insert(task_id.to_string(), flag.clone());
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
