pub mod activities;
pub mod events;
#[cfg(test)]
mod tests;

use anyhow::Result;
use causal::{reactor, reactors, Context, Events};
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::events::SystemEvent;

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::coalescing::activities::coalescer::Coalescer;
use crate::domains::coalescing::events::CoalescingEvent;
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::scheduling::activities::budget::OperationCost;
use crate::domains::scheduling::events::SchedulingEvent;

pub const BASE_FEED_INTERVAL: u64 = 3600;
const MAX_BACKOFF_SECONDS: i32 = 604_800;

fn is_generate_situations(e: &LifecycleEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, LifecycleEvent::GenerateSituationsRequested { .. })
}

fn is_coalesce_requested(e: &LifecycleEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, LifecycleEvent::CoalesceRequested { .. })
}

fn is_feed_group_requested(e: &LifecycleEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, LifecycleEvent::FeedGroupRequested { .. })
}

pub(crate) fn result_to_events(
    result: &activities::types::CoalescingResult,
) -> (Vec<SystemEvent>, CoalescingEvent, Vec<SchedulingEvent>) {
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

    // Auto-schedule feeds for newly created groups
    let schedule_events: Vec<SchedulingEvent> = result.new_groups.iter().map(|group| {
        SchedulingEvent::ScheduleCreated {
            schedule_id: format!("group_feed_{}", group.group_id),
            flow_type: "group_feed".into(),
            scope: serde_json::json!({ "group_id": group.group_id.to_string() }),
            timeout: BASE_FEED_INTERVAL,
            base_timeout: Some(BASE_FEED_INTERVAL),
            recurring: true,
            region_id: None,
        }
    }).collect();

    (system_events, completed, schedule_events)
}

#[reactors]
pub mod reactors {
    use super::*;

    #[reactor(on = LifecycleEvent, id = "coalescing:coalesce_signals", filter = is_generate_situations)]
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
        let result = coalescer.run(None).await?;

        let mut events = Events::new();
        let (system_events, completed, schedule_events) = result_to_events(&result);
        for se in system_events {
            events.push(se);
        }
        for se in schedule_events {
            events.push(se);
        }
        events.push(completed);

        Ok(events)
    }

    #[reactor(on = LifecycleEvent, id = "coalescing:coalesce_from_seed", filter = is_coalesce_requested)]
    async fn coalesce_from_seed(
        event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let seed_signal_id = match &event {
            LifecycleEvent::CoalesceRequested { seed_signal_id, .. } => *seed_signal_id,
            _ => None,
        };

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
        let result = coalescer.run(seed_signal_id).await?;

        let mut events = Events::new();
        let (system_events, completed, schedule_events) = result_to_events(&result);
        for se in system_events {
            events.push(se);
        }
        for se in schedule_events {
            events.push(se);
        }
        events.push(completed);

        Ok(events)
    }

    #[reactor(on = LifecycleEvent, id = "coalescing:feed_group", filter = is_feed_group_requested)]
    async fn feed_group(
        event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let group_id = match &event {
            LifecycleEvent::FeedGroupRequested { group_id, .. } => *group_id,
            _ => return Ok(Events::new()),
        };

        let deps = ctx.deps();

        let (graph, ai, embedder) = match (deps.graph.as_ref(), deps.ai.as_ref()) {
            (Some(g), Some(a)) => (g.clone(), a.clone(), deps.embedder.clone()),
            _ => {
                info!("Feed group skipped: missing graph or AI deps");
                let mut events = Events::new();
                events.push(CoalescingEvent::GroupFeedCompleted {
                    group_id,
                    signals_added: 0,
                    queries_refined: false,
                });
                return Ok(events);
            }
        };

        let group = match graph.get_group_brief(group_id).await? {
            Some(g) => g,
            None => {
                warn!(%group_id, "Feed group: group not found");
                let mut events = Events::new();
                events.push(CoalescingEvent::GroupFeedCompleted {
                    group_id,
                    signals_added: 0,
                    queries_refined: false,
                });
                return Ok(events);
            }
        };

        let coalescer = Coalescer::new(graph, ai, embedder);
        let (fed_signals, refined_queries) = coalescer.feed_single_group(&group).await?;

        let mut events = Events::new();

        for fed in &fed_signals {
            events.push(SystemEvent::SignalAddedToGroup {
                signal_id: fed.signal_id,
                group_id: fed.group_id,
                confidence: fed.confidence,
            });
        }

        if let Some(queries) = &refined_queries {
            events.push(SystemEvent::GroupQueriesRefined {
                group_id,
                queries: queries.clone(),
            });
        }

        let signals_added = fed_signals.len() as u32;
        let queries_refined = refined_queries.is_some();

        events.push(CoalescingEvent::GroupFeedCompleted {
            group_id,
            signals_added,
            queries_refined,
        });

        // Backoff adjustment: double on empty feed, reset on success
        let schedule_id = format!("group_feed_{}", group_id);
        if signals_added == 0 {
            // Look up current timeout from DB to double it
            let current_timeout = if let Some(pool) = &deps.pg_pool {
                sqlx::query_scalar::<_, i32>(
                    "SELECT timeout FROM schedules WHERE schedule_id = $1",
                )
                .bind(&schedule_id)
                .fetch_optional(pool)
                .await
                .ok()
                .flatten()
                .unwrap_or(BASE_FEED_INTERVAL as i32)
            } else {
                BASE_FEED_INTERVAL as i32
            };
            let new_timeout = (current_timeout * 2).min(MAX_BACKOFF_SECONDS);
            events.push(SchedulingEvent::ScheduleCadenceAdjusted {
                schedule_id,
                new_timeout,
                reason: "empty feed — backing off".into(),
            });
        } else {
            events.push(SchedulingEvent::ScheduleCadenceAdjusted {
                schedule_id,
                new_timeout: BASE_FEED_INTERVAL as i32,
                reason: "signals found — resetting to base".into(),
            });
        }

        Ok(events)
    }
}
