pub mod activities;
pub mod events;

#[cfg(test)]
mod completion_tests;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};
use tracing::info;



use rootsignal_common::{Block, ChecklistItem};

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::enrichment::events::EnrichmentEvent;
use crate::domains::scrape::events::ScrapeEvent;

// ── Enrichment filters: response scrape done + own fact not yet recorded ──

fn response_done_actor_extraction_pending(e: &ScrapeEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    if !e.is_completion() { return false; }
    let (_, state) = ctx.singleton::<PipelineState>();
    state.response_scrape_done() && !state.actors_extracted
}

fn response_done_diversity_pending(e: &ScrapeEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    if !e.is_completion() { return false; }
    let (_, state) = ctx.singleton::<PipelineState>();
    state.response_scrape_done() && !state.diversity_scored
}

fn response_done_actor_stats_pending(e: &ScrapeEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    if !e.is_completion() { return false; }
    let (_, state) = ctx.singleton::<PipelineState>();
    state.response_scrape_done() && !state.actor_stats_computed
}

fn response_done_actor_location_pending(e: &ScrapeEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    if !e.is_completion() { return false; }
    let (_, state) = ctx.singleton::<PipelineState>();
    state.response_scrape_done() && !state.actors_located
}

fn describe_enrichment_gate(ctx: &Context<ScoutEngineDeps>) -> Vec<Block> {
    let (_, state) = ctx.singleton::<PipelineState>();
    vec![
        Block::Checklist {
            label: "Response scrape".into(),
            items: vec![
                ChecklistItem { text: "Web".into(), done: state.response_web_done },
                ChecklistItem { text: "Social".into(), done: state.response_social_done },
                ChecklistItem { text: "Topics".into(), done: state.topic_discovery_done },
            ],
        },
        Block::Checklist {
            label: "Enrichment".into(),
            items: vec![
                ChecklistItem { text: "Actors extracted".into(), done: state.actors_extracted },
                ChecklistItem { text: "Diversity scored".into(), done: state.diversity_scored },
                ChecklistItem { text: "Actor stats computed".into(), done: state.actor_stats_computed },
                ChecklistItem { text: "Actors located".into(), done: state.actors_located },
            ],
        },
    ]
}

#[handlers]
pub mod handlers {
    use super::*;

    // ---------------------------------------------------------------
    // Enrichment handlers: each listens for scrape completion + state gate
    // ---------------------------------------------------------------

    #[handle(on = ScrapeEvent, id = "enrichment:extract_actors", filter = response_done_actor_extraction_pending, describe = describe_enrichment_gate)]
    async fn extract_actors(
        _event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();

        let (region, graph) = match (state.run_scope.region(), deps.graph.as_ref()) {
            (Some(r), Some(g)) => (r, g),
            _ => {
                ctx.logger.debug("Skipped actor extraction: missing region or graph");
                return Ok(events![EnrichmentEvent::ActorsExtracted]);
            }
        };

        let consumed_pin_ids = {
            let (_, state) = ctx.singleton::<PipelineState>();
            state
                .source_plan
                .as_ref()
                .map(|s| s.consumed_pin_ids.clone())
                .unwrap_or_default()
        };

        // Pin cleanup
        let mut all_events = Events::new();
        if !consumed_pin_ids.is_empty() {
            info!(count = consumed_pin_ids.len(), "Emitting PinsConsumed for consumed pins");
            all_events.push(rootsignal_common::events::SystemEvent::PinsConsumed {
                pin_ids: consumed_pin_ids,
            });
        }

        // Actor extraction
        info!("=== Actor Extraction ===");
        let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();
        let ai = deps.ai.as_ref().expect("guarded by enrichment trigger");
        let (actor_stats, actor_events) =
            activities::actor_extractor::run_actor_extraction(
                &*deps.store,
                graph,
                ai.as_ref(),
                min_lat,
                max_lat,
                min_lng,
                max_lng,
            )
            .await;
        info!("{actor_stats}");

        all_events.extend(actor_events);
        all_events.push(EnrichmentEvent::ActorsExtracted);
        Ok(all_events)
    }

    #[handle(on = ScrapeEvent, id = "enrichment:score_diversity", filter = response_done_diversity_pending, describe = describe_enrichment_gate)]
    async fn score_diversity(
        _event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();

        let graph = match deps.graph.as_ref() {
            Some(g) => g,
            None => {
                ctx.logger.debug("Skipped diversity metrics: missing graph");
                return Ok(events![EnrichmentEvent::DiversityScored]);
            }
        };

        info!("=== Diversity Metrics ===");
        let mut all_events = activities::diversity::compute_diversity_events(graph, &[]).await;
        all_events.push(EnrichmentEvent::DiversityScored);
        Ok(all_events)
    }

    #[handle(on = ScrapeEvent, id = "enrichment:compute_actor_stats", filter = response_done_actor_stats_pending, describe = describe_enrichment_gate)]
    async fn compute_actor_stats(
        _event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();

        let graph = match deps.graph.as_ref() {
            Some(g) => g,
            None => {
                ctx.logger.debug("Skipped actor stats: missing graph");
                return Ok(events![EnrichmentEvent::ActorStatsComputed]);
            }
        };

        info!("=== Actor Stats ===");
        let mut all_events = activities::actor_stats::compute_actor_stats_events(graph).await;
        all_events.push(EnrichmentEvent::ActorStatsComputed);
        Ok(all_events)
    }

    #[handle(on = ScrapeEvent, id = "enrichment:resolve_actor_locations", filter = response_done_actor_location_pending, describe = describe_enrichment_gate)]
    async fn resolve_actor_locations(
        _event: ScrapeEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();

        let actors = match deps.store.list_all_actors().await {
            Ok(a) => a,
            Err(e) => {
                ctx.logger.debug(&format!("Skipped actor location: failed to list actors — {e}"));
                return Ok(events![EnrichmentEvent::ActorsLocated]);
            }
        };

        let mut all_events = if actors.is_empty() {
            Events::new()
        } else {
            activities::actor_location::triangulate_actor_location_events(&*deps.store, &actors).await
        };
        all_events.push(EnrichmentEvent::ActorsLocated);
        Ok(all_events)
    }

}
