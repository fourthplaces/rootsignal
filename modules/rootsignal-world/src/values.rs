use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::{GeoPoint, SourceRole};

/// When something happens. Enough to put it on a calendar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    /// Start of the first/next occurrence (None = unknown)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starts_at: Option<DateTime<Utc>>,
    /// End of the occurrence (None = open-ended or unknown)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ends_at: Option<DateTime<Utc>>,
    /// True if this is a whole-day event (ignore time component of starts_at/ends_at)
    #[serde(default)]
    pub all_day: bool,
    /// RFC 5545 recurrence rule (e.g. "FREQ=WEEKLY;BYDAY=SA")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rrule: Option<String>,
    /// IANA timezone (e.g. "America/Chicago") for local time rendering
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

/// Where something is. Enough to put it on a map and give directions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    /// Coordinates with precision level
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<GeoPoint>,
    /// Human-readable name (e.g. "Lake Harriet Bandshell")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Street address if known
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
}

/// A tag with its computed weight.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagFact {
    pub slug: String,
    pub name: String,
    pub weight: f64,
}

/// World-layer source changes — observable facts about a source.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "field", content = "value", rename_all = "snake_case")]
pub enum WorldSourceChange {
    Weight { old: f64, new: f64 },
    Url { old: String, new: String },
    Role { old: SourceRole, new: SourceRole },
    Active { old: bool, new: bool },
}
