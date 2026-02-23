use std::collections::HashMap;

use chrono::{DateTime, Utc};
use tracing::info;

use rootsignal_common::{is_web_query, DiscoveryMethod, SourceNode, SourceRole};

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
            exploration_min_stale_days: 5,
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
            let a_stale = a
                .last_scraped
                .map(|t| (now - t).num_days())
                .unwrap_or(i64::MAX);
            let b_stale = b
                .last_scraped
                .map(|t| (now - t).num_days())
                .unwrap_or(i64::MAX);
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

        let cadence_hours = source
            .cadence_hours
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

/// Like `cadence_hours_for_weight` but applies exponential backoff based on
/// consecutive empty runs. Queries that keep returning nothing get scraped
/// less frequently, freeing budget for productive queries.
///
/// - 0-1 empties: 1x cadence (benefit of the doubt)
/// - 2 empties: 2x
/// - 3 empties: 4x
/// - 4 empties: 8x
/// - dormancy threshold+: dormant (u32::MAX — only resurrected via cold tier scheduling)
///
/// SocialGraphFollow sources use a lower dormancy threshold (3 empty runs)
/// since they're speculative and should be pruned faster.
pub fn cadence_hours_with_backoff(
    weight: f64,
    consecutive_empty_runs: u32,
    method: &DiscoveryMethod,
) -> u32 {
    let base = cadence_hours_for_weight(weight);
    let threshold = dormancy_threshold(method);
    if consecutive_empty_runs >= threshold {
        return u32::MAX; // Dormant
    }
    let multiplier = match consecutive_empty_runs {
        0..=1 => 1,
        2 => 2,
        3 => 4,
        4 => 8,
        _ => return u32::MAX,
    };
    base.saturating_mul(multiplier)
}

/// Number of consecutive empty runs before a source goes dormant.
/// SocialGraphFollow sources are speculative — they go dormant faster.
pub fn dormancy_threshold(method: &DiscoveryMethod) -> u32 {
    match method {
        DiscoveryMethod::SocialGraphFollow => 3,
        _ => 5,
    }
}

/// Returns true if a source should be considered dormant
/// (only eligible for resurrection via cold-tier scheduling).
pub fn is_dormant(consecutive_empty_runs: u32, method: &DiscoveryMethod) -> bool {
    consecutive_empty_runs >= dormancy_threshold(method)
}

// =============================================================================
// Web Query Tiered Scheduling
// =============================================================================

/// Result of web query scheduling.
#[derive(Debug)]
pub struct WebQueryScheduleResult {
    /// Canonical keys of queries selected for this run.
    pub scheduled: Vec<String>,
    /// How many went into each tier.
    pub hot: usize,
    pub warm: usize,
    pub cold: usize,
    /// How many were skipped (not scheduled).
    pub skipped: usize,
}

