// News scanning domain: global RSS → beacon detection via seesaw.

pub mod activities;
pub mod aggregate;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};

use crate::core::engine::ScoutEngineDeps;
use crate::domains::lifecycle::events::LifecycleEvent;

fn is_news_scan_requested(e: &LifecycleEvent) -> bool {
    matches!(e, LifecycleEvent::NewsScanRequested)
}

#[handlers]
pub mod handlers {
    use super::*;

    /// NewsScanRequested → scan RSS feeds, emit BeaconDetected events.
    #[handle(on = LifecycleEvent, id = "news_scanning:scan", filter = is_news_scan_requested)]
    async fn scan_news(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let mut out = events![];
        activities::scan_news(&deps, &mut out).await;
        Ok(out)
    }
}
