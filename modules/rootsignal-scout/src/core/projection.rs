//! Infrastructure handlers: persist, reduce, project.
//!
//! These run before domain handlers and replicate the old engine's
//! PERSIST → REDUCE → PROJECT flow.
//!
//! All four use `on_any()` so they fire for every event type —
//! both the legacy `ScoutEvent` and per-domain events as they're introduced.

use std::sync::Arc;

use chrono::Utc;
use rootsignal_events::AppendEvent;
use seesaw_core::{events, on_any, AnyEvent, Context, Events, Handler};

use rootsignal_common::events::{Event, Eventlike, SystemEvent, WorldEvent};

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::ScoutEvent;
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::enrichment::events::EnrichmentEvent;
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::signals::events::SignalEvent;

/// Priority-0 handler: persist every event to rootsignal's Postgres event store.
///
/// Constructs an `AppendEvent` from the event and writes it to
/// `rootsignal_events::EventStore`. Child events get `caused_by` linking
/// via `parent_event_id()` from seesaw's context.
pub fn persist_handler() -> Handler<ScoutEngineDeps> {
    on_any()
        .id("persist")
        .priority(0)
        .then(
            |event: AnyEvent, ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();
                if let Some(ref event_store) = deps.event_store {
                    // Downcast to known event types for persistence
                    let (event_type, payload) =
                        if let Some(e) = event.downcast_ref::<ScoutEvent>() {
                            (e.event_type_str(), e.to_persist_payload())
                        } else if let Some(e) = event.downcast_ref::<LifecycleEvent>() {
                            (e.event_type_str(), e.to_persist_payload())
                        } else if let Some(e) = event.downcast_ref::<SignalEvent>() {
                            (e.event_type_str(), e.to_persist_payload())
                        } else if let Some(e) = event.downcast_ref::<DiscoveryEvent>() {
                            (e.event_type_str(), e.to_persist_payload())
                        } else if let Some(e) = event.downcast_ref::<EnrichmentEvent>() {
                            (e.event_type_str(), e.to_persist_payload())
                        } else if let Some(e) = event.downcast_ref::<WorldEvent>() {
                            (e.event_type().to_string(), Event::World(e.clone()).to_payload())
                        } else if let Some(e) = event.downcast_ref::<SystemEvent>() {
                            (e.event_type().to_string(), Event::System(e.clone()).to_payload())
                        } else {
                            // Unknown event type — skip persistence
                            return Ok::<Events, anyhow::Error>(events![]);
                        };

                    let mut append = AppendEvent::new(event_type, payload)
                        .with_run_id(&deps.run_id)
                        .with_id(ctx.current_event_id());

                    if let Some(parent_id) = ctx.parent_event_id() {
                        append = append.with_parent_id(parent_id);
                    }

                    event_store.append(append).await.map_err(|e| {
                        anyhow::anyhow!("Event persist failed: {e}")
                    })?;
                }

                Ok::<Events, anyhow::Error>(events![])
            },
        )
}

/// Priority-1 handler: apply every event to the shared PipelineState.
///
/// Replaces the old `ScoutReducer::reduce()` call in the dispatch loop.
/// The aggregate's `apply()` method handles all state transitions.
pub fn apply_to_aggregate_handler() -> Handler<ScoutEngineDeps> {
    on_any()
        .id("state_updater")
        .priority(1)
        .then(
            |event: AnyEvent, ctx: Context<ScoutEngineDeps>| async move {
                if let Some(e) = event.downcast_ref::<ScoutEvent>() {
                    let mut state = ctx.deps().state.write().await;
                    state.apply(e.clone());
                } else if let Some(e) = event.downcast_ref::<SignalEvent>() {
                    let mut state = ctx.deps().state.write().await;
                    state.apply_signal(e);
                } else if let Some(e) = event.downcast_ref::<DiscoveryEvent>() {
                    let mut state = ctx.deps().state.write().await;
                    state.apply_discovery(e);
                }
                // LifecycleEvent and EnrichmentEvent are no-ops for aggregate state.
                Ok::<Events, anyhow::Error>(events![])
            },
        )
}

