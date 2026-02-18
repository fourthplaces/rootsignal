use chrono::{DateTime, Utc};
use tracing::info;

use rootsignal_common::{SourceNode, SourceRole};

/// Determines which sources to scrape this run based on weight, cadence, and exploration policy.
pub struct SourceScheduler {
    /// Fraction of scrape slots reserved for exploring low-weight sources (default 0.1).
    exploration_ratio: f64,
    /// Sources below this weight are considered "low weight" for exploration.
    exploration_weight_threshold: f64,
    /// Minimum days since last scrape before a low-weight source is eligible for exploration.
    exploration_min_stale_days: i64,
}

/// Result of scheduling: which sources to scrape and why.
pub struct ScheduleResult {
    /// Sources selected for normal scraping (above cadence threshold).
    pub scheduled: Vec<ScheduledSource>,
    /// Sources selected for exploration (random sampling of low-weight stale sources).
    pub exploration: Vec<ScheduledSource>,
    /// Sources skipped (not yet due based on cadence).
    pub skipped: usize,
    /// Convenience partition: canonical keys of sources with role=Tension or Mixed (Phase A).
    pub tension_phase: Vec<String>,
    /// Convenience partition: canonical keys of sources with role=Response (Phase B).
    pub response_phase: Vec<String>,
}

pub struct ScheduledSource {
    pub canonical_key: String,
    pub reason: ScheduleReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleReason {
    /// Due based on weight-derived cadence.
    Cadence,
    /// Never been scraped.
    NeverScraped,
    /// Selected for exploration (random sampling of low-weight sources).
    Exploration,
}

impl SourceScheduler {
    pub fn new() -> Self {
        Self {
            exploration_ratio: 0.10,
            exploration_weight_threshold: 0.3,
            exploration_min_stale_days: 14,
        }
    }

    /// Schedule sources for this run. Returns which to scrape and which to skip.
    pub fn schedule(&self, sources: &[SourceNode], now: DateTime<Utc>) -> ScheduleResult {
        let mut scheduled = Vec::new();
        let mut exploration_candidates = Vec::new();
        let mut skipped = 0usize;

        for source in sources {
            if self.should_scrape(source, now) {
                scheduled.push(ScheduledSource {
                    canonical_key: source.canonical_key.clone(),
                    reason: if source.last_scraped.is_none() {
                        ScheduleReason::NeverScraped
                    } else {
                        ScheduleReason::Cadence
                    },
                });
            } else if self.is_exploration_candidate(source, now) {
                exploration_candidates.push(source);
            } else {
                skipped += 1;
            }
        }

        // Reserve exploration slots: 10% of total scheduled, minimum 1 if candidates exist
        let total_slots = scheduled.len() + exploration_candidates.len();
        let exploration_slots = if exploration_candidates.is_empty() {
            0
        } else {
            ((total_slots as f64 * self.exploration_ratio).ceil() as usize).max(1)
        };

        // Pick exploration sources — deterministic: sort by staleness (most stale first)
        let mut exploration_candidates: Vec<_> = exploration_candidates.into_iter().collect();
        exploration_candidates.sort_by(|a, b| {
            let a_stale = a.last_scraped.map(|t| (now - t).num_days()).unwrap_or(i64::MAX);
            let b_stale = b.last_scraped.map(|t| (now - t).num_days()).unwrap_or(i64::MAX);
            b_stale.cmp(&a_stale) // most stale first
        });

        let exploration: Vec<ScheduledSource> = exploration_candidates
            .into_iter()
            .take(exploration_slots)
            .map(|s| ScheduledSource {
                canonical_key: s.canonical_key.clone(),
                reason: ScheduleReason::Exploration,
            })
            .collect();

        let exploration_picked = exploration.len();
        if exploration_picked > 0 {
            info!(
                scheduled = scheduled.len(),
                exploration = exploration_picked,
                skipped,
                "Source scheduling complete"
            );
        }

        // Build role lookup for partition
        let role_map: std::collections::HashMap<&str, SourceRole> = sources
            .iter()
            .map(|s| (s.canonical_key.as_str(), s.source_role))
            .collect();

        // Partition all scheduled+exploration keys by source role
        let all_keys: Vec<&str> = scheduled
            .iter()
            .chain(exploration.iter())
            .map(|s| s.canonical_key.as_str())
            .collect();

        let mut tension_phase = Vec::new();
        let mut response_phase = Vec::new();
        for key in all_keys {
            match role_map.get(key).copied().unwrap_or(SourceRole::Mixed) {
                SourceRole::Response => response_phase.push(key.to_string()),
                // Tension and Mixed both go in Phase A
                SourceRole::Tension | SourceRole::Mixed => tension_phase.push(key.to_string()),
            }
        }

        ScheduleResult {
            scheduled,
            exploration,
            skipped,
            tension_phase,
            response_phase,
        }
    }

