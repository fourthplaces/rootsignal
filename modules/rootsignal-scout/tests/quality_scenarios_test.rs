//! Layer 2: Scenario-driven quality + geo filter tests.
//!
//! Pure functions, no LLM, no infrastructure. Validates `quality::score()` and
//! `geo_filter::geo_check()` against realistic extraction patterns matching the
//! fixture content in `tests/fixtures/`.
//!
//! Run with: cargo test -p rootsignal-scout --test quality_scenarios_test

use chrono::Utc;
use uuid::Uuid;

use rootsignal_common::{
    AidNode, GatheringNode, GeoAccuracy, GeoPoint, GeoPrecision, NeedNode, Node, NodeMeta,
    NoticeNode, SensitivityLevel, Severity, TensionNode, Urgency,
};
use rootsignal_scout::enrichment::quality;
use rootsignal_scout::pipeline::geo_filter::{self, GeoFilterConfig, GeoVerdict};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_meta() -> NodeMeta {
    NodeMeta {
        id: Uuid::new_v4(),
        title: "Test signal".into(),
        summary: "A test signal".into(),
        sensitivity: SensitivityLevel::General,
        confidence: 0.0,
        freshness_score: 1.0,
        corroboration_count: 0,
        location: None,
        location_name: None,
        source_url: "https://example.com".into(),
        extracted_at: Utc::now(),
        content_date: None,
        last_confirmed_active: Utc::now(),
        source_diversity: 1,
        external_ratio: 0.0,
        cause_heat: 0.0,
        implied_queries: vec![],
        channel_diversity: 1,
        mentioned_actors: vec![],
        author_actor: None,
    }
}

