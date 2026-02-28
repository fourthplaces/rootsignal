//! Seesaw handlers for the signals domain.
//!
//! Each handler wraps an existing activity function with seesaw's
//! `on::<ScoutEvent>().extract().then()` pattern.

use seesaw_core::{handler::Emit, on, Context, Handler};
use uuid::Uuid;

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::{PipelineEvent, ScoutEvent};
use crate::pipeline::handlers::{creation, dedup};

/// Wrapper to resolve the Vec<E>/E Emit ambiguity.
/// Returns Emit<ScoutEvent> which has only one Into<Emit<ScoutEvent>> path.
fn batch(events: Vec<ScoutEvent>) -> Emit<ScoutEvent> {
    Emit::Batch(events)
}

/// SignalsExtracted → run 4-layer dedup on the extracted batch.
pub fn dedup_handler() -> Handler<ScoutEngineDeps> {
    on::<ScoutEvent>()
        .id("signals:dedup")
        .extract(|e: &ScoutEvent| match e {
            ScoutEvent::Pipeline(PipelineEvent::SignalsExtracted { url, .. }) => {
                Some(url.clone())
            }
            _ => None,
        })
        .then::<ScoutEngineDeps, _, _, _, _, ScoutEvent>(
            |url: String, ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();
                let state = deps.state.read().await;
                let pipe = deps.pipeline_deps.read().await;
                let pipe = pipe.as_ref().expect("pipeline_deps set by dispatch");
                let events =
                    dedup::handle_signals_extracted(&url, &state, pipe).await?;
                Ok(batch(events))
            },
        )
}

/// NewSignalAccepted → emit World + System + Citation events, trigger wiring.
pub fn create_handler() -> Handler<ScoutEngineDeps> {
    on::<ScoutEvent>()
        .id("signals:create")
        .extract(|e: &ScoutEvent| match e {
            ScoutEvent::Pipeline(PipelineEvent::NewSignalAccepted {
                node_id,
                source_url,
                ..
            }) => Some((*node_id, source_url.clone())),
            _ => None,
        })
        .then::<ScoutEngineDeps, _, _, _, _, ScoutEvent>(
            |(node_id, source_url): (Uuid, String),
             ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();
                let state = deps.state.read().await;
                let pipe = deps.pipeline_deps.read().await;
                let pipe = pipe.as_ref().expect("pipeline_deps set by dispatch");
                let events =
                    creation::handle_create(node_id, &source_url, &state, pipe)
                        .await?;
                Ok(batch(events))
            },
        )
}

/// CrossSourceMatchDetected → emit citation + corroboration + scoring events.
pub fn corroborate_handler() -> Handler<ScoutEngineDeps> {
    on::<ScoutEvent>()
        .id("signals:corroborate")
        .extract(|e: &ScoutEvent| match e {
            ScoutEvent::Pipeline(PipelineEvent::CrossSourceMatchDetected {
                existing_id,
                node_type,
                source_url,
                similarity,
            }) => Some((*existing_id, *node_type, source_url.clone(), *similarity)),
            _ => None,
        })
        .then::<ScoutEngineDeps, _, _, _, _, ScoutEvent>(
            |(existing_id, node_type, source_url, similarity): (
                Uuid,
                rootsignal_common::types::NodeType,
                String,
                f64,
            ),
             ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();
                let pipe = deps.pipeline_deps.read().await;
                let pipe = pipe.as_ref().expect("pipeline_deps set by dispatch");
                let events = creation::handle_corroborate(
                    existing_id,
                    node_type,
                    &source_url,
                    similarity,
                    pipe,
                )
                .await?;
                Ok(batch(events))
            },
        )
}

/// SameSourceReencountered → emit citation + freshness events.
pub fn refresh_handler() -> Handler<ScoutEngineDeps> {
    on::<ScoutEvent>()
        .id("signals:refresh")
        .extract(|e: &ScoutEvent| match e {
            ScoutEvent::Pipeline(PipelineEvent::SameSourceReencountered {
                existing_id,
                node_type,
                source_url,
                ..
            }) => Some((*existing_id, *node_type, source_url.clone())),
            _ => None,
        })
        .then::<ScoutEngineDeps, _, _, _, _, ScoutEvent>(
            |(existing_id, node_type, source_url): (
                Uuid,
                rootsignal_common::types::NodeType,
                String,
            ),
             ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();
                let pipe = deps.pipeline_deps.read().await;
                let pipe = pipe.as_ref().expect("pipeline_deps set by dispatch");
                let events = creation::handle_refresh(
                    existing_id,
                    node_type,
                    &source_url,
                    pipe,
                )
                .await?;
                Ok(batch(events))
            },
        )
}

/// SignalReaderd → wire edges (source, actor, resources, tags).
pub fn signal_stored_handler() -> Handler<ScoutEngineDeps> {
    on::<ScoutEvent>()
        .id("signals:wire_edges")
        .extract(|e: &ScoutEvent| match e {
            ScoutEvent::Pipeline(PipelineEvent::SignalReaderd {
                node_id,
                node_type,
                source_url,
                canonical_key,
            }) => Some((
                *node_id,
                *node_type,
                source_url.clone(),
                canonical_key.clone(),
            )),
            _ => None,
        })
        .then::<ScoutEngineDeps, _, _, _, _, ScoutEvent>(
            |(node_id, node_type, source_url, canonical_key): (
                Uuid,
                rootsignal_common::types::NodeType,
                String,
                String,
            ),
             ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();
                let state = deps.state.read().await;
                let pipe = deps.pipeline_deps.read().await;
                let pipe = pipe.as_ref().expect("pipeline_deps set by dispatch");
                let events = creation::handle_signal_stored(
                    node_id,
                    node_type,
                    &source_url,
                    &canonical_key,
                    &state,
                    pipe,
                )
                .await?;
                Ok(batch(events))
            },
        )
}
