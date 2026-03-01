//! Seesaw handlers for the expansion domain.
//!
//! Thin wrapper that delegates to activity functions.

use std::sync::Arc;

use seesaw_core::{events, on, Context, Events, Handler};

use rootsignal_graph::GraphWriter;

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::PipelinePhase;
use crate::domains::expansion::activities;
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

                let expansion = Expansion::new(&writer, &*deps.embedder, &region.name);
                let phase = ScrapePhase::new(
                    deps.store.clone(),
                    deps.extractor.as_ref().expect("extractor set").clone(),
                    deps.embedder.clone(),
                    deps.fetcher.as_ref().expect("fetcher set").clone(),
                    region.clone(),
                    deps.run_id.clone(),
                );

                let state = deps.state.read().await;
                let output = activities::expand_and_discover(
                    &expansion,
                    Some(&phase),
                    &state,
                    &writer,
                    &region.name,
                    deps.anthropic_api_key.as_deref(),
                    budget,
                    &*deps.embedder,
                    &run_log,
                )
                .await;
                drop(state);

                // Apply state updates
                let mut state = deps.state.write().await;
                state.apply_expansion_output(output.expansion);
                if let Some(topic_scrape) = output.topic_scrape {
                    state.apply_scrape_output(topic_scrape);
                }
                drop(state);

                Ok(Events::batch(output.events).add(LifecycleEvent::PhaseCompleted {
                    phase: PipelinePhase::Expansion,
                }))
            },
        )
}
