//! Lifecycle domain activity functions: pure logic extracted from handlers.

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use tracing::{info, warn};

use rootsignal_common::{is_web_query, DiscoveryMethod, SourceNode};
use rootsignal_graph::GraphStore;

use crate::core::aggregate::{ScheduleOutput, ScheduledData};
use crate::infra::util::sanitize_url;
use crate::traits::SignalReader;

/// Reap expired signals, return EntityExpired events.
pub async fn reap_expired(store: &dyn SignalReader) -> seesaw_core::Events {
    let expired = match store.find_expired_signals().await {
        Ok(e) => e,
        Err(e) => {
            warn!(error = %e, "Failed to find expired signals, continuing");
            return seesaw_core::Events::new();
        }
    };

    let mut events = seesaw_core::Events::new();
    let mut gatherings = 0u64;
    let mut needs = 0u64;
    let mut stale = 0u64;

    for (signal_id, node_type, reason) in &expired {
        events.push(rootsignal_common::events::SystemEvent::EntityExpired {
            signal_id: *signal_id,
            node_type: *node_type,
            reason: reason.clone(),
        });
        match node_type {
            rootsignal_common::types::NodeType::Gathering => gatherings += 1,
            rootsignal_common::types::NodeType::Need => needs += 1,
            _ => stale += 1,
        }
    }

    if gatherings + needs + stale > 0 {
        info!(gatherings, needs, stale, "Expired signals removed");
    }

    events
}

/// Load, boost, and schedule sources. Returns ScheduleOutput for state application.
pub async fn schedule_sources(
    writer: &GraphStore,
    region: &rootsignal_common::ScoutScope,
) -> ScheduleOutput {
    // Load sources
    let mut all_sources = match writer
        .get_sources_for_region(region.center_lat, region.center_lng, region.radius_km)
        .await
    {
        Ok(sources) => {
            let curated = sources
                .iter()
                .filter(|s| s.discovery_method == DiscoveryMethod::Curated)
                .count();
            let discovered = sources.len() - curated;
            info!(
                total = sources.len(),
                curated, discovered, "Loaded region-scoped sources from graph"
            );
            sources
        }
        Err(e) => {
            warn!(error = %e, "Failed to load sources from graph");
            Vec::new()
        }
    };

    // Actor sources — inject known actor accounts with elevated priority
    let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();
    let actor_pairs = match writer
        .find_actors_in_region(min_lat, max_lat, min_lng, max_lng)
        .await
    {
        Ok(pairs) => {
            let actor_count = pairs.len();
            let source_count: usize = pairs.iter().map(|(_, s)| s.len()).sum();
            if actor_count > 0 {
                info!(
                    actors = actor_count,
                    sources = source_count,
                    "Loaded actor accounts for region"
                );
            }
            pairs
        }
        Err(e) => {
            warn!(error = %e, "Failed to load actor accounts, continuing without");
            Vec::new()
        }
    };

    // Boost existing entity sources or add new ones
    for (_actor, sources) in &actor_pairs {
        for source in sources {
            if let Some(existing) = all_sources
                .iter_mut()
                .find(|s| s.canonical_key == source.canonical_key)
            {
                existing.weight = existing.weight.max(0.7);
                existing.cadence_hours =
                    Some(existing.cadence_hours.map(|h| h.min(12)).unwrap_or(12));
            } else {
                all_sources.push(source.clone());
            }
        }
    }

    // Pin consumption
    let existing_keys: HashSet<String> = all_sources
        .iter()
        .map(|s| s.canonical_key.clone())
        .collect();
    let consumed_pin_ids = match writer
        .find_pins_in_region(min_lat, max_lat, min_lng, max_lng)
        .await
    {
        Ok(pins) => {
            let mut ids = Vec::new();
            for (pin, source) in pins {
                if !existing_keys.contains(&source.canonical_key) {
                    all_sources.push(source);
                }
                ids.push(pin.id);
            }
            if !ids.is_empty() {
                info!(pins = ids.len(), "Consumed pins from region");
            }
            ids
        }
        Err(e) => {
            warn!(error = %e, "Failed to load pins, continuing without");
            Vec::new()
        }
    };

    // Schedule sources
    let now_schedule = Utc::now();
    let scheduler = crate::domains::scheduling::activities::scheduler::SourceScheduler::new();
    let schedule = scheduler.schedule(&all_sources, now_schedule);
    let scheduled_keys: HashSet<String> = schedule
        .scheduled
        .iter()
        .chain(schedule.exploration.iter())
        .map(|s| s.canonical_key.clone())
        .collect();

    let tension_phase_keys: HashSet<String> =
        schedule.tension_phase.iter().cloned().collect();
    let response_phase_keys: HashSet<String> =
        schedule.response_phase.iter().cloned().collect();

    info!(
        scheduled = schedule.scheduled.len(),
        exploration = schedule.exploration.len(),
        skipped = schedule.skipped,
        tension_phase = tension_phase_keys.len(),
        response_phase = response_phase_keys.len(),
        "Source scheduling complete"
    );

    // Web query tiered scheduling
    let wq_schedule = crate::domains::scheduling::activities::scheduler::schedule_web_queries(
        &all_sources,
        0,
        now_schedule,
    );
    let wq_scheduled_keys: HashSet<String> =
        wq_schedule.scheduled.into_iter().collect();

    let scheduled_sources: Vec<SourceNode> = all_sources
        .iter()
        .filter(|s| {
            if !scheduled_keys.contains(&s.canonical_key) {
                return false;
            }
            if !is_web_query(&s.canonical_value) {
                return true;
            }
            wq_scheduled_keys.contains(&s.canonical_key)
        })
        .cloned()
        .collect();

    let tension_count = scheduled_sources
        .iter()
        .filter(|s| tension_phase_keys.contains(&s.canonical_key))
        .count() as u32;
    let response_count = scheduled_sources
        .iter()
        .filter(|s| response_phase_keys.contains(&s.canonical_key))
        .count() as u32;

    // Populate actor contexts for location fallback
    let mut actor_contexts = HashMap::new();
    for (actor, sources) in &actor_pairs {
        let actor_ctx = rootsignal_common::ActorContext {
            actor_name: actor.name.clone(),
            bio: actor.bio.clone(),
            location_name: actor.location_name.clone(),
            location_lat: actor.location_lat,
            location_lng: actor.location_lng,
            discovery_depth: actor.discovery_depth,
        };
        for source in sources {
            actor_contexts.insert(source.canonical_key.clone(), actor_ctx.clone());
        }
    }

    // Build URL→canonical_key mappings
    let mut url_mappings = HashMap::new();
    for s in &all_sources {
        if let Some(ref url) = s.url {
            url_mappings
                .entry(sanitize_url(url))
                .or_insert_with(|| s.canonical_key.clone());
        }
    }

    ScheduleOutput {
        scheduled_data: ScheduledData {
            all_sources,
            scheduled_sources,
            tension_phase_keys,
            response_phase_keys,
            scheduled_keys,
            consumed_pin_ids,
        },
        actor_contexts,
        url_mappings,
        tension_count,
        response_count,
    }
}
