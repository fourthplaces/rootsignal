//! Geographic filtering for extracted signals.
//!
//! Pure functions that decide whether a signal belongs to a scout's geographic
//! scope based on coordinates, location names, and geo-terms. Extracted from
//! the inline logic in `scrape_phase::store_signals()`.

use rootsignal_common::{haversine_km, GeoPoint, GeoPrecision, Node, NodeMeta};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Result of evaluating a single node against the geographic filter.
#[derive(Debug, Clone, PartialEq)]
pub enum GeoVerdict {
    /// Signal is within geographic scope.
    Accept,
    /// Signal is plausibly local but confidence should be penalised.
    AcceptWithPenalty(f32),
    /// Signal is outside geographic scope — drop it.
    Reject,
}

/// Counters produced by a batch filter run.
#[derive(Debug, Default)]
pub struct GeoFilterStats {
    pub filtered: u32,
}

/// Everything the geo-filter needs to know about the scout's region.
pub struct GeoFilterConfig<'a> {
    pub center_lat: f64,
    pub center_lng: f64,
    pub radius_km: f64,
    pub geo_terms: &'a [String],
}

// ---------------------------------------------------------------------------
// Pure decision functions
// ---------------------------------------------------------------------------

/// Geographic relevance check — current production logic.
///
/// 1. Has coordinates within radius → Accept
/// 2. Has coordinates outside radius → Reject
/// 3. No coordinates, `location_name` matches a geo-term → Accept
/// 4. No coordinates, `location_name` present but no match, known source → AcceptWithPenalty
/// 5. No coordinates, `location_name` present but no match, not known source → Reject
/// 6. No coordinates, no usable `location_name` → Accept (benefit of the doubt)
///
/// NOTE: Case 6 is the known bug — signals with no geographic signal at all
/// are accepted and later get center backfill. This causes cross-region
/// contamination (Giessen bug c2c27655).
pub fn geo_check(meta: &NodeMeta, config: &GeoFilterConfig, is_known_source: bool) -> GeoVerdict {
    // --- Cases 1 & 2: signal has coordinates ---
    if let Some(loc) = &meta.location {
        let dist = haversine_km(config.center_lat, config.center_lng, loc.lat, loc.lng);
        return if dist <= config.radius_km {
            GeoVerdict::Accept
        } else {
            GeoVerdict::Reject
        };
    }

    // --- No coordinates: inspect location_name ---
    let loc_name = meta.location_name.as_deref().unwrap_or("");

    if !loc_name.is_empty() && loc_name != "<UNKNOWN>" {
        // Case 3: location_name matches a geo-term → accept
        let loc_lower = loc_name.to_lowercase();
        if config
            .geo_terms
            .iter()
            .any(|term| loc_lower.contains(&term.to_lowercase()))
        {
            return GeoVerdict::Accept;
        }

        // Case 4: known source, name present but no match → accept with penalty
        if is_known_source {
            return GeoVerdict::AcceptWithPenalty(0.8);
        }

        // Case 5: non-local, name doesn't match → reject
        return GeoVerdict::Reject;
    }

    // Case 6 (FIXED): no coordinates, no usable location_name → reject.
    // Previously accepted with "benefit of the doubt", causing cross-region
    // contamination (Giessen bug c2c27655).
    GeoVerdict::Reject
}

/// Backfill region-center coordinates on a signal that passed the geo-filter
/// but has no coordinates. Only backfills when a usable `location_name` is
/// present — signals with no name should have been rejected by `geo_check`,
/// but this is a safety belt.
///
/// Returns `true` if coordinates were written.
pub fn backfill_center_coords(meta: &mut NodeMeta, config: &GeoFilterConfig) -> bool {
    if meta.location.is_some() {
        return false;
    }

    // Only backfill when there's a usable location_name
    let has_name = meta
        .location_name
        .as_deref()
        .map(|n| !n.is_empty() && n != "<UNKNOWN>")
        .unwrap_or(false);

    if !has_name {
        return false;
    }

    meta.location = Some(GeoPoint {
        lat: config.center_lat,
        lng: config.center_lng,
        precision: GeoPrecision::Approximate,
    });
    true
}

