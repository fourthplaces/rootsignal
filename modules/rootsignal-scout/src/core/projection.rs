//! Infrastructure projections: project events to Neo4j, maintain scout_runs table.
//!
//! Persistence is handled by seesaw's unified `Store` trait via
//! `PostgresStore`. Aggregator state is handled by seesaw's
//! registered aggregators (priority 1).
//!
//! Projections here:
//! - `neo4j_projection_handler` — project events to Neo4j graph (Handler, needs priority control)
//! - `scout_runs_projection` — maintain the scout_runs lookup table
//! - `system_log_projection` — print SystemLog events to stdout

use std::sync::Arc;

use chrono::Utc;
use rootsignal_graph::GraphProjector;
use seesaw_core::{events, on_any, project, AnyEvent, Context, Events, Handler, Projection};
use tracing::info;

use rootsignal_common::events::{Event, EventDomain, SystemEvent, WorldEvent};
use rootsignal_common::telemetry_events::TelemetryEvent;

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::core::pipeline_events::PipelineEvent;
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::enrichment::events::EnrichmentEvent;
use crate::domains::expansion::events::ExpansionEvent;
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::scheduling::events::{ScheduledScope, SchedulingEvent};
use crate::domains::scrape::events::ScrapeEvent;
use crate::domains::signals::events::SignalEvent;
use crate::domains::situation_weaving::events::SituationWeavingEvent;
use crate::domains::supervisor::events::SupervisorEvent;
use crate::domains::synthesis::events::SynthesisEvent;

// Priority-0: event persistence — handled by seesaw's unified Store (PostgresStore in production).
// Priority-1: aggregate state — handled by seesaw aggregators.

/// Priority-2 handler: project events to Neo4j graph.
///
/// Captures `GraphProjector` via closure — not on `ScoutEngineDeps`.
/// Routes events by `EventDomain` — exhaustive match ensures compile-time
/// safety when new domains are added.
pub fn neo4j_projection_handler(projector: GraphProjector) -> Handler<ScoutEngineDeps> {
    let projector = Arc::new(projector);
    on_any()
        .id("neo4j_projection")
        .priority(2)
        .then(move |event: AnyEvent, ctx: Context<ScoutEngineDeps>| {
            let projector = projector.clone();
            async move {
                // Classify the event into its domain. Each arm is explicit —
                // adding a new EventDomain variant without handling it here
                // will fail to compile.
                let (domain, event_type, payload) = classify_event(&event);

                // Exhaustive match — no wildcard. Mirrors GraphProjector::project().
                match domain {
                    EventDomain::Fact => {}
                    EventDomain::Discovery | EventDomain::Pipeline => {}
                    EventDomain::Scrape => return Ok(events![]),
                    EventDomain::Signal => return Ok(events![]),
                    EventDomain::Lifecycle => return Ok(events![]),
                    EventDomain::Enrichment => return Ok(events![]),
                    EventDomain::Expansion => return Ok(events![]),
                    EventDomain::Synthesis => return Ok(events![]),
                    EventDomain::SituationWeaving => return Ok(events![]),
                    EventDomain::Supervisor => return Ok(events![]),
                    EventDomain::Scheduling => return Ok(events![]),
                    EventDomain::Curiosity => return Ok(events![]),
                }

                let (event_type, payload) = match (event_type, payload) {
                    (Some(t), Some(p)) => (t, p),
                    _ => return Ok(events![]),
                };

                let deps = ctx.deps();
                let persisted = seesaw_core::types::PersistedEvent {
                    position: 0,
                    event_id: ctx.current_event_id(),
                    parent_id: ctx.parent_event_id(),
                    correlation_id: ctx.correlation_id,
                    event_type,
                    payload,
                    created_at: Utc::now(),
                    aggregate_type: None,
                    aggregate_id: None,
                    version: None,
                    metadata: {
                        let mut m = serde_json::Map::new();
                        m.insert("run_id".into(), serde_json::json!(deps.run_id.to_string()));
                        m.insert("schema_v".into(), serde_json::json!(1));
                        m
                    },
                    ephemeral: None,
                    persistent: true,
                };
                projector.project(&persisted).await?;
                Ok(events![])
            }
        })
}

