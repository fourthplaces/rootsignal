pub mod activities;
pub mod events;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};

use rootsignal_common::{Block, ChecklistItem};

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::situation_weaving::events::SituationWeavingEvent;
use crate::domains::supervisor::events::SupervisorEvent;
use crate::domains::synthesis::events::all_synthesis_roles;

fn is_weaving_done(e: &SituationWeavingEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(
        e,
        SituationWeavingEvent::SituationsWeaved | SituationWeavingEvent::NothingToWeave { .. }
    )
}

fn describe_supervisor_gate(ctx: &Context<ScoutEngineDeps>) -> Vec<Block> {
    let (_, state) = ctx.singleton::<PipelineState>();
    let all = all_synthesis_roles();
    let done = &state.completed_synthesis_roles;
    vec![
        Block::Checklist {
            label: "Synthesis roles".into(),
            items: all.iter().map(|r| ChecklistItem {
                text: format!("{r:?}"),
                done: done.contains(r),
            }).collect(),
        },
        Block::Label {
            text: "Waiting for situation weaving".into(),
        },
    ]
}

#[handlers]
pub mod handlers {
    use super::*;

    /// SituationsWeaved or NothingToWeave → supervise region, emit SupervisionCompleted.
    #[handle(on = SituationWeavingEvent, id = "supervisor:run_supervisor", filter = is_weaving_done, describe = describe_supervisor_gate)]
    async fn run_supervisor(
        _event: SituationWeavingEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();
        let mut out = events![SupervisorEvent::SupervisionCompleted];
        activities::supervise(&deps, state.run_scope.region(), &mut out).await;
        Ok(out)
    }
}
