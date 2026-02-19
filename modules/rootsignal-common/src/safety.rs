use serde::{Deserialize, Serialize};

/// Sensitivity classification — enforced at schema level.
/// Determines coordinate precision reduction before data reaches any public API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensitivityLevel {
    /// Volunteer events, cleanups, public meetings. Full precision.
    General,
    /// Organizing, advocacy, political action. Neighborhood-level precision.
    Elevated,
    /// Enforcement activity, vulnerable populations, sanctuary networks. City/region only.
    Sensitive,
}

impl SensitivityLevel {
    /// Returns the coordinate fuzz radius in degrees (approximate).
    /// General: no fuzz, Elevated: ~0.5km, Sensitive: ~5km
    pub fn fuzz_radius(&self) -> f64 {
        match self {
            SensitivityLevel::General => 0.0,
            SensitivityLevel::Elevated => 0.005, // ~500m
            SensitivityLevel::Sensitive => 0.05, // ~5km
        }
    }
}

use crate::types::{GeoPoint, GeoPrecision};

/// Reduce coordinate precision based on sensitivity level.
/// General: exact coordinates returned.
/// Elevated: snapped to neighborhood centroid (~500m grid).
/// Sensitive: snapped to city-level centroid (~5km grid).
pub fn fuzz_location(point: GeoPoint, sensitivity: SensitivityLevel) -> GeoPoint {
    let radius = sensitivity.fuzz_radius();
    if radius == 0.0 {
        return point;
    }

    // Snap to grid: round coordinates to the nearest grid cell
    let precision = if radius >= 0.01 {
        GeoPrecision::City
    } else {
        GeoPrecision::Neighborhood
    };

    let lat = (point.lat / radius).round() * radius;
    let lng = (point.lng / radius).round() * radius;

    GeoPoint {
        lat,
        lng,
        precision,
    }
}

use regex::Regex;
use std::sync::LazyLock;

static PHONE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d{3}[-.\s]?\d{3}[-.\s]?\d{4}\b").unwrap());
static EMAIL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Z|a-z]{2,}\b").unwrap());
static SSN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").unwrap());
static ADDRESS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b\d{1,5}\s+[A-Z][a-z]+(?:\s+[A-Z][a-z]+)*\s+(?:St|Ave|Blvd|Dr|Ln|Rd|Way|Ct|Pl|Cir|Ter)\b").unwrap()
});

/// Check if text contains PII patterns. Returns descriptions of what was found.
pub fn detect_pii(text: &str) -> Vec<String> {
    let mut findings = Vec::new();

    if PHONE_RE.is_match(text) {
        findings.push("phone number detected".to_string());
    }
    if EMAIL_RE.is_match(text) {
        findings.push("email address detected".to_string());
    }
    if SSN_RE.is_match(text) {
        findings.push("SSN pattern detected".to_string());
    }
    if ADDRESS_RE.is_match(text) {
        findings.push("street address detected".to_string());
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzz_general_is_exact() {
        let point = GeoPoint {
            lat: 44.9778,
            lng: -93.2650,
            precision: GeoPrecision::Exact,
        };
        let fuzzed = fuzz_location(point, SensitivityLevel::General);
        assert_eq!(fuzzed.lat, 44.9778);
        assert_eq!(fuzzed.lng, -93.2650);
    }

    #[test]
    fn test_fuzz_elevated_reduces_precision() {
        let point = GeoPoint {
            lat: 44.9778,
            lng: -93.2650,
            precision: GeoPrecision::Exact,
        };
        let fuzzed = fuzz_location(point, SensitivityLevel::Elevated);
        assert_eq!(fuzzed.precision, GeoPrecision::Neighborhood);
        // Should be snapped to 0.005 grid
        assert!((fuzzed.lat - 44.9778).abs() < 0.005);
    }

    #[test]
    fn test_fuzz_sensitive_city_level() {
        let point = GeoPoint {
            lat: 44.9778,
            lng: -93.2650,
            precision: GeoPrecision::Exact,
        };
        let fuzzed = fuzz_location(point, SensitivityLevel::Sensitive);
        assert_eq!(fuzzed.precision, GeoPrecision::City);
    }

    #[test]
    fn test_detect_pii_phone() {
        let findings = detect_pii("Call me at 612-555-1234 for info");
        assert!(!findings.is_empty());
    }

    #[test]
    fn test_detect_pii_email() {
        let findings = detect_pii("Contact john@example.com");
        assert!(!findings.is_empty());
    }

    #[test]
    fn test_detect_pii_clean() {
        let findings = detect_pii("Join us at the community center on Saturday for a park cleanup");
        assert!(findings.is_empty());
    }
}
