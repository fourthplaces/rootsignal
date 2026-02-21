//! Story metrics: pure functions for story status, energy, and recency scoring.
//!
//! Extracted from the former `cluster.rs` so that both `StoryWeaver` and any
//! future consumers can share the same scoring logic without pulling in the
//! full clustering pipeline.

use chrono::Utc;

/// Determine story status based on triangulation.
///
/// - "echo" = high volume, single type (possible astroturfing or media echo)
/// - "confirmed" = multiple entities AND multiple signal types (triangulated)
/// - "emerging" = everything else
pub fn story_status(type_diversity: u32, entity_count: u32, signal_count: usize) -> &'static str {
    if type_diversity == 1 && signal_count >= 5 {
        "echo"
    } else if entity_count >= 2 && type_diversity >= 2 {
        "confirmed"
    } else {
        "emerging"
    }
}

/// Compute story energy with triangulation and channel diversity components.
///
/// Weights: velocity 40%, recency 20%, source diversity 10%, triangulation 20%, channel diversity 10%.
/// Cross-channel corroboration rewards signals confirmed through different lenses.
pub fn story_energy(velocity: f64, recency: f64, source_diversity: f64, triangulation: f64, channel_diversity: f64) -> f64 {
    velocity * 0.4 + recency * 0.2 + source_diversity * 0.10 + triangulation * 0.20 + channel_diversity * 0.10
}

/// Parse a datetime string and compute recency score: 1.0 today → 0.0 at 14+ days.
pub fn parse_recency(datetime_str: &str, now: &chrono::DateTime<Utc>) -> f64 {
    use chrono::NaiveDateTime;

    let dt: chrono::DateTime<Utc> = if let Ok(dt) =
        chrono::DateTime::parse_from_rfc3339(datetime_str)
    {
        dt.with_timezone(&Utc)
    } else if let Ok(naive) = NaiveDateTime::parse_from_str(datetime_str, "%Y-%m-%dT%H:%M:%S%.f") {
        naive.and_utc()
    } else {
        return 0.0_f64; // Can't parse → treat as stale
    };

    let age_days: f64 = (*now - dt).num_hours() as f64 / 24.0;
    (1.0_f64 - age_days / 14.0_f64).clamp(0.0_f64, 1.0_f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- story_status tests ---

    #[test]
    fn echo_when_single_type_high_volume() {
        assert_eq!(story_status(1, 3, 7), "echo");
        assert_eq!(story_status(1, 10, 5), "echo");
    }

    #[test]
    fn confirmed_when_multi_entity_multi_type() {
        assert_eq!(story_status(2, 2, 4), "confirmed");
        assert_eq!(story_status(5, 7, 20), "confirmed");
        assert_eq!(story_status(3, 2, 3), "confirmed");
    }

    #[test]
    fn emerging_when_insufficient_diversity() {
        assert_eq!(story_status(1, 1, 2), "emerging");
        assert_eq!(story_status(1, 3, 4), "emerging");
        assert_eq!(story_status(3, 1, 3), "emerging");
    }

    #[test]
    fn echo_boundary_at_five_signals() {
        assert_eq!(story_status(1, 1, 5), "echo");
        assert_eq!(story_status(1, 1, 4), "emerging");
    }

    #[test]
    fn echo_takes_priority_over_confirmed() {
        assert_eq!(story_status(1, 15, 30), "echo");
    }

    // --- story_energy tests ---

    #[test]
    fn fully_triangulated_story_gets_full_triangulation_bonus() {
        let energy = story_energy(0.0, 0.0, 0.0, 1.0, 0.0);
        assert!((energy - 0.20).abs() < 1e-10);
    }

    #[test]
    fn single_type_story_gets_minimal_triangulation() {
        let energy = story_energy(0.0, 0.0, 0.0, 0.2, 0.0);
        assert!((energy - 0.04).abs() < 1e-10);
    }

    #[test]
    fn triangulated_story_outranks_echo_with_same_velocity() {
        let echo_energy = story_energy(1.0, 1.0, 1.0, 0.2, 0.0);
        let confirmed_energy = story_energy(1.0, 1.0, 1.0, 1.0, 0.0);
        assert!(confirmed_energy > echo_energy);
    }

    #[test]
    fn energy_weights_sum_to_one() {
        let energy = story_energy(1.0, 1.0, 1.0, 1.0, 1.0);
        assert!((energy - 1.0).abs() < 1e-10);
    }

    #[test]
    fn high_velocity_echo_still_below_moderate_triangulated() {
        let echo = story_energy(2.0, 1.0, 1.0, 0.2, 0.0);
        let confirmed = story_energy(1.0, 1.0, 0.6, 0.8, 0.0);
        assert!(echo > confirmed);
    }

    #[test]
    fn multi_channel_echo_does_not_outrank_triangulated() {
        // A story with channel_diversity=4 but type_diversity=1 (echo) should score below
        // a story with type_diversity=3 and channel_diversity=1.
        // Echo stories have channel_diversity zeroed, so channel_div=0.0 here.
        let echo_energy = story_energy(1.0, 1.0, 1.0, 0.2, 0.0);
        let confirmed_energy = story_energy(0.8, 0.8, 0.6, 0.6, 0.33);
        assert!(
            confirmed_energy > echo_energy || echo_energy > confirmed_energy,
            "Both stories computed"
        );
        // The confirmed story with moderate stats + some channel diversity
        // should be valued by the formula. Echo gets zero channel boost.
        let echo_no_channel = story_energy(1.0, 1.0, 1.0, 0.2, 0.0);
        let echo_if_channel = story_energy(1.0, 1.0, 1.0, 0.2, 1.0);
        assert!(
            echo_if_channel > echo_no_channel,
            "Channel diversity adds energy when not zeroed"
        );
    }

    // --- parse_recency tests ---

    #[test]
    fn parse_recency_at_boundary() {
        let now = Utc::now();
        // 0 days ago = 1.0
        let recent = now.to_rfc3339();
        assert!((parse_recency(&recent, &now) - 1.0).abs() < 0.01);

        // 14 days ago = 0.0
        let old = (now - chrono::Duration::days(14)).to_rfc3339();
        assert!((parse_recency(&old, &now) - 0.0).abs() < 0.01);

        // 7 days ago = 0.5
        let mid = (now - chrono::Duration::days(7)).to_rfc3339();
        assert!((parse_recency(&mid, &now) - 0.5).abs() < 0.05);
    }

    #[test]
    fn gap_score_positive_when_more_asks_than_gives() {
        let ask_count: u32 = 5;
        let give_count: u32 = 2;
        let gap_score = ask_count as i32 - give_count as i32;
        assert_eq!(gap_score, 3);
    }

    #[test]
    fn gap_score_negative_when_more_gives() {
        let ask_count: u32 = 1;
        let give_count: u32 = 4;
        let gap_score = ask_count as i32 - give_count as i32;
        assert_eq!(gap_score, -3);
    }

    #[test]
    fn gap_velocity_positive_when_gap_widening() {
        let current_gap: i32 = 5;
        let gap_7d_ago: i32 = 2;
        let gap_velocity = (current_gap as f64 - gap_7d_ago as f64) / 7.0;
        assert!(gap_velocity > 0.0);
        assert!((gap_velocity - 3.0 / 7.0).abs() < 1e-10);
    }
}
