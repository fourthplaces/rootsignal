// Signals domain: dedup, creation, wiring.

pub mod activities;
pub mod events;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};
use uuid::Uuid;

use crate::core::aggregate::ExtractedBatch;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::signals::activities::{creation, dedup};
use crate::domains::signals::events::SignalEvent;

#[handlers]
pub mod handlers {
    use super::*;

    /// SignalsExtracted → run 4-layer dedup on the extracted batch.
    #[handle(on = [SignalEvent::SignalsExtracted], id = "signals:dedup", extract(url, batch))]
    async fn dedup(
        url: String,
        batch: Box<ExtractedBatch>,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let state = deps.state.read().await;
        let events = dedup::deduplicate_extracted_batch(&url, &batch, &state, deps).await?;
        Ok(Events::batch(events))
    }

    /// NewSignalAccepted → emit World + System + Citation events, trigger wiring.
    #[handle(on = [SignalEvent::NewSignalAccepted], id = "signals:create", extract(node_id, source_url))]
    async fn signal_creation(
        node_id: Uuid,
        source_url: String,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let state = deps.state.read().await;
        creation::emit_new_signal_events(node_id, &source_url, &state, deps).await
    }

    /// CrossSourceMatchDetected → emit citation + corroboration + scoring events.
    #[handle(on = [SignalEvent::CrossSourceMatchDetected], id = "signals:corroborate", extract(existing_id, node_type, source_url, similarity))]
    async fn corroborate(
        existing_id: Uuid,
        node_type: rootsignal_common::types::NodeType,
        source_url: String,
        similarity: f64,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        creation::emit_corroboration_events(existing_id, node_type, &source_url, similarity, deps)
            .await
    }

    /// SameSourceReencountered → emit citation + freshness events.
    #[handle(on = [SignalEvent::SameSourceReencountered], id = "signals:refresh", extract(existing_id, node_type, source_url))]
    async fn refresh(
        existing_id: Uuid,
        node_type: rootsignal_common::types::NodeType,
        source_url: String,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        creation::emit_freshness_events(existing_id, node_type, &source_url, deps).await
    }

    /// SignalCreated → wire edges (source, actor, resources, tags).
    #[handle(on = [SignalEvent::SignalCreated], id = "signals:wire_edges", extract(node_id, node_type, source_url, canonical_key))]
    async fn wire_signal_edges(
        node_id: Uuid,
        node_type: rootsignal_common::types::NodeType,
        source_url: String,
        canonical_key: String,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let state = deps.state.read().await;
        creation::wire_signal_edges(node_id, node_type, &source_url, &canonical_key, &state, deps)
            .await
    }
}
