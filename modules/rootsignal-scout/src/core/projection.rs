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
/// Terminal events are domain facts — the projection observes them
/// and writes stats to the scout_runs table.
fn is_terminal_event(event: &AnyEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
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

/// Projection: maintain the `scout_runs` lookup table.
///
/// On `ScoutRunRequested`: INSERT a new row.
/// On terminal events: UPDATE with stats JSONB (finished_at handled by post_settle_cleanup).
pub fn scout_runs_projection() -> Projection<ScoutEngineDeps> {
    project("scout_runs_projection").then(move |event: AnyEvent, ctx: Context<ScoutEngineDeps>| {
        async move {
            // INSERT on ScoutRunRequested
            if let Some(LifecycleEvent::ScoutRunRequested { run_id, scope }) = event.downcast_ref::<LifecycleEvent>() {
                let deps = ctx.deps();
                if let Some(pool) = &deps.pg_pool {
                    let region = scope
                        .region()
                        .map(|r| r.name.as_str())
                        .unwrap_or("unknown");
                    let scope_json = scope
                        .region()
                        .and_then(|r| serde_json::to_value(r).ok());
                    sqlx::query(
                        "INSERT INTO scout_runs (run_id, region, scope, started_at) \
                         VALUES ($1, $2, $3, now()) \
                         ON CONFLICT (run_id) DO NOTHING",
                    )
                    .bind(run_id.to_string())
                    .bind(region)
                    .bind(&scope_json)
                    .execute(pool)
                    .await?;
                }
                return Ok(());
            }

            // UPDATE stats on terminal events
            if is_terminal_event(&event, &ctx) {
                let deps = ctx.deps();

                let spent_cents = if let Some(ref budget) = deps.budget {
                    budget.log_status();
                    budget.total_spent()
                } else {
                    0
                };

                let state = ctx.aggregate::<PipelineState>().curr;
                let mut final_stats = state.stats.clone();
                final_stats.spent_cents = spent_cents;
                info!("{}", final_stats);

                if let Some(pool) = &deps.pg_pool {
                    let stats_json = serde_json::to_value(&final_stats)?;
                    sqlx::query(
                        "UPDATE scout_runs SET stats = $2, spent_cents = $3 WHERE run_id = $1",
                    )
                    .bind(deps.run_id.to_string())
                    .bind(stats_json)
                    .bind(spent_cents as i64)
                    .execute(pool)
                    .await?;
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

