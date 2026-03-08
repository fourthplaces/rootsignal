pub mod activities;
pub mod events;

#[cfg(test)]
mod completion_tests;

use std::time::Duration;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, AnyEvent, Context, Events};
use tracing::info;
use uuid::Uuid;

use rootsignal_common::events::{SystemEvent, WorldEvent};
use rootsignal_scout_supervisor::checks::batch_review::{self, SignalForReview};

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::enrichment::events::EnrichmentEvent;
use crate::domains::signals::events::SignalEvent;

// ── Gate filter: emits EnrichmentReady exactly once when review + response scrape complete ──

fn enrichment_gate_ready(ctx: &Context<ScoutEngineDeps>) -> bool {
    let (_, state) = ctx.singleton::<PipelineState>();
    state.review_complete() && state.response_scrape_done() && !state.enrichment_ready
}

// ── Enrichment filters: fire on EnrichmentReady, guarded by per-fact flags ──

fn actor_extraction_pending(e: &EnrichmentEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, EnrichmentEvent::EnrichmentReady)
}

fn diversity_pending(e: &EnrichmentEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, EnrichmentEvent::EnrichmentReady)
}

fn actor_stats_pending(e: &EnrichmentEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, EnrichmentEvent::EnrichmentReady)
}

fn actor_location_pending(e: &EnrichmentEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, EnrichmentEvent::EnrichmentReady)
}

fn is_signal_world_event(e: &WorldEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    e.is_signal()
}

#[handlers]
pub mod handlers {
    use super::*;

    // ---------------------------------------------------------------
    // Gate: collapses [SystemEvent, SignalEvent] fan-in into one EnrichmentReady
    // ---------------------------------------------------------------

    #[handle(on = [SystemEvent, SignalEvent], id = "enrichment:review_gate", filter = enrichment_gate_ready)]
    async fn review_gate(
        _event: AnyEvent,
        _ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        Ok(events![EnrichmentEvent::EnrichmentReady])
    }

    // ---------------------------------------------------------------
    // Enrichment handlers: fire exactly once on EnrichmentReady
    // ---------------------------------------------------------------

    #[handle(on = EnrichmentEvent, id = "enrichment:extract_actors", filter = actor_extraction_pending)]
    async fn extract_actors(
        _event: EnrichmentEvent,
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

    #[handle(on = EnrichmentEvent, id = "enrichment:score_diversity", filter = diversity_pending)]
    async fn score_diversity(
        _event: EnrichmentEvent,
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

    #[handle(on = EnrichmentEvent, id = "enrichment:compute_actor_stats", filter = actor_stats_pending)]
    async fn compute_actor_stats(
        _event: EnrichmentEvent,
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

    #[handle(on = EnrichmentEvent, id = "enrichment:resolve_actor_locations", filter = actor_location_pending)]
    async fn resolve_actor_locations(
        _event: EnrichmentEvent,
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

    // ---------------------------------------------------------------
    // Signal review: Batcher accumulates signals, flushes to reviewer
    // ---------------------------------------------------------------

    #[handle(on = WorldEvent, id = "enrichment:submit_signal_for_review", filter = is_signal_world_event)]
    async fn submit_signal_for_review(
        event: WorldEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();

        let (ai, region) = match (deps.ai.as_ref(), {
            let (_, state) = ctx.singleton::<PipelineState>();
            state.run_scope.region().cloned()
        }) {
            (Some(a), Some(r)) => (a.clone(), r),
            _ => return Ok(Events::new()),
        };

        let signal_id = match event.signal_id() {
            Some(id) => id,
            None => return Ok(Events::new()),
        };

        let review_item = activities::signal_reviewer::signal_for_review(
            &event,
            &deps.run_id.to_string(),
        );
        let review_item = match review_item {
            Some(r) => r,
            None => return Ok(Events::new()),
        };

        let result = deps.batcher
            .submit("signal_review", signal_id, review_item)
            .flush_when(|batch| batch.len() >= 10 || batch.age() >= Duration::from_secs(5))
            .then(move |items: Vec<SignalForReview>| {
                let ai = ai;
                let region = region;
                async move {
                    // Collect signal IDs before passing to reviewer
                    let ids: Vec<Uuid> = items.iter()
                        .filter_map(|s| Uuid::parse_str(&s.id).ok())
                        .collect();

                    let output = batch_review::review_batch(&*ai, &region, items, &[]).await?;

                    let mut results: std::collections::HashMap<Uuid, rootsignal_common::events::SystemEvent> =
                        std::collections::HashMap::new();
                    for verdict in &output.verdicts {
                        let sid = Uuid::parse_str(&verdict.signal_id).unwrap_or(Uuid::nil());
                        let event = match verdict.decision.as_str() {
                            "reject" => rootsignal_common::events::SystemEvent::ReviewVerdictReached {
                                signal_id: sid,
                                old_status: "staged".into(),
                                new_status: "rejected".into(),
                                reason: verdict.rejection_reason.clone().unwrap_or_else(|| "unspecified".into()),
                            },
                            _ => rootsignal_common::events::SystemEvent::ReviewVerdictReached {
                                signal_id: sid,
                                old_status: "staged".into(),
                                new_status: "live".into(),
                                reason: "passed_review".into(),
                            },
                        };
                        results.insert(sid, event);
                    }

                    // Default to pass for signals the LLM didn't return a verdict for
                    for id in &ids {
                        results.entry(*id).or_insert_with(|| {
                            rootsignal_common::events::SystemEvent::ReviewVerdictReached {
                                signal_id: *id,
                                old_status: "staged".into(),
                                new_status: "live".into(),
                                reason: "passed_review".into(),
                            }
                        });
                    }

                    Ok(results)
                }
            })
            .await?;

        Ok(events![result])
    }

}
