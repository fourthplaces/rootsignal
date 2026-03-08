//! Lifecycle domain activity functions: pure logic extracted from handlers.

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use tracing::{info, warn};

use uuid::Uuid;

use rootsignal_common::system_events::StaleSignal;
use rootsignal_common::{is_web_query, ActorNode, DiscoveryMethod, SourceNode};
use rootsignal_graph::GraphQueries;

use crate::core::aggregate::{SourcePlan, SourcePlanOutput};
use crate::domains::scheduling::activities::selector::{self as selector, select_web_queries};
use crate::infra::util::sanitize_url;
use crate::traits::SignalReader;

/// Find signals that have gone stale. Returns raw data — the handler wraps it in an event.
pub async fn find_stale_signals(store: &dyn SignalReader) -> Vec<StaleSignal> {
    let stale = match store.find_expired_signals().await {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "Failed to find stale signals, continuing");
            return Vec::new();
        }
    };

    let signals: Vec<StaleSignal> = stale
        .into_iter()
        .map(|(signal_id, node_type, reason)| StaleSignal {
            signal_id,
            node_type,
            reason,
        })
        .collect();

    if !signals.is_empty() {
        info!(count = signals.len(), "Stale signals found");
    }

    signals
}

/// Build a source plan from an explicit list of sources.
///
/// All sources are selected unconditionally. Partitions by SourceRole into
/// tension/response phase keys.
pub fn build_source_plan_from_list(sources: &[SourceNode]) -> SourcePlanOutput {
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

    finalize_plan(
        SourcePlan {
            all_sources: sources.to_vec(),
            selected_sources: sources.to_vec(),
            tension_phase_keys,
            response_phase_keys,
            selected_keys,
            consumed_pin_ids: Vec::new(),
        },
        HashMap::new(),
    )
}

/// Build a source plan from the graph: load region sources, boost actor accounts,
/// consume pins, then select by cadence.
pub async fn build_source_plan_from_region(
    graph: &dyn GraphQueries,
    region: &rootsignal_common::ScoutScope,
) -> SourcePlanOutput {
    let bounds = region.bounding_box();

    let mut all_sources = load_region_sources(graph, region).await;
    let actor_pairs = load_actor_sources(graph, bounds, &mut all_sources).await;
    let consumed_pin_ids = consume_region_pins(graph, bounds, &mut all_sources).await;

    let (selected_sources, selected_keys, tension_phase_keys, response_phase_keys) =
        apply_cadence_selection(&all_sources);

    let actor_contexts = build_actor_contexts(&actor_pairs);

    finalize_plan(
        SourcePlan {
            all_sources,
            selected_sources,
            tension_phase_keys,
            response_phase_keys,
            selected_keys,
            consumed_pin_ids,
        },
        actor_contexts,
    )
}

/// Load all sources scoped to a geographic region.
async fn load_region_sources(
    graph: &dyn GraphQueries,
    region: &rootsignal_common::ScoutScope,
) -> Vec<SourceNode> {
    match graph
        .get_sources_for_region(region.center_lat, region.center_lng, region.radius_km)
        .await
    {
        Ok(sources) => {
            let curated = sources
                .iter()
                .filter(|s| s.discovery_method == DiscoveryMethod::Curated)
                .count();
            info!(
                total = sources.len(),
                curated,
                discovered = sources.len() - curated,
                "Loaded region sources"
            );
            sources
        }
        Err(e) => {
            warn!(error = %e, "Failed to load region sources");
            Vec::new()
        }
    }
}

/// Load actor accounts in a region and merge their sources into the pool.
///
/// Existing sources get elevated priority (weight ≥ 0.7, cadence ≤ 12h).
/// New actor sources are appended. Returns actor-source pairs for building
/// actor contexts later.
async fn load_actor_sources(
    graph: &dyn GraphQueries,
    (min_lat, max_lat, min_lng, max_lng): (f64, f64, f64, f64),
    all_sources: &mut Vec<SourceNode>,
) -> Vec<(ActorNode, Vec<SourceNode>)> {
    let actor_pairs = match graph
        .find_actors_in_region(min_lat, max_lat, min_lng, max_lng)
        .await
    {
        Ok(pairs) => {
            let actor_count = pairs.len();
            let source_count: usize = pairs.iter().map(|(_, s)| s.len()).sum();
            if actor_count > 0 {
                info!(actors = actor_count, sources = source_count, "Loaded actor accounts");
            }
            pairs
        }
        Err(e) => {
            warn!(error = %e, "Failed to load actor accounts, continuing without");
            return Vec::new();
        }
    };

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

    actor_pairs
}

/// Consume user pins in a region: add their sources to the pool, return consumed pin IDs.
async fn consume_region_pins(
    graph: &dyn GraphQueries,
    (min_lat, max_lat, min_lng, max_lng): (f64, f64, f64, f64),
    all_sources: &mut Vec<SourceNode>,
) -> Vec<Uuid> {
    let existing_keys: HashSet<String> = all_sources
        .iter()
        .map(|s| s.canonical_key.clone())
        .collect();

    match graph
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
    }
}

/// Apply cadence selection and web query tiered selection to the source pool.
///
/// Returns (selected_sources, selected_keys, tension_phase_keys, response_phase_keys).
fn apply_cadence_selection(
    all_sources: &[SourceNode],
) -> (Vec<SourceNode>, HashSet<String>, HashSet<String>, HashSet<String>) {
    let now_ts = Utc::now();
    let selection = selector::select_sources(all_sources, now_ts);

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

    // Web query tiered selection — further filters web queries by priority tier
    let wq_selection = select_web_queries(all_sources, 0, now_ts);
    let wq_selected_keys: HashSet<String> = wq_selection.scheduled.into_iter().collect();

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

    (selected_sources, selected_keys, tension_phase_keys, response_phase_keys)
}

/// Map actor-source pairs into a lookup of source key → actor context (for location fallback).
fn build_actor_contexts(
    actor_pairs: &[(ActorNode, Vec<SourceNode>)],
) -> HashMap<String, rootsignal_common::ActorContext> {
    let mut contexts = HashMap::new();
    for (actor, sources) in actor_pairs {
        let ctx = rootsignal_common::ActorContext {
            actor_name: actor.name.clone(),
            bio: actor.bio.clone(),
            location_name: actor.location_name.clone(),
            location_lat: actor.location_lat,
            location_lng: actor.location_lng,
            discovery_depth: actor.discovery_depth,
        };
        for source in sources {
            contexts.insert(source.canonical_key.clone(), ctx.clone());
        }
    }
    contexts
}

/// Compute derived fields (counts, URL mappings) from a SourcePlan.
fn finalize_plan(
    source_plan: SourcePlan,
    actor_contexts: HashMap<String, rootsignal_common::ActorContext>,
) -> SourcePlanOutput {
    let tension_count = source_plan.selected_sources.iter()
        .filter(|s| source_plan.tension_phase_keys.contains(&s.canonical_key))
        .count() as u32;
    let response_count = source_plan.selected_sources.iter()
        .filter(|s| source_plan.response_phase_keys.contains(&s.canonical_key))
        .count() as u32;

    let mut url_mappings = HashMap::new();
    for s in &source_plan.all_sources {
        if let Some(ref url) = s.url {
            url_mappings
                .entry(sanitize_url(url))
                .or_insert_with(|| s.canonical_key.clone());
        }
    }

    info!(
        selected = source_plan.selected_sources.len(),
        tension = tension_count,
        response = response_count,
        "Source plan finalized"
    );

    SourcePlanOutput {
        source_plan,
        actor_contexts,
        url_mappings,
        tension_count,
        response_count,
    }
}
