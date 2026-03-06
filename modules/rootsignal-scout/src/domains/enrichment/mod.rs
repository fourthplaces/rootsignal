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
use crate::domains::enrichment::events::{
    all_enrichment_roles, EnrichmentEvent, EnrichmentRole,
};
use crate::domains::scrape::events::ScrapeEvent;
use crate::domains::lifecycle::events::LifecycleEvent;

// ── Enrichment role filters: response roles done + own role not started ──

/// Enrichment gates fire on any scrape completion event (including ResponseScrapeSkipped).
fn is_scrape_completion(e: &ScrapeEvent) -> bool {
    e.completed_role().is_some() || matches!(e, ScrapeEvent::ResponseScrapeSkipped { .. })
}

fn response_scrape_done(state: &PipelineState) -> bool {
    !state.expected_response_roles.is_empty()
        && state.completed_scrape_roles.is_superset(&state.expected_response_roles)
}

fn response_done_actor_extraction_pending(e: &ScrapeEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    if !is_scrape_completion(e) { return false; }
    let (_, state) = ctx.singleton::<PipelineState>();
    response_scrape_done(&state)
        && !state.completed_enrichment_roles.contains(&EnrichmentRole::ActorExtraction)
}

fn response_done_diversity_pending(e: &ScrapeEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    if !is_scrape_completion(e) { return false; }
    let (_, state) = ctx.singleton::<PipelineState>();
    response_scrape_done(&state)
        && !state.completed_enrichment_roles.contains(&EnrichmentRole::Diversity)
}

fn response_done_actor_stats_pending(e: &ScrapeEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    if !is_scrape_completion(e) { return false; }
    let (_, state) = ctx.singleton::<PipelineState>();
    response_scrape_done(&state)
        && !state.completed_enrichment_roles.contains(&EnrichmentRole::ActorStats)
}

fn response_done_actor_location_pending(e: &ScrapeEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    if !is_scrape_completion(e) { return false; }
    let (_, state) = ctx.singleton::<PipelineState>();
    response_scrape_done(&state)
        && !state.completed_enrichment_roles.contains(&EnrichmentRole::ActorLocation)
}

fn all_enrichment_done(e: &EnrichmentEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    if !matches!(e, EnrichmentEvent::EnrichmentRoleCompleted { .. }) { return false; }
    let (_, state) = ctx.singleton::<PipelineState>();
    state.completed_enrichment_roles.is_superset(&all_enrichment_roles())
}

fn describe_enrichment_gate(ctx: &Context<ScoutEngineDeps>) -> Vec<Block> {
    let (_, state) = ctx.singleton::<PipelineState>();
    let scrape_done = &state.completed_scrape_roles;
    let enrichment_done = &state.completed_enrichment_roles;
    let all = all_enrichment_roles();
    vec![
        Block::Checklist {
            label: "Scrape prerequisites".into(),
            items: state.expected_response_roles.iter().map(|r| ChecklistItem {
                text: format!("{r:?}"),
                done: scrape_done.contains(r),
            }).collect(),
        },
        Block::Checklist {
            label: "Enrichment roles".into(),
            items: all.iter().map(|r| ChecklistItem {
                text: format!("{r:?}"),
                done: enrichment_done.contains(r),
            }).collect(),
        },
    ]
}

#[handlers]
pub mod handlers {
    use super::*;

    // ---------------------------------------------------------------
    // Role handlers: each listens for scrape completion + state gate
    // ---------------------------------------------------------------

    /// Pin cleanup + actor extraction → EnrichmentRoleCompleted(ActorExtraction)
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
                return Ok(events![EnrichmentEvent::EnrichmentRoleCompleted {
                    role: EnrichmentRole::ActorExtraction,
                }]);
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
        all_events.push(EnrichmentEvent::EnrichmentRoleCompleted {
            role: EnrichmentRole::ActorExtraction,
        });
        Ok(all_events)
    }

    /// Diversity metrics → EnrichmentRoleCompleted(Diversity)
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
                return Ok(events![EnrichmentEvent::EnrichmentRoleCompleted {
                    role: EnrichmentRole::Diversity,
                }]);
            }
        };

        info!("=== Diversity Metrics ===");
        let mut all_events = activities::diversity::compute_diversity_events(graph, &[]).await;
        all_events.push(EnrichmentEvent::EnrichmentRoleCompleted {
            role: EnrichmentRole::Diversity,
        });
        Ok(all_events)
    }

    /// Actor stats → EnrichmentRoleCompleted(ActorStats)
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
                return Ok(events![EnrichmentEvent::EnrichmentRoleCompleted {
                    role: EnrichmentRole::ActorStats,
                }]);
            }
        };

        info!("=== Actor Stats ===");
        let mut all_events = activities::actor_stats::compute_actor_stats_events(graph).await;
        all_events.push(EnrichmentEvent::EnrichmentRoleCompleted {
            role: EnrichmentRole::ActorStats,
        });
        Ok(all_events)
    }

    /// Actor location triangulation → EnrichmentRoleCompleted(ActorLocation)
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
                return Ok(events![EnrichmentEvent::EnrichmentRoleCompleted {
                    role: EnrichmentRole::ActorLocation,
                }]);
            }
        };

        let mut all_events = if actors.is_empty() {
            Events::new()
        } else {
            activities::actor_location::triangulate_actor_location_events(&*deps.store, &actors).await
        };
        all_events.push(EnrichmentEvent::EnrichmentRoleCompleted {
            role: EnrichmentRole::ActorLocation,
        });
        Ok(all_events)
    }

    // ---------------------------------------------------------------
    // Metrics: all 4 enrichment roles done → update source weights
    // ---------------------------------------------------------------

    /// All enrichment roles done → update source weights/cadence, emit MetricsCompleted.
    #[handle(on = EnrichmentEvent, id = "enrichment:update_source_weights", filter = all_enrichment_done, describe = describe_enrichment_gate)]
    async fn update_source_weights(
        _event: EnrichmentEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let (_, state) = ctx.singleton::<PipelineState>();

        // Requires graph + region — skip in tests
        let (region, graph) = match (state.run_scope.region(), deps.graph.as_ref()) {
            (Some(r), Some(g)) => (r, g),
            _ => {
                ctx.logger.debug("Skipped source metrics: missing region or graph");
                return Ok(events![LifecycleEvent::MetricsCompleted]);
            }
        };

        let (_, state) = ctx.singleton::<PipelineState>();
        let all_sources = state
            .source_plan
            .as_ref()
            .map(|s| s.all_sources.clone())
            .unwrap_or_default();
        let source_signal_counts = state.source_signal_counts.clone();
        let query_api_errors = state.query_api_errors.clone();

        let metric_events = activities::compute_source_metrics(
            graph,
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