/// Classify a live event into its domain, event_type string, and payload.
///
/// Returns `(domain, Option<event_type>, Option<payload>)`.
/// For non-projectable events within a projectable domain, event_type/payload
/// are None (the handler skips them).
fn classify_event(
    event: &AnyEvent,
) -> (EventDomain, Option<String>, Option<serde_json::Value>) {
    use seesaw_core::event::Event as _;

    if let Some(e) = event.downcast_ref::<WorldEvent>() {
        (
            EventDomain::Fact,
            Some(e.durable_name().to_string()),
            Some(serde_json::to_value(e).unwrap()),
        )
    } else if let Some(e) = event.downcast_ref::<SystemEvent>() {
        (
            EventDomain::Fact,
            Some(e.durable_name().to_string()),
            Some(serde_json::to_value(e).unwrap()),
        )
    } else if let Some(e) = event.downcast_ref::<TelemetryEvent>() {
        (
            EventDomain::Fact,
            Some(e.durable_name().to_string()),
            Some(serde_json::to_value(e).unwrap()),
        )
    } else if let Some(e) = event.downcast_ref::<DiscoveryEvent>() {
        if e.is_projectable() {
            (EventDomain::Discovery, Some(e.durable_name().to_string()), Some(serde_json::to_value(e).unwrap()))
        } else {
            (EventDomain::Discovery, None, None)
        }
    } else if let Some(e) = event.downcast_ref::<PipelineEvent>() {
        if e.is_projectable() {
            (EventDomain::Pipeline, Some(e.durable_name().to_string()), Some(serde_json::to_value(e).unwrap()))
        } else {
            (EventDomain::Pipeline, None, None)
        }
    } else if event.downcast_ref::<ScrapeEvent>().is_some() {
        (EventDomain::Scrape, None, None)
    } else if let Some(e) = event.downcast_ref::<SignalEvent>() {
        (
            EventDomain::Signal,
            Some(e.durable_name().to_string()),
            Some(serde_json::to_value(e).unwrap()),
        )
    } else if event.downcast_ref::<LifecycleEvent>().is_some() {
        (EventDomain::Lifecycle, None, None)
    } else if event.downcast_ref::<EnrichmentEvent>().is_some() {
        (EventDomain::Enrichment, None, None)
    } else if event.downcast_ref::<ExpansionEvent>().is_some() {
        (EventDomain::Expansion, None, None)
    } else if event.downcast_ref::<SynthesisEvent>().is_some() {
        (EventDomain::Synthesis, None, None)
    } else if event.downcast_ref::<SituationWeavingEvent>().is_some() {
        (EventDomain::SituationWeaving, None, None)
    } else if event.downcast_ref::<SupervisorEvent>().is_some() {
        (EventDomain::Supervisor, None, None)
    } else if let Some(e) = event.downcast_ref::<SchedulingEvent>() {
        (EventDomain::Scheduling, Some(e.durable_name().to_string()), Some(serde_json::to_value(e).unwrap()))
    } else {
        tracing::debug!("neo4j_projection: unrecognized event type (not in any known domain)");
        (EventDomain::Fact, None, None)
    }
}

/// Detect terminal events that mean a run is finished.
///
/// Terminal events are domain facts — the causal chain always reaches one.
/// `NothingToWeave` / `NothingToSupervise` ensure every engine variant terminates.
fn is_terminal_event(event: &AnyEvent) -> bool {
    if event.downcast_ref::<SynthesisEvent>()
        .is_some_and(|e| matches!(e, SynthesisEvent::SeverityInferred))
    {
        return true;
    }

    if event.downcast_ref::<SupervisorEvent>()
        .is_some_and(|e| matches!(e, SupervisorEvent::SupervisionCompleted | SupervisorEvent::NothingToSupervise { .. }))
    { return true; }

    false
}

/// Priority-3 handler: observe terminal events, write stats, emit ScoutRunCompleted.
///
/// Lives inside the causal chain — ScoutRunCompleted is caused by the terminal
/// domain event, not injected from outside. The projection reacts to
/// ScoutRunCompleted to write `finished_at`.
pub fn run_completion_handler() -> Handler<ScoutEngineDeps> {
    on_any()
        .id("run_completion")
        .priority(3)
        .then(move |event: AnyEvent, ctx: Context<ScoutEngineDeps>| {
            async move {
                if !is_terminal_event(&event) {
                    return Ok(events![]);
                }

                let deps = ctx.deps();
                let state = ctx.aggregate::<PipelineState>().curr;
                let final_stats = state.stats.clone();
                info!("{}", final_stats);

                if let Some(pool) = &deps.pg_pool {
                    let stats_json = serde_json::to_value(&final_stats)?;
                    sqlx::query(
                        "UPDATE scout_runs SET stats = $2, spent_cents = $3 WHERE run_id = $1",
                    )
                    .bind(deps.run_id.to_string())
                    .bind(stats_json)
                    .bind(final_stats.spent_cents as i64)
                    .execute(pool)
                    .await?;
                }

                Ok(events![LifecycleEvent::ScoutRunCompleted {
                    run_id: deps.run_id,
                    finished_at: Utc::now(),
                }])
            }
        })
}

