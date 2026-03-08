pub mod activities;
pub mod events;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::signals::activities::dedup;
use crate::domains::signals::events::SignalEvent;
use crate::domains::scrape::events::ScrapeEvent;

fn is_scrape_completed(e: &ScrapeEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    e.is_completion()
}

#[handlers]
pub mod handlers {
    use super::*;

    /// Scrape completed → run 4-layer dedup on all extracted batches.
    #[handle(on = ScrapeEvent, id = "signals:dedup_signals", filter = is_scrape_completed)]
    async fn dedup_signals(
        event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let extracted_batches = event.into_extracted_batches();

        if extracted_batches.is_empty() {
            return Ok(events![SignalEvent::NoNewSignals]);
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

        // If dedup ran but created zero new signals, emit NoNewSignals
        // so downstream gates see review_complete() as 0==0.
        let world_type = std::any::TypeId::of::<rootsignal_common::events::WorldEvent>();
        let has_world_events = all_events.iter().any(|e| e.type_id == world_type);
        if !has_world_events {
            all_events.push(SignalEvent::NoNewSignals);
        }

        Ok(all_events)
    }
}
