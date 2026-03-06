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

use rootsignal_common::events::{Event, EventDomain, Eventlike, SystemEvent, WorldEvent};
use rootsignal_common::telemetry_events::TelemetryEvent;

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::core::pipeline_events::PipelineEvent;
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::enrichment::events::EnrichmentEvent;
use crate::domains::expansion::events::ExpansionEvent;
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::scrape::events::ScrapeEvent;
use crate::domains::signals::events::SignalEvent;
use crate::domains::situation_weaving::events::SituationWeavingEvent;
use crate::domains::supervisor::events::SupervisorEvent;
use crate::domains::synthesis::events::{all_synthesis_roles, SynthesisEvent};

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
                }

                let (event_type, payload) = match (event_type, payload) {
                    (Some(t), Some(p)) => (t, p),
                    _ => return Ok(events![]),
                };

                let deps = ctx.deps();
                let stored = rootsignal_events::StoredEvent {
                    seq: 0,
                    ts: Utc::now(),
                    event_type,
                    parent_seq: None,
                    caused_by_seq: None,
                    run_id: Some(deps.run_id.to_string()),
                    actor: None,
                    payload,
                    schema_v: 1,
                    id: Some(ctx.current_event_id()),
                    parent_id: ctx.parent_event_id(),
                    correlation_id: None,
                    aggregate_type: None,
                    aggregate_id: None,
                    handler_id: None,
                };
                projector.project(&stored).await?;
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
    if let Some(e) = event.downcast_ref::<WorldEvent>() {
        (
            EventDomain::Fact,
            Some(e.event_type().to_string()),
            Some(Event::World(e.clone()).to_payload()),
        )
    } else if let Some(e) = event.downcast_ref::<SystemEvent>() {
        (
            EventDomain::Fact,
            Some(e.event_type().to_string()),
            Some(Event::System(e.clone()).to_payload()),
        )
    } else if let Some(e) = event.downcast_ref::<TelemetryEvent>() {
        (
            EventDomain::Fact,
            Some(e.event_type().to_string()),
            Some(Event::Telemetry(e.clone()).to_payload()),
        )
    } else if let Some(e) = event.downcast_ref::<DiscoveryEvent>() {
        if e.is_projectable() {
            (EventDomain::Discovery, Some(e.event_type_str()), Some(e.to_persist_payload()))
        } else {
            (EventDomain::Discovery, None, None)
        }
    } else if let Some(e) = event.downcast_ref::<PipelineEvent>() {
        if e.is_projectable() {
            (EventDomain::Pipeline, Some(e.event_type_str()), Some(e.to_persist_payload()))
        } else {
            (EventDomain::Pipeline, None, None)
        }
    } else if event.downcast_ref::<ScrapeEvent>().is_some() {
        (EventDomain::Scrape, None, None)
    } else if event.downcast_ref::<SignalEvent>().is_some() {
        (EventDomain::Signal, None, None)
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
    } else {
        // Genuinely unknown event type — log at debug, not warn.
        // If this fires frequently, a new event type needs adding above.
        tracing::debug!("neo4j_projection: unrecognized event type (not in any known domain)");
        (EventDomain::Fact, None, None)
    }
}

/// Detect terminal events that mean a run is finished.
///
/// Terminal events are domain facts — the projection observes them
/// and writes stats to the scout_runs table.
fn is_terminal_event(event: &AnyEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    if event.downcast_ref::<ScrapeEvent>()
        .is_some_and(|e| matches!(e, ScrapeEvent::ResponseScrapeSkipped { .. }))
    { return true; }

    if event.downcast_ref::<SynthesisEvent>()
        .is_some_and(|e| matches!(e, SynthesisEvent::SynthesisRoleCompleted { .. }))
    {
        let (_, state) = ctx.singleton::<PipelineState>();
        if state.completed_synthesis_roles.is_superset(&all_synthesis_roles()) {
            return true;
        }
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

                if let Some(ref budget) = deps.budget {
                    budget.log_status();
                }

                let (_, state) = ctx.singleton::<PipelineState>();
                info!("{}", state.stats);

                if let Some(pool) = &deps.pg_pool {
                    let stats_json = serde_json::to_value(&state.stats)?;
                    sqlx::query(
                        "UPDATE scout_runs SET stats = $2 WHERE run_id = $1",
                    )
                    .bind(deps.run_id.to_string())
                    .bind(stats_json)
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
