//! Seesaw handlers for the lifecycle domain: reap, schedule, finalize.
//!
//! Thin wrappers that delegate to activity functions in `activities.rs`.

use std::sync::Arc;

use seesaw_core::{events, on, Context, Events, Handler};
use tracing::info;

use rootsignal_graph::GraphWriter;

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::PipelinePhase;
use crate::domains::lifecycle::activities;
use crate::domains::lifecycle::events::LifecycleEvent;

/// EngineStarted → reap expired signals, emit PhaseCompleted(ReapExpired).
pub fn reap_handler() -> Handler<ScoutEngineDeps> {
    on::<LifecycleEvent>()
        .id("lifecycle:reap")
        .filter(|e: &LifecycleEvent| {
            matches!(e, LifecycleEvent::EngineStarted { .. })
        })
        .then(
            |_event: Arc<LifecycleEvent>, ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();
                let scout_events = activities::reap_expired(&*deps.store).await;

                Ok(Events::batch(scout_events).add(LifecycleEvent::PhaseCompleted {
                    phase: PipelinePhase::ReapExpired,
                }))
            },
        )
}

/// PhaseCompleted(ReapExpired) → load + schedule sources, stash in state, emit SourcesScheduled.
pub fn schedule_handler() -> Handler<ScoutEngineDeps> {
    on::<LifecycleEvent>()
        .id("lifecycle:schedule")
        .filter(|e: &LifecycleEvent| {
            matches!(
                e,
                LifecycleEvent::PhaseCompleted { phase }
                    if matches!(phase, PipelinePhase::ReapExpired)
            )
        })
        .then(
            |_event: Arc<LifecycleEvent>, ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();

                // Requires graph_client + region — skip in tests
                let (region, graph_client) = match (deps.region.as_ref(), deps.graph_client.as_ref())
                {
                    (Some(r), Some(g)) => (r, g),
                    _ => return Ok(events![]),
                };
                let writer = GraphWriter::new(graph_client.clone());

                let output = activities::schedule_sources(&writer, region).await;

                let tension_count = output.tension_count;
                let response_count = output.response_count;

                let mut state = deps.state.write().await;
                state.apply_schedule_output(output);
                drop(state);

                Ok(events![LifecycleEvent::SourcesScheduled {
                    tension_count,
                    response_count,
                }])
            },
        )
}

/// PhaseCompleted(Synthesis) → save run stats, emit RunCompleted.
pub fn finalize_handler() -> Handler<ScoutEngineDeps> {
    on::<LifecycleEvent>()
        .id("lifecycle:finalize")
        .filter(|e: &LifecycleEvent| {
            matches!(
                e,
                LifecycleEvent::PhaseCompleted { phase }
                    if matches!(phase, PipelinePhase::Synthesis)
            )
        })
        .then(
            |_event: Arc<LifecycleEvent>, ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();

                let state = deps.state.read().await;
                let stats = state.stats.clone();
                drop(state);

                if let Some(ref budget) = deps.budget {
                    budget.log_status();
                }

                info!("{}", stats);
                Ok(events![LifecycleEvent::RunCompleted { stats }])
            },
        )
}
