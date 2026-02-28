//! Seesaw handlers for the expansion domain.

use std::sync::Arc;

use seesaw_core::{events, on, Context, Events, Handler};
use tracing::info;

use rootsignal_graph::GraphWriter;

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::PipelinePhase;
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::pipeline::expansion::Expansion;
use crate::pipeline::scrape_phase::ScrapePhase;

/// MetricsCompleted → signal expansion + end-of-run discovery, emit PhaseCompleted(Expansion).
pub fn expansion_handler() -> Handler<ScoutEngineDeps> {
    on::<LifecycleEvent>()
        .id("expansion:expand")
        .filter(|e: &LifecycleEvent| {
            matches!(e, LifecycleEvent::MetricsCompleted)
        })
        .then(
            |_event: Arc<LifecycleEvent>, ctx: Context<ScoutEngineDeps>| async move {
                let deps = ctx.deps();

                // Requires region + graph_client + budget — skip in tests
                let (region, graph_client, budget) = match (
                    deps.region.as_ref(),
                    deps.graph_client.as_ref(),
                    deps.budget.as_ref(),
                ) {
                    (Some(r), Some(g), Some(b)) => (r, g, b),
                    _ => {
                        return Ok(events![LifecycleEvent::PhaseCompleted {
                            phase: PipelinePhase::Expansion,
                        }]);
                    }
                };
                let writer = GraphWriter::new(graph_client.clone());

                let run_log = match deps.pg_pool.as_ref() {
                    Some(pool) => {
                        crate::infra::run_log::RunLogger::new(
                            deps.run_id.clone(),
                            region.name.clone(),
                            pool.clone(),
                        )
                        .await
                    }
                    None => crate::infra::run_log::RunLogger::noop(),
                };

                let mut state = std::mem::take(&mut *deps.state.write().await);

                // Signal expansion — create sources from implied queries
                let expansion = Expansion::new(&writer, &*deps.embedder, &region.name);
                let expansion_sources = expansion.run(&mut state, &run_log).await;

                let mut collected_events = Vec::new();
                if !expansion_sources.is_empty() {
                    collected_events.extend(ScrapePhase::register_sources_events(
                        expansion_sources,
                        "signal_expansion",
                    ));
                }

                // End-of-run discovery
                let end_discoverer = crate::discovery::source_finder::SourceFinder::new(
                    &writer,
                    &region.name,
                    &region.name,
                    deps.anthropic_api_key.as_deref(),
                    budget,
                )
                .with_embedder(&*deps.embedder);
                let (end_stats, end_social_topics, end_sources) = end_discoverer.run().await;
                if !end_sources.is_empty() {
                    collected_events.extend(ScrapePhase::register_sources_events(
                        end_sources,
                        "source_finder",
                    ));
                }
                if end_stats.actor_sources + end_stats.gap_sources > 0 {
                    info!("{end_stats}");
                }

                // End-of-run topic discovery
                if !end_social_topics.is_empty() {
                    info!(
                        count = end_social_topics.len(),
                        "Consuming end-of-run social topics"
                    );
                    let phase = ScrapePhase::new(
                        deps.store.clone(),
                        deps.extractor.as_ref().expect("extractor set").clone(),
                        deps.embedder.clone(),
                        deps.fetcher.as_ref().expect("fetcher set").clone(),
                        region.clone(),
                        deps.run_id.clone(),
                    );
                    let topic_events = phase
                        .discover_from_topics(&end_social_topics, &mut state, &run_log)
                        .await;
                    collected_events.extend(topic_events);
                }

                // Put state back
                *deps.state.write().await = state;

                Ok(Events::batch(collected_events).add(LifecycleEvent::PhaseCompleted {
                    phase: PipelinePhase::Expansion,
                }))
            },
        )
}