    /// Check if a source is due for scraping based on its weight-derived cadence.
    fn should_scrape(&self, source: &SourceNode, now: DateTime<Utc>) -> bool {
        let last = match source.last_scraped {
            Some(t) => t,
            None => return true, // Never scraped — always due
        };

        let cadence_hours = source.cadence_hours
            .unwrap_or_else(|| cadence_hours_for_weight(source.weight));
        let hours_since = (now - last).num_hours();

        hours_since >= cadence_hours as i64
    }

    /// Check if a source is eligible for exploration sampling.
    fn is_exploration_candidate(&self, source: &SourceNode, now: DateTime<Utc>) -> bool {
        if source.weight >= self.exploration_weight_threshold {
            return false;
        }
        match source.last_scraped {
            Some(t) => (now - t).num_days() >= self.exploration_min_stale_days,
            None => true,
        }
    }
}

/// Map weight to scrape cadence in hours.
pub fn cadence_hours_for_weight(weight: f64) -> u32 {
    if weight > 0.8 {
        6
    } else if weight > 0.5 {
        24
    } else if weight > 0.2 {
        72
    } else {
        168 // 7 days
    }
}

/// Compute source weight from observable metrics.
///
/// Formula: `base_yield * tension_bonus * recency_factor * diversity_factor`
///
/// - `base_yield`: rolling signals/scrapes ratio with Bayesian smoothing for n < 5
/// - `tension_bonus`: 1.0 + (tension_signals / total_signals), capped at 2.0
/// - `recency_factor`: decays to 0.5 if no signal in 30+ days
/// - `diversity_factor`: 1.5x if signals get corroborated by other sources
pub fn compute_weight(
    signals_produced: u32,
    signals_corroborated: u32,
    scrape_count: u32,
    tension_count: u32,
    last_produced_signal: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> f64 {
    // Base yield with Bayesian smoothing
    let prior_yield = 0.3;
    let k = 3.0; // smoothing strength
    let n = scrape_count as f64;
    let actual_yield = if scrape_count > 0 {
        (signals_produced as f64 / scrape_count as f64).min(1.0)
    } else {
        0.0
    };
    let base_yield = if n < 5.0 {
        (actual_yield * n + prior_yield * k) / (n + k)
    } else {
        actual_yield
    };

    // Tension bonus: sources producing tension signals get boosted
    let tension_bonus = if signals_produced > 0 {
        (1.0 + tension_count as f64 / signals_produced as f64).min(2.0)
    } else {
        1.0
    };

    // Recency factor: decay if no signal in 30+ days
    let recency_factor = match last_produced_signal {
        Some(t) => {
            let days = (now - t).num_days();
            if days < 30 {
                1.0
            } else {
                0.5_f64.max(1.0 - (days - 30) as f64 / 60.0)
            }
        }
        None => 0.7, // Never produced — slight penalty but not harsh
    };

    // Diversity factor: corroboration means this source provides independent evidence
    let diversity_factor = if signals_produced > 0 && signals_corroborated > 0 {
        let corroboration_ratio = signals_corroborated as f64 / signals_produced as f64;
        1.0 + (corroboration_ratio * 0.5).min(0.5) // up to 1.5x
    } else {
        1.0
    };

    let raw = base_yield * tension_bonus * recency_factor * diversity_factor;

    // Clamp to [0.1, 1.0]
    raw.clamp(0.1, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use rootsignal_common::{DiscoveryMethod, SourceType};
    use uuid::Uuid;

    fn make_source(weight: f64, last_scraped: Option<DateTime<Utc>>) -> SourceNode {
        SourceNode {
            id: Uuid::new_v4(),
            canonical_key: format!("test:web:{}", Uuid::new_v4()),
            canonical_value: "test".to_string(),
            url: Some("https://example.com".to_string()),
            source_type: SourceType::Web,
            discovery_method: DiscoveryMethod::Curated,
            city: "test".to_string(),
            created_at: Utc::now(),
            last_scraped,
            last_produced_signal: None,
            signals_produced: 0,
            signals_corroborated: 0,
            consecutive_empty_runs: 0,
            active: true,
            gap_context: None,
            weight,
            cadence_hours: None,
            avg_signals_per_scrape: 0.0,
            total_cost_cents: 0,
            last_cost_cents: 0,
            taxonomy_stats: None,
            quality_penalty: 1.0,
            source_role: SourceRole::default(),
        }
    }

    #[test]
    fn never_scraped_always_scheduled() {
        let scheduler = SourceScheduler::new();
        let sources = vec![make_source(0.5, None)];
        let result = scheduler.schedule(&sources, Utc::now());
        assert_eq!(result.scheduled.len(), 1);
        assert_eq!(result.scheduled[0].reason, ScheduleReason::NeverScraped);
    }

    #[test]
    fn high_weight_source_scraped_every_6_hours() {
        let scheduler = SourceScheduler::new();
        let now = Utc::now();

        // Last scraped 7 hours ago, weight > 0.8 → cadence is 6h → due
        let sources = vec![make_source(0.9, Some(now - Duration::hours(7)))];
        let result = scheduler.schedule(&sources, now);
        assert_eq!(result.scheduled.len(), 1);

        // Last scraped 3 hours ago → not due
        let sources = vec![make_source(0.9, Some(now - Duration::hours(3)))];
        let result = scheduler.schedule(&sources, now);
        assert_eq!(result.scheduled.len(), 0);
    }

    #[test]
    fn low_weight_source_scraped_every_7_days() {
        let scheduler = SourceScheduler::new();
        let now = Utc::now();

        // Weight 0.1, last scraped 8 days ago → due (cadence = 168h = 7 days)
        let sources = vec![make_source(0.1, Some(now - Duration::days(8)))];
        let result = scheduler.schedule(&sources, now);
        assert_eq!(result.scheduled.len(), 1);

        // Last scraped 3 days ago → not due
        let sources = vec![make_source(0.1, Some(now - Duration::days(3)))];
        let result = scheduler.schedule(&sources, now);
        assert_eq!(result.scheduled.len(), 0);
    }

    #[test]
    fn exploration_picks_stale_low_weight_sources() {
        let scheduler = SourceScheduler::new();
        let now = Utc::now();

        // 10 high-weight sources (recently scraped so they're scheduled via cadence)
        let mut sources: Vec<SourceNode> = (0..10)
            .map(|_| make_source(0.9, Some(now - Duration::hours(7))))
            .collect();

        // 3 low-weight sources scraped 15 days ago — with weight 0.15, cadence is 168h (7 days),
        // so they ARE due by cadence. Set last_scraped to 5 days ago so they're NOT due by
        // cadence (168h > 120h) but ARE stale enough for exploration (>14 days stale? No...)
        // Actually, exploration requires 14+ days stale. Let's use a custom cadence_hours override
        // to make them not due by cadence but eligible for exploration.
        for _ in 0..3 {
            let mut s = make_source(0.15, Some(now - Duration::days(15)));
            // Override cadence to 30 days so they're not due by cadence
            s.cadence_hours = Some(720);
            sources.push(s);
        }

        let result = scheduler.schedule(&sources, now);
        assert_eq!(result.scheduled.len(), 10);
        assert!(!result.exploration.is_empty(), "Should have exploration picks");
        assert!(result.exploration.len() <= 3);
        for e in &result.exploration {
            assert_eq!(e.reason, ScheduleReason::Exploration);
        }
    }

    #[test]
    fn cadence_hours_mapping() {
        assert_eq!(cadence_hours_for_weight(0.9), 6);
        assert_eq!(cadence_hours_for_weight(0.6), 24);
        assert_eq!(cadence_hours_for_weight(0.3), 72);
        assert_eq!(cadence_hours_for_weight(0.1), 168);
    }

    #[test]
    fn weight_formula_bayesian_smoothing() {
        let now = Utc::now();

        // New source with 1 scrape, 1 signal — should be smoothed toward prior
        let w = compute_weight(1, 0, 1, 0, Some(now), now);
        // Without smoothing: 1.0. With smoothing (k=3, prior=0.3): (1.0*1 + 0.3*3)/(1+3) = 1.9/4 = 0.475
        assert!(w < 0.6, "Bayesian smoothing should reduce weight for n=1: {w}");

        // Source with 10 scrapes, 5 signals — no smoothing needed
        let w = compute_weight(5, 0, 10, 0, Some(now), now);
        // actual_yield = 0.5, no smoothing
        assert!((w - 0.5).abs() < 0.1, "Established source weight should be ~0.5: {w}");
    }

    #[test]
    fn weight_tension_bonus() {
        let now = Utc::now();
        // Use 5 signals out of 10 scrapes (base_yield=0.5) so bonus is visible before clamping
        let base = compute_weight(5, 0, 10, 0, Some(now), now);
        let with_tension = compute_weight(5, 0, 10, 3, Some(now), now);
        assert!(with_tension > base, "Tension bonus should increase weight: base={base}, with_tension={with_tension}");
    }

    #[test]
    fn weight_recency_decay() {
        let now = Utc::now();
        let recent = compute_weight(5, 0, 10, 0, Some(now - Duration::days(5)), now);
        let stale = compute_weight(5, 0, 10, 0, Some(now - Duration::days(60)), now);
        assert!(stale < recent, "Stale source should have lower weight: recent={recent}, stale={stale}");
    }

    #[test]
    fn weight_clamped_to_floor() {
        let now = Utc::now();
        // 0 signals, 50 scrapes, very stale → should hit floor of 0.1
        let w = compute_weight(0, 0, 50, 0, Some(now - Duration::days(90)), now);
        assert!((w - 0.1).abs() < 0.01, "Weight should be clamped to floor: {w}");
    }

    #[test]
    fn weight_corroboration_bonus() {
        let now = Utc::now();
        // Use 5 signals out of 10 scrapes so bonus is visible before clamping
        let base = compute_weight(5, 0, 10, 0, Some(now), now);
        let corroborated = compute_weight(5, 3, 10, 0, Some(now), now);
        assert!(corroborated > base, "Corroboration should boost weight: base={base}, corroborated={corroborated}");
    }
}
