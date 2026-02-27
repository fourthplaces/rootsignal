use chrono::Utc;
use rootsignal_common::types::{DiscoveryMethod, Severity, SourceNode, SourceRole};
use rootsignal_graph::severity_inference::{infer_notice_severity, is_source_trusted};
use uuid::Uuid;

fn trusted_source() -> SourceNode {
    SourceNode {
        id: Uuid::new_v4(),
        canonical_key: "https://trusted-local-news.org".to_string(),
        canonical_value: "https://trusted-local-news.org".to_string(),
        url: Some("https://trusted-local-news.org".to_string()),
        discovery_method: DiscoveryMethod::Curated,
        created_at: Utc::now(),
        last_scraped: None,
        last_produced_signal: None,
        signals_produced: 50,
        signals_corroborated: 10,
        consecutive_empty_runs: 0,
        active: true,
        gap_context: None,
        weight: 0.8,
        cadence_hours: Some(24),
        avg_signals_per_scrape: 5.0,
        quality_penalty: 1.0,
        source_role: SourceRole::Mixed,
        scrape_count: 30,
    }
}

fn new_source() -> SourceNode {
    SourceNode {
        id: Uuid::new_v4(),
        canonical_key: "https://unknown-blog.example".to_string(),
        canonical_value: "https://unknown-blog.example".to_string(),
        url: Some("https://unknown-blog.example".to_string()),
        discovery_method: DiscoveryMethod::SignalExpansion,
        created_at: Utc::now(),
        last_scraped: None,
        last_produced_signal: None,
        signals_produced: 1,
        signals_corroborated: 0,
        consecutive_empty_runs: 0,
        active: true,
        gap_context: None,
        weight: 0.5,
        cadence_hours: None,
        avg_signals_per_scrape: 1.0,
        quality_penalty: 1.0,
        source_role: SourceRole::Mixed,
        scrape_count: 2,
    }
}

#[test]
fn known_reliable_source_grounded_in_tension_is_high_priority() {
    let source = trusted_source();
    let trusted = is_source_trusted(&source);

    let severity = infer_notice_severity(Severity::Medium, trusted, true, 0, 0);

    assert_eq!(severity, Severity::High);
}

#[test]
fn known_reliable_source_still_visible_before_tension_link_exists() {
    let source = trusted_source();
    let trusted = is_source_trusted(&source);

    let severity = infer_notice_severity(Severity::Low, trusted, false, 0, 0);

    assert_eq!(severity, Severity::Medium);
}

#[test]
fn unverified_source_without_grounding_does_not_escalate() {
    let source = new_source();
    let trusted = is_source_trusted(&source);

    let severity = infer_notice_severity(Severity::High, trusted, false, 0, 0);

    assert_eq!(severity, Severity::Low);
}

#[test]
fn multiple_independent_sources_confirming_same_grounded_threat_is_high_priority() {
    let source = new_source();
    let trusted = is_source_trusted(&source);

    let severity = infer_notice_severity(Severity::Medium, trusted, true, 3, 2);

    assert_eq!(severity, Severity::High);
}

#[test]
fn lone_unverified_report_linked_to_tension_stays_at_extracted_level() {
    let source = new_source();
    let trusted = is_source_trusted(&source);

    let severity = infer_notice_severity(Severity::Medium, trusted, true, 0, 1);

    assert_eq!(severity, Severity::Medium);
}

#[test]
fn new_source_with_little_history_is_not_considered_trusted() {
    let source = new_source();

    assert!(!is_source_trusted(&source));
}
