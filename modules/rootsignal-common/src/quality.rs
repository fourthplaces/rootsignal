use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeoAccuracy {
    /// Address or venue extracted
    High,
    /// Neighborhood or zip code
    Medium,
    /// City-level fallback
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionQuality {
    /// Has action_url AND (has timing OR is ongoing)
    pub actionable: bool,
    /// Location extracted with at least city-level precision
    pub has_location: bool,
    /// Has an action URL
    pub has_action_url: bool,
    /// Has timing information
    pub has_timing: bool,
    /// Confidence in location correctness
    pub geo_accuracy: GeoAccuracy,
    /// Fraction of type-specific required fields populated (0.0-1.0)
    pub completeness: f32,
    /// Weighted confidence score (0.0-1.0)
    pub confidence: f32,
}

/// Confidence thresholds for display tiers
pub const CONFIDENCE_DISPLAY_FULL: f32 = 0.6;
pub const CONFIDENCE_DISPLAY_LIMITED: f32 = 0.4;

/// Freshness threshold — signals not confirmed within this many days are hidden (unless ongoing)
pub const FRESHNESS_MAX_DAYS: i64 = 30;

/// Need signals expire after this many days (fundraisers, volunteer calls, etc.)
pub const NEED_EXPIRE_DAYS: i64 = 60;

/// Notice signals expire after this many days (PSAs, advisories stay relevant longer)
pub const NOTICE_EXPIRE_DAYS: i64 = 90;

/// Grace period after a gathering ends before it's hidden (hours).
/// Allows same-day gatherings to remain visible until the day is over.
// GAP: 12h is too aggressive — one-time gatherings vanish immediately. Recurring gatherings
// survive only because `is_recurring = true` bypasses the check entirely, but we don't
// store recurrence rules (frequency, next_occurrence) so there's no way to compute
// upcoming dates. Needs: (1) `recurrence_rule` + `next_occurrence` fields on GatheringNode,
// (2) extractor prompt to populate them, (3) scout to recompute next_occurrence each run,
// (4) expiry clause to use next_occurrence for recurring gatherings. For now, bumped to 7 days
// so past one-time gatherings linger longer on the map.
pub const GATHERING_PAST_GRACE_HOURS: i64 = 168; // 7 days
