// Lifecycle domain: reap, schedule, finalize.

pub mod activities;
pub mod events;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};
use tracing::info;

use rootsignal_graph::GraphStore;

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::PipelinePhase;
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
        let scout_events = activities::reap_expired(&*deps.store).await;

        Ok(Events::batch(scout_events).add(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::ReapExpired,
        }))
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
        let writer = GraphStore::new(graph_client.clone());

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
    }

    /// PhaseCompleted(Synthesis) → save run stats, emit RunCompleted.
    #[handle(on = LifecycleEvent, id = "lifecycle:finalize", filter = is_synthesis_completed)]
    async fn finalize(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
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
}
