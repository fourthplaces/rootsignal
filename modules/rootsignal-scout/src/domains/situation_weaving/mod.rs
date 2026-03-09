// Situation weaving domain: assign signals to situations, source boost, curiosity.

pub mod activities;
pub mod events;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};

use rootsignal_common::{Block, ChecklistItem};

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::situation_weaving::events::SituationWeavingEvent;
use crate::domains::synthesis::events::SynthesisEvent;

fn is_severity_inferred(e: &SynthesisEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, SynthesisEvent::SeverityInferred)
}

fn describe_weaving_gate(ctx: &Context<ScoutEngineDeps>) -> Vec<Block> {
    let state = ctx.aggregate::<PipelineState>().curr;
    vec![
        Block::Checklist {
            label: "Synthesis".into(),
            items: vec![
                ChecklistItem { text: "Similarity".into(), done: state.similarity_computed },
                ChecklistItem { text: "Response mapping".into(), done: state.responses_mapped },
                ChecklistItem { text: "Severity".into(), done: state.severity_inferred },
            ],
        },
    ]
}

#[handlers]
pub mod handlers {
    use super::*;

    /// All synthesis done → weave situations, emit SituationsWeaved.
    #[handle(on = SynthesisEvent, id = "situation_weaving:weave_situations", filter = is_severity_inferred, describe = describe_weaving_gate)]
    async fn weave_situations(
        _event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let state = ctx.aggregate::<PipelineState>().curr;
        let mut all_events = activities::weave_situations(&deps, state.run_scope.region()).await;
        all_events.push(SituationWeavingEvent::SituationsWeaved);
        Ok(all_events)
    }
}