fn mpls_geo_terms() -> Vec<String> {
    vec![
        "minneapolis".into(),
        "minnesota".into(),
        "twin cities".into(),
        "powderhorn".into(),
        "phillips".into(),
        "cedar-riverside".into(),
        "midtown".into(),
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

// ===========================================================================
// Scenario: community_garden_event.txt → Gathering
// ===========================================================================

/// A complete Gathering (Powderhorn Community Garden spring volunteer day)
/// should score maximum confidence: exact geo, action URL, and timing.
#[test]
fn community_garden_gathering_scores_high_confidence() {
    let node = Node::Gathering(GatheringNode {
        meta: NodeMeta {
            title: "Spring Volunteer Day at Powderhorn Community Garden".into(),
            summary: "Annual spring kickoff — preparing raised beds, compost, planting".into(),
            location: Some(GeoPoint {
                lat: 44.9486,
                lng: -93.2636,
                precision: GeoPrecision::Exact,
            }),
            location_name: Some("Powderhorn Community Garden, 3524 15th Ave S".into()),
            source_url: "https://powderhornpark.org/events".into(),
            mentioned_actors: vec![
                "Powderhorn Park Neighborhood Association".into(),
                "Cafe Racer".into(),
                "Briva Health".into(),
            ],
            ..test_meta()
        },
        starts_at: Some(
            chrono::NaiveDate::from_ymd_opt(2026, 4, 12)
                .unwrap()
                .and_hms_opt(14, 0, 0) // 9am CDT = 14:00 UTC
                .unwrap()
                .and_utc(),
        ),
        ends_at: Some(
            chrono::NaiveDate::from_ymd_opt(2026, 4, 12)
                .unwrap()
                .and_hms_opt(18, 0, 0)
                .unwrap()
                .and_utc(),
        ),
        action_url: "https://eventbrite.com/powderhorn-spring-2026".into(),
        organizer: Some("Powderhorn Park Neighborhood Association".into()),
        is_recurring: false,
    });

    let q = quality::score(&node);
    assert!(
        q.confidence >= 0.95,
        "Complete gathering with exact geo should score ~1.0, got {}",
        q.confidence
    );
    assert!(q.actionable, "Gathering with URL + date should be actionable");
    assert!(q.has_location);
    assert!(q.has_action_url);
    assert!(q.has_timing);
    assert_eq!(q.geo_accuracy, rootsignal_common::GeoAccuracy::High);
}

/// The same Gathering should pass geo-check — coords are within 30km of
/// Minneapolis center.
#[test]
fn community_garden_passes_geo_check() {
    let meta = NodeMeta {
        location: Some(GeoPoint {
            lat: 44.9486,
            lng: -93.2636,
            precision: GeoPrecision::Exact,
        }),
        location_name: Some("Powderhorn Community Garden".into()),
        ..test_meta()
    };
    let terms = mpls_geo_terms();
    let config = mpls_config(&terms);
    assert_eq!(geo_filter::geo_check(&meta, &config, false), GeoVerdict::Accept);
}

// ===========================================================================
// Scenario: food_shelf_give.txt → Aid
// ===========================================================================

/// A food shelf Aid with exact location, action URL, and ongoing status
/// should score high confidence.
#[test]
fn food_shelf_aid_scores_high_confidence() {
    let node = Node::Aid(AidNode {
        meta: NodeMeta {
            title: "Briva Health Community Food Shelf".into(),
            summary: "Free food shelf — no ID, proof of income, or appointment needed".into(),
            location: Some(GeoPoint {
                lat: 44.9696,
                lng: -93.2466,
                precision: GeoPrecision::Exact,
            }),
            location_name: Some("420 15th Ave S, Minneapolis, MN 55454".into()),
            source_url: "https://brivahealth.org/food-shelf".into(),
            ..test_meta()
        },
        action_url: "https://brivahealth.org/volunteer".into(),
        availability: Some("Tue-Fri 10-4, 1st & 3rd Sat 10-1".into()),
        is_ongoing: true,
    });

    let q = quality::score(&node);
    assert!(
        q.confidence >= 0.8,
        "Food shelf with location + URL + ongoing should score high: {}",
        q.confidence
    );
    assert!(q.actionable, "Aid with action_url + is_ongoing should be actionable");
    assert!(q.has_location);
    assert!(q.has_action_url);
}

/// Aid without an action URL should not be actionable, even if ongoing.
#[test]
fn food_shelf_aid_without_url_not_actionable() {
    let node = Node::Aid(AidNode {
        meta: NodeMeta {
            title: "Briva Health Community Food Shelf".into(),
            summary: "Free food shelf".into(),
            location: Some(GeoPoint {
                lat: 44.9696,
                lng: -93.2466,
                precision: GeoPrecision::Exact,
            }),
            location_name: Some("420 15th Ave S".into()),
            ..test_meta()
        },
        action_url: String::new(),
        availability: Some("Tue-Fri 10-4".into()),
        is_ongoing: true,
    });

    let q = quality::score(&node);
    assert!(!q.actionable, "Aid without action_url should not be actionable");
}

/// Food shelf location (420 15th Ave S) should pass geo check with coords.
#[test]
fn food_shelf_passes_geo_check_with_coords() {
    let meta = NodeMeta {
        location: Some(GeoPoint {
            lat: 44.9696,
            lng: -93.2466,
            precision: GeoPrecision::Exact,
        }),
        location_name: Some("420 15th Ave S, Minneapolis".into()),
        ..test_meta()
    };
    let terms = mpls_geo_terms();
    let config = mpls_config(&terms);
    assert_eq!(geo_filter::geo_check(&meta, &config, false), GeoVerdict::Accept);
}

// ===========================================================================
// Scenario: urgent_community_tension.txt → Tension + Gathering + Aid
// ===========================================================================

/// ICE enforcement Tension with neighborhood-level geo and no action URL
/// should still have moderate confidence (location helps, but no URL lowers it).
#[test]
fn ice_enforcement_tension_moderate_confidence() {
    let node = Node::Tension(TensionNode {
        meta: NodeMeta {
            title: "ICE Enforcement Activity in Phillips Neighborhood".into(),
            summary: "Multiple reports of ICE vehicles and plainclothes agents near Lake St and Bloomington Ave".into(),
            sensitivity: SensitivityLevel::Sensitive,
            location: Some(GeoPoint {
                lat: 44.9486,
                lng: -93.2476,
                precision: GeoPrecision::Neighborhood,
            }),
            location_name: Some("Phillips neighborhood, Minneapolis".into()),
            mentioned_actors: vec![
                "Minneapolis Immigrant Rights Coalition".into(),
                "MIRC".into(),
            ],
            ..test_meta()
        },
        severity: Severity::High,
        category: Some("immigration".into()),
        what_would_help: Some("Legal support, community safe spaces, know-your-rights information".into()),
    });

    let q = quality::score(&node);
    // Tension: completeness = location (1/1) = 1.0, geo = Medium (0.7)
    // confidence = 1.0 * 0.5 + 0.7 * 0.5 = 0.85
    assert!(
        (q.confidence - 0.85).abs() < 0.01,
        "Tension with neighborhood geo: expected ~0.85, got {}",
        q.confidence
    );
    assert!(!q.actionable, "Tension signals are never actionable (no action_url concept)");
}

/// A Tension without coordinates but with a matching location name should
/// pass geo filter.
#[test]
fn tension_without_coords_passes_geo_on_name_match() {
    let meta = NodeMeta {
        location: None,
        location_name: Some("Phillips neighborhood, Minneapolis".into()),
        ..test_meta()
    };
    let terms = mpls_geo_terms();
    let config = mpls_config(&terms);
    // "Phillips" and "Minneapolis" are both in geo_terms → Accept
    assert_eq!(geo_filter::geo_check(&meta, &config, false), GeoVerdict::Accept);
}

/// Emergency community meeting from the tension fixture should be a
/// complete Gathering with high confidence.
#[test]
fn emergency_meeting_gathering_is_actionable() {
    let node = Node::Gathering(GatheringNode {
        meta: NodeMeta {
            title: "Emergency Community Meeting".into(),
            summary: "Wednesday 6 PM at Sagrado Corazón Church to coordinate ICE response".into(),
            location: Some(GeoPoint {
                lat: 44.9480,
                lng: -93.2380,
                precision: GeoPrecision::Exact,
            }),
            location_name: Some("Sagrado Corazón Church, 2018 E. Lake St".into()),
            ..test_meta()
        },
        starts_at: Some(Utc::now()), // placeholder for the actual date
        ends_at: None,
        action_url: String::new(),
        organizer: None,
        is_recurring: false,
    });

    let q = quality::score(&node);
    // Has location (1) + timing (1) but no action_url (0): completeness = 2/3
    // Geo = High (1.0), confidence = 0.667 * 0.5 + 1.0 * 0.5 = 0.833
    assert!(
        q.confidence > 0.8,
        "Emergency meeting with location + timing: {}",
        q.confidence
    );
    // Not actionable: has timing but no action_url
    assert!(!q.actionable, "Gathering without distinct action_url is not actionable");
}

/// Legal aid from the tension fixture — an Aid signal with location.
#[test]
fn legal_aid_signal_scores_reasonably() {
    let node = Node::Aid(AidNode {
        meta: NodeMeta {
            title: "Legal Support for Immigrants".into(),
            summary: "MIRAC legal aid available at Centro de Trabajadores Unidos".into(),
            sensitivity: SensitivityLevel::Elevated,
            location: Some(GeoPoint {
                lat: 44.9480,
                lng: -93.2476,
                precision: GeoPrecision::Exact,
            }),
            location_name: Some("Centro de Trabajadores Unidos, 2104 Bloomington Ave".into()),
            ..test_meta()
        },
        action_url: String::new(),
        availability: Some("9-5 weekdays".into()),
        is_ongoing: true,
    });

    let q = quality::score(&node);
    // has location (1), no action_url (0), is_ongoing (1) → completeness 2/3
    // geo High (1.0) → confidence = 0.667*0.5 + 1.0*0.5 = 0.833
    assert!(
        q.confidence > 0.8,
        "Legal aid with location + ongoing: {}",
        q.confidence
    );
    assert!(
        !q.actionable,
        "Aid without action_url should not be actionable even with is_ongoing"
    );
}

// ===========================================================================
// Geo filter edge cases from realistic extraction patterns
// ===========================================================================

/// A signal extracted from a national page with no local coordinates
/// and a non-matching location name should be rejected.
#[test]
fn national_signal_rejected_by_geo() {
    let meta = NodeMeta {
        location: None,
        location_name: Some("Washington, D.C.".into()),
        ..test_meta()
    };
    let terms = mpls_geo_terms();
    let config = mpls_config(&terms);
    assert_eq!(geo_filter::geo_check(&meta, &config, false), GeoVerdict::Reject);
}

/// A signal from a known source with a neighborhood name not in geo_terms
/// should get accepted with penalty.
#[test]
fn known_source_unfamiliar_neighborhood_gets_penalty() {
    let meta = NodeMeta {
        location: None,
        location_name: Some("Longfellow".into()), // not in our geo_terms
        ..test_meta()
    };
    let terms = mpls_geo_terms();
    let config = mpls_config(&terms);
    assert_eq!(
        geo_filter::geo_check(&meta, &config, true),
        GeoVerdict::AcceptWithPenalty(0.8)
    );
}

/// Batch filtering removes out-of-scope signals and keeps local ones.
#[test]
fn batch_filter_keeps_local_drops_remote() {
    let terms = mpls_geo_terms();
    let config = mpls_config(&terms);

    let local_gathering = Node::Gathering(GatheringNode {
        meta: NodeMeta {
            title: "Powderhorn Garden Day".into(),
            summary: "Local event".into(),
            location: Some(GeoPoint {
                lat: 44.9486,
                lng: -93.2636,
                precision: GeoPrecision::Exact,
            }),
            location_name: Some("Powderhorn".into()),
            ..test_meta()
        },
        starts_at: Some(Utc::now()),
        ends_at: None,
        action_url: "https://example.com/rsvp".into(),
        organizer: None,
        is_recurring: false,
    });

    let remote_tension = Node::Tension(TensionNode {
        meta: NodeMeta {
            title: "Remote event".into(),
            summary: "Not in Minneapolis".into(),
            location: Some(GeoPoint {
                lat: 30.2672,
                lng: -97.7431,
                precision: GeoPrecision::Exact,
            }),
            location_name: Some("Austin, TX".into()),
            ..test_meta()
        },
        severity: Severity::Medium,
        category: None,
        what_would_help: None,
    });

    let nameless = Node::Aid(AidNode {
        meta: NodeMeta {
            title: "Mystery aid".into(),
            summary: "No location at all".into(),
            ..test_meta()
        },
        action_url: "https://example.com".into(),
        availability: None,
        is_ongoing: false,
    });

    let (accepted, stats) = geo_filter::filter_nodes(
        vec![local_gathering, remote_tension, nameless],
        &config,
        false,
    );
    assert_eq!(accepted.len(), 1, "only local gathering should survive");
    assert_eq!(stats.filtered, 2);
}

// ===========================================================================
// Adversarial: Geo filter substring false positives
// ===========================================================================

/// "New Minneapolis" contains "minneapolis" as a substring → currently matches.
/// This documents the known false-positive behavior of substring geo matching.
#[test]
fn geo_substring_false_positive_new_minneapolis() {
    let meta = NodeMeta {
        location: None,
        location_name: Some("New Minneapolis, North Dakota".into()),
        ..test_meta()
    };
    let terms = mpls_geo_terms();
    let config = mpls_config(&terms);
    // BUG: substring matching causes this to Accept when it should Reject.
    // This test documents the current (broken) behavior.
    assert_eq!(
        geo_filter::geo_check(&meta, &config, false),
        GeoVerdict::Accept,
        "Known false positive: 'New Minneapolis' matches 'minneapolis' substring"
    );
}

/// "Minneapolitan Garden Club" contains "minneapolis" as a substring.
#[test]
fn geo_substring_false_positive_minneapolitan() {
    let meta = NodeMeta {
        location: None,
        location_name: Some("Minneapolitan Garden Club, St. Paul".into()),
        ..test_meta()
    };
    let terms = mpls_geo_terms();
    let config = mpls_config(&terms);
    // BUG: substring matching. "minneapolitan" contains "minneapolis"? No!
    // "minneapolitan" does NOT contain "minneapolis" — it's "minneapolitan" not "minneapolis".
    // Let's verify: "minneapolitan".contains("minneapolis") → false (extra 'tan')
    // Actually... "minneapolitan" contains "minneapoli" but not "minneapolis"
    // Wait: m-i-n-n-e-a-p-o-l-i-t-a-n vs m-i-n-n-e-a-p-o-l-i-s
    // At index 10: 't' vs 's'. So "minneapolitan" does NOT contain "minneapolis".
    assert_eq!(
        geo_filter::geo_check(&meta, &config, false),
        GeoVerdict::Reject,
        "Minneapolitan does NOT contain 'minneapolis' — different suffix"
    );
}

/// Short geo terms like "MN" could match inside unrelated words.
/// "Magnetic North" lowercased → "magnetic north" which contains "mn"? No.
/// But "Bemidji, MN" contains "mn" → this is actually correct behavior.
#[test]
fn geo_short_term_mn_matches_correctly() {
    let terms = vec!["mn".into()];
    let config = GeoFilterConfig {
        center_lat: 44.9778,
        center_lng: -93.2650,
        radius_km: 30.0,
        geo_terms: &terms,
    };
    // "Bemidji, MN" → should match (correct)
    let meta_bemidji = NodeMeta {
        location: None,
        location_name: Some("Bemidji, MN".into()),
        ..test_meta()
    };
    assert_eq!(geo_filter::geo_check(&meta_bemidji, &config, false), GeoVerdict::Accept);

    // "Omni Hotel" → "omni hotel" contains "mn"? Let's check: o-m-n-i → "mn" at index 1-2!
    // This is a false positive.
    let meta_omni = NodeMeta {
        location: None,
        location_name: Some("Omni Hotel, Dallas".into()),
        ..test_meta()
    };
    assert_eq!(
        geo_filter::geo_check(&meta_omni, &config, false),
        GeoVerdict::Accept,
        "Known false positive: 'omni' contains 'mn' substring"
    );
}

// ===========================================================================
// Adversarial: Geo filter with invalid/degenerate coordinates
// ===========================================================================

/// NaN latitude should produce NaN distance → fails the `<= radius_km` check
/// (NaN comparisons are always false), so it should reject.
#[test]
fn geo_nan_coords_rejected() {
    let meta = NodeMeta {
        location: Some(GeoPoint {
            lat: f64::NAN,
            lng: -93.2650,
            precision: GeoPrecision::Exact,
        }),
        ..test_meta()
    };
    let terms = mpls_geo_terms();
    let config = mpls_config(&terms);
    // haversine_km(44.97, -93.26, NaN, -93.26) → NaN
    // NaN <= 30.0 → false → Reject
    assert_eq!(
        geo_filter::geo_check(&meta, &config, false),
        GeoVerdict::Reject,
        "NaN coordinates should be rejected (NaN <= radius is false)"
    );
}

/// Infinity latitude → haversine returns NaN or Infinity → Reject.
#[test]
fn geo_infinity_coords_rejected() {
    let meta = NodeMeta {
        location: Some(GeoPoint {
            lat: f64::INFINITY,
            lng: -93.2650,
            precision: GeoPrecision::Exact,
        }),
        ..test_meta()
    };
    let terms = mpls_geo_terms();
    let config = mpls_config(&terms);
    assert_eq!(
        geo_filter::geo_check(&meta, &config, false),
        GeoVerdict::Reject,
        "Infinity coordinates should be rejected"
    );
}

/// Out-of-range latitude (> 90°) still produces a haversine result — it will
/// be some distance away and likely rejected, but this documents the behavior.
#[test]
fn geo_out_of_range_lat_still_computes() {
    let meta = NodeMeta {
        location: Some(GeoPoint {
            lat: 200.0, // invalid: latitude > 90
            lng: -93.2650,
            precision: GeoPrecision::Exact,
        }),
        ..test_meta()
    };
    let terms = mpls_geo_terms();
    let config = mpls_config(&terms);
    let verdict = geo_filter::geo_check(&meta, &config, false);
    // haversine_km will produce some value (not NaN) because sin/cos still work
    // on out-of-range inputs. The distance will be large → Reject.
    assert_eq!(
        verdict,
        GeoVerdict::Reject,
        "Out-of-range latitude (200°) should produce large distance → Reject"
    );
}

/// Coordinates exactly at the radius boundary (30km) should Accept (<=).
#[test]
fn geo_coords_exactly_at_boundary() {
    // 30km due north of Minneapolis center ≈ lat 45.2476
    // 1° lat ≈ 111.32 km, so 30km ≈ 0.2695°
    let boundary_lat = 44.9778 + 30.0 / 111.32;
    let meta = NodeMeta {
        location: Some(GeoPoint {
            lat: boundary_lat,
            lng: -93.2650,
            precision: GeoPrecision::Exact,
        }),
        ..test_meta()
    };
    let terms = mpls_geo_terms();
    let config = mpls_config(&terms);
    let dist = rootsignal_common::haversine_km(44.9778, -93.2650, boundary_lat, -93.2650);
    // Should be very close to 30.0km. Accept if <= 30.0.
    assert!(
        (dist - 30.0).abs() < 0.1,
        "Boundary point should be ~30km away, got {dist}km"
    );
    assert_eq!(
        geo_filter::geo_check(&meta, &config, false),
        GeoVerdict::Accept,
        "Point at exactly the radius boundary should Accept (<=)"
    );
}

/// Point just barely outside the radius should Reject.
#[test]
fn geo_coords_just_outside_boundary() {
    // 30.5km north
    let outside_lat = 44.9778 + 30.5 / 111.32;
    let meta = NodeMeta {
        location: Some(GeoPoint {
            lat: outside_lat,
            lng: -93.2650,
            precision: GeoPrecision::Exact,
        }),
        ..test_meta()
    };
    let terms = mpls_geo_terms();
    let config = mpls_config(&terms);
    assert_eq!(
        geo_filter::geo_check(&meta, &config, false),
        GeoVerdict::Reject,
        "Point 30.5km away should Reject"
    );
}

// ===========================================================================
// Adversarial: Quality scoring edge cases
// ===========================================================================

/// Notice nodes are always actionable — even with zero fields filled.
/// This is by design but has quality implications.
#[test]
fn notice_always_actionable_even_empty() {
    let node = Node::Notice(NoticeNode {
        meta: NodeMeta {
            title: "Policy Update".into(),
            summary: "Vague policy thing".into(),
            // no location, nothing
            ..test_meta()
        },
        severity: Severity::Low,
        category: None,
        effective_date: None,
        source_authority: None,
    });

    let q = quality::score(&node);
    assert!(
        q.actionable,
        "Notice should always be actionable by design"
    );
    // But confidence should be low: completeness 0/1, geo Low (0.3)
    // confidence = 0.0 * 0.5 + 0.3 * 0.5 = 0.15
    assert!(
        q.confidence < 0.2,
        "Empty Notice should have very low confidence: {}",
        q.confidence
    );
}

/// A Gathering whose action_url equals its source_url should NOT count as
/// having a real action URL (quality.rs line 33 checks this).
#[test]
fn gathering_action_url_same_as_source_url_not_actionable() {
    let source = "https://example.com/news/article";
    let node = Node::Gathering(GatheringNode {
        meta: NodeMeta {
            title: "Event from news article".into(),
            summary: "Some event".into(),
            source_url: source.into(),
            location: Some(GeoPoint {
                lat: 44.97,
                lng: -93.26,
                precision: GeoPrecision::Exact,
            }),
            ..test_meta()
        },
        starts_at: Some(Utc::now()),
        ends_at: None,
        action_url: source.into(), // same as source_url!
        organizer: None,
        is_recurring: false,
    });

    let q = quality::score(&node);
    assert!(
        !q.has_action_url,
        "action_url == source_url should not count as having a real action URL"
    );
    assert!(
        !q.actionable,
        "Gathering with action_url == source_url should not be actionable"
    );
}

/// A Need with action_url is always actionable (no timing requirement).
#[test]
fn need_with_url_is_actionable_without_timing() {
    let node = Node::Need(NeedNode {
        meta: NodeMeta {
            title: "Winter Coat Drive".into(),
            summary: "Need 500 coats by Jan 31".into(),
            location: Some(GeoPoint {
                lat: 44.97,
                lng: -93.26,
                precision: GeoPrecision::Exact,
            }),
            ..test_meta()
        },
        urgency: Urgency::High,
        what_needed: Some("Winter coats".into()),
        action_url: Some("https://donate.example.com".into()),
        goal: Some("500 coats".into()),
    });

    let q = quality::score(&node);
    assert!(q.actionable, "Need with action_url should be actionable");
    assert!(!q.has_timing, "Need never has timing in quality scorer");
    // completeness: location (1) + action_url (1) = 2/2 = 1.0
    // geo High (1.0) → confidence = 1.0 * 0.5 + 1.0 * 0.5 = 1.0
    assert!(
        (q.confidence - 1.0).abs() < 0.01,
        "Complete Need should have max confidence: {}",
        q.confidence
    );
}

/// A Need without an action_url should NOT be actionable.
#[test]
fn need_without_url_not_actionable() {
    let node = Node::Need(NeedNode {
        meta: test_meta(),
        urgency: Urgency::Critical,
        what_needed: Some("Emergency housing".into()),
        action_url: None,
        goal: None,
    });

    let q = quality::score(&node);
    assert!(!q.actionable, "Need without action_url should not be actionable");
}

/// Aid with is_ongoing=false and no other timing → completeness 2/3 at best.
/// Documents that one-time distributions score lower than ongoing services.
#[test]
fn one_time_aid_distribution_scores_lower_than_ongoing() {
    let base_meta = NodeMeta {
        location: Some(GeoPoint {
            lat: 44.97,
            lng: -93.26,
            precision: GeoPrecision::Exact,
        }),
        ..test_meta()
    };

    let ongoing = Node::Aid(AidNode {
        meta: base_meta.clone(),
        action_url: "https://example.com/food".into(),
        availability: Some("Every weekday".into()),
        is_ongoing: true,
    });

    let one_time = Node::Aid(AidNode {
        meta: base_meta,
        action_url: "https://example.com/food".into(),
        availability: Some("Saturday March 15 only".into()),
        is_ongoing: false,
    });

    let q_ongoing = quality::score(&ongoing);
    let q_one_time = quality::score(&one_time);

    assert!(
        q_ongoing.confidence > q_one_time.confidence,
        "Ongoing aid ({}) should score higher than one-time ({})",
        q_ongoing.confidence,
        q_one_time.confidence
    );
    assert!(q_ongoing.actionable, "Ongoing aid with URL should be actionable");
    assert!(!q_one_time.actionable, "One-time aid (is_ongoing=false) with URL should NOT be actionable");
}

/// Quality scorer treats backfilled Approximate coords as Low accuracy,
/// which means a signal with a precise address name but no coords gets
/// penalized more than one with imprecise coords that happen to be present.
#[test]
fn backfilled_approximate_scores_lower_than_provided_neighborhood() {
    // Signal A: has precise address name, but coords were backfilled → Approximate
    let node_a = Node::Tension(TensionNode {
        meta: NodeMeta {
            title: "Issue at 3524 15th Ave S".into(),
            summary: "Specific address issue".into(),
            location: Some(GeoPoint {
                lat: 44.9778,
                lng: -93.2650,
                precision: GeoPrecision::Approximate, // backfilled
            }),
            location_name: Some("3524 15th Ave S, Minneapolis".into()),
            ..test_meta()
        },
        severity: Severity::Medium,
        category: None,
        what_would_help: None,
    });

    // Signal B: vague neighborhood name, but has Neighborhood-precision coords
    let node_b = Node::Tension(TensionNode {
        meta: NodeMeta {
            title: "Issue in Powderhorn area".into(),
            summary: "Neighborhood-level issue".into(),
            location: Some(GeoPoint {
                lat: 44.9486,
                lng: -93.2636,
                precision: GeoPrecision::Neighborhood,
            }),
            location_name: Some("Powderhorn area".into()),
            ..test_meta()
        },
        severity: Severity::Medium,
        category: None,
        what_would_help: None,
    });

    let q_a = quality::score(&node_a);
    let q_b = quality::score(&node_b);

    // Approximate → GeoAccuracy::Low (0.3), Neighborhood → GeoAccuracy::Medium (0.7)
    // So B scores higher even though A has a more precise address.
    assert_eq!(q_a.geo_accuracy, GeoAccuracy::Low);
    assert_eq!(q_b.geo_accuracy, GeoAccuracy::Medium);
    assert!(
        q_b.confidence > q_a.confidence,
        "Known quality inversion: backfilled Approximate ({}) scores lower than Neighborhood ({})",
        q_a.confidence,
        q_b.confidence
    );
}

// ===========================================================================
// Adversarial: Batch filtering with mixed edge cases
// ===========================================================================

/// Batch filter with all degenerate signals: NaN coords, empty names, etc.
/// Nothing should survive.
#[test]
fn batch_filter_all_degenerate_signals_rejected() {
    let terms = mpls_geo_terms();
    let config = mpls_config(&terms);

    let nan_node = Node::Gathering(GatheringNode {
        meta: NodeMeta {
            location: Some(GeoPoint {
                lat: f64::NAN,
                lng: f64::NAN,
                precision: GeoPrecision::Exact,
            }),
            ..test_meta()
        },
        starts_at: None,
        ends_at: None,
        action_url: String::new(),
        organizer: None,
        is_recurring: false,
    });

    let empty_name = Node::Aid(AidNode {
        meta: NodeMeta {
            location: None,
            location_name: Some(String::new()),
            ..test_meta()
        },
        action_url: String::new(),
        availability: None,
        is_ongoing: false,
    });

    let unknown_name = Node::Tension(TensionNode {
        meta: NodeMeta {
            location: None,
            location_name: Some("<UNKNOWN>".into()),
            ..test_meta()
        },
        severity: Severity::Low,
        category: None,
        what_would_help: None,
    });

    let no_info = Node::Need(NeedNode {
        meta: test_meta(), // no location, no name
        urgency: Urgency::Low,
        what_needed: None,
        action_url: None,
        goal: None,
    });

    let (accepted, stats) = geo_filter::filter_nodes(
        vec![nan_node, empty_name, unknown_name, no_info],
        &config,
        false,
    );
    assert_eq!(accepted.len(), 0, "No degenerate signal should survive");
    assert_eq!(stats.filtered, 4);
}