/// Test-only handler: capture every event into a shared Vec for inspection.
///
/// Only registered when `ScoutEngineDeps.captured_events` is Some.
/// Wraps per-domain events in ScoutEvent variants so test assertions
/// can use a single `event_type_str()` path.
pub fn capture_handler(
    sink: Arc<std::sync::Mutex<Vec<ScoutEvent>>>,
) -> Handler<ScoutEngineDeps> {
    on_any()
        .id("test_capture")
        .priority(0) // same as persist — runs first
        .then(move |event: AnyEvent, _ctx: Context<ScoutEngineDeps>| {
            let sink = sink.clone();
            async move {
                if let Some(e) = event.downcast_ref::<ScoutEvent>() {
                    sink.lock().unwrap().push(e.clone());
                } else if let Some(e) = event.downcast_ref::<LifecycleEvent>() {
                    sink.lock().unwrap().push(ScoutEvent::Lifecycle(e.clone()));
                } else if let Some(e) = event.downcast_ref::<SignalEvent>() {
                    sink.lock().unwrap().push(ScoutEvent::Signal(e.clone()));
                } else if let Some(e) = event.downcast_ref::<DiscoveryEvent>() {
                    sink.lock().unwrap().push(ScoutEvent::Discovery(e.clone()));
                } else if let Some(e) = event.downcast_ref::<EnrichmentEvent>() {
                    sink.lock().unwrap().push(ScoutEvent::Enrichment(e.clone()));
                } else if let Some(e) = event.downcast_ref::<WorldEvent>() {
                    sink.lock().unwrap().push(ScoutEvent::World(e.clone()));
                } else if let Some(e) = event.downcast_ref::<SystemEvent>() {
                    sink.lock().unwrap().push(ScoutEvent::System(e.clone()));
                }
                Ok::<Events, anyhow::Error>(events![])
            }
        })
}

/// Priority-2 handler: project events to Neo4j graph.
///
/// Only processes projectable events (World, System, and select Pipeline events).
/// Constructs a `rootsignal_events::StoredEvent` for the `GraphProjector`.
pub fn project_to_graph_handler() -> Handler<ScoutEngineDeps> {
    on_any()
        .id("neo4j_projection")
        .priority(2)
        .then(
            |event: AnyEvent, ctx: Context<ScoutEngineDeps>| async move {
                // Check event projectability — World/System always project,
                // ScoutEvent/DiscoveryEvent check is_projectable().
                let (event_type, payload) =
                    if let Some(e) = event.downcast_ref::<WorldEvent>() {
                        (e.event_type().to_string(), Event::World(e.clone()).to_payload())
                    } else if let Some(e) = event.downcast_ref::<SystemEvent>() {
                        (e.event_type().to_string(), Event::System(e.clone()).to_payload())
                    } else if let Some(e) = event.downcast_ref::<ScoutEvent>() {
                        if !e.is_projectable() {
                            return Ok::<Events, anyhow::Error>(events![]);
                        }
                        (e.event_type_str(), e.to_persist_payload())
                    } else if let Some(e) = event.downcast_ref::<DiscoveryEvent>() {
                        if !e.is_projectable() {
                            return Ok::<Events, anyhow::Error>(events![]);
                        }
                        (e.event_type_str(), e.to_persist_payload())
                    } else {
                        // LifecycleEvent, SignalEvent, EnrichmentEvent are not projectable
                        return Ok::<Events, anyhow::Error>(events![]);
                    };

                let deps = ctx.deps();
                if let Some(ref projector) = deps.graph_projector {
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
                    };
                    projector.project(&stored).await?;
                }
                Ok::<Events, anyhow::Error>(events![])
            },
        )
}
