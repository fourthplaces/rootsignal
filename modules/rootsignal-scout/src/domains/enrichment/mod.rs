// Enrichment domain: actor extraction, quality, link promotion.

pub mod activities;
pub mod events;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};

use rootsignal_graph::GraphStore;

use crate::core::engine::ScoutEngineDeps;
use crate::core::events::PipelinePhase;
use crate::domains::enrichment::events::EnrichmentEvent;
use crate::domains::lifecycle::events::LifecycleEvent;

fn is_response_scrape_completed(e: &LifecycleEvent) -> bool {
    matches!(
        e,
        LifecycleEvent::PhaseCompleted { phase }
            if matches!(phase, PipelinePhase::ResponseScrape)
    )
}

fn is_actor_enrichment_completed(e: &LifecycleEvent) -> bool {
    matches!(
        e,
        LifecycleEvent::PhaseCompleted { phase }
            if matches!(phase, PipelinePhase::ActorEnrichment)
    )
}

#[handlers]
pub mod handlers {
    use super::*;

    /// PhaseCompleted(ResponseScrape) → enrich actor locations from signal evidence.
    #[handle(on = LifecycleEvent, id = "enrichment:actor_location", filter = is_response_scrape_completed)]
    async fn actor_location(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();

        let actors = match deps.store.list_all_actors().await {
            Ok(a) => a,
            Err(_) => return Ok(events![]),
        };
        if actors.is_empty() {
            return Ok(events![]);
        }

        let mut all_events =
            activities::actor_location::collect_actor_location_events(&*deps.store, &actors).await;
        let count = all_events.len();
        if count == 0 {
            return Ok(events![]);
        }
        all_events.push(EnrichmentEvent::ActorEnrichmentCompleted {
            actors_updated: count as u32,
        });
        Ok(all_events)
    }

    /// PhaseCompleted(ResponseScrape) → delete pins, actor extraction, embedding + metric enrichment,
    /// emit PhaseCompleted(ActorEnrichment).
    #[handle(on = LifecycleEvent, id = "enrichment:post_scrape", filter = is_response_scrape_completed)]
    async fn signal_enrichment(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();

        // Requires graph_client + region — skip in tests
        let (region, graph_client) = match (deps.region.as_ref(), deps.graph_client.as_ref()) {
            (Some(r), Some(g)) => (r, g),
            _ => {
                return Ok(events![LifecycleEvent::PhaseCompleted {
                    phase: PipelinePhase::ActorEnrichment,
                }]);
            }
        };

        let consumed_pin_ids = {
            let state = deps.state.read().await;
            state
                .scheduled
                .as_ref()
                .map(|s| s.consumed_pin_ids.clone())
                .unwrap_or_default()
        };

        let actor_events = activities::enrich_scraped_signals(
            &*deps.store,
            graph_client,
            region,
            deps.anthropic_api_key.as_deref().unwrap_or(""),
            &*deps.embedder,
            &consumed_pin_ids,
        )
        .await;

        let mut all_events = actor_events;
        all_events.push(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::ActorEnrichment,
        });
        Ok(all_events)
    }

    /// PhaseCompleted(ActorEnrichment) → update source weights/cadence, emit MetricsCompleted.
    #[handle(on = LifecycleEvent, id = "enrichment:metrics", filter = is_actor_enrichment_completed)]
    async fn source_weight(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();

        // Requires graph_client + region — skip in tests
        let (region, graph_client) = match (deps.region.as_ref(), deps.graph_client.as_ref()) {
            (Some(r), Some(g)) => (r, g),
            _ => {
                return Ok(events![LifecycleEvent::MetricsCompleted]);
            }
        };
        let writer = GraphStore::new(graph_client.clone());

        let state = deps.state.read().await;
        let all_sources = state
            .scheduled
            .as_ref()
            .map(|s| s.all_sources.clone())
            .unwrap_or_default();
        let source_signal_counts = state.source_signal_counts.clone();
        let query_api_errors = state.query_api_errors.clone();
        drop(state);

        let metric_events = activities::update_source_weights(
            &writer,
            &region.name,
            &all_sources,
            &source_signal_counts,
            &query_api_errors,
        )
        .await;

        if let Some(ref budget) = deps.budget {
            budget.log_status();
        }

        let mut all_events = metric_events;
        all_events.push(LifecycleEvent::MetricsCompleted);
        Ok(all_events)
    }
}
