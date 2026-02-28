//! Infrastructure handlers: persist, reduce, project.
//!
//! These run before domain handlers and replicate the old engine's
//! PERSIST → REDUCE → PROJECT flow.

use std::sync::Arc;

use chrono::Utc;
use rootsignal_events::AppendEvent;
use seesaw_core::{on, Context, Handler};

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::ScoutEvent;

/// Priority-0 handler: persist every event to rootsignal's Postgres event store.
///
/// Constructs an `AppendEvent` from the `ScoutEvent` and writes it to
/// `rootsignal_events::EventStore`. Child events get `caused_by` linking
/// via `parent_event_id()` from seesaw's context.
pub fn persist_handler() -> Handler<ScoutEngineDeps> {
    on::<ScoutEvent>()
        .id("persist")
        .priority(0)
        .then(
            |event: Arc<ScoutEvent>, ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();
                if let Some(ref event_store) = deps.event_store {
                    let mut append =
                        AppendEvent::new(event.event_type_str(), event.to_persist_payload())
                            .with_run_id(&deps.run_id)
                            .with_id(ctx.current_event_id());

                    if let Some(parent_id) = ctx.parent_event_id() {
                        append = append.with_parent_id(parent_id);
                    }

                    event_store.append(append).await.map_err(|e| {
                        anyhow::anyhow!("Event persist failed: {e}")
                    })?;
                }

                Ok::<(), anyhow::Error>(())
            },
        )
}

/// Priority-1 handler: apply every event to the shared PipelineState.
///
/// Replaces the old `ScoutReducer::reduce()` call in the dispatch loop.
/// The aggregate's `apply()` method handles all state transitions.
pub fn state_updater() -> Handler<ScoutEngineDeps> {
    on::<ScoutEvent>()
        .id("state_updater")
        .priority(1)
        .then(
            |event: Arc<ScoutEvent>, ctx: Context<ScoutEngineDeps>| async move {
                let mut state = ctx.deps().state.write().await;
                state.apply((*event).clone());
                Ok::<(), anyhow::Error>(())
            },
        )
}

/// Test-only handler: capture every event into a shared Vec for inspection.
///
/// Only registered when `ScoutEngineDeps.captured_events` is Some.
pub fn capture_handler(
    sink: Arc<std::sync::Mutex<Vec<ScoutEvent>>>,
) -> Handler<ScoutEngineDeps> {
    on::<ScoutEvent>()
        .id("test_capture")
        .priority(0) // same as persist — runs first
        .then(move |event: Arc<ScoutEvent>, _ctx: Context<ScoutEngineDeps>| {
            let sink = sink.clone();
            async move {
                sink.lock().unwrap().push((*event).clone());
                Ok::<(), anyhow::Error>(())
            }
        })
}

/// Priority-2 handler: project events to Neo4j graph.
///
/// Only processes projectable events (World, System, and select Pipeline events).
/// Constructs a `rootsignal_events::StoredEvent` for the `GraphProjector`.
pub fn neo4j_handler() -> Handler<ScoutEngineDeps> {
    on::<ScoutEvent>()
        .id("neo4j_projection")
        .priority(2)
        .filter(|e: &ScoutEvent| e.is_projectable())
        .then(
            |event: Arc<ScoutEvent>, ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();
                if let Some(ref projector) = deps.graph_projector {
                    let stored = rootsignal_events::StoredEvent {
                        seq: 0,
                        ts: Utc::now(),
                        event_type: event.event_type_str(),
                        parent_seq: None,
                        caused_by_seq: None,
                        run_id: Some(deps.run_id.clone()),
                        actor: None,
                        payload: event.to_persist_payload(),
                        schema_v: 1,
                        id: Some(ctx.current_event_id()),
                        parent_id: ctx.parent_event_id(),
                    };
                    projector.project(&stored).await?;
                }
                Ok::<(), anyhow::Error>(())
            },
        )
}
