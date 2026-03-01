// Expansion domain: signal expansion and end-of-run discovery.

pub mod activities;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};

use rootsignal_graph::GraphStore;

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::PipelinePhase;
use crate::domains::expansion::activities::expansion::Expansion;
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::scrape::activities::scrape_phase::ScrapePhase;
use crate::infra::run_log::RunLogger;

fn is_metrics_completed(e: &LifecycleEvent) -> bool {
    matches!(e, LifecycleEvent::MetricsCompleted)
}

#[handlers]
pub mod handlers {
    use super::*;

    /// MetricsCompleted → signal expansion + end-of-run discovery, emit PhaseCompleted(Expansion).
    #[handle(on = LifecycleEvent, id = "expansion:expand", filter = is_metrics_completed)]
    async fn expansion(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
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
        let writer = GraphStore::new(graph_client.clone());

        let run_log = match deps.pg_pool.as_ref() {
            Some(pool) => {
                RunLogger::new(
                    deps.run_id.clone(),
                    region.name.clone(),
                    pool.clone(),
                )
                .await
            }
            None => RunLogger::noop(),
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

        let mut all_events = output.events;
        all_events.push(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::Expansion,
        });
        Ok(all_events)
    }
}