/// Projection: maintain the `scout_runs` lookup table.
///
/// On `ScoutRunRequested`: INSERT a new row with flow metadata.
/// On `ScoutRunCompleted`: UPDATE `finished_at` from the event timestamp.
pub fn scout_runs_projection() -> Projection<ScoutEngineDeps> {
    project("scout_runs_projection").then(move |event: AnyEvent, ctx: Context<ScoutEngineDeps>| {
        async move {
            if let Some(lifecycle) = event.downcast_ref::<LifecycleEvent>() {
                match lifecycle {
                    LifecycleEvent::ScoutRunRequested {
                        run_id, scope, region_id, flow_type, source_ids, task_id, ..
                    } => {
                        let deps = ctx.deps();
                        if let Some(pool) = &deps.pg_pool {
                            let region = scope
                                .region()
                                .map(|r| r.name.as_str())
                                .unwrap_or("unknown");
                            let scope_json = scope
                                .region()
                                .and_then(|r| serde_json::to_value(r).ok());
                            let source_ids_json = source_ids.as_ref()
                                .and_then(|ids| serde_json::to_value(ids).ok());
                            sqlx::query(
                                "INSERT INTO scout_runs (run_id, region, region_id, flow_type, source_ids, scope, task_id, started_at) \
                                 VALUES ($1, $2, $3, $4, $5, $6, $7, now()) \
                                 ON CONFLICT (run_id) DO UPDATE SET \
                                   region_id = COALESCE(EXCLUDED.region_id, scout_runs.region_id), \
                                   flow_type = COALESCE(EXCLUDED.flow_type, scout_runs.flow_type), \
                                   source_ids = COALESCE(EXCLUDED.source_ids, scout_runs.source_ids), \
                                   task_id = COALESCE(EXCLUDED.task_id, scout_runs.task_id)",
                            )
                            .bind(run_id.to_string())
                            .bind(region)
                            .bind(region_id.as_deref())
                            .bind(flow_type.as_str())
                            .bind(&source_ids_json)
                            .bind(&scope_json)
                            .bind(task_id.as_deref())
                            .execute(pool)
                            .await?;
                        }
                    }
                    LifecycleEvent::ScoutRunCompleted { run_id, finished_at } => {
                        let deps = ctx.deps();
                        if let Some(pool) = &deps.pg_pool {
                            sqlx::query(
                                "UPDATE scout_runs SET finished_at = $2 WHERE run_id = $1 AND finished_at IS NULL",
                            )
                            .bind(run_id.to_string())
                            .bind(finished_at)
                            .execute(pool)
                            .await?;
                        }
                    }
                    _ => {}
                }
            }

            Ok(())
        }
    })
}

/// Projection: print SystemLog events to stdout via tracing.
pub fn system_log_projection() -> Projection<ScoutEngineDeps> {
    project("system_log_stdout").then(move |event: AnyEvent, _ctx: Context<ScoutEngineDeps>| {
        async move {
            if let Some(TelemetryEvent::SystemLog { message, .. }) =
                event.downcast_ref::<TelemetryEvent>()
            {
                tracing::info!(target: "system_log", "{}", message);
            }
            Ok(())
        }
    })
}

/// Projection: persist `ScrapeScheduled` events to the `scheduled_scrapes` table.
pub fn scheduled_scrapes_projection() -> Projection<ScoutEngineDeps> {
    project("scheduled_scrapes_projection").then(
        move |event: AnyEvent, ctx: Context<ScoutEngineDeps>| async move {
            let Some(SchedulingEvent::ScrapeScheduled {
                scope,
                run_after,
                reason,
            }) = event.downcast_ref::<SchedulingEvent>()
            else {
                return Ok(());
            };

            let deps = ctx.deps();
            let Some(pool) = &deps.pg_pool else {
                return Ok(());
            };

            let (scope_type, scope_data) = match scope {
                ScheduledScope::Sources { source_ids } => (
                    "sources",
                    serde_json::to_value(source_ids)
                        .expect("source_ids serialization should never fail"),
                ),
                ScheduledScope::Region { region } => (
                    "region",
                    serde_json::to_value(region)
                        .expect("region serialization should never fail"),
                ),
            };

            // ON CONFLICT DO NOTHING — unique index prevents duplicate pending schedules
            sqlx::query(
                "INSERT INTO scheduled_scrapes (scope_type, scope_data, run_after, reason) \
                 VALUES ($1, $2, $3, $4) \
                 ON CONFLICT DO NOTHING",
            )
            .bind(scope_type)
            .bind(&scope_data)
            .bind(run_after)
            .bind(reason)
            .execute(pool)
            .await?;

            info!(
                scope_type,
                reason,
                run_after = %run_after,
                "Deferred scrape scheduled"
            );

            Ok(())
        },
    )
}

/// Test-only handler: capture every event into a shared Vec for inspection.
///
/// Only registered when `ScoutEngineDeps.captured_events` is Some.
/// Stores raw `AnyEvent`s — test code uses `downcast_ref` to inspect.
pub fn capture_handler(
    sink: Arc<std::sync::Mutex<Vec<AnyEvent>>>,
) -> Handler<ScoutEngineDeps> {
    on_any()
        .id("test_capture")
        .priority(0)
        .then(move |event: AnyEvent, _ctx: Context<ScoutEngineDeps>| {
            let sink = sink.clone();
            async move {
                sink.lock().unwrap().push(event);
                Ok::<Events, anyhow::Error>(events![])
            }
        })
}

