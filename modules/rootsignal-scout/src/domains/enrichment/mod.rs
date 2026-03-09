pub mod activities;
pub mod events;

#[cfg(test)]
mod completion_tests;
#[cfg(test)]
mod source_claim_integration_tests;

use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, AnyEvent, Context, Events};
use uuid::Uuid;

use rootsignal_common::events::{SystemEvent, WorldEvent};
use rootsignal_scout_supervisor::checks::batch_review::{self, SignalForReview};

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::enrichment::events::EnrichmentEvent;
use crate::domains::signals::events::SignalEvent;

// ── Filters ──

fn enrichment_gate_ready(ctx: &Context<ScoutEngineDeps>) -> bool {
    let state = ctx.aggregate::<PipelineState>().curr;
    state.review_complete() && state.response_scrape_done() && !state.enrichment_ready
}

fn is_enrichment_ready(e: &EnrichmentEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    matches!(e, EnrichmentEvent::EnrichmentReady)
}

fn is_signal_world_event(e: &WorldEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    e.is_signal()
}

// ── Event constructors: map activity domain types → system events ──

fn actor_location_event(update: activities::actor_location::ActorLocationUpdate) -> SystemEvent {
    SystemEvent::ActorLocationIdentified {
        actor_id: update.actor_id,
        location_lat: update.lat,
        location_lng: update.lng,
        location_name: update.name,
    }
}

fn review_verdict(signal_id: Uuid, decision: &str, rejection_reason: Option<&str>) -> SystemEvent {
    match decision {
        "reject" => SystemEvent::ReviewVerdictReached {
            signal_id,
            old_status: "staged".into(),
            new_status: "rejected".into(),
            reason: rejection_reason.unwrap_or("unspecified").to_string(),
        },
        _ => SystemEvent::ReviewVerdictReached {
            signal_id,
            old_status: "staged".into(),
            new_status: "live".into(),
            reason: "passed_review".into(),
        },
    }
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
    // Enrichment: runs all enrichment steps sequentially
    // ---------------------------------------------------------------

    #[handle(on = EnrichmentEvent, id = "enrichment:run_enrichment", filter = is_enrichment_ready)]
    async fn run_enrichment(
        _event: EnrichmentEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let state = ctx.aggregate::<PipelineState>().curr;
        let mut all_events = Events::new();

        // Mark consumed pins from source plan
        let consumed_pin_ids = state
            .source_plan
            .as_ref()
            .map(|s| s.consumed_pin_ids.clone())
            .unwrap_or_default();
        if !consumed_pin_ids.is_empty() {
            all_events.push(SystemEvent::PinsConsumed { pin_ids: consumed_pin_ids });
        }

        // Score signal diversity + compute per-actor signal counts
        if let Some(graph) = deps.graph.as_deref() {
            let (metrics, stats) = tokio::join!(
                activities::diversity::compute_diversity_scores(graph, &[]),
                activities::actor_stats::compute_actor_stats(graph),
            );
            if !metrics.is_empty() {
                all_events.push(SystemEvent::SignalDiversityComputed { metrics });
            }
            if !stats.is_empty() {
                all_events.push(SystemEvent::ActorStatsComputed { stats });
            }
        }

        // Triangulate actor locations + fetch profiles concurrently
        if let Ok(actors) = deps.store.list_all_actors().await {
            if !actors.is_empty() {
                let location_fut = activities::actor_location::triangulate_all_actors(&*deps.store, &actors);
                let profile_fut = async {
                    match deps.fetcher.as_deref() {
                        Some(fetcher) => activities::profile_enrichment::enrich_actor_profiles(fetcher, &actors).await,
                        None => vec![],
                    }
                };
                let (location_updates, profile_events) = tokio::join!(location_fut, profile_fut);
                for update in location_updates {
                    all_events.push(actor_location_event(update));
                }
                for event in profile_events {
                    all_events.push(event);
                }

                // Claim sources from actor profile external_urls
                let claim = activities::source_claimer::claim_profile_sources(&*deps.store, &actors).await?;
                for event in claim.link_events {
                    all_events.push(event);
                }
                if !claim.new_sources.is_empty() {
                    all_events.push(DiscoveryEvent::SourcesDiscovered {
                        sources: claim.new_sources,
                        discovered_by: "profile_link".into(),
                    });
                }

                // SERP expansion for actors with thin source coverage
                let region_name = state.run_scope.region().map(|r| r.name.as_str());
                let serp_sources = activities::actor_serp_expansion::expand_actors_via_serp(&actors, region_name);
                if !serp_sources.is_empty() {
                    all_events.push(DiscoveryEvent::SourcesDiscovered {
                        sources: serp_sources,
                        discovered_by: "actor_serp_expansion".into(),
                    });
                }
            }
        }

        all_events.push(crate::domains::expansion::events::ExpansionEvent::ExpansionReady);
        Ok(all_events)
    }

    // ---------------------------------------------------------------
    // Signal review: batch-accumulate signals, flush to LLM reviewer
    // ---------------------------------------------------------------

    #[handle(on = WorldEvent, id = "enrichment:submit_signal_for_review", filter = is_signal_world_event)]
    async fn submit_signal_for_review(
        event: WorldEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();

        let ai = match deps.ai.as_ref() {
            Some(a) => a.clone(),
            None => return Ok(Events::new()),
        };
        let region = {
            let state = ctx.aggregate::<PipelineState>().curr;
            state.run_scope.region().cloned()
        };

        let signal_id = match event.signal_id() {
            Some(id) => id,
            None => return Ok(Events::new()),
        };

        let review_item = match activities::signal_reviewer::signal_for_review(&event, &deps.run_id.to_string()) {
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
                    let ids: Vec<Uuid> = items.iter()
                        .filter_map(|s| Uuid::parse_str(&s.id).ok())
                        .collect();

                    let output = batch_review::review_batch(&*ai, region.as_ref(), items, &[]).await?;

                    let mut verdicts: HashMap<Uuid, SystemEvent> = HashMap::new();
                    for v in &output.verdicts {
                        let sid = Uuid::parse_str(&v.signal_id).unwrap_or(Uuid::nil());
                        verdicts.insert(sid, review_verdict(sid, &v.decision, v.rejection_reason.as_deref()));
                    }
                    for id in &ids {
                        verdicts.entry(*id).or_insert_with(|| review_verdict(*id, "pass", None));
                    }

                    Ok(verdicts)
                }
            })
            .await?;

        Ok(events![result])
    }
}
