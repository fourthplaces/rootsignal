// News scanning domain: global RSS signal extraction via seesaw.

pub mod activities;

use anyhow::Result;
use causal::{events, reactor, reactors, Context, Events};

use crate::core::engine::ScoutEngineDeps;
use crate::domains::lifecycle::events::LifecycleEvent;

fn is_news_scan_requested(e: &LifecycleEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, LifecycleEvent::NewsScanRequested)
}

#[reactors]
pub mod reactors {
    use super::*;

    /// NewsScanRequested → scan RSS feeds for signals.
    #[reactor(on = LifecycleEvent, id = "news_scanning:scan_news", filter = is_news_scan_requested)]
    async fn scan_news(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let mut out = events![];
        activities::scan_news(&deps, deps.daily_budget_cents, &mut out).await;
        Ok(out)
    }
}
