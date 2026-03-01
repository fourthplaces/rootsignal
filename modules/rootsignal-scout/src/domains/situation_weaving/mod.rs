// Situation weaving domain: assign signals to situations, source boost, curiosity.

pub mod activities;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::PipelinePhase;
use crate::domains::lifecycle::events::LifecycleEvent;

fn is_synthesis_completed(e: &LifecycleEvent) -> bool {
    matches!(
        e,
        LifecycleEvent::PhaseCompleted { phase }
            if matches!(phase, PipelinePhase::Synthesis)
    )
}

#[handlers]
pub mod handlers {
    use super::*;

    /// PhaseCompleted(Synthesis) → weave situations, emit PhaseCompleted(SituationWeaving).
    #[handle(on = LifecycleEvent, id = "situation_weaving:run", filter = is_synthesis_completed)]
    async fn situation_weaving(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let mut all_events = activities::weave_situations(&deps).await;
        all_events.push(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::SituationWeaving,
        });
        Ok(all_events)
    }
}
