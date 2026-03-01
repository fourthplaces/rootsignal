// Lifecycle domain: reap, schedule, finalize.

pub mod activities;
pub mod events;

use std::sync::Arc;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, on, Context, Events, Handler};
use tracing::info;

use rootsignal_graph::GraphReader;

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::PipelinePhase;
use events::LifecycleEvent;

fn is_engine_started(e: &LifecycleEvent) -> bool {
    matches!(e, LifecycleEvent::EngineStarted { .. })
}

fn is_reap_completed(e: &LifecycleEvent) -> bool {
    matches!(e, LifecycleEvent::PhaseCompleted { phase } if matches!(phase, PipelinePhase::ReapExpired))
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

        let mut state = deps.state.write().await;
        state.apply_schedule_output(output);
        drop(state);

        Ok(events![LifecycleEvent::SourcesScheduled {
            tension_count,
            response_count,
        }])
    }
}

// ---------------------------------------------------------------------------
// Standalone finalize handlers — one per engine variant
// ---------------------------------------------------------------------------

async fn finalize_impl(ctx: Context<ScoutEngineDeps>) -> Result<Events> {
    let deps = ctx.deps();

    let state = deps.state.read().await;
    let stats = state.stats.clone();
    drop(state);

    if let Some(ref budget) = deps.budget {
        budget.log_status();
    }

    info!("{}", stats);
    Ok(events![LifecycleEvent::RunCompleted { stats }])
}

/// Finalize handler for the scrape chain: triggers on PhaseCompleted(Synthesis).
pub fn scrape_finalize_handler() -> Handler<ScoutEngineDeps> {
    on::<LifecycleEvent>()
        .id("lifecycle:finalize")
        .filter(|e: &LifecycleEvent| {
            matches!(
                e,
                LifecycleEvent::PhaseCompleted { phase }
                    if matches!(phase, PipelinePhase::Synthesis)
            )
        })
        .then(|_event: Arc<LifecycleEvent>, ctx: Context<ScoutEngineDeps>| {
            finalize_impl(ctx)
        })
}

/// Finalize handler for the full chain: triggers on PhaseCompleted(Supervisor).
pub fn full_finalize_handler() -> Handler<ScoutEngineDeps> {
    on::<LifecycleEvent>()
        .id("lifecycle:finalize")
        .filter(|e: &LifecycleEvent| {
            matches!(
                e,
                LifecycleEvent::PhaseCompleted { phase }
                    if matches!(phase, PipelinePhase::Supervisor)
            )
        })
        .then(|_event: Arc<LifecycleEvent>, ctx: Context<ScoutEngineDeps>| {
            finalize_impl(ctx)
        })
}
