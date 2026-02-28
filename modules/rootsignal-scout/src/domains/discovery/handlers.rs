//! Seesaw handlers for the discovery domain.

use std::sync::Arc;

use seesaw_core::{handler::Emit, on, Context, Handler};

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::{PipelineEvent, ScoutEvent};
use crate::pipeline::handlers::bootstrap;

fn batch(events: Vec<ScoutEvent>) -> Emit<ScoutEvent> {
    Emit::Batch(events)
}

/// EngineStarted → seed sources when the region has none.
pub fn bootstrap_handler() -> Handler<ScoutEngineDeps> {
    on::<ScoutEvent>()
        .id("discovery:bootstrap")
        .filter(|e: &ScoutEvent| {
            matches!(
                e,
                ScoutEvent::Pipeline(PipelineEvent::EngineStarted { .. })
            )
        })
        .then::<ScoutEngineDeps, _, _, _, _, ScoutEvent>(
            |event: Arc<ScoutEvent>, ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();
                let state = deps.state.read().await;
                let pipe = deps.pipeline_deps.read().await;
                let pipe = pipe.as_ref().expect("pipeline_deps set by dispatch");
                let events =
                    bootstrap::handle_engine_started(&state, pipe).await?;
                Ok(batch(events))
            },
        )
}
