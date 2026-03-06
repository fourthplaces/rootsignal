//! Lifecycle domain activity functions: pure logic extracted from handlers.

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use tracing::{info, warn};

use rootsignal_common::events::SystemEvent;
use rootsignal_common::types::NodeType;
use rootsignal_common::{is_web_query, DiscoveryMethod, SourceNode};
use rootsignal_graph::GraphReader;

use crate::core::aggregate::{SourcePlan, SourcePlanOutput};
use crate::domains::scheduling::activities::scheduler::{self as scheduler, schedule_web_queries};
use crate::infra::util::sanitize_url;
use crate::traits::SignalReader;

/// Find signals that have gone stale and emit SignalsExpired events (batched by type).
pub async fn find_stale_signals(store: &dyn SignalReader) -> seesaw_core::Events {
    let stale = match store.find_expired_signals().await {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "Failed to find stale signals, continuing");
            return seesaw_core::Events::new();
        }
    };

    if stale.is_empty() {
        return seesaw_core::Events::new();
    }

    // Group by (node_type, reason) → one event per group
    let mut groups: HashMap<(NodeType, String), Vec<Uuid>> = HashMap::new();
    for (signal_id, node_type, reason) in &stale {
        groups
            .entry((*node_type, reason.clone()))
            .or_default()
            .push(*signal_id);
    }

    let mut events = seesaw_core::Events::new();
    for ((node_type, reason), signal_ids) in groups {
        info!(node_type = ?node_type, reason, count = signal_ids.len(), "Stale signals found");
        events.push(SystemEvent::SignalsExpired {
            signal_ids,
            node_type,
            reason,
        });
    }
    events
}

/// Prepare input sources directly (no graph, no cadence, no exploration).
///
/// All sources are selected unconditionally. Partitions by SourceRole into
/// tension/response phase keys.
pub fn prepare_input_sources(sources: &[SourceNode]) -> SourcePlanOutput {
    use rootsignal_common::SourceRole;

    let selected_keys: HashSet<String> = sources
        .iter()
        .map(|s| s.canonical_key.clone())
        .collect();

    let mut tension_phase_keys = HashSet::new();
    let mut response_phase_keys = HashSet::new();
    for s in sources {
        match s.source_role {
            SourceRole::Response => {
                response_phase_keys.insert(s.canonical_key.clone());
            }
            SourceRole::Concern => {
                tension_phase_keys.insert(s.canonical_key.clone());
            }
            SourceRole::Mixed => {
                tension_phase_keys.insert(s.canonical_key.clone());
                response_phase_keys.insert(s.canonical_key.clone());
            }
        }
    }

    let tension_count = sources
        .iter()
        .filter(|s| tension_phase_keys.contains(&s.canonical_key))
        .count() as u32;
    let response_count = sources
        .iter()
        .filter(|s| response_phase_keys.contains(&s.canonical_key))
        .count() as u32;

    let mut url_mappings = HashMap::new();
    for s in sources {
        if let Some(ref url) = s.url {
            url_mappings
                .entry(sanitize_url(url))
                .or_insert_with(|| s.canonical_key.clone());
        }
    }

    info!(
        sources = sources.len(),
        tension = tension_count,
        response = response_count,
        "Prepared input sources (direct)"
    );

    SourcePlanOutput {
        source_plan: SourcePlan {
            all_sources: sources.to_vec(),
            selected_sources: sources.to_vec(),
            tension_phase_keys,
            response_phase_keys,
            selected_keys,
            consumed_pin_ids: Vec::new(),
        },
        actor_contexts: HashMap::new(),
        url_mappings,
        tension_count,
        response_count,
    }
}

/// Load, select, and prepare sources for this run. Returns the source plan.
pub async fn prepare_sources(
    graph: &GraphReader,
    region: &rootsignal_common::ScoutScope,
) -> SourcePlanOutput {
    // Load sources
    let mut all_sources = match graph
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
    let actor_pairs = match graph
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
    let consumed_pin_ids = match graph
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

    // Select sources by cadence + exploration rules
    let now_ts = Utc::now();
    let selection = scheduler::schedule(&all_sources, now_ts);
    let selected_keys: HashSet<String> = selection
        .scheduled
        .iter()
        .chain(selection.exploration.iter())
        .map(|s| s.canonical_key.clone())
        .collect();

    let tension_phase_keys: HashSet<String> =
        selection.tension_phase.iter().cloned().collect();
    let response_phase_keys: HashSet<String> =
        selection.response_phase.iter().cloned().collect();

    info!(
        selected = selection.scheduled.len(),
        exploration = selection.exploration.len(),
        skipped = selection.skipped,
        tension_phase = tension_phase_keys.len(),
        response_phase = response_phase_keys.len(),
        "Source selection complete"
    );

    // Web query tiered selection
    let wq_selection = schedule_web_queries(
        &all_sources,
        0,
        now_ts,
    );
    let wq_selected_keys: HashSet<String> =
        wq_selection.scheduled.into_iter().collect();

    let selected_sources: Vec<SourceNode> = all_sources
        .iter()
        .filter(|s| {
            if !selected_keys.contains(&s.canonical_key) {
                return false;
            }
            if !is_web_query(&s.canonical_value) {
                return true;
            }
            wq_selected_keys.contains(&s.canonical_key)
        })
        .cloned()
        .collect();

    let tension_count = selected_sources
        .iter()
        .filter(|s| tension_phase_keys.contains(&s.canonical_key))
        .count() as u32;
    let response_count = selected_sources
        .iter()
        .filter(|s| response_phase_keys.contains(&s.canonical_key))
        .count() as u32;

    // Build actor contexts for location fallback
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

    SourcePlanOutput {
        source_plan: SourcePlan {
            all_sources,
            selected_sources,
            tension_phase_keys,
            response_phase_keys,
            selected_keys,
            consumed_pin_ids,
        },
        actor_contexts,
        url_mappings,
        tension_count,
        response_count,
    }
}
