//! Layer 2: Scenario-driven quality scoring tests.
//!
//! Pure functions, no LLM, no infrastructure. Validates `quality::score()`
//! against realistic extraction patterns matching the fixture content in
//! `tests/fixtures/`.
//!
//! Run with: cargo test -p rootsignal-scout --test quality_scenarios_test

use chrono::Utc;
use uuid::Uuid;

use rootsignal_common::{
    AidNode, GatheringNode, GeoAccuracy, GeoPoint, GeoPrecision, NeedNode, Node, NodeMeta,
    NoticeNode, ReviewStatus, SensitivityLevel, Severity, TensionNode, Urgency,
};
use rootsignal_scout::enrichment::quality;

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
        corroboration_count: 0,
        about_location: None,
        about_location_name: None,
        from_location: None,
        source_url: "https://example.com".into(),
        extracted_at: Utc::now(),
        published_at: None,
        last_confirmed_active: Utc::now(),
        source_diversity: 1,
        cause_heat: 0.0,
        implied_queries: vec![],
        channel_diversity: 1,
        review_status: ReviewStatus::Staged,
        was_corrected: false,
        corrections: None,
        rejection_reason: None,
        mentioned_actors: Vec::new(),
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
            about_location: Some(GeoPoint {
                lat: 44.9486,
                lng: -93.2636,
                precision: GeoPrecision::Exact,
            }),
            about_location_name: Some("Powderhorn Community Garden, 3524 15th Ave S".into()),
            from_location: None,
            source_url: "https://powderhornpark.org/events".into(),
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
    assert!(
        q.actionable,
        "Gathering with URL + date should be actionable"
    );
    assert!(q.has_location);
    assert!(q.has_action_url);
    assert!(q.has_timing);
    assert_eq!(q.geo_accuracy, rootsignal_common::GeoAccuracy::High);
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
            about_location: Some(GeoPoint {
                lat: 44.9696,
                lng: -93.2466,
                precision: GeoPrecision::Exact,
            }),
            about_location_name: Some("420 15th Ave S, Minneapolis, MN 55454".into()),
            from_location: None,
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
    assert!(
        q.actionable,
        "Aid with action_url + is_ongoing should be actionable"
    );
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
            about_location: Some(GeoPoint {
                lat: 44.9696,
                lng: -93.2466,
                precision: GeoPrecision::Exact,
            }),
            about_location_name: Some("420 15th Ave S".into()),
            from_location: None,
            ..test_meta()
        },
        action_url: String::new(),
        availability: Some("Tue-Fri 10-4".into()),
        is_ongoing: true,
    });

    let q = quality::score(&node);
    assert!(
        !q.actionable,
        "Aid without action_url should not be actionable"
    );
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
            about_location: Some(GeoPoint {
                lat: 44.9486,
                lng: -93.2476,
                precision: GeoPrecision::Neighborhood,
            }),
            about_location_name: Some("Phillips neighborhood, Minneapolis".into()),
            from_location: None,
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
    assert!(
        !q.actionable,
        "Tension signals are never actionable (no action_url concept)"
    );
}

/// Emergency community meeting from the tension fixture should be a
/// complete Gathering with high confidence.
#[test]
fn emergency_meeting_gathering_is_actionable() {
    let node = Node::Gathering(GatheringNode {
        meta: NodeMeta {
            title: "Emergency Community Meeting".into(),
            summary: "Wednesday 6 PM at Sagrado Corazón Church to coordinate ICE response".into(),
            about_location: Some(GeoPoint {
                lat: 44.9480,
                lng: -93.2380,
                precision: GeoPrecision::Exact,
            }),
            about_location_name: Some("Sagrado Corazón Church, 2018 E. Lake St".into()),
            from_location: None,
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
    assert!(
        !q.actionable,
        "Gathering without distinct action_url is not actionable"
    );
}

/// Legal aid from the tension fixture — an Aid signal with location.
#[test]
fn legal_aid_signal_scores_reasonably() {
    let node = Node::Aid(AidNode {
        meta: NodeMeta {
            title: "Legal Support for Immigrants".into(),
            summary: "MIRAC legal aid available at Centro de Trabajadores Unidos".into(),
            sensitivity: SensitivityLevel::Elevated,
            about_location: Some(GeoPoint {
                lat: 44.9480,
                lng: -93.2476,
                precision: GeoPrecision::Exact,
            }),
            about_location_name: Some("Centro de Trabajadores Unidos, 2104 Bloomington Ave".into()),
            from_location: None,
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
            ..test_meta()
        },
        severity: Severity::Low,
        category: None,
        effective_date: None,
        source_authority: None,
    });

    let q = quality::score(&node);
    assert!(q.actionable, "Notice should always be actionable by design");
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
            about_location: Some(GeoPoint {
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
            about_location: Some(GeoPoint {
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
    assert!(
        !q.actionable,
        "Need without action_url should not be actionable"
    );
}

/// Aid with is_ongoing=false and no other timing → completeness 2/3 at best.
/// Documents that one-time distributions score lower than ongoing services.
#[test]
fn one_time_aid_distribution_scores_lower_than_ongoing() {
    let base_meta = NodeMeta {
        about_location: Some(GeoPoint {
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
    assert!(
        q_ongoing.actionable,
        "Ongoing aid with URL should be actionable"
    );
    assert!(
        !q_one_time.actionable,
        "One-time aid (is_ongoing=false) with URL should NOT be actionable"
    );
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
            about_location: Some(GeoPoint {
                lat: 44.9778,
                lng: -93.2650,
                precision: GeoPrecision::Approximate, // backfilled
            }),
            about_location_name: Some("3524 15th Ave S, Minneapolis".into()),
            from_location: None,
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
            about_location: Some(GeoPoint {
                lat: 44.9486,
                lng: -93.2636,
                precision: GeoPrecision::Neighborhood,
            }),
            about_location_name: Some("Powderhorn area".into()),
            from_location: None,
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
