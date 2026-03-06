pub mod activities;
pub mod events;

#[cfg(test)]
mod completion_tests;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};
use tracing::info;



use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::core::events::PipelinePhase;
use crate::core::pipeline_events::PipelineEvent;
use crate::domains::enrichment::events::{
    all_enrichment_roles, EnrichmentEvent, EnrichmentRole,
};
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

fn is_enrichment_role_completed(e: &EnrichmentEvent) -> bool {
    matches!(e, EnrichmentEvent::EnrichmentRoleCompleted { .. })
}

#[handlers]
pub mod handlers {
    use super::*;

    // ---------------------------------------------------------------
    // Role handlers: each listens for PhaseCompleted(ResponseScrape)
    // ---------------------------------------------------------------

    /// Pin cleanup + actor extraction → EnrichmentRoleCompleted(ActorExtraction)
    #[handle(on = LifecycleEvent, id = "enrichment:actor_extraction", filter = is_response_scrape_completed)]
    async fn actor_extraction(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();

        let (region, graph) = match (deps.run_scope.region(), deps.graph.as_ref()) {
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
    #[handle(on = LifecycleEvent, id = "enrichment:diversity", filter = is_response_scrape_completed)]
    async fn diversity(
        _event: LifecycleEvent,
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
    #[handle(on = LifecycleEvent, id = "enrichment:actor_stats", filter = is_response_scrape_completed)]
    async fn actor_stats(
        _event: LifecycleEvent,
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
    #[handle(on = LifecycleEvent, id = "enrichment:actor_location", filter = is_response_scrape_completed)]
    async fn actor_location(
        _event: LifecycleEvent,
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
    // Completion: all 4 roles done → PhaseCompleted(ActorEnrichment)
    // ---------------------------------------------------------------

    #[handle(on = EnrichmentEvent, id = "enrichment:phase_complete", filter = is_enrichment_role_completed)]
    async fn phase_complete(
        _event: EnrichmentEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (_, state) = ctx.singleton::<PipelineState>();

        // Idempotency: if this phase already completed, skip
        if state.completed_phases.contains(&PipelinePhase::ActorEnrichment) {
            return Ok(events![PipelineEvent::HandlerSkipped {
                handler_id: "enrichment:phase_complete".into(),
                reason: "ActorEnrichment already completed".into(),
            }]);
        }

        if state
            .completed_enrichment_roles
            .is_superset(&all_enrichment_roles())
        {
            info!("All enrichment roles complete, emitting PhaseCompleted");
            Ok(events![LifecycleEvent::PhaseCompleted {
                phase: PipelinePhase::ActorEnrichment,
            }])
        } else {
            let completed: Vec<_> = state.completed_enrichment_roles.iter().collect();
            let expected: Vec<_> = all_enrichment_roles().into_iter().collect();
            Ok(events![PipelineEvent::HandlerSkipped {
                handler_id: "enrichment:phase_complete".into(),
                reason: format!("waiting for ActorEnrichment: completed {completed:?}, need {expected:?}"),
            }])
        }
    }

    // ---------------------------------------------------------------
    // Source metrics: unchanged, triggers on PhaseCompleted(ActorEnrichment)
    // ---------------------------------------------------------------

    /// PhaseCompleted(ActorEnrichment) → update source weights/cadence, emit MetricsCompleted.
    #[handle(on = LifecycleEvent, id = "enrichment:metrics", filter = is_actor_enrichment_completed)]
    async fn update_source_weights(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();

        // Requires graph + region — skip in tests
        let (region, graph) = match (deps.run_scope.region(), deps.graph.as_ref()) {
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
