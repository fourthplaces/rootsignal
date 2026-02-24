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
    let recent: Vec<&SignalLocation> = signals
        .iter()
        .filter(|s| s.observed_at >= cutoff)
        .collect();

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
        let entry = votes.entry(bio.name.as_str()).or_insert((0, bio.lat, bio.lng));
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
            if sorted.iter().any(|(name, (count, _, _))| *count == *top_count && *name == cur.name.as_str()) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn loc(name: &str, lat: f64, lng: f64) -> ActorLocation {
        ActorLocation { lat, lng, name: name.to_string() }
    }

    fn signal(name: &str, lat: f64, lng: f64, days_ago: i64) -> SignalLocation {
        SignalLocation {
            lat,
            lng,
            name: name.to_string(),
            observed_at: Utc::now() - Duration::days(days_ago),
        }
    }

    const MAX_AGE: u64 = 90;

    // Phillips neighborhood
    const PHILLIPS: (f64, f64) = (44.9489, -93.2601);
    // Powderhorn neighborhood
    const POWDERHORN: (f64, f64) = (44.9367, -93.2393);

    #[test]
    fn corroborated_bio_overrides_signal_mode() {
        let bio = loc("Phillips", PHILLIPS.0, PHILLIPS.1);
        let signals = vec![
            signal("Phillips", PHILLIPS.0, PHILLIPS.1, 5),
            signal("Powderhorn", POWDERHORN.0, POWDERHORN.1, 3),
            signal("Powderhorn", POWDERHORN.0, POWDERHORN.1, 7),
        ];
        let result = triangulate_actor_location(None, Some(&bio), &signals, MAX_AGE);
        assert_eq!(result.as_ref().map(|l| l.name.as_str()), Some("Phillips"));
    }

    #[test]
    fn uncorroborated_bio_treated_as_one_signal() {
        let bio = loc("Phillips", PHILLIPS.0, PHILLIPS.1);
        let signals = vec![
            signal("Powderhorn", POWDERHORN.0, POWDERHORN.1, 1),
            signal("Powderhorn", POWDERHORN.0, POWDERHORN.1, 3),
            signal("Powderhorn", POWDERHORN.0, POWDERHORN.1, 5),
        ];
        let result = triangulate_actor_location(None, Some(&bio), &signals, MAX_AGE);
        assert_eq!(result.as_ref().map(|l| l.name.as_str()), Some("Powderhorn"));
    }

    #[test]
    fn signal_mode_used_when_no_bio() {
        let signals = vec![
            signal("Phillips", PHILLIPS.0, PHILLIPS.1, 2),
            signal("Phillips", PHILLIPS.0, PHILLIPS.1, 4),
            signal("Phillips", PHILLIPS.0, PHILLIPS.1, 6),
            signal("Powderhorn", POWDERHORN.0, POWDERHORN.1, 1),
        ];
        let result = triangulate_actor_location(None, None, &signals, MAX_AGE);
        assert_eq!(result.as_ref().map(|l| l.name.as_str()), Some("Phillips"));
    }

    #[test]
    fn tie_keeps_current_location() {
        let current = loc("Phillips", PHILLIPS.0, PHILLIPS.1);
        let signals = vec![
            signal("Phillips", PHILLIPS.0, PHILLIPS.1, 2),
            signal("Phillips", PHILLIPS.0, PHILLIPS.1, 4),
            signal("Powderhorn", POWDERHORN.0, POWDERHORN.1, 1),
            signal("Powderhorn", POWDERHORN.0, POWDERHORN.1, 3),
        ];
        let result = triangulate_actor_location(Some(&current), None, &signals, MAX_AGE);
        assert_eq!(result.as_ref().map(|l| l.name.as_str()), Some("Phillips"));
    }

    #[test]
    fn minimum_two_signals_required_for_change() {
        let signals = vec![
            signal("Phillips", PHILLIPS.0, PHILLIPS.1, 2),
        ];
        let result = triangulate_actor_location(None, None, &signals, MAX_AGE);
        assert!(result.is_none(), "single signal is not enough evidence");
    }

    #[test]
    fn old_signals_excluded_from_mode() {
        let signals = vec![
            signal("Phillips", PHILLIPS.0, PHILLIPS.1, 120),
            signal("Phillips", PHILLIPS.0, PHILLIPS.1, 100),
            signal("Phillips", PHILLIPS.0, PHILLIPS.1, 95),
            signal("Powderhorn", POWDERHORN.0, POWDERHORN.1, 5),
            signal("Powderhorn", POWDERHORN.0, POWDERHORN.1, 10),
        ];
        let result = triangulate_actor_location(None, None, &signals, MAX_AGE);
        assert_eq!(result.as_ref().map(|l| l.name.as_str()), Some("Powderhorn"));
    }

    #[test]
    fn no_signals_returns_current_location() {
        let current = loc("Phillips", PHILLIPS.0, PHILLIPS.1);
        let result = triangulate_actor_location(Some(&current), None, &[], MAX_AGE);
        assert_eq!(result, Some(current));
    }

    #[test]
    fn actor_moves_from_phillips_to_powderhorn_over_time() {
        let current = loc("Phillips", PHILLIPS.0, PHILLIPS.1);
        let signals = vec![
            // Old Phillips signals (beyond max age)
            signal("Phillips", PHILLIPS.0, PHILLIPS.1, 120),
            signal("Phillips", PHILLIPS.0, PHILLIPS.1, 110),
            signal("Phillips", PHILLIPS.0, PHILLIPS.1, 100),
            signal("Phillips", PHILLIPS.0, PHILLIPS.1, 95),
            signal("Phillips", PHILLIPS.0, PHILLIPS.1, 92),
            // Recent Powderhorn signals
            signal("Powderhorn", POWDERHORN.0, POWDERHORN.1, 5),
            signal("Powderhorn", POWDERHORN.0, POWDERHORN.1, 10),
            signal("Powderhorn", POWDERHORN.0, POWDERHORN.1, 20),
        ];
        let result = triangulate_actor_location(Some(&current), None, &signals, MAX_AGE);
        assert_eq!(result.as_ref().map(|l| l.name.as_str()), Some("Powderhorn"));
    }
}
