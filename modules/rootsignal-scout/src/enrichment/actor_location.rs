//! Actor location triangulation.
//!
//! Determines an actor's location from two evidence sources:
//! 1. Bio text ("Based in Phillips") — parsed upstream, passed as `bio_location`.
//! 2. Mode of recent signal `about_location` values — the neighborhood that
//!    appears most frequently in the actor's recent signals.
//!
//! Rules:
//! - A bio location corroborated by at least one signal wins outright.
//! - An uncorroborated bio is treated as a single signal vote.
//! - Signal mode (most frequent location) is used when there's no bio.
//! - Ties preserve the actor's current location (inertia).
//! - At least 2 signals are required to change an actor's location.
//! - Signals older than `max_age_days` are excluded.

use chrono::{DateTime, Utc};

/// A single signal location observation for triangulation.
#[derive(Debug, Clone)]
pub struct SignalLocation {
    pub lat: f64,
    pub lng: f64,
    pub name: String,
    pub observed_at: DateTime<Utc>,
}

/// The current (or absent) location of an actor.
#[derive(Debug, Clone, PartialEq)]
pub struct ActorLocation {
    pub lat: f64,
    pub lng: f64,
    pub name: String,
}

/// Triangulate an actor's location from bio + recent signal observations.
///
/// Returns `None` if there isn't enough evidence to determine a location.
pub fn triangulate_actor_location(
    current: Option<&ActorLocation>,
    bio_location: Option<&ActorLocation>,
    signals: &[SignalLocation],
    max_age_days: u64,
) -> Option<ActorLocation> {
    let cutoff = Utc::now() - chrono::Duration::days(max_age_days as i64);

    // Filter to recent signals only
    let recent: Vec<&SignalLocation> = signals.iter().filter(|s| s.observed_at >= cutoff).collect();

    // No recent signals and no bio — preserve current location
    if recent.is_empty() && bio_location.is_none() {
        return current.cloned();
    }

    // Count votes per location name
    let mut votes: std::collections::HashMap<&str, (usize, f64, f64)> =
        std::collections::HashMap::new();
    for s in &recent {
        let entry = votes.entry(s.name.as_str()).or_insert((0, s.lat, s.lng));
        entry.0 += 1;
    }

    // Bio location: if corroborated by at least one signal, it wins outright.
    // If uncorroborated, it counts as one additional vote.
    if let Some(bio) = bio_location {
        if votes.contains_key(bio.name.as_str()) {
            return Some(bio.clone());
        }
        // Uncorroborated bio = one vote
        let entry = votes
            .entry(bio.name.as_str())
            .or_insert((0, bio.lat, bio.lng));
        entry.0 += 1;
    }

    // Find the mode (most frequent location)
    let total_votes: usize = votes.values().map(|(c, _, _)| c).sum();
    if total_votes < 2 {
        return current.cloned();
    }

    let mut sorted: Vec<_> = votes.into_iter().collect();
    sorted.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));

    let (top_name, (top_count, top_lat, top_lng)) = &sorted[0];
    let is_tie = sorted.len() > 1 && sorted[1].1 .0 == *top_count;

    if is_tie {
        // Tie → preserve current location (inertia)
        if let Some(cur) = current {
            // If current location is one of the tied leaders, keep it
            if sorted
                .iter()
                .any(|(name, (count, _, _))| *count == *top_count && *name == cur.name.as_str())
            {
                return Some(cur.clone());
            }
        }
        // No current location or current not in tie — pick the top (arbitrary but deterministic)
    }

    Some(ActorLocation {
        lat: *top_lat,
        lng: *top_lng,
        name: top_name.to_string(),
    })
}

/// Max age in days for signal observations used in triangulation.
const ENRICHMENT_MAX_AGE_DAYS: u64 = 90;

/// Enrich actor locations by triangulating from their authored signals.
///
/// For each actor, queries signals via `store`, builds the evidence set,
/// calls `triangulate_actor_location`, and persists the result via engine dispatch.
///
/// Returns the count of actors whose location was updated.
pub async fn enrich_actor_locations(
    store: &dyn crate::traits::SignalReader,
    engine: &crate::pipeline::ScoutEngine,
    deps: &crate::pipeline::state::PipelineDeps,
    actors: &[(
        rootsignal_common::ActorNode,
        Vec<rootsignal_common::SourceNode>,
    )],
) -> u32 {
    use rootsignal_common::events::WorldEvent;
    use crate::pipeline::events::ScoutEvent;

    let mut updated = 0;
    let mut state = crate::pipeline::state::PipelineState::new(std::collections::HashMap::new());
    for (actor, _sources) in actors {
        let signals = match store.get_signals_for_actor(actor.id).await {
            Ok(s) => s,
            Err(_) => continue,
        };

        let signal_locs: Vec<SignalLocation> = signals
            .iter()
            .map(|(lat, lng, name, ts)| SignalLocation {
                lat: *lat,
                lng: *lng,
                name: name.clone(),
                observed_at: *ts,
            })
            .collect();

        let current = match (
            &actor.location_lat,
            &actor.location_lng,
            &actor.location_name,
        ) {
            (Some(lat), Some(lng), Some(name)) => Some(ActorLocation {
                lat: *lat,
                lng: *lng,
                name: name.clone(),
            }),
            _ => None,
        };

        // Extract bio location: if the actor's bio text contains a known signal
        // location name (case-insensitive substring), use that signal's coordinates.
        let bio_location = actor.bio.as_ref().and_then(|bio| {
            let bio_lower = bio.to_lowercase();
            signal_locs.iter().find_map(|sl| {
                if !sl.name.is_empty() && bio_lower.contains(&sl.name.to_lowercase()) {
                    Some(ActorLocation {
                        lat: sl.lat,
                        lng: sl.lng,
                        name: sl.name.clone(),
                    })
                } else {
                    None
                }
            })
        });

        let result = triangulate_actor_location(
            current.as_ref(),
            bio_location.as_ref(),
            &signal_locs,
            ENRICHMENT_MAX_AGE_DAYS,
        );

        if let Some(new_loc) = result {
            let changed = current.as_ref().map_or(true, |c| c.name != new_loc.name);
            if changed {
                let event = ScoutEvent::World(WorldEvent::ActorLocationIdentified {
                    actor_id: actor.id,
                    location_lat: new_loc.lat,
                    location_lng: new_loc.lng,
                    location_name: if new_loc.name.is_empty() {
                        None
                    } else {
                        Some(new_loc.name.clone())
                    },
                });
                if engine.dispatch(event, &mut state, deps).await.is_ok() {
                    updated += 1;
                }
            }
        }
    }
    updated
}
