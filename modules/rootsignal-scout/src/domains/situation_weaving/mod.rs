// Situation weaving domain: assign signals to situations, source boost, curiosity.

pub mod activities;
pub mod events;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::situation_weaving::events::SituationWeavingEvent;
use crate::domains::synthesis::events::{all_synthesis_roles, SynthesisEvent};

fn all_synthesis_done_and_not_yet_weaved(e: &SynthesisEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    if !matches!(e, SynthesisEvent::SynthesisRoleCompleted { .. }) {
        return false;
    }
    let (_, state) = ctx.singleton::<PipelineState>();
    state.completed_synthesis_roles.is_superset(&all_synthesis_roles())
}

#[handlers]
pub mod handlers {
    use super::*;

    /// All synthesis roles done → weave situations, emit SituationsWeaved.
    #[handle(on = SynthesisEvent, id = "situation_weaving:run", filter = all_synthesis_done_and_not_yet_weaved)]
    async fn situation_weaving(
        _event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();
        let mut all_events = activities::weave_situations(&deps, state.run_scope.region()).await;
        all_events.push(SituationWeavingEvent::SituationsWeaved);
        Ok(all_events)
    }
}
