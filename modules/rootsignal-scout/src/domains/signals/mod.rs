pub mod activities;
pub mod events;

use anyhow::Result;
use seesaw_core::{handle, handlers, Context, Events};

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::signals::activities::dedup;
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
}
