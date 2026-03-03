// Lifecycle domain: reap, schedule, finalize.

pub mod activities;
pub mod events;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};
use tracing::info;

use rootsignal_common::events::SystemEvent;
use rootsignal_graph::GraphReader;

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::core::events::PipelinePhase;
use crate::core::pipeline_events::PipelineEvent;
use events::LifecycleEvent;

fn is_engine_started(e: &LifecycleEvent) -> bool {
    matches!(e, LifecycleEvent::EngineStarted { .. })
}

fn is_reap_completed(e: &LifecycleEvent) -> bool {
    matches!(e, LifecycleEvent::PhaseCompleted { phase } if matches!(phase, PipelinePhase::ReapExpired))
}

fn is_synthesis_completed(e: &LifecycleEvent) -> bool {
    matches!(e, LifecycleEvent::PhaseCompleted { phase } if matches!(phase, PipelinePhase::Synthesis))
}

fn is_supervisor_completed(e: &LifecycleEvent) -> bool {
    matches!(e, LifecycleEvent::PhaseCompleted { phase } if matches!(phase, PipelinePhase::Supervisor))
}

#[handlers]
pub mod handlers {
    use super::*;

    /// EngineStarted → reap expired signals, emit PhaseCompleted(ReapExpired).
    #[handle(on = LifecycleEvent, id = "lifecycle:reap", filter = is_engine_started)]
    async fn reap(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let mut events = activities::reap_expired(&*deps.store).await;
        events.push(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::ReapExpired,
        });
        Ok(events)
    }

    /// PhaseCompleted(ReapExpired) → load + schedule sources, stash in state, emit SourcesScheduled.
    #[handle(on = LifecycleEvent, id = "lifecycle:schedule", filter = is_reap_completed)]
    async fn schedule(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();

        // Requires graph_client + region — skip in tests
        let (region, graph_client) = match (deps.region.as_ref(), deps.graph_client.as_ref()) {
            (Some(r), Some(g)) => (r, g),
            _ => return Ok(events![]),
        };
        let graph = GraphReader::new(graph_client.clone());

        let output = activities::schedule_sources(&graph, region).await;

        let tension_count = output.tension_count;
        let response_count = output.response_count;

        Ok(events![
            PipelineEvent::ScheduleResolved {
                scheduled_data: output.scheduled_data,
                actor_contexts: output.actor_contexts,
                url_mappings: output.url_mappings,
            },
            LifecycleEvent::SourcesScheduled {
                tension_count,
                response_count,
            },
        ])
    }
}

// ---------------------------------------------------------------------------
// Standalone finalize handlers — one per engine variant
// ---------------------------------------------------------------------------

async fn finalize_impl(ctx: Context<ScoutEngineDeps>) -> Result<Events> {
    let deps = ctx.deps();

    let (_, state) = ctx.singleton::<PipelineState>();
    let stats = state.stats.clone();

    if let Some(ref budget) = deps.budget {
        budget.log_status();
    }

    info!("{}", stats);
    let mut result = events![LifecycleEvent::RunCompleted { stats }];
    if let (Some(tid), Some(status)) = (&deps.task_id, &deps.completion_phase_status) {
        result.push(SystemEvent::TaskPhaseTransitioned {
            task_id: tid.clone(),
            phase: String::new(),
            status: status.clone(),
        });
    }
    Ok(result)
}

/// Finalize handler for the scrape chain: triggers on PhaseCompleted(Synthesis).
#[handle(on = LifecycleEvent, id = "lifecycle:finalize", filter = is_synthesis_completed)]
pub async fn scrape_finalize(
    _event: LifecycleEvent,
    ctx: Context<ScoutEngineDeps>,
) -> Result<Events> {
    finalize_impl(ctx).await
}

/// Finalize handler for the full chain: triggers on PhaseCompleted(Supervisor).
#[handle(on = LifecycleEvent, id = "lifecycle:finalize", filter = is_supervisor_completed)]
pub async fn full_finalize(
    _event: LifecycleEvent,
    ctx: Context<ScoutEngineDeps>,
) -> Result<Events> {
    finalize_impl(ctx).await
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use seesaw_core::AnyEvent;

    use crate::core::engine::{build_engine, ScoutEngineDeps};
    use crate::core::events::PipelinePhase;
    use crate::domains::lifecycle::events::LifecycleEvent;
    use rootsignal_common::events::SystemEvent;

    fn build_test_engine(
        task_id: Option<&str>,
        completion_phase_status: Option<&str>,
    ) -> (seesaw_core::Engine<ScoutEngineDeps>, Arc<Mutex<Vec<AnyEvent>>>) {
        let sink = Arc::new(Mutex::new(Vec::new()));
        let mut deps = ScoutEngineDeps::new(
            Arc::new(crate::testing::MockSignalReader::new()),
            Arc::new(crate::infra::embedder::NoOpEmbedder),
            "test",
        );
        deps.task_id = task_id.map(String::from);
        deps.completion_phase_status = completion_phase_status.map(String::from);
        deps.captured_events = Some(sink.clone());
        let engine = build_engine(deps);
        (engine, sink)
    }

    #[tokio::test]
    async fn finalize_emits_task_phase_transitioned_when_task_id_is_set() {
        let (engine, sink) = build_test_engine(Some("task-1"), Some("scrape_complete"));

        engine
            .emit(LifecycleEvent::PhaseCompleted {
                phase: PipelinePhase::Synthesis,
            })
            .settled()
            .await
            .unwrap();

        let events = sink.lock().unwrap();
        let has_run_completed = events
            .iter()
            .any(|e| e.downcast_ref::<LifecycleEvent>().is_some_and(|le| matches!(le, LifecycleEvent::RunCompleted { .. })));
        let phase_transition = events
            .iter()
            .find_map(|e| e.downcast_ref::<SystemEvent>())
            .and_then(|se| match se {
                SystemEvent::TaskPhaseTransitioned { task_id, status, .. } => {
                    Some((task_id.clone(), status.clone()))
                }
                _ => None,
            });

        assert!(has_run_completed, "should emit RunCompleted");
        let (tid, status) = phase_transition.expect("should emit TaskPhaseTransitioned");
        assert_eq!(tid, "task-1");
        assert_eq!(status, "scrape_complete");
    }

    #[tokio::test]
    async fn finalize_does_not_emit_task_phase_transitioned_without_task_id() {
        let (engine, sink) = build_test_engine(None, None);

        engine
            .emit(LifecycleEvent::PhaseCompleted {
                phase: PipelinePhase::Synthesis,
            })
            .settled()
            .await
            .unwrap();

        let events = sink.lock().unwrap();
        let has_phase_transition = events
            .iter()
            .any(|e| e.downcast_ref::<SystemEvent>().is_some_and(|se| matches!(se, SystemEvent::TaskPhaseTransitioned { .. })));

        assert!(!has_phase_transition, "should NOT emit TaskPhaseTransitioned when task_id is None");
    }
}
