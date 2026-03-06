pub mod activities;
pub mod events;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};
use uuid::Uuid;

use rootsignal_common::types::NodeType;

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::signals::activities::{creation, dedup};
use crate::domains::signals::events::SignalEvent;
use crate::domains::scrape::events::ScrapeEvent;

fn is_scrape_completed(e: &ScrapeEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    e.completed_role().is_some()
}

#[handlers]
pub mod handlers {
    use super::*;

    /// Scrape completed → run 4-layer dedup on all extracted batches.
    #[handle(on = ScrapeEvent, id = "signals:dedup", filter = is_scrape_completed)]
    async fn dedup(
        event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let extracted_batches = event.into_extracted_batches();

        if extracted_batches.is_empty() {
            return Ok(Events::new());
        }

        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();
        let mut all_events = Events::new();

        for extraction in &extracted_batches {
            let events = dedup::deduplicate_extracted_batch(
                &extraction.url,
                &extraction.canonical_key,
                &extraction.batch,
                &state,
                deps,
            ).await?;
            all_events.extend(events);
        }

        Ok(all_events)
    }

    /// SignalCreated → wire edges (source, actor, resources, tags).
    #[handle(on = [SignalEvent::SignalCreated], id = "signals:wire_edges", extract(node_id, node_type, source_url, canonical_key))]
    async fn wire_signal_edges(
        node_id: Uuid,
        node_type: NodeType,
        source_url: String,
        canonical_key: String,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();
        creation::wire_signal_edges(node_id, node_type, &source_url, &canonical_key, &state, deps)
            .await
    }
}
