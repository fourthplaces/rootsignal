pub mod activities;
pub mod events;
#[cfg(test)]
mod tests;

use anyhow::Result;
use seesaw_core::{handle, handlers, Context, Events};
use tracing::info;

use rootsignal_common::events::SystemEvent;

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::coalescing::activities::coalescer::Coalescer;
use crate::domains::coalescing::events::CoalescingEvent;
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::scheduling::activities::budget::OperationCost;

fn is_generate_situations(e: &LifecycleEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, LifecycleEvent::GenerateSituationsRequested { .. })
}

pub(crate) fn result_to_events(
    result: &activities::types::CoalescingResult,
) -> (Vec<SystemEvent>, CoalescingEvent) {
    let mut system_events = vec![];

    for group in &result.new_groups {
        system_events.push(SystemEvent::GroupCreated {
            group_id: group.group_id,
            label: group.label.clone(),
            queries: group.queries.clone(),
            seed_signal_id: group.signal_ids.first().map(|(id, _)| *id),
        });

        for (signal_id, confidence) in group.signal_ids.iter().skip(1) {
            system_events.push(SystemEvent::SignalAddedToGroup {
                signal_id: *signal_id,
                group_id: group.group_id,
                confidence: *confidence,
            });
        }
    }

    for fed in &result.fed_signals {
        system_events.push(SystemEvent::SignalAddedToGroup {
            signal_id: fed.signal_id,
            group_id: fed.group_id,
            confidence: fed.confidence,
        });
    }

    for (group_id, queries) in &result.refined_queries {
        system_events.push(SystemEvent::GroupQueriesRefined {
            group_id: *group_id,
            queries: queries.clone(),
        });
    }

    let completed = CoalescingEvent::CoalescingCompleted {
        new_groups: result.new_groups.len() as u32,
        fed_signals: result.fed_signals.len() as u32,
        refined_groups: result.refined_queries.len() as u32,
    };

    (system_events, completed)
}

#[handlers]
pub mod handlers {
    use super::*;

    #[handle(on = LifecycleEvent, id = "coalescing:coalesce_signals", filter = is_generate_situations)]
    async fn coalesce_signals(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let state = ctx.aggregate::<PipelineState>().curr;

        let (graph, ai, embedder) = match (deps.graph.as_ref(), deps.ai.as_ref()) {
            (Some(g), Some(a)) => (g.clone(), a.clone(), deps.embedder.clone()),
            _ => {
                info!("Coalescing skipped: missing graph or AI deps");
                let mut events = Events::new();
                events.push(CoalescingEvent::CoalescingSkipped {
                    reason: "missing graph or AI deps".into(),
                });
                return Ok(events);
            }
        };

        if !state.has_budget(OperationCost::COALESCING) {
            info!("Coalescing skipped: insufficient budget");
            let mut events = Events::new();
            events.push(CoalescingEvent::CoalescingSkipped {
                reason: "insufficient budget".into(),
            });
            return Ok(events);
        }

        let coalescer = Coalescer::new(graph, ai, embedder);
        let result = coalescer.run().await?;

        let mut events = Events::new();
        let (system_events, completed) = result_to_events(&result);
        for se in system_events {
            events.push(se);
        }
        events.push(completed);

        Ok(events)
    }
}