// ---------------------------------------------------------------------------
// Batch orchestrator
// ---------------------------------------------------------------------------

/// Filter a batch of nodes through the geo-check, apply confidence penalties
/// and coordinate backfill, and return the surviving nodes with stats.
pub fn filter_nodes(
    nodes: Vec<Node>,
    config: &GeoFilterConfig,
    is_known_source: bool,
) -> (Vec<Node>, GeoFilterStats) {
    let mut stats = GeoFilterStats::default();
    let mut accepted = Vec::with_capacity(nodes.len());

    for mut node in nodes {
        let verdict = match node.meta() {
            Some(meta) => geo_check(meta, config, is_known_source),
            None => GeoVerdict::Accept, // Evidence nodes have no meta — pass through
        };

        match verdict {
            GeoVerdict::Accept => {
                // Backfill coords if needed
                if let Some(meta) = node.meta_mut() {
                    backfill_center_coords(meta, config);
                }
                accepted.push(node);
            }
            GeoVerdict::AcceptWithPenalty(factor) => {
                if let Some(meta) = node.meta_mut() {
                    meta.confidence *= factor;
                    backfill_center_coords(meta, config);
                }
                accepted.push(node);
            }
            GeoVerdict::Reject => {
                stats.filtered += 1;
            }
        }
    }

    (accepted, stats)
}

