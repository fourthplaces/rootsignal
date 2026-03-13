pub mod activities;
pub mod events;

use anyhow::Result;
use causal::{reactor, reactors, Context, Events};
use uuid::Uuid;

use crate::core::engine::ScoutEngineDeps;
use crate::domains::cluster_weaving::events::ClusterWeavingEvent;
use crate::domains::lifecycle::events::LifecycleEvent;

fn is_cluster_weave(e: &LifecycleEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, LifecycleEvent::ClusterWeaveRequested { .. })
}

#[reactors]
pub mod reactors {
    use super::*;

    #[reactor(on = LifecycleEvent, id = "cluster_weaving:weave_cluster", filter = is_cluster_weave)]
    async fn weave_cluster(
        event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let group_id = match &event {
            LifecycleEvent::ClusterWeaveRequested { group_id, .. } => *group_id,
            _ => return Ok(Events::new()),
        };

        let deps = ctx.deps();
        let mut all_events = activities::weave_cluster(&deps, group_id).await;
        if all_events.is_empty() {
            all_events.push(ClusterWeavingEvent::ClusterWeaveSkipped {
                reason: "No events produced — group may be empty or not found".into(),
            });
        } else {
            all_events.push(ClusterWeavingEvent::ClusterWeaveCompleted);
        }
        Ok(all_events)
    }
}