/// Schedule web queries for a run using tiered priority.
///
/// - **Hot tier (60%)**: Top-scoring by `weight * tension_heat`, non-dormant,
///   per-tension cap of 3.
/// - **Warm tier (25%)**: Random sample from mid-range, non-dormant.
/// - **Cold tier (15%)**: Random sample from never-scraped or dormant queries
///   (seasonal resurrection).
///
/// `max_per_run` defaults to 50 if 0. Callers should pass `Config.max_web_queries_per_run`.
pub fn schedule_web_queries(
    sources: &[SourceNode],
    max_per_run: usize,
    now: DateTime<Utc>,
) -> WebQueryScheduleResult {
    let max = if max_per_run == 0 { 50 } else { max_per_run };

    // Filter to active web query sources only
    let web_queries: Vec<&SourceNode> = sources
        .iter()
        .filter(|s| is_web_query(&s.canonical_value) && s.active)
        .collect();

    if web_queries.is_empty() {
        return WebQueryScheduleResult {
            scheduled: vec![],
            hot: 0,
            warm: 0,
            cold: 0,
            skipped: 0,
        };
    }

    // Partition into non-dormant, dormant, and never-scraped
    let mut scoreable: Vec<(&SourceNode, f64)> = Vec::new();
    let mut cold_pool: Vec<&SourceNode> = Vec::new();

    for s in &web_queries {
        if s.last_scraped.is_none() || is_dormant(s.consecutive_empty_runs, &s.discovery_method) {
            cold_pool.push(s);
        } else {
            let tension_heat = extract_heat_from_gap_context(s.gap_context.as_deref());
            let score = s.weight as f64 * (1.0 + tension_heat);
            scoreable.push((s, score));
        }
    }

    // Sort scoreable by score descending
    scoreable.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Tier budgets
    let hot_budget = (max as f64 * 0.60).ceil() as usize;
    let warm_budget = (max as f64 * 0.25).ceil() as usize;
    let cold_budget = max
        .saturating_sub(hot_budget)
        .saturating_sub(warm_budget)
        .max((max as f64 * 0.15).ceil() as usize);

    // Hot tier: top-scoring with per-tension cap
    let mut hot_selected: Vec<String> = Vec::new();
    let mut tension_counts: HashMap<String, usize> = HashMap::new();
    let mut warm_candidates: Vec<&SourceNode> = Vec::new();
    const PER_TENSION_CAP: usize = 3;

    for (s, _score) in &scoreable {
        if hot_selected.len() >= hot_budget {
            warm_candidates.push(s);
            continue;
        }

        let tension_key = extract_tension_from_gap_context(s.gap_context.as_deref());
        let count = tension_counts.entry(tension_key.clone()).or_default();
        if *count >= PER_TENSION_CAP {
            warm_candidates.push(s);
            continue;
        }

        *count += 1;
        hot_selected.push(s.canonical_key.clone());
    }

    // Warm tier: deterministic sample from remaining non-dormant (take every Nth)
    let warm_selected: Vec<String> = if warm_candidates.is_empty() {
        vec![]
    } else {
        let step = (warm_candidates.len() as f64 / warm_budget.max(1) as f64).ceil() as usize;
        warm_candidates
            .iter()
            .step_by(step.max(1))
            .take(warm_budget)
            .map(|s| s.canonical_key.clone())
            .collect()
    };

    // Cold tier: deterministic sample from never-scraped + dormant (most stale first)
    let mut cold_sorted = cold_pool;
    cold_sorted.sort_by(|a, b| {
        let a_stale = a
            .last_scraped
            .map(|t| (now - t).num_days())
            .unwrap_or(i64::MAX);
        let b_stale = b
            .last_scraped
            .map(|t| (now - t).num_days())
            .unwrap_or(i64::MAX);
        b_stale.cmp(&a_stale)
    });
    let cold_selected: Vec<String> = cold_sorted
        .iter()
        .take(cold_budget)
        .map(|s| s.canonical_key.clone())
        .collect();

    let total_scheduled = hot_selected.len() + warm_selected.len() + cold_selected.len();
    let skipped = web_queries.len().saturating_sub(total_scheduled);

    info!(
        total = web_queries.len(),
        hot = hot_selected.len(),
        warm = warm_selected.len(),
        cold = cold_selected.len(),
        skipped,
        "Web query scheduling complete"
    );

    let mut scheduled = Vec::with_capacity(total_scheduled);
    let hot_count = hot_selected.len();
    let warm_count = warm_selected.len();
    let cold_count = cold_selected.len();
    scheduled.extend(hot_selected);
    scheduled.extend(warm_selected);
    scheduled.extend(cold_selected);

    WebQueryScheduleResult {
        scheduled,
        hot: hot_count,
        warm: warm_count,
        cold: cold_count,
        skipped,
    }
}

/// Extract tension heat from gap_context string.
/// Looks for "heat=" pattern (from briefing format) or defaults to 0.0.
fn extract_heat_from_gap_context(gap_context: Option<&str>) -> f64 {
    let ctx = match gap_context {
        Some(c) => c,
        None => return 0.0,
    };
    // Pattern: "heat=0.7" or "cause_heat: 0.7"
    for pattern in &["heat=", "cause_heat: ", "heat: "] {
        if let Some(pos) = ctx.find(pattern) {
            let start = pos + pattern.len();
            let end = ctx[start..]
                .find(|c: char| !c.is_ascii_digit() && c != '.')
                .map(|i| start + i)
                .unwrap_or(ctx.len());
            if let Ok(heat) = ctx[start..end].parse::<f64>() {
                return heat;
            }
        }
    }
    0.0
}

