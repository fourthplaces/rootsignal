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
    /// PII was detected post-extraction
    pub pii_detected: bool,
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

/// Corroboration threshold for sensitive signals
pub const SENSITIVE_CORROBORATION_MIN: u32 = 2;

/// Freshness threshold — signals not confirmed within this many days are hidden (unless ongoing)
pub const FRESHNESS_MAX_DAYS: i64 = 30;

/// Ask signals expire after this many days (fundraisers, volunteer calls, etc.)
pub const ASK_EXPIRE_DAYS: i64 = 60;

/// Grace period after an event ends before it's hidden (hours).
/// Allows same-day events to remain visible until the day is over.
pub const EVENT_PAST_GRACE_HOURS: i64 = 12;
