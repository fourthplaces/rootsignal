//! Conversion tests: ExtractionResponse → ExtractionResult via Extractor::convert_signals().
//!
//! Each test: hand-craft ExtractionResponse JSON → convert_signals() → assert.
//! No I/O, no LLM, no Neo4j.

use chrono::Datelike;
use rootsignal_common::*;
use rootsignal_scout::pipeline::extractor::{ExtractionResponse, Extractor};

fn parse_response(json: &str) -> ExtractionResponse {
    serde_json::from_str(json).expect("invalid test JSON")
}

// ---------------------------------------------------------------------------
// Filtering
// ---------------------------------------------------------------------------

#[test]
fn junk_title_filtered() {
    let response = parse_response(
        r#"{
        "signals": [{
            "signal_type": "aid",
            "title": "Unable to extract content from this page",
            "summary": "Error",
            "sensitivity": "general"
        }]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    assert!(
        result.nodes.is_empty(),
        "junk signal should be filtered out"
    );
    assert_eq!(result.rejected.len(), 1);
    assert_eq!(result.rejected[0].reason, "junk_extraction");
}

#[test]
fn page_not_found_filtered() {
    let response = parse_response(
        r#"{
        "signals": [{
            "signal_type": "notice",
            "title": "Page not found - 404",
            "summary": "Missing page",
            "sensitivity": "general"
        }]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    assert!(result.nodes.is_empty());
    assert_eq!(result.rejected[0].reason, "junk_extraction");
}

#[test]
fn non_firsthand_signal_rejected() {
    let response = parse_response(
        r#"{
        "signals": [{
            "signal_type": "tension",
            "title": "Political commentary on housing",
            "summary": "Opinion piece",
            "sensitivity": "general",
            "is_firsthand": false
        }]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    assert!(result.nodes.is_empty());
    assert_eq!(result.rejected.len(), 1);
    assert_eq!(result.rejected[0].reason, "not_firsthand");
}

#[test]
fn firsthand_null_signal_kept() {
    let response = parse_response(
        r#"{
        "signals": [{
            "signal_type": "aid",
            "title": "Free food shelf",
            "summary": "Open Tuesdays",
            "sensitivity": "general"
        }]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    assert_eq!(
        result.nodes.len(),
        1,
        "missing is_firsthand should default to keep"
    );
}

#[test]
fn firsthand_true_signal_kept() {
    let response = parse_response(
        r#"{
        "signals": [{
            "signal_type": "need",
            "title": "My family needs winter coats",
            "summary": "Direct plea",
            "sensitivity": "sensitive",
            "is_firsthand": true
        }]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    assert_eq!(result.nodes.len(), 1);
}

// ---------------------------------------------------------------------------
// Signal type mapping
// ---------------------------------------------------------------------------

#[test]
fn unknown_signal_type_skipped() {
    let response = parse_response(
        r#"{
        "signals": [{
            "signal_type": "unknown_thing",
            "title": "Something",
            "summary": "Whatever",
            "sensitivity": "general"
        }]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    assert!(
        result.nodes.is_empty(),
        "unknown signal type should be skipped"
    );
    assert!(
        result.rejected.is_empty(),
        "unknown type is skipped, not rejected"
    );
}

#[test]
fn all_five_signal_types_convert() {
    let response = parse_response(
        r#"{
        "signals": [
            {"signal_type": "gathering", "title": "G", "summary": "s", "sensitivity": "general"},
            {"signal_type": "aid", "title": "A", "summary": "s", "sensitivity": "general"},
            {"signal_type": "need", "title": "N", "summary": "s", "sensitivity": "general"},
            {"signal_type": "notice", "title": "O", "summary": "s", "sensitivity": "general"},
            {"signal_type": "tension", "title": "T", "summary": "s", "sensitivity": "general"}
        ]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    assert_eq!(result.nodes.len(), 5);
    assert!(matches!(result.nodes[0], Node::Gathering(_)));
    assert!(matches!(result.nodes[1], Node::Aid(_)));
    assert!(matches!(result.nodes[2], Node::Need(_)));
    assert!(matches!(result.nodes[3], Node::Notice(_)));
    assert!(matches!(result.nodes[4], Node::Tension(_)));
}

// ---------------------------------------------------------------------------
// Enum mapping
// ---------------------------------------------------------------------------

#[test]
fn sensitivity_string_converts_to_typed_level() {
    let response = parse_response(
        r#"{
        "signals": [
            {"signal_type": "aid", "title": "A", "summary": "s", "sensitivity": "sensitive"},
            {"signal_type": "aid", "title": "B", "summary": "s", "sensitivity": "elevated"},
            {"signal_type": "aid", "title": "C", "summary": "s", "sensitivity": "general"},
            {"signal_type": "aid", "title": "D", "summary": "s", "sensitivity": "nonsense"}
        ]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    assert_eq!(
        result.nodes[0].meta().unwrap().sensitivity,
        SensitivityLevel::Sensitive
    );
    assert_eq!(
        result.nodes[1].meta().unwrap().sensitivity,
        SensitivityLevel::Elevated
    );
    assert_eq!(
        result.nodes[2].meta().unwrap().sensitivity,
        SensitivityLevel::General
    );
    assert_eq!(
        result.nodes[3].meta().unwrap().sensitivity,
        SensitivityLevel::General,
        "unknown defaults to General"
    );
}

#[test]
fn severity_string_converts_to_typed_level() {
    let response = parse_response(
        r#"{
        "signals": [
            {"signal_type": "tension", "title": "A", "summary": "s", "sensitivity": "general", "severity": "critical"},
            {"signal_type": "tension", "title": "B", "summary": "s", "sensitivity": "general", "severity": "high"},
            {"signal_type": "tension", "title": "C", "summary": "s", "sensitivity": "general", "severity": "medium"},
            {"signal_type": "tension", "title": "D", "summary": "s", "sensitivity": "general", "severity": "low"},
            {"signal_type": "tension", "title": "E", "summary": "s", "sensitivity": "general", "severity": "nonsense"}
        ]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    let severities: Vec<_> = result
        .nodes
        .iter()
        .map(|n| match n {
            Node::Tension(t) => t.severity.clone(),
            _ => panic!("expected tension"),
        })
        .collect();

    assert_eq!(
        severities,
        vec![
            Severity::Critical,
            Severity::High,
            Severity::Medium,
            Severity::Low,
            Severity::Medium
        ]
    );
}

#[test]
fn urgency_string_converts_to_typed_level() {
    let response = parse_response(
        r#"{
        "signals": [
            {"signal_type": "need", "title": "A", "summary": "s", "sensitivity": "general", "urgency": "critical"},
            {"signal_type": "need", "title": "B", "summary": "s", "sensitivity": "general", "urgency": "high"},
            {"signal_type": "need", "title": "C", "summary": "s", "sensitivity": "general", "urgency": "low"},
            {"signal_type": "need", "title": "D", "summary": "s", "sensitivity": "general", "urgency": "nonsense"},
            {"signal_type": "need", "title": "E", "summary": "s", "sensitivity": "general"}
        ]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    let urgencies: Vec<_> = result
        .nodes
        .iter()
        .map(|n| match n {
            Node::Need(nd) => nd.urgency.clone(),
            _ => panic!("expected need"),
        })
        .collect();

    assert_eq!(
        urgencies,
        vec![
            Urgency::Critical,
            Urgency::High,
            Urgency::Low,
            Urgency::Medium,
            Urgency::Medium
        ]
    );
}

// ---------------------------------------------------------------------------
// Date parsing
// ---------------------------------------------------------------------------

#[test]
fn gathering_date_parsed_from_rfc3339() {
    let response = parse_response(
        r#"{
        "signals": [{
            "signal_type": "gathering",
            "title": "Spring event",
            "summary": "s",
            "sensitivity": "general",
            "starts_at": "2026-04-12T18:00:00Z"
        }]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    if let Node::Gathering(g) = &result.nodes[0] {
        let starts = g.starts_at.expect("should have starts_at");
        assert_eq!(starts.date_naive().month(), 4);
        assert_eq!(starts.date_naive().day(), 12);
        assert_eq!(starts.date_naive().year(), 2026);
    } else {
        panic!("expected Gathering");
    }
}

#[test]
fn invalid_date_becomes_none() {
    let response = parse_response(
        r#"{
        "signals": [{
            "signal_type": "gathering",
            "title": "Bad date event",
            "summary": "s",
            "sensitivity": "general",
            "starts_at": "not-a-date"
        }]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    if let Node::Gathering(g) = &result.nodes[0] {
        assert!(g.starts_at.is_none(), "invalid date should become None");
    } else {
        panic!("expected Gathering");
    }
}

#[test]
fn missing_date_becomes_none() {
    let response = parse_response(
        r#"{
        "signals": [{
            "signal_type": "gathering",
            "title": "No date event",
            "summary": "s",
            "sensitivity": "general"
        }]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    if let Node::Gathering(g) = &result.nodes[0] {
        assert!(g.starts_at.is_none());
    } else {
        panic!("expected Gathering");
    }
}

// ---------------------------------------------------------------------------
// Geo precision
// ---------------------------------------------------------------------------

#[test]
fn geo_precision_string_converts_to_typed_precision() {
    let response = parse_response(
        r#"{
        "signals": [
            {"signal_type": "aid", "title": "A", "summary": "s", "sensitivity": "general",
             "latitude": 44.97, "longitude": -93.26, "geo_precision": "exact"},
            {"signal_type": "aid", "title": "B", "summary": "s", "sensitivity": "general",
             "latitude": 44.97, "longitude": -93.26, "geo_precision": "neighborhood"},
            {"signal_type": "aid", "title": "C", "summary": "s", "sensitivity": "general",
             "latitude": 44.97, "longitude": -93.26, "geo_precision": "other"},
            {"signal_type": "aid", "title": "D", "summary": "s", "sensitivity": "general"}
        ]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    let loc0 = result.nodes[0].meta().unwrap().about_location.unwrap();
    assert_eq!(loc0.precision, GeoPrecision::Exact);

    let loc1 = result.nodes[1].meta().unwrap().about_location.unwrap();
    assert_eq!(loc1.precision, GeoPrecision::Neighborhood);

    let loc2 = result.nodes[2].meta().unwrap().about_location.unwrap();
    assert_eq!(
        loc2.precision,
        GeoPrecision::Approximate,
        "unknown precision defaults to Approximate"
    );

    assert!(
        result.nodes[3].meta().unwrap().about_location.is_none(),
        "no lat/lng means no location"
    );
}

// ---------------------------------------------------------------------------
// Source URL fallback
// ---------------------------------------------------------------------------

#[test]
fn signal_source_url_overrides_page_url() {
    let response = parse_response(
        r#"{
        "signals": [{
            "signal_type": "aid",
            "title": "Food shelf",
            "summary": "s",
            "sensitivity": "general",
            "source_url": "https://specific-post.com/123"
        }]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://page-level.com");

    assert_eq!(
        result.nodes[0].meta().unwrap().source_url,
        "https://specific-post.com/123"
    );
}

#[test]
fn missing_signal_source_url_falls_back_to_page() {
    let response = parse_response(
        r#"{
        "signals": [{
            "signal_type": "aid",
            "title": "Food shelf",
            "summary": "s",
            "sensitivity": "general"
        }]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://page-level.com");

    assert_eq!(
        result.nodes[0].meta().unwrap().source_url,
        "https://page-level.com"
    );
}

#[test]
fn empty_signal_source_url_falls_back_to_page() {
    let response = parse_response(
        r#"{
        "signals": [{
            "signal_type": "aid",
            "title": "Food shelf",
            "summary": "s",
            "sensitivity": "general",
            "source_url": ""
        }]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://page-level.com");

    assert_eq!(
        result.nodes[0].meta().unwrap().source_url,
        "https://page-level.com"
    );
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

#[test]
fn aid_defaults_is_ongoing_true() {
    let response = parse_response(
        r#"{
        "signals": [{
            "signal_type": "aid",
            "title": "Food shelf",
            "summary": "s",
            "sensitivity": "general"
        }]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    if let Node::Aid(a) = &result.nodes[0] {
        assert!(a.is_ongoing, "aid should default to is_ongoing=true");
    } else {
        panic!("expected Aid");
    }
}

#[test]
fn gathering_defaults_is_recurring_false() {
    let response = parse_response(
        r#"{
        "signals": [{
            "signal_type": "gathering",
            "title": "One-time event",
            "summary": "s",
            "sensitivity": "general"
        }]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    if let Node::Gathering(g) = &result.nodes[0] {
        assert!(
            !g.is_recurring,
            "gathering should default to is_recurring=false"
        );
    } else {
        panic!("expected Gathering");
    }
}

#[test]
fn gathering_action_url_falls_back_to_source_url() {
    let response = parse_response(
        r#"{
        "signals": [{
            "signal_type": "gathering",
            "title": "Event",
            "summary": "s",
            "sensitivity": "general"
        }]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://page.com/events");

    if let Node::Gathering(g) = &result.nodes[0] {
        assert_eq!(g.action_url, "https://page.com/events");
    } else {
        panic!("expected Gathering");
    }
}

// ---------------------------------------------------------------------------
// Tags
// ---------------------------------------------------------------------------

#[test]
fn tags_are_slugified_during_conversion() {
    let response = parse_response(
        r#"{
        "signals": [{
            "signal_type": "aid",
            "title": "Food shelf",
            "summary": "s",
            "sensitivity": "general",
            "tags": ["Community Garden", "FOOD pantry", "mutual-aid"]
        }]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    assert_eq!(result.signal_tags.len(), 1);
    let (_, tags) = &result.signal_tags[0];
    assert!(tags.contains(&"community-garden".to_string()));
    assert!(tags.contains(&"food-pantry".to_string()));
    assert!(tags.contains(&"mutual-aid".to_string()));
}

// ---------------------------------------------------------------------------
// Resource tags
// ---------------------------------------------------------------------------

#[test]
fn resource_tags_paired_with_signal() {
    let response = parse_response(
        r#"{
        "signals": [{
            "signal_type": "aid",
            "title": "Food shelf",
            "summary": "s",
            "sensitivity": "general",
            "resources": [
                {"slug": "food", "role": "offers", "confidence": 0.9}
            ]
        }]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    assert_eq!(result.resource_tags.len(), 1);
    let (_, resources) = &result.resource_tags[0];
    assert_eq!(resources[0].slug, "food");
    assert_eq!(resources[0].role, "offers");
}

// ---------------------------------------------------------------------------
// Implied queries
// ---------------------------------------------------------------------------

#[test]
fn implied_queries_aggregated_across_signals() {
    let response = parse_response(
        r#"{
        "signals": [
            {"signal_type": "tension", "title": "A", "summary": "s", "sensitivity": "general",
             "implied_queries": ["legal aid Minneapolis"]},
            {"signal_type": "need", "title": "B", "summary": "s", "sensitivity": "general",
             "implied_queries": ["emergency housing Minneapolis", "shelter beds available"]}
        ]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    assert_eq!(result.implied_queries.len(), 3);
    assert!(result
        .implied_queries
        .contains(&"legal aid Minneapolis".to_string()));
    assert!(result
        .implied_queries
        .contains(&"emergency housing Minneapolis".to_string()));
}

// ---------------------------------------------------------------------------
// Schedule / RRULE
// ---------------------------------------------------------------------------

#[test]
fn valid_rrule_produces_schedule() {
    // NOTE: the rrule crate expects iCalendar DTSTART format (20260401T180000Z),
    // not RFC3339. When starts_at is absent, the fallback "20260101T000000Z" is
    // iCalendar-compatible, so RRULE validation succeeds.
    let response = parse_response(
        r#"{
        "signals": [{
            "signal_type": "gathering",
            "title": "Weekly meetup",
            "summary": "s",
            "sensitivity": "general",
            "rrule": "FREQ=WEEKLY;BYDAY=WE"
        }]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    assert_eq!(result.schedules.len(), 1);
    let (_, schedule) = &result.schedules[0];
    assert_eq!(schedule.rrule.as_deref(), Some("FREQ=WEEKLY;BYDAY=WE"));
}

#[test]
fn rrule_with_rfc3339_starts_at_is_discarded() {
    // This documents a known issue: when starts_at is RFC3339 (from the LLM),
    // the DTSTART string fed to the rrule parser is invalid iCalendar format,
    // so the RRULE is silently discarded even if it's valid.
    let response = parse_response(
        r#"{
        "signals": [{
            "signal_type": "gathering",
            "title": "Weekly meetup",
            "summary": "s",
            "sensitivity": "general",
            "starts_at": "2026-04-01T18:00:00Z",
            "rrule": "FREQ=WEEKLY;BYDAY=WE",
            "schedule_text": "Every Wednesday"
        }]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    assert_eq!(result.schedules.len(), 1);
    let (_, schedule) = &result.schedules[0];
    assert!(
        schedule.rrule.is_none(),
        "RFC3339 starts_at breaks rrule validation"
    );
    assert_eq!(schedule.schedule_text.as_deref(), Some("Every Wednesday"));
}

#[test]
fn invalid_rrule_falls_back_to_schedule_text() {
    let response = parse_response(
        r#"{
        "signals": [{
            "signal_type": "gathering",
            "title": "Weekly meetup",
            "summary": "s",
            "sensitivity": "general",
            "starts_at": "2026-04-01T18:00:00Z",
            "rrule": "NOT_A_VALID_RRULE",
            "schedule_text": "Every Wednesday at 6pm"
        }]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    assert_eq!(result.schedules.len(), 1);
    let (_, schedule) = &result.schedules[0];
    assert!(
        schedule.rrule.is_none(),
        "invalid RRULE should be discarded"
    );
    assert_eq!(
        schedule.schedule_text.as_deref(),
        Some("Every Wednesday at 6pm")
    );
}

#[test]
fn schedule_not_created_for_non_schedule_types() {
    let response = parse_response(
        r#"{
        "signals": [{
            "signal_type": "tension",
            "title": "Ongoing issue",
            "summary": "s",
            "sensitivity": "general",
            "rrule": "FREQ=WEEKLY;BYDAY=MO"
        }]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    assert!(
        result.schedules.is_empty(),
        "tension signals should not get schedules"
    );
}

// ---------------------------------------------------------------------------
// Mixed: valid + junk + non-firsthand in same response
// ---------------------------------------------------------------------------

#[test]
fn mixed_valid_and_invalid_signals_filters_junk_keeps_good() {
    let response = parse_response(
        r#"{
        "signals": [
            {"signal_type": "aid", "title": "Real food shelf", "summary": "s", "sensitivity": "general"},
            {"signal_type": "tension", "title": "Unable to extract this page", "summary": "s", "sensitivity": "general"},
            {"signal_type": "need", "title": "Political take on housing", "summary": "s", "sensitivity": "general", "is_firsthand": false},
            {"signal_type": "gathering", "title": "Real community meeting", "summary": "s", "sensitivity": "general"}
        ]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    assert_eq!(
        result.nodes.len(),
        2,
        "should keep only the 2 valid signals"
    );
    assert_eq!(
        result.rejected.len(),
        2,
        "should reject junk + non-firsthand"
    );

    let titles: Vec<&str> = result
        .nodes
        .iter()
        .map(|n| n.meta().unwrap().title.as_str())
        .collect();
    assert!(titles.contains(&"Real food shelf"));
    assert!(titles.contains(&"Real community meeting"));
}

// ---------------------------------------------------------------------------
// Community report notices
// ---------------------------------------------------------------------------

#[test]
fn community_warning_extracted_as_notice_with_community_report_category() {
    let response = parse_response(
        r#"{
        "signals": [{
            "signal_type": "notice",
            "title": "ICE spotted near Rosemount transit center",
            "summary": "Community members reporting enforcement vehicles near the transit center, avoid the area if possible",
            "sensitivity": "sensitive",
            "severity": "high",
            "category": "community_report",
            "is_firsthand": true
        }]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    assert_eq!(result.nodes.len(), 1);
    if let Node::Notice(n) = &result.nodes[0] {
        assert_eq!(n.category.as_deref(), Some("community_report"));
        assert_eq!(n.severity, Severity::High);
        assert_eq!(n.meta.sensitivity, SensitivityLevel::Sensitive);
    } else {
        panic!("expected Notice");
    }
}

#[test]
fn notice_category_passes_through_from_extraction() {
    let response = parse_response(
        r#"{
        "signals": [{
            "signal_type": "notice",
            "title": "Great weather today in Minneapolis",
            "summary": "Sunny skies and warm temperatures",
            "sensitivity": "general",
            "severity": "low",
            "category": "community_report"
        }]
    }"#,
    );

    let result = Extractor::convert_signals(response, "https://example.com");

    assert_eq!(result.nodes.len(), 1);
    if let Node::Notice(n) = &result.nodes[0] {
        assert_eq!(
            n.category.as_deref(),
            Some("community_report"),
            "category passes through as-is; LLM prompt constrains what gets this label"
        );
    } else {
        panic!("expected Notice");
    }
}
