//! Seesaw handlers for the enrichment domain.

use std::sync::Arc;

use seesaw_core::{handler::Emit, on, Context, Handler};

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::{PipelineEvent, PipelinePhase, ScoutEvent};
use crate::enrichment::actor_location;

fn batch(events: Vec<ScoutEvent>) -> Emit<ScoutEvent> {
    Emit::Batch(events)
}

/// PhaseCompleted(ResponseScrape) → enrich actor locations from signal evidence.
pub fn actor_location_handler() -> Handler<ScoutEngineDeps> {
    on::<ScoutEvent>()
        .id("enrichment:actor_location")
        .filter(|e: &ScoutEvent| {
            matches!(
                e,
                ScoutEvent::Pipeline(PipelineEvent::PhaseCompleted { phase })
                    if matches!(phase, PipelinePhase::ResponseScrape)
            )
        })
        .then::<ScoutEngineDeps, _, _, _, _, ScoutEvent>(
            |_event: Arc<ScoutEvent>, ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();
                let pipe = deps.pipeline_deps.read().await;
                let pipe = pipe.as_ref().expect("pipeline_deps set by dispatch");

                let actors = match pipe.store.list_all_actors().await {
                    Ok(a) => a,
                    Err(_) => return Ok(Emit::None),
                };
                if actors.is_empty() {
                    return Ok(Emit::None);
                }

                let events =
                    actor_location::collect_actor_location_events(&*pipe.store, &actors).await;
                if events.is_empty() {
                    return Ok(Emit::None);
                }
                let actors_updated = events.len() as u32;
                let mut all_events = events;
                all_events.push(ScoutEvent::Pipeline(
                    PipelineEvent::ActorEnrichmentCompleted { actors_updated },
                ));
                Ok(batch(all_events))
            },
        )
}
