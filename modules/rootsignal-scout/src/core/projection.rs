//! Infrastructure handlers: project events to Neo4j, maintain scout_runs table.
//!
//! Persistence is handled by seesaw's built-in `persist_and_hydrate` via
//! the `SeesawEventStoreAdapter`. Aggregator state is handled by seesaw's
//! registered aggregators (priority 1).
//!
//! Priority-2 handlers here:
//! - `neo4j_projection_handler` — project events to Neo4j graph
//! - `scout_runs_handler` — maintain the scout_runs lookup table

use std::sync::Arc;

use chrono::Utc;
use rootsignal_graph::GraphProjector;
use seesaw_core::{events, on_any, AnyEvent, Context, Events, Handler};

use rootsignal_common::events::{Event, Eventlike, SystemEvent, WorldEvent};
use rootsignal_common::telemetry_events::TelemetryEvent;

use crate::core::engine::ScoutEngineDeps;
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::lifecycle::events::LifecycleEvent;

// Priority-0: event persistence — handled by seesaw's persist_and_hydrate + SeesawEventStoreAdapter.
// Priority-1: aggregate state — handled by seesaw aggregators.

/// Priority-2 handler: project events to Neo4j graph.
///
/// Captures `GraphProjector` via closure — not on `ScoutEngineDeps`.
/// Only processes projectable events (World, System, and select Discovery events).
pub fn neo4j_projection_handler(projector: GraphProjector) -> Handler<ScoutEngineDeps> {
    let projector = Arc::new(projector);
    on_any()
        .id("neo4j_projection")
        .priority(2)
        .then(move |event: AnyEvent, ctx: Context<ScoutEngineDeps>| {
            let projector = projector.clone();
            async move {
                let (event_type, payload) =
                    if let Some(e) = event.downcast_ref::<WorldEvent>() {
                        (e.event_type().to_string(), Event::World(e.clone()).to_payload())
                    } else if let Some(e) = event.downcast_ref::<SystemEvent>() {
                        (e.event_type().to_string(), Event::System(e.clone()).to_payload())
                    } else if let Some(e) = event.downcast_ref::<DiscoveryEvent>() {
                        if !e.is_projectable() {
                            return Ok(events![]);
                        }
                        (e.event_type_str(), e.to_persist_payload())
                    } else {
                        return Ok(events![]);
                    };

                let deps = ctx.deps();
                let stored = rootsignal_events::StoredEvent {
                    seq: 0,
                    ts: Utc::now(),
                    event_type,
                    parent_seq: None,
                    caused_by_seq: None,
                    run_id: Some(deps.run_id.clone()),
                    actor: None,
                    payload,
                    schema_v: 1,
                    id: Some(ctx.current_event_id()),
                    parent_id: ctx.parent_event_id(),
                    correlation_id: None,
                    aggregate_type: None,
                    aggregate_id: None,
                };
                projector.project(&stored).await?;
                Ok(events![])
            }
        })
}

/// Priority-2 handler: maintain the `scout_runs` lookup table.
///
/// On `EngineStarted`: INSERT a new row.
/// On `RunCompleted`: UPDATE with finished_at and stats JSONB.
pub fn scout_runs_handler() -> Handler<ScoutEngineDeps> {
    on_any()
        .id("scout_runs_projection")
        .priority(2)
        .then(move |event: AnyEvent, ctx: Context<ScoutEngineDeps>| {
            async move {
                let Some(lifecycle) = event.downcast_ref::<LifecycleEvent>() else {
                    return Ok(events![]);
                };

                let deps = ctx.deps();
                let Some(pool) = &deps.pg_pool else {
                    return Ok(events![]);
                };

                match lifecycle {
                    LifecycleEvent::EngineStarted { run_id } => {
                        let region = deps
                            .region
                            .as_ref()
                            .map(|r| r.name.as_str())
                            .unwrap_or("unknown");
                        sqlx::query(
                            "INSERT INTO scout_runs (run_id, region, started_at) \
                             VALUES ($1, $2, now()) \
                             ON CONFLICT (run_id) DO NOTHING",
                        )
                        .bind(run_id)
                        .bind(region)
                        .execute(pool)
                        .await?;
                    }
                    LifecycleEvent::RunCompleted { stats } => {
                        let stats_json = serde_json::to_value(stats)?;
                        sqlx::query(
                            "UPDATE scout_runs SET finished_at = now(), stats = $2 \
                             WHERE run_id = $1",
                        )
                        .bind(&deps.run_id)
                        .bind(stats_json)
                        .execute(pool)
                        .await?;
                    }
                    _ => {}
                }

                Ok(events![])
            }
        })
}

/// Priority-2 handler: print SystemLog events to stdout via tracing.
pub fn system_log_handler() -> Handler<ScoutEngineDeps> {
    on_any()
        .id("system_log_stdout")
        .priority(2)
        .then(move |event: AnyEvent, _ctx: Context<ScoutEngineDeps>| {
            async move {
                if let Some(TelemetryEvent::SystemLog { message, .. }) =
                    event.downcast_ref::<TelemetryEvent>()
                {
                    tracing::info!(target: "system_log", "{}", message);
                }
                Ok(events![])
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
