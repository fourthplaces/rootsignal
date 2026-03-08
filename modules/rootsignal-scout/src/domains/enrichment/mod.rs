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

fn is_enrichment_ready(e: &EnrichmentEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
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
    // Enrichment: single handler runs all enrichment steps sequentially
    // ---------------------------------------------------------------

    #[handle(on = EnrichmentEvent, id = "enrichment:run_enrichment", filter = is_enrichment_ready)]
    async fn run_enrichment(
        _event: EnrichmentEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let mut all_events = Events::new();

        // ── Actor extraction ──
        {
            let (_, state) = ctx.singleton::<PipelineState>();
            match (state.run_scope.region(), deps.graph.as_ref()) {
                (Some(region), Some(graph)) => {
                    let consumed_pin_ids = state
                        .source_plan
                        .as_ref()
                        .map(|s| s.consumed_pin_ids.clone())
                        .unwrap_or_default();

                    if !consumed_pin_ids.is_empty() {
                        info!(count = consumed_pin_ids.len(), "Emitting PinsConsumed for consumed pins");
                        all_events.push(SystemEvent::PinsConsumed {
                            pin_ids: consumed_pin_ids,
                        });
                    }

                    info!("=== Actor Extraction ===");
                    let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();
                    let ai = deps.ai.as_ref().expect("guarded by enrichment trigger");
                    let result = activities::actor_extractor::run_actor_extraction(
                        &*deps.store,
                        graph,
                        ai.as_ref(),
                        min_lat,
                        max_lat,
                        min_lng,
                        max_lng,
                    )
                    .await;
                    info!("{}", result.stats);
                    for actor in result.new_actors {
                        all_events.push(SystemEvent::ActorIdentified {
                            actor_id: actor.actor_id,
                            name: actor.name,
                            actor_type: actor.actor_type,
                            canonical_key: actor.canonical_key,
                            domains: vec![],
                            social_urls: vec![],
                            description: String::new(),
                            bio: None,
                            location_lat: actor.location_lat,
                            location_lng: actor.location_lng,
                            location_name: None,
                        });
                    }
                    for link in result.actor_links {
                        all_events.push(SystemEvent::ActorLinkedToSignal {
                            actor_id: link.actor_id,
                            signal_id: link.signal_id,
                            role: link.role,
                        });
                    }
                }
                _ => {
                    ctx.logger.debug("Skipped actor extraction: missing region or graph");
                }
            }
        }

        // ── Diversity scoring ──
        if let Some(graph) = deps.graph.as_ref() {
            info!("=== Diversity Metrics ===");
            let metrics = activities::diversity::compute_diversity_scores(graph, &[]).await;
            if !metrics.is_empty() {
                all_events.push(SystemEvent::SignalDiversityComputed { metrics });
            }
        } else {
            ctx.logger.debug("Skipped diversity metrics: missing graph");
        }

        // ── Actor stats ──
        if let Some(graph) = deps.graph.as_ref() {
            info!("=== Actor Stats ===");
            let stats = activities::actor_stats::compute_actor_stats(graph).await;
            if !stats.is_empty() {
                all_events.push(SystemEvent::ActorStatsComputed { stats });
            }
        } else {
            ctx.logger.debug("Skipped actor stats: missing graph");
        }

        // ── Actor location ──
        match deps.store.list_all_actors().await {
            Ok(actors) if !actors.is_empty() => {
                for update in activities::actor_location::triangulate_all_actors(&*deps.store, &actors).await {
                    all_events.push(SystemEvent::ActorLocationIdentified {
                        actor_id: update.actor_id,
                        location_lat: update.lat,
                        location_lng: update.lng,
                        location_name: update.name,
                    });
                }
            }
            Ok(_) => {}
            Err(e) => {
                ctx.logger.debug(&format!("Skipped actor location: failed to list actors — {e}"));
            }
        }

        // Single handler → single ExpansionReady, no sibling fan-out problem
        all_events.push(crate::domains::expansion::events::ExpansionEvent::ExpansionReady);

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
