// Scrape domain: tension and response scrape phase handlers.

pub mod activities;
pub mod events;

#[cfg(test)]
mod boundary_tests;
#[cfg(test)]
mod chain_tests;
#[cfg(test)]
pub mod simweb_adapter;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};
use tracing::info;

use rootsignal_graph::GraphReader;

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::PipelinePhase;
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::scrape::activities::{build_run_logger, scrape_response, scrape_tension};
use crate::domains::scrape::activities::scrape_phase::ScrapePhase;

fn is_sources_scheduled(e: &LifecycleEvent) -> bool {
    matches!(e, LifecycleEvent::SourcesScheduled { .. })
}

fn is_mid_run_discovery_completed(e: &LifecycleEvent) -> bool {
    matches!(
        e,
        LifecycleEvent::PhaseCompleted { phase }
            if matches!(phase, PipelinePhase::MidRunDiscovery)
    )
}

#[handlers]
pub mod handlers {
    use super::*;

    /// SourcesScheduled → scrape tension sources (web + social), emit PhaseCompleted(TensionScrape).
    #[handle(on = LifecycleEvent, id = "scrape:tension", filter = is_sources_scheduled)]
    async fn tension_scrape(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        info!("=== Phase A: Find Problems ===");
        let deps = ctx.deps();

        let phase = ScrapePhase::new(
            deps.store.clone(),
            deps.extractor.as_ref().expect("extractor set").clone(),
            deps.embedder.clone(),
            deps.fetcher.as_ref().expect("fetcher set").clone(),
            deps.region.as_ref().expect("region set").clone(),
            deps.run_id.clone(),
        );

        let region_name = deps.region.as_ref().map(|r| r.name.as_str()).unwrap_or("");
        let run_log = build_run_logger(&deps.run_id, region_name, deps.pg_pool.as_ref()).await;

        let state = deps.state.read().await;
        let mut output = scrape_tension(&phase, &state, &run_log).await;
        drop(state);

        let events = output.take_events();
        let mut state = deps.state.write().await;
        state.apply_scrape_output(output);
        drop(state);

        let mut all_events = events;
        all_events.push(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::TensionScrape,
        });
        Ok(all_events)
    }

    /// PhaseCompleted(MidRunDiscovery) → scrape response sources + social + topics,
    /// emit PhaseCompleted(ResponseScrape).
    #[handle(on = LifecycleEvent, id = "scrape:response", filter = is_mid_run_discovery_completed)]
    async fn response_scrape(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        info!("=== Phase B: Find Responses ===");
        let deps = ctx.deps();

        // Requires region + graph_client — skip in tests
        let (region, graph_client) = match (deps.region.as_ref(), deps.graph_client.as_ref()) {
            (Some(r), Some(g)) => (r, g),
            _ => {
                return Ok(events![LifecycleEvent::PhaseCompleted {
                    phase: PipelinePhase::ResponseScrape,
                }]);
            }
        };
        let graph = GraphReader::new(graph_client.clone());

        let phase = ScrapePhase::new(
            deps.store.clone(),
            deps.extractor.as_ref().expect("extractor set").clone(),
            deps.embedder.clone(),
            deps.fetcher.as_ref().expect("fetcher set").clone(),
            region.clone(),
            deps.run_id.clone(),
        );

        let run_log = build_run_logger(&deps.run_id, &region.name, deps.pg_pool.as_ref()).await;

        // Drain social topics before reading state
        let social_topics = {
            let mut state = deps.state.write().await;
            std::mem::take(&mut state.social_topics)
        };

        let state = deps.state.read().await;
        let mut output =
            scrape_response(&phase, &state, social_topics, &graph, region, &run_log).await;
        drop(state);

        let events = output.take_events();
        let mut state = deps.state.write().await;
        state.apply_scrape_output(output);
        drop(state);

        let mut all_events = events;
        all_events.push(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::ResponseScrape,
        });
        Ok(all_events)
    }
}