/// Extract a tension key from gap_context for per-tension capping.
/// Looks for "Related: ..." or "Tension: ..." patterns.
fn extract_tension_from_gap_context(gap_context: Option<&str>) -> String {
    let ctx = match gap_context {
        Some(c) => c,
        None => return "unknown".to_string(),
    };
    for prefix in &["Related: ", "Tension: ", "response discovery for \""] {
        if let Some(pos) = ctx.find(prefix) {
            let start = pos + prefix.len();
            let rest = &ctx[start..];
            // Take until next delimiter
            let end = rest
                .find(|c: char| c == '|' || c == '"' || c == '\n')
                .unwrap_or(rest.len());
            let tension = rest[..end].trim();
            if !tension.is_empty() && tension != "none" {
                return tension.to_lowercase();
            }
        }
    }
    "unknown".to_string()
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
    use rootsignal_common::DiscoveryMethod;
    use uuid::Uuid;

    fn make_source(weight: f64, last_scraped: Option<DateTime<Utc>>) -> SourceNode {
        SourceNode {
            id: Uuid::new_v4(),
            canonical_key: format!("test:{}", Uuid::new_v4()),
            canonical_value: "https://example.com".to_string(),
            url: Some("https://example.com".to_string()),
            discovery_method: DiscoveryMethod::Curated,
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
            quality_penalty: 1.0,
            source_role: SourceRole::default(),
            scrape_count: 0,
            center_lat: None,
            center_lng: None,
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
        assert!(
            !result.exploration.is_empty(),
            "Should have exploration picks"
        );
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
        assert!(
            w < 0.6,
            "Bayesian smoothing should reduce weight for n=1: {w}"
        );

        // Source with 10 scrapes, 5 signals — no smoothing needed
        let w = compute_weight(5, 0, 10, 0, Some(now), now);
        // actual_yield = 0.5, no smoothing
        assert!(
            (w - 0.5).abs() < 0.1,
            "Established source weight should be ~0.5: {w}"
        );
    }

    #[test]
    fn weight_tension_bonus() {
        let now = Utc::now();
        // Use 5 signals out of 10 scrapes (base_yield=0.5) so bonus is visible before clamping
        let base = compute_weight(5, 0, 10, 0, Some(now), now);
        let with_tension = compute_weight(5, 0, 10, 3, Some(now), now);
        assert!(
            with_tension > base,
            "Tension bonus should increase weight: base={base}, with_tension={with_tension}"
        );
    }

    #[test]
    fn weight_recency_decay() {
        let now = Utc::now();
        let recent = compute_weight(5, 0, 10, 0, Some(now - Duration::days(5)), now);
        let stale = compute_weight(5, 0, 10, 0, Some(now - Duration::days(60)), now);
        assert!(
            stale < recent,
            "Stale source should have lower weight: recent={recent}, stale={stale}"
        );
    }

    #[test]
    fn weight_clamped_to_floor() {
        let now = Utc::now();
        // 0 signals, 50 scrapes, very stale → should hit floor of 0.1
        let w = compute_weight(0, 0, 50, 0, Some(now - Duration::days(90)), now);
        assert!(
            (w - 0.1).abs() < 0.01,
            "Weight should be clamped to floor: {w}"
        );
    }

    #[test]
    fn weight_corroboration_bonus() {
        let now = Utc::now();
        // Use 5 signals out of 10 scrapes so bonus is visible before clamping
        let base = compute_weight(5, 0, 10, 0, Some(now), now);
        let corroborated = compute_weight(5, 3, 10, 0, Some(now), now);
        assert!(
            corroborated > base,
            "Corroboration should boost weight: base={base}, corroborated={corroborated}"
        );
    }

    #[test]
    fn weight_uses_actual_scrape_count_not_signal_count() {
        let now = Utc::now();
        // 5 signals from 20 scrapes = 25% yield
        let w = compute_weight(5, 0, 20, 0, Some(now), now);
        assert!(
            w < 0.35,
            "5 signals / 20 scrapes should give low weight: {w}"
        );

        // Bug behavior: 5 signals / 5 "scrapes" = 100% yield
        let w_bug = compute_weight(5, 0, 5, 0, Some(now), now);
        assert!(w_bug > 0.5, "Bug inflates weight: {w_bug}");

        // Confirm they're meaningfully different
        assert!(
            w_bug > w * 1.5,
            "Bug should produce significantly higher weight"
        );
    }

    #[test]
    fn low_weight_source_reaches_exploration_not_just_cadence() {
        let scheduler = SourceScheduler::new();
        let now = Utc::now();
        // Weight 0.15, last scraped 6 days ago.
        // Cadence for weight 0.15 = 168h (7 days) → NOT due by cadence (6 < 7).
        // With min_stale_days=5, IS eligible for exploration (6 >= 5).
        let sources = vec![make_source(0.15, Some(now - Duration::days(6)))];
        let result = scheduler.schedule(&sources, now);
        assert_eq!(
            result.scheduled.len(),
            0,
            "Should NOT be scheduled by cadence"
        );
        assert_eq!(
            result.exploration.len(),
            1,
            "Should be picked for exploration"
        );
    }

    // --- Exponential Backoff ---

    #[test]
    fn backoff_no_penalty_for_first_empty() {
        let base = cadence_hours_for_weight(0.5); // 72
        let default = &DiscoveryMethod::GapAnalysis;
        let with_backoff = cadence_hours_with_backoff(0.5, 0, default);
        assert_eq!(base, with_backoff, "0 empties: no backoff");
        let with_backoff = cadence_hours_with_backoff(0.5, 1, default);
        assert_eq!(base, with_backoff, "1 empty: no backoff yet");
    }

    #[test]
    fn backoff_doubles_at_two_empties() {
        let base = cadence_hours_for_weight(0.5); // 72
        let with_backoff = cadence_hours_with_backoff(0.5, 2, &DiscoveryMethod::GapAnalysis);
        assert_eq!(with_backoff, base * 2, "2 empties: 2x");
    }

    #[test]
    fn backoff_quadruples_at_three_empties() {
        let base = cadence_hours_for_weight(0.5); // 72
        let with_backoff = cadence_hours_with_backoff(0.5, 3, &DiscoveryMethod::GapAnalysis);
        assert_eq!(with_backoff, base * 4, "3 empties: 4x");
    }

    #[test]
    fn backoff_8x_at_four_empties() {
        let base = cadence_hours_for_weight(0.5); // 72
        let with_backoff = cadence_hours_with_backoff(0.5, 4, &DiscoveryMethod::GapAnalysis);
        assert_eq!(with_backoff, base * 8, "4 empties: 8x");
    }

    #[test]
    fn backoff_dormant_at_five_empties() {
        let default = &DiscoveryMethod::GapAnalysis;
        let cadence = cadence_hours_with_backoff(0.9, 5, default);
        assert_eq!(cadence, u32::MAX, "5+ empties: dormant");
        let cadence = cadence_hours_with_backoff(0.3, 10, default);
        assert_eq!(cadence, u32::MAX, "10 empties: still dormant");
    }

    #[test]
    fn is_dormant_threshold() {
        let default = &DiscoveryMethod::GapAnalysis;
        assert!(!is_dormant(4, default));
        assert!(is_dormant(5, default));
        assert!(is_dormant(10, default));
    }

    #[test]
    fn social_graph_follow_dormant_at_three() {
        let sgf = &DiscoveryMethod::SocialGraphFollow;
        assert!(!is_dormant(2, sgf));
        assert!(is_dormant(3, sgf));
        let cadence = cadence_hours_with_backoff(0.5, 3, sgf);
        assert_eq!(cadence, u32::MAX, "SocialGraphFollow: dormant at 3 empties");
    }

    // --- Web Query Tiered Scheduling ---

    fn make_web_query(
        weight: f64,
        last_scraped: Option<DateTime<Utc>>,
        consecutive_empty_runs: u32,
        gap_context: Option<&str>,
    ) -> SourceNode {
        SourceNode {
            id: Uuid::new_v4(),
            canonical_key: format!("test:{}", Uuid::new_v4()),
            canonical_value: "test query".to_string(),
            url: None,
            discovery_method: DiscoveryMethod::GapAnalysis,
            created_at: Utc::now(),
            last_scraped,
            last_produced_signal: None,
            signals_produced: 0,
            signals_corroborated: 0,
            consecutive_empty_runs,
            active: true,
            gap_context: gap_context.map(|s| s.to_string()),
            weight,
            cadence_hours: None,
            avg_signals_per_scrape: 0.0,
            quality_penalty: 1.0,
            source_role: SourceRole::Response,
            scrape_count: 0,
            center_lat: None,
            center_lng: None,
        }
    }

    #[test]
    fn wq_schedule_empty_input() {
        let result = schedule_web_queries(&[], 50, Utc::now());
        assert!(result.scheduled.is_empty());
        assert_eq!(result.hot, 0);
        assert_eq!(result.warm, 0);
        assert_eq!(result.cold, 0);
    }

    #[test]
    fn wq_schedule_respects_max_per_run() {
        let now = Utc::now();
        let sources: Vec<SourceNode> = (0..100)
            .map(|_| make_web_query(0.5, Some(now - Duration::hours(48)), 0, None))
            .collect();
        let result = schedule_web_queries(&sources, 20, now);
        assert!(
            result.scheduled.len() <= 20,
            "Should not exceed max: {}",
            result.scheduled.len()
        );
    }

    #[test]
    fn wq_schedule_never_scraped_in_cold_tier() {
        let now = Utc::now();
        let mut sources: Vec<SourceNode> = (0..5)
            .map(|_| make_web_query(0.5, Some(now - Duration::hours(48)), 0, None))
            .collect();
        // Add never-scraped queries
        for _ in 0..5 {
            sources.push(make_web_query(0.3, None, 0, None));
        }
        let result = schedule_web_queries(&sources, 50, now);
        assert!(
            result.cold > 0,
            "Never-scraped queries should be in cold tier"
        );
    }

    #[test]
    fn wq_schedule_dormant_in_cold_tier() {
        let now = Utc::now();
        let mut sources: Vec<SourceNode> = (0..5)
            .map(|_| make_web_query(0.5, Some(now - Duration::hours(48)), 0, None))
            .collect();
        // Add dormant queries (5+ consecutive empty)
        for _ in 0..3 {
            sources.push(make_web_query(0.1, Some(now - Duration::days(30)), 7, None));
        }
        let result = schedule_web_queries(&sources, 50, now);
        assert!(result.cold > 0, "Dormant queries should be in cold tier");
    }

    #[test]
    fn wq_schedule_per_tension_cap() {
        let now = Utc::now();
        // 10 queries all for the same tension
        let sources: Vec<SourceNode> = (0..10)
            .map(|_| {
                make_web_query(
                    0.8,
                    Some(now - Duration::hours(48)),
                    0,
                    Some("Related: same tension | Gap: unmet_tension"),
                )
            })
            .collect();
        let result = schedule_web_queries(&sources, 50, now);
        // Hot tier should cap at 3 for same tension
        assert!(
            result.hot <= 3,
            "Per-tension cap should limit hot tier: got {}",
            result.hot
        );
    }

    #[test]
    fn wq_schedule_skips_inactive() {
        let now = Utc::now();
        let mut source = make_web_query(0.5, Some(now - Duration::hours(48)), 0, None);
        source.active = false;
        let result = schedule_web_queries(&[source], 50, now);
        assert!(
            result.scheduled.is_empty(),
            "Inactive sources should be skipped"
        );
    }

    #[test]
    fn wq_schedule_skips_non_web_query() {
        let now = Utc::now();
        let mut source = make_web_query(0.5, Some(now - Duration::hours(48)), 0, None);
        // Make it a URL source (not a web query) so it gets filtered out
        source.canonical_value = "https://example.com".to_string();
        source.url = Some("https://example.com".to_string());
        let result = schedule_web_queries(&[source], 50, now);
        assert!(
            result.scheduled.is_empty(),
            "Non-web-query sources should be filtered out"
        );
    }

    // --- Helper extraction tests ---

    #[test]
    fn extract_heat_parses_gap_context() {
        assert!((extract_heat_from_gap_context(Some("heat=0.7")) - 0.7).abs() < 0.01);
        assert!((extract_heat_from_gap_context(Some("cause_heat: 0.85")) - 0.85).abs() < 0.01);
        assert!((extract_heat_from_gap_context(None) - 0.0).abs() < 0.01);
        assert!((extract_heat_from_gap_context(Some("no heat here")) - 0.0).abs() < 0.01);
    }

    #[test]
    fn extract_tension_parses_gap_context() {
        assert_eq!(
            extract_tension_from_gap_context(Some(
                "Curiosity: reason | Gap: unmet_tension | Related: food desert"
            )),
            "food desert"
        );
        assert_eq!(
            extract_tension_from_gap_context(Some("Tension: Housing crisis")),
            "housing crisis"
        );
        assert_eq!(
            extract_tension_from_gap_context(Some(
                "Response Scout: response discovery for \"ICE raids\""
            )),
            "ice raids"
        );
        assert_eq!(extract_tension_from_gap_context(None), "unknown");
    }
}
