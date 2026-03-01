// Supervisor domain: issue detection, duplicate merging, cause heat, beacons.

pub mod activities;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::PipelinePhase;
use crate::domains::lifecycle::events::LifecycleEvent;

fn is_situation_weaving_completed(e: &LifecycleEvent) -> bool {
    matches!(
        e,
        LifecycleEvent::PhaseCompleted { phase }
            if matches!(phase, PipelinePhase::SituationWeaving)
    )
}

#[handlers]
pub mod handlers {
    use super::*;

    /// PhaseCompleted(SituationWeaving) → supervise region, emit PhaseCompleted(Supervisor).
    #[handle(on = LifecycleEvent, id = "supervisor:run", filter = is_situation_weaving_completed)]
    async fn supervisor(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let mut out = events![LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::Supervisor,
        }];
        activities::supervise(&deps, &mut out).await;
        Ok(out)
    }
}
