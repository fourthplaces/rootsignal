pub mod activities;
pub mod events;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::situation_weaving::events::SituationWeavingEvent;
use crate::domains::supervisor::events::SupervisorEvent;

fn is_weaving_done(e: &SituationWeavingEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(
        e,
        SituationWeavingEvent::SituationsWeaved | SituationWeavingEvent::NothingToWeave { .. }
    )
}

#[handlers]
pub mod handlers {
    use super::*;

    /// SituationsWeaved or NothingToWeave → supervise region, emit SupervisionCompleted.
    #[handle(on = SituationWeavingEvent, id = "supervisor:run_supervisor", filter = is_weaving_done)]
    async fn run_supervisor(
        _event: SituationWeavingEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let state = ctx.aggregate::<PipelineState>().curr;
        let mut out = events![SupervisorEvent::SupervisionCompleted];
        activities::supervise(&deps, state.run_scope.region(), &mut out).await;
        Ok(out)
    }
}
