//! Seesaw handlers for the signals domain.
//!
//! Each handler wraps an existing activity function with seesaw's
//! `on::<SignalEvent>().extract().then()` pattern.

use std::sync::Arc;

use seesaw_core::{events, on, Context, Events, Handler};
use uuid::Uuid;

use crate::core::aggregate::ExtractedBatch;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::signals::events::SignalEvent;
use crate::domains::signals::activities::{creation, dedup};

/// SignalsExtracted → run 4-layer dedup on the extracted batch.
pub fn dedup_handler() -> Handler<ScoutEngineDeps> {
    on::<SignalEvent>()
        .id("signals:dedup")
        .extract(|e: &SignalEvent| match e {
            SignalEvent::SignalsExtracted { url, batch, .. } => {
                Some((url.clone(), batch.clone()))
            }
            _ => None,
        })
        .then(
            |(url, batch): (String, Box<ExtractedBatch>),
             ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();
                let state = deps.state.read().await;
                let events =
                    dedup::handle_signals_extracted(&url, &batch, &state, deps).await?;
                Ok(Events::batch(events))
            },
        )
}

/// NewSignalAccepted → emit World + System + Citation events, trigger wiring.
pub fn create_handler() -> Handler<ScoutEngineDeps> {
    on::<SignalEvent>()
        .id("signals:create")
        .extract(|e: &SignalEvent| match e {
            SignalEvent::NewSignalAccepted {
                node_id,
                source_url,
                ..
            } => Some((*node_id, source_url.clone())),
            _ => None,
        })
        .then(
            |(node_id, source_url): (Uuid, String),
             ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();
                let state = deps.state.read().await;
                creation::handle_create(node_id, &source_url, &state, deps).await
            },
        )
}

/// CrossSourceMatchDetected → emit citation + corroboration + scoring events.
pub fn corroborate_handler() -> Handler<ScoutEngineDeps> {
    on::<SignalEvent>()
        .id("signals:corroborate")
        .extract(|e: &SignalEvent| match e {
            SignalEvent::CrossSourceMatchDetected {
                existing_id,
                node_type,
                source_url,
                similarity,
            } => Some((*existing_id, *node_type, source_url.clone(), *similarity)),
            _ => None,
        })
        .then(
            |(existing_id, node_type, source_url, similarity): (
                Uuid,
                rootsignal_common::types::NodeType,
                String,
                f64,
            ),
             ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();
                creation::handle_corroborate(
                    existing_id,
                    node_type,
                    &source_url,
                    similarity,
                    deps,
                )
                .await
            },
        )
}

/// SameSourceReencountered → emit citation + freshness events.
pub fn refresh_handler() -> Handler<ScoutEngineDeps> {
    on::<SignalEvent>()
        .id("signals:refresh")
        .extract(|e: &SignalEvent| match e {
            SignalEvent::SameSourceReencountered {
                existing_id,
                node_type,
                source_url,
                ..
            } => Some((*existing_id, *node_type, source_url.clone())),
            _ => None,
        })
        .then(
            |(existing_id, node_type, source_url): (
                Uuid,
                rootsignal_common::types::NodeType,
                String,
            ),
             ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();
                creation::handle_refresh(
                    existing_id,
                    node_type,
                    &source_url,
                    deps,
                )
                .await
            },
        )
}

/// SignalCreated → wire edges (source, actor, resources, tags).
pub fn signal_created_handler() -> Handler<ScoutEngineDeps> {
    on::<SignalEvent>()
        .id("signals:wire_edges")
        .extract(|e: &SignalEvent| match e {
            SignalEvent::SignalCreated {
                node_id,
                node_type,
                source_url,
                canonical_key,
            } => Some((
                *node_id,
                *node_type,
                source_url.clone(),
                canonical_key.clone(),
            )),
            _ => None,
        })
        .then(
            |(node_id, node_type, source_url, canonical_key): (
                Uuid,
                rootsignal_common::types::NodeType,
                String,
                String,
            ),
             ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();
                let state = deps.state.read().await;
                creation::handle_signal_stored(
                    node_id,
                    node_type,
                    &source_url,
                    &canonical_key,
                    &state,
                    deps,
                )
                .await
            },
        )
}
