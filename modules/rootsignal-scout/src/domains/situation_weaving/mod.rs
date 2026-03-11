// Situation weaving domain: assign signals to situations, source boost, curiosity.

pub mod activities;
pub mod events;

use anyhow::Result;
use seesaw_core::{handle, handlers, Context, Events};

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::scheduling::activities::budget::OperationCost;
use crate::domains::situation_weaving::events::SituationWeavingEvent;

fn is_generate_situations(e: &LifecycleEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, LifecycleEvent::GenerateSituationsRequested { .. })
}

#[handlers]
pub mod handlers {
    use super::*;

    /// GenerateSituationsRequested → weave situations, emit SituationsWeaved.
    #[handle(on = LifecycleEvent, id = "situation_weaving:weave_situations", filter = is_generate_situations)]
    async fn weave_situations(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let state = ctx.aggregate::<PipelineState>().curr;
        let has_budget = state.has_budget(OperationCost::CLAUDE_HAIKU_STORY_WEAVE);
        let mut all_events = activities::weave_situations(&deps, state.run_scope.region(), has_budget).await;
        all_events.push(SituationWeavingEvent::SituationsWeaved);
        Ok(all_events)
    }
}
