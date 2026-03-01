//! Seesaw handlers for the scrape domain: tension and response scraping.
//!
//! Thin wrappers that delegate to activity functions in `activities.rs`.

use std::sync::Arc;

use seesaw_core::{events, on, Context, Handler};
use tracing::info;

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::{PipelinePhase, ScoutEvent};
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::scrape::activities::{make_run_log, partition_into_events, scrape_response, scrape_tension};
use crate::domains::scrape::activities::scrape_phase::ScrapePhase;

use rootsignal_graph::GraphWriter;

/// SourcesScheduled → scrape tension sources (web + social), emit PhaseCompleted(TensionScrape).
pub fn tension_scrape_handler() -> Handler<ScoutEngineDeps> {
    on::<LifecycleEvent>()
        .id("scrape:tension")
        .filter(|e: &LifecycleEvent| {
            matches!(e, LifecycleEvent::SourcesScheduled { .. })
        })
        .then(
            |_event: Arc<LifecycleEvent>, ctx: Context<ScoutEngineDeps>| async move {
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
                let run_log = make_run_log(&deps.run_id, region_name, deps.pg_pool.as_ref()).await;

                let state = deps.state.read().await;
                let mut output = scrape_tension(&phase, &state, &run_log).await;
                drop(state);

                let events = output.take_events();
                let mut state = deps.state.write().await;
                state.apply_scrape_output(output);
                drop(state);

                Ok(partition_into_events(
                    events,
                    LifecycleEvent::PhaseCompleted {
                        phase: PipelinePhase::TensionScrape,
                    },
                ))
            },
        )
}

/// PhaseCompleted(MidRunDiscovery) → scrape response sources + social + topics,
/// emit PhaseCompleted(ResponseScrape).
pub fn response_scrape_handler() -> Handler<ScoutEngineDeps> {
    on::<LifecycleEvent>()
        .id("scrape:response")
        .filter(|e: &LifecycleEvent| {
            matches!(
                e,
                LifecycleEvent::PhaseCompleted { phase }
                    if matches!(phase, PipelinePhase::MidRunDiscovery)
            )
        })
        .then(
            |_event: Arc<LifecycleEvent>, ctx: Context<ScoutEngineDeps>| async move {
                info!("=== Phase B: Find Responses ===");
                let deps = ctx.deps();

                // Requires region + graph_client — skip in tests
                let (region, graph_client) = match (deps.region.as_ref(), deps.graph_client.as_ref())
                {
                    (Some(r), Some(g)) => (r, g),
                    _ => {
                        return Ok(events![LifecycleEvent::PhaseCompleted {
                            phase: PipelinePhase::ResponseScrape,
                        }]);
                    }
                };
                let writer = GraphWriter::new(graph_client.clone());

                let phase = ScrapePhase::new(
                    deps.store.clone(),
                    deps.extractor.as_ref().expect("extractor set").clone(),
                    deps.embedder.clone(),
                    deps.fetcher.as_ref().expect("fetcher set").clone(),
                    region.clone(),
                    deps.run_id.clone(),
                );

                let run_log =
                    make_run_log(&deps.run_id, &region.name, deps.pg_pool.as_ref()).await;

                // Drain social topics before reading state
                let social_topics = {
                    let mut state = deps.state.write().await;
                    std::mem::take(&mut state.social_topics)
                };

                let state = deps.state.read().await;
                let mut output =
                    scrape_response(&phase, &state, social_topics, &writer, region, &run_log)
                        .await;
                drop(state);

                let events = output.take_events();
                let mut state = deps.state.write().await;
                state.apply_scrape_output(output);
                drop(state);

                Ok(partition_into_events(
                    events,
                    LifecycleEvent::PhaseCompleted {
                        phase: PipelinePhase::ResponseScrape,
                    },
                ))
            },
        )
}
