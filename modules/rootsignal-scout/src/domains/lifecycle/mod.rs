// Lifecycle domain: reap, schedule, finalize.

pub mod activities;
pub mod events;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};
use tracing::info;



use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::core::events::PipelinePhase;
use crate::core::pipeline_events::PipelineEvent;
use events::LifecycleEvent;

fn is_scout_run_requested(e: &LifecycleEvent) -> bool {
    matches!(e, LifecycleEvent::ScoutRunRequested { .. })
}

fn is_find_stale_completed(e: &LifecycleEvent) -> bool {
    matches!(e, LifecycleEvent::PhaseCompleted { phase } if matches!(phase, PipelinePhase::FindStale))
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

    /// ScoutRunRequested → find stale signals, emit PhaseCompleted(FindStale).
    #[handle(on = LifecycleEvent, id = "lifecycle:find_stale", filter = is_scout_run_requested)]
    async fn find_stale(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let mut events = activities::find_stale_signals(&*deps.store).await;
        events.push(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::FindStale,
        });
        Ok(events)
    }

    /// PhaseCompleted(FindStale) → load + select sources, stash plan in state, emit SourcesPrepared.
    #[handle(on = LifecycleEvent, id = "lifecycle:prepare_sources", filter = is_find_stale_completed)]
    async fn prepare_sources(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();

        // Branch on run modality
        let output = match deps.run_scope.input_sources() {
            Some(sources) => activities::prepare_input_sources(sources),
            None => {
                let (region, graph) = match (deps.run_scope.region(), deps.graph.as_ref()) {
                    (Some(r), Some(g)) => (r, g),
                    _ => return Ok(events![PipelineEvent::HandlerSkipped {
                        handler_id: "lifecycle:prepare_sources".into(),
                        reason: "missing region or graph (test environment)".into(),
                    }]),
                };
                activities::prepare_sources(graph, region).await
            }
        };

        Ok(events![
            LifecycleEvent::SourcesPrepared {
                tension_count: output.tension_count,
                response_count: output.response_count,
                source_plan: output.source_plan,
                actor_contexts: output.actor_contexts,
                url_mappings: output.url_mappings,
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
    Ok(events![LifecycleEvent::RunCompleted { stats }])
}

/// Finalize handler for the scrape chain: triggers on PhaseCompleted(Synthesis).
#[handle(on = LifecycleEvent, id = "lifecycle:scrape_finalize", filter = is_synthesis_completed)]
pub async fn scrape_finalize(
    _event: LifecycleEvent,
    ctx: Context<ScoutEngineDeps>,
) -> Result<Events> {
    finalize_impl(ctx).await
}

/// Finalize handler for the full chain: triggers on PhaseCompleted(Supervisor).
#[handle(on = LifecycleEvent, id = "lifecycle:full_finalize", filter = is_supervisor_completed)]
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

    fn build_test_engine(
        task_id: Option<&str>,
    ) -> (seesaw_core::Engine<ScoutEngineDeps>, Arc<Mutex<Vec<AnyEvent>>>) {
        let sink = Arc::new(Mutex::new(Vec::new()));
        let mut deps = ScoutEngineDeps::new(
            Arc::new(crate::testing::MockSignalReader::new()),
            Arc::new(crate::infra::embedder::NoOpEmbedder),
            "test",
        );
        deps.task_id = task_id.map(String::from);
        deps.captured_events = Some(sink.clone());
        let engine = build_engine(deps, None);
        (engine, sink)
    }

    #[tokio::test]
    async fn finalize_emits_run_completed() {
        let (engine, sink) = build_test_engine(Some("task-1"));

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

        assert!(has_run_completed, "should emit RunCompleted");
    }

    #[tokio::test]
    async fn handler_failure_counted_in_run_completed_stats() {
        use crate::core::pipeline_events::PipelineEvent;

        let (engine, sink) = build_test_engine(Some("task-1"));

        engine
            .emit(PipelineEvent::HandlerFailed {
                handler_id: "scrape:fetch".to_string(),
                source_event_type: "ScrapeEvent".to_string(),
                error: "connection timeout".to_string(),
                attempts: 3,
            })
            .settled()
            .await
            .unwrap();

        engine
            .emit(PipelineEvent::HandlerFailed {
                handler_id: "synthesis:linker".to_string(),
                source_event_type: "SynthesisEvent".to_string(),
                error: "panicked at 'index out of bounds'".to_string(),
                attempts: 1,
            })
            .settled()
            .await
            .unwrap();

        // Trigger finalize
        engine
            .emit(LifecycleEvent::PhaseCompleted {
                phase: PipelinePhase::Synthesis,
            })
            .settled()
            .await
            .unwrap();

        let events = sink.lock().unwrap();
        let stats = events
            .iter()
            .filter_map(|e| e.downcast_ref::<LifecycleEvent>())
            .find_map(|le| match le {
                LifecycleEvent::RunCompleted { stats } => Some(stats),
                _ => None,
            })
            .expect("should emit RunCompleted");

        assert_eq!(stats.handler_failures, 2, "RunCompleted stats should carry accumulated handler failure count");
    }
}