// ===========================================================================
// Unit tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rootsignal_common::{GatheringNode, SensitivityLevel};
    use uuid::Uuid;

    /// Build a minimal NodeMeta for testing.
    fn test_meta(location: Option<GeoPoint>, location_name: Option<&str>) -> NodeMeta {
        NodeMeta {
            id: Uuid::new_v4(),
            title: "Test signal".to_string(),
            summary: "A test signal".to_string(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.9,
            freshness_score: 1.0,
            corroboration_count: 0,
            location,
            location_name: location_name.map(|s| s.to_string()),
            source_url: "https://example.com".to_string(),
            extracted_at: Utc::now(),
            last_confirmed_active: Utc::now(),
            source_diversity: 1,
            external_ratio: 0.0,
            cause_heat: 0.0,
            implied_queries: Vec::new(),
            channel_diversity: 1,
            mentioned_actors: Vec::new(),
        }
    }

    fn mpls_terms() -> Vec<String> {
        vec![
            "Minneapolis".to_string(),
            "Minnesota".to_string(),
            "Twin Cities".to_string(),
            "MN".to_string(),
        ]
    }

    fn mpls_config(terms: &[String]) -> GeoFilterConfig<'_> {
        GeoFilterConfig {
            center_lat: 44.9778,
            center_lng: -93.2650,
            radius_km: 30.0,
            geo_terms: terms,
        }
    }

    fn giessen_config(terms: &[String]) -> GeoFilterConfig<'_> {
        GeoFilterConfig {
            center_lat: 50.6214,
            center_lng: 8.6567,
            radius_km: 20.0,
            geo_terms: terms,
        }
    }

    fn make_node(meta: NodeMeta) -> Node {
        Node::Gathering(GatheringNode {
            meta,
            starts_at: None,
            ends_at: None,
            action_url: String::new(),
            organizer: None,
            is_recurring: false,
        })
    }

    // ===================================================================
    // Tests for EXISTING behavior (should pass with buggy code)
    // ===================================================================

    #[test]
    fn accept_coords_within_radius() {
        let terms = mpls_terms();
        let config = mpls_config(&terms);
        let meta = test_meta(
            Some(GeoPoint { lat: 44.98, lng: -93.27, precision: GeoPrecision::Exact }),
            None,
        );
        assert_eq!(geo_check(&meta, &config, false), GeoVerdict::Accept);
    }

    #[test]
    fn reject_coords_outside_radius() {
        // Austin TX — far from Minneapolis
        let meta = test_meta(
            Some(GeoPoint { lat: 30.2672, lng: -97.7431, precision: GeoPrecision::Exact }),
            None,
        );
        let terms = mpls_terms();
        let config = mpls_config(&terms);
        assert_eq!(geo_check(&meta, &config, false), GeoVerdict::Reject);
    }

    #[test]
    fn reject_coords_different_continent() {
        let terms = vec!["Giessen".to_string()];
        let config = giessen_config(&terms);
        // Minneapolis coords against Giessen profile
        let meta = test_meta(
            Some(GeoPoint { lat: 44.9778, lng: -93.2650, precision: GeoPrecision::Exact }),
            None,
        );
        assert_eq!(geo_check(&meta, &config, false), GeoVerdict::Reject);
    }

    #[test]
    fn accept_location_name_exact_match() {
        let terms = mpls_terms();
        let config = mpls_config(&terms);
        let meta = test_meta(None, Some("Minneapolis"));
        assert_eq!(geo_check(&meta, &config, false), GeoVerdict::Accept);
    }

    #[test]
    fn accept_location_name_case_insensitive() {
        let terms = mpls_terms();
        let config = mpls_config(&terms);
        let meta = test_meta(None, Some("MINNEAPOLIS"));
        assert_eq!(geo_check(&meta, &config, false), GeoVerdict::Accept);
    }

    #[test]
    fn accept_location_name_contains_match() {
        let terms = mpls_terms();
        let config = mpls_config(&terms);
        let meta = test_meta(None, Some("South Minneapolis, MN"));
        assert_eq!(geo_check(&meta, &config, false), GeoVerdict::Accept);
    }

    #[test]
    fn accept_location_name_state_level_match() {
        let terms = mpls_terms();
        let config = mpls_config(&terms);
        let meta = test_meta(None, Some("Minnesota"));
        assert_eq!(geo_check(&meta, &config, false), GeoVerdict::Accept);
    }

    #[test]
    fn reject_non_local_location_name() {
        let terms = mpls_terms();
        let config = mpls_config(&terms);
        let meta = test_meta(None, Some("Austin, Texas"));
        assert_eq!(geo_check(&meta, &config, false), GeoVerdict::Reject);
    }

    #[test]
    fn accept_known_source_with_penalty() {
        let terms = mpls_terms();
        let config = mpls_config(&terms);
        let meta = test_meta(None, Some("Uptown"));
        assert_eq!(
            geo_check(&meta, &config, true),
            GeoVerdict::AcceptWithPenalty(0.8)
        );
    }

    #[test]
    fn reject_unknown_location_name() {
        let terms = mpls_terms();
        let config = mpls_config(&terms);
        let meta = test_meta(None, Some("<UNKNOWN>"));
        assert_eq!(geo_check(&meta, &config, false), GeoVerdict::Reject);
    }

    #[test]
    fn backfill_when_no_coords() {
        let terms = mpls_terms();
        let config = mpls_config(&terms);
        let mut meta = test_meta(None, Some("Minneapolis"));
        assert!(backfill_center_coords(&mut meta, &config));
        let loc = meta.location.unwrap();
        assert!((loc.lat - 44.9778).abs() < 0.001);
        assert_eq!(loc.precision, GeoPrecision::Approximate);
    }

    #[test]
    fn no_backfill_when_coords_exist() {
        let terms = mpls_terms();
        let config = mpls_config(&terms);
        let mut meta = test_meta(
            Some(GeoPoint { lat: 44.98, lng: -93.27, precision: GeoPrecision::Exact }),
            Some("Minneapolis"),
        );
        assert!(!backfill_center_coords(&mut meta, &config));
        assert!((meta.location.unwrap().lat - 44.98).abs() < 0.001);
    }

    // Fixed: no longer backfills when no name
    #[test]
    fn no_backfill_when_no_name_current_behavior() {
        let terms = mpls_terms();
        let config = mpls_config(&terms);
        let mut meta = test_meta(None, None);
        assert!(!backfill_center_coords(&mut meta, &config));
        assert!(meta.location.is_none());
    }

    // ===================================================================
    // Tests for CORRECT behavior (THE FIX) — these should FAIL now
    // ===================================================================

    #[test]
    fn reject_no_coords_no_name_none() {
        let terms = mpls_terms();
        let config = mpls_config(&terms);
        let meta = test_meta(None, None);
        assert_eq!(geo_check(&meta, &config, false), GeoVerdict::Reject);
    }

    #[test]
    fn reject_no_coords_no_name_empty() {
        let terms = mpls_terms();
        let config = mpls_config(&terms);
        let meta = test_meta(None, Some(""));
        assert_eq!(geo_check(&meta, &config, false), GeoVerdict::Reject);
    }

    #[test]
    fn reject_no_coords_no_name_even_if_known_source() {
        let terms = mpls_terms();
        let config = mpls_config(&terms);
        let meta = test_meta(None, None);
        assert_eq!(geo_check(&meta, &config, true), GeoVerdict::Reject);
    }

    #[test]
    fn no_backfill_when_no_name() {
        let terms = mpls_terms();
        let config = mpls_config(&terms);
        let mut meta = test_meta(None, None);
        backfill_center_coords(&mut meta, &config);
        assert!(meta.location.is_none(), "should NOT backfill when no location_name");
    }

    #[test]
    fn no_backfill_when_name_is_unknown() {
        let terms = mpls_terms();
        let config = mpls_config(&terms);
        let mut meta = test_meta(None, Some("<UNKNOWN>"));
        backfill_center_coords(&mut meta, &config);
        assert!(meta.location.is_none(), "should NOT backfill <UNKNOWN>");
    }

    #[test]
    fn giessen_bug_reproduction_nameless_nodes_rejected() {
        let terms = vec!["Giessen".to_string(), "Gießen".to_string(), "Hessen".to_string()];
        let config = giessen_config(&terms);

        let nodes: Vec<Node> = (0..5)
            .map(|_| make_node(test_meta(None, None)))
            .collect();

        let (accepted, stats) = filter_nodes(nodes, &config, false);
        assert!(accepted.is_empty(), "all nameless nodes should be rejected, got {}", accepted.len());
        assert_eq!(stats.filtered, 5);
    }

    // ===================================================================
    // Tests for behavior that should be unchanged by the fix
    // ===================================================================

    #[test]
    fn mixed_batch_filters_correctly() {
        let terms = mpls_terms();
        let config = mpls_config(&terms);

        let nodes = vec![
            // Accept: coords within radius
            make_node(test_meta(
                Some(GeoPoint { lat: 44.98, lng: -93.27, precision: GeoPrecision::Exact }),
                None,
            )),
            // Accept: location_name matches
            make_node(test_meta(None, Some("Minneapolis"))),
            // Reject: no coords, no name (AFTER fix)
            make_node(test_meta(None, None)),
            // Reject: coords outside radius
            make_node(test_meta(
                Some(GeoPoint { lat: 30.2672, lng: -97.7431, precision: GeoPrecision::Exact }),
                None,
            )),
            // Reject: non-local name, not known source
            make_node(test_meta(None, Some("Austin, Texas"))),
        ];

        let (accepted, stats) = filter_nodes(nodes, &config, false);
        assert_eq!(accepted.len(), 2, "only 2 nodes should pass, got {}", accepted.len());
        assert_eq!(stats.filtered, 3);
    }

    #[test]
    fn batch_backfill_applied_to_accepted_with_name() {
        let terms = mpls_terms();
        let config = mpls_config(&terms);

        let nodes = vec![make_node(test_meta(None, Some("Minneapolis")))];

        let (accepted, _) = filter_nodes(nodes, &config, false);
        assert_eq!(accepted.len(), 1);
        let loc = accepted[0].meta().unwrap().location.as_ref().unwrap();
        assert!((loc.lat - 44.9778).abs() < 0.001);
        assert_eq!(loc.precision, GeoPrecision::Approximate);
    }

    #[test]
    fn known_source_penalty_applied_in_batch() {
        let terms = mpls_terms();
        let config = mpls_config(&terms);

        let nodes = vec![make_node(test_meta(None, Some("Uptown")))];

        let (accepted, _) = filter_nodes(nodes, &config, true);
        assert_eq!(accepted.len(), 1);
        let confidence = accepted[0].meta().unwrap().confidence;
        assert!((confidence - 0.72).abs() < 0.01, "0.9 * 0.8 = 0.72, got {confidence}");
    }
}
