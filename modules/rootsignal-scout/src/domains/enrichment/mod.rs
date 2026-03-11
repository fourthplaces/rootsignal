pub mod activities;
pub mod events;

#[cfg(test)]
mod completion_tests;
#[cfg(test)]
mod geocoding_tests;
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

// ── Search name construction ──

/// Build a geocodable search name by appending geographic context.
///
/// If the name already contains a comma (e.g. "Rochester, Minnesota"), it's
/// assumed to be qualified and is returned as-is. Otherwise, actor location
/// takes priority over region as the more specific context.
pub fn build_search_name(name: &str, region: Option<&str>, actor_location: Option<&str>) -> String {
    if name.contains(',') {
        return name.to_string();
    }
    if let Some(loc) = actor_location {
        return format!("{name}, {loc}");
    }
    if let Some(reg) = region {
        return format!("{name}, {reg}");
    }
    name.to_string()
}

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

fn has_locations_needing_geocoding(e: &WorldEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    e.locations().iter().any(|loc| {
        loc.name.as_ref().is_some_and(|n| !n.trim().is_empty())
    })
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
            new_status: "accepted".into(),
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
            None => {
                let signal_id = match event.signal_id() {
                    Some(id) => id,
                    None => return Ok(Events::new()),
                };
                return Ok(events![review_verdict(signal_id, "pass", None)]);
            }
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

    // ---------------------------------------------------------------
    // Geocode locations: reactive, fires on every signal with locations
    // ---------------------------------------------------------------

    #[handle(on = WorldEvent, id = "enrichment:geocode_locations", filter = has_locations_needing_geocoding)]
    async fn geocode_locations(
        event: WorldEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let geocoder = match deps.geocoder.as_deref() {
            Some(g) => g,
            None => {
                ctx.logger.warn("Geocoder not available, skipping geocode_locations");
                return Ok(Events::new());
            }
        };

        let signal_id = match event.signal_id() {
            Some(id) => id,
            None => {
                ctx.logger.debug("No signal_id on event, skipping geocode_locations");
                return Ok(Events::new());
            }
        };

        let state = ctx.aggregate::<PipelineState>().curr;
        let region_name = state.run_scope.region().map(|r| r.name.as_str());
        let actor_location = event.url()
            .and_then(|u| state.url_to_canonical_key.get(u))
            .and_then(|ck| state.actor_contexts.get(ck))
            .and_then(|ac| ac.location_name.as_deref());

        let mut out = Events::new();

        for loc in event.locations() {
            let name = match loc.name.as_deref() {
                Some(n) if !n.trim().is_empty() => n.trim(),
                _ => continue,
            };

            let search_name = build_search_name(name, region_name, actor_location);

            match geocoder.geocode(&search_name, None, None).await {
                Ok(Some(result)) => {
                    ctx.logger.info(&format!(
                        "Geocoded '{}' → ({:.4}, {:.4}) precision={}",
                        search_name, result.lat, result.lng, result.precision
                    ));
                    out.push(SystemEvent::LocationGeocoded {
                        signal_id,
                        location_name: name.to_string(),
                        lat: result.lat,
                        lng: result.lng,
                        address: result.address,
                        precision: result.precision,
                        timezone: result.timezone,
                    });
                }
                Ok(None) => {
                    ctx.logger.warn(&format!("No geocoding result for '{search_name}'"));
                }
                Err(e) => {
                    ctx.logger.warn(&format!("Geocoding failed for '{search_name}': {e}"));
                }
            }
        }

        Ok(out)
    }
}

#[cfg(test)]
mod build_search_name_tests {
    use super::build_search_name;

    #[test]
    fn search_name_appends_region_for_bare_names() {
        assert_eq!(
            build_search_name("Rochester", Some("Minnesota"), None),
            "Rochester, Minnesota"
        );
    }

    #[test]
    fn search_name_prefers_actor_location_over_region() {
        assert_eq!(
            build_search_name("Lake Street", Some("Minnesota"), Some("Minneapolis, MN")),
            "Lake Street, Minneapolis, MN"
        );
    }

    #[test]
    fn search_name_preserves_already_qualified_names() {
        assert_eq!(
            build_search_name("Rochester, Minnesota", Some("Minnesota"), Some("Minneapolis, MN")),
            "Rochester, Minnesota"
        );
    }

    #[test]
    fn search_name_passes_through_without_context() {
        assert_eq!(
            build_search_name("Rochester", None, None),
            "Rochester"
        );
    }
}
