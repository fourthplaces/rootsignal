//! Infrastructure handlers: persist, reduce, project.
//!
//! These run before domain handlers and replicate the old engine's
//! PERSIST → REDUCE → PROJECT flow.
//!
//! All use `on_any` so they fire for every event type.

use std::sync::Arc;

use chrono::Utc;
use rootsignal_events::AppendEvent;
use seesaw_core::{events, handle, handlers, on_any, AnyEvent, Context, Events, Handler};

use rootsignal_common::events::{Event, Eventlike, SystemEvent, WorldEvent};

use crate::core::engine::ScoutEngineDeps;
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::enrichment::events::EnrichmentEvent;
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::scrape::events::ScrapeEvent;
use crate::domains::signals::events::SignalEvent;

#[handlers]
pub mod handlers {
    use super::*;

    /// Priority-0 handler: persist every event to rootsignal's Postgres event store.
    ///
    /// Constructs an `AppendEvent` from the event and writes it to
    /// `rootsignal_events::EventStore`. Child events get `caused_by` linking
    /// via `parent_event_id()` from seesaw's context.
    #[handle(on_any, id = "persist", priority = 0)]
    async fn persist(event: AnyEvent, ctx: Context<ScoutEngineDeps>) -> anyhow::Result<Events> {
        let deps = ctx.deps();
        if let Some(ref event_store) = deps.event_store {
            // Downcast to known event types for persistence
            let (event_type, payload) =
                if let Some(e) = event.downcast_ref::<LifecycleEvent>() {
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
                    return Ok(events![]);
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

        Ok(events![])
    }

    /// Priority-1 handler: apply every event to the shared PipelineState.
    ///
    /// Replaces the old `ScoutReducer::reduce()` call in the dispatch loop.
    /// The aggregate's `apply()` method handles all state transitions.
    #[handle(on_any, id = "state_updater", priority = 1)]
    async fn apply_to_aggregate(
        event: AnyEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> anyhow::Result<()> {
        if let Some(e) = event.downcast_ref::<SignalEvent>() {
            let mut state = ctx.deps().state.write().await;
            state.apply_signal(e);
        } else if let Some(e) = event.downcast_ref::<DiscoveryEvent>() {
            let mut state = ctx.deps().state.write().await;
            state.apply_discovery(e);
        } else if let Some(e) = event.downcast_ref::<ScrapeEvent>() {
            let mut state = ctx.deps().state.write().await;
            state.apply_scrape(e);
        }
        // LifecycleEvent and EnrichmentEvent are no-ops for aggregate state.
        Ok(())
    }

    /// Priority-2 handler: project events to Neo4j graph.
    ///
    /// Only processes projectable events (World, System, and select Pipeline events).
    /// Constructs a `rootsignal_events::StoredEvent` for the `GraphProjector`.
    #[handle(on_any, id = "neo4j_projection", priority = 2)]
    async fn project_to_graph(
        event: AnyEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> anyhow::Result<Events> {
        // Check event projectability — World/System always project,
        // DiscoveryEvent checks is_projectable().
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
                // LifecycleEvent, SignalEvent, EnrichmentEvent are not projectable
                return Ok(events![]);
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
        Ok(events![])
    }
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
        .priority(0) // same as persist — runs first
        .then(move |event: AnyEvent, _ctx: Context<ScoutEngineDeps>| {
            let sink = sink.clone();
            async move {
                sink.lock().unwrap().push(event);
                Ok::<Events, anyhow::Error>(events![])
            }
        })
}
