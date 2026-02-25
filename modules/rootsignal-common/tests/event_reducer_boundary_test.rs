//! Reducer boundary tests.
//!
//! These verify the contract between the Event enum and the reducer pattern:
//! - Every event has a deterministic event_type string
//! - Events cleanly serialize to JSONB and back
//! - Observability events are distinguishable from graph-mutating events
//! - The event_type tag matches the serde tag exactly
//! - Schema evolution works (old payloads missing new fields still deserialize)

use chrono::Utc;
use rootsignal_common::events::{Event, FieldCorrection, TagFact};
use rootsignal_common::types::*;
use rootsignal_common::safety::SensitivityLevel;
use serde_json::json;
use uuid::Uuid;

// =========================================================================
// Reducer classification — which events produce graph changes?
// =========================================================================

/// Events that the reducer should act on (produce graph mutations).
const GRAPH_MUTATING_TYPES: &[&str] = &[
    "signal_discovered",
    "signal_corroborated",
    "signal_refreshed",
    "signal_confidence_scored",
    "signal_fields_corrected",
    "signal_rejected",
    "signal_expired",
    "signal_purged",
    "signal_deduplicated",
    "review_verdict_reached",
    "implied_queries_consumed",
    "citation_recorded",
    "orphaned_citations_cleaned",
    "source_registered",
    "source_updated",
    "source_deactivated",
    "source_removed",
    "source_scrape_recorded",
    "source_link_discovered",
    "expansion_query_collected",
    "expansion_source_created",
    "actor_identified",
    "actor_linked_to_signal",
    "actor_linked_to_source",
    "actor_stats_updated",
    "actor_location_identified",
    "duplicate_actors_merged",
    "orphaned_actors_cleaned",
    "relationship_established",
    "situation_identified",
    "situation_evolved",
    "situation_promoted",
    "dispatch_created",
    "tags_aggregated",
    "tag_suppressed",
    "tags_merged",
    "lint_correction_applied",
    "lint_rejection_issued",
    "empty_signals_cleaned",
    "fake_coordinates_nulled",
    "schedule_recorded",
    "pin_created",
    "pins_removed",
    "demand_signal_received",
    "demand_aggregated",
    "submission_received",
];

/// Events that the reducer should ignore (observability / no-ops).
const OBSERVABILITY_TYPES: &[&str] = &[
    "url_scraped",
    "feed_scraped",
    "social_scraped",
    "social_topics_searched",
    "search_performed",
    "llm_extraction_completed",
    "budget_checkpoint",
    "bootstrap_completed",
    "agent_web_searched",
    "agent_page_read",
    "agent_future_query",
    "lint_batch_completed",
    "signal_dropped_no_date",
];

#[test]
fn observability_events_are_distinct_from_graph_events() {
    // No overlap between the two sets
    for obs in OBSERVABILITY_TYPES {
        assert!(
            !GRAPH_MUTATING_TYPES.contains(obs),
            "{obs} appears in both observability and graph-mutating lists"
        );
    }
}

#[test]
fn all_event_types_are_classified() {
    // Build every event variant and check its type is in one of the two lists
    let all_events = build_all_events();
    for event in &all_events {
        let et = event.event_type();
        let classified = GRAPH_MUTATING_TYPES.contains(&et) || OBSERVABILITY_TYPES.contains(&et);
        assert!(
            classified,
            "Event type '{}' is not classified as graph-mutating or observability",
            et
        );
    }
}

#[test]
fn no_duplicate_event_types() {
    let all_events = build_all_events();
    let mut seen = std::collections::HashSet::new();
    for event in &all_events {
        let et = event.event_type();
        assert!(seen.insert(et), "Duplicate event_type: {et}");
    }
}

// =========================================================================
// Serde tag ↔ event_type() consistency
// =========================================================================

#[test]
fn event_type_method_matches_serde_tag_for_all_variants() {
    let all_events = build_all_events();
    for event in &all_events {
        let method_type = event.event_type();
        let payload = event.to_payload();
        let serde_type = payload["type"].as_str().unwrap_or("<missing>");
        assert_eq!(
            method_type, serde_type,
            "event_type() = '{}' but serde tag = '{}' for {:?}",
            method_type, serde_type, std::mem::discriminant(event)
        );
    }
}

// =========================================================================
// Round-trip serialization
// =========================================================================

#[test]
fn every_event_variant_roundtrips_through_json() {
    let all_events = build_all_events();
    for event in &all_events {
        let payload = event.to_payload();
        let roundtripped = Event::from_payload(&payload).unwrap_or_else(|e| {
            panic!(
                "Failed to deserialize event_type='{}': {e}\nPayload: {}",
                event.event_type(),
                serde_json::to_string_pretty(&payload).unwrap()
            )
        });
        assert_eq!(
            event.event_type(),
            roundtripped.event_type(),
            "Round-trip changed event type"
        );
    }
}

// =========================================================================
// Schema evolution — old payloads missing new fields still deserialize
// =========================================================================

#[test]
fn signal_discovered_missing_optional_fields_deserializes() {
    // Simulate an old-format payload that doesn't have type-specific fields
    let old_payload = json!({
        "type": "signal_discovered",
        "signal_id": "550e8400-e29b-41d4-a716-446655440000",
        "node_type": "gathering",
        "title": "Old Signal",
        "summary": "From before we added organizer field",
        "sensitivity": "general",
        "confidence": 0.8,
        "source_url": "https://example.com",
        "extracted_at": "2026-01-01T00:00:00Z",
        "implied_queries": [],
        "mentioned_actors": []
        // NOTE: no starts_at, ends_at, organizer, etc.
    });

    let event = Event::from_payload(&old_payload).unwrap();
    match event {
        Event::SignalDiscovered { organizer, starts_at, is_recurring, .. } => {
            assert!(organizer.is_none(), "Missing optional fields should be None");
            assert!(starts_at.is_none());
            assert!(is_recurring.is_none());
        }
        _ => panic!("Expected SignalDiscovered"),
    }
}

#[test]
fn signal_corroborated_missing_summary_deserializes() {
    let old_payload = json!({
        "type": "signal_corroborated",
        "signal_id": "550e8400-e29b-41d4-a716-446655440000",
        "node_type": "gathering",
        "new_source_url": "https://example.com/source2",
        "similarity": 0.92,
        "new_corroboration_count": 3
        // NOTE: no summary field
    });

    let event = Event::from_payload(&old_payload).unwrap();
    match event {
        Event::SignalCorroborated { summary, .. } => {
            assert!(summary.is_none());
        }
        _ => panic!("Expected SignalCorroborated"),
    }
}

#[test]
fn extra_fields_in_payload_are_ignored() {
    // Forward compatibility: new fields added by a newer producer should be ignored
    let future_payload = json!({
        "type": "url_scraped",
        "url": "https://example.com",
        "strategy": "web_page",
        "success": true,
        "content_bytes": 1024,
        "new_future_field": "this doesn't exist yet",
        "another_future_thing": 42
    });

    let event = Event::from_payload(&future_payload).unwrap();
    assert_eq!(event.event_type(), "url_scraped");
}

// =========================================================================
// Event payloads carry facts, not derived values
// =========================================================================

#[test]
fn signal_discovered_has_no_embedding_field() {
    let event = Event::SignalDiscovered {
        signal_id: Uuid::new_v4(),
        node_type: NodeType::Gathering,
        title: "Test".into(),
        summary: "Test".into(),
        sensitivity: SensitivityLevel::General,
        confidence: 0.8,
        source_url: "https://example.com".into(),
        extracted_at: Utc::now(),
        content_date: None,
        about_location: None,
        about_location_name: None,
        from_location: None,
        implied_queries: vec![],
        mentioned_actors: vec![],
        author_actor: None,
        starts_at: None,
        ends_at: None,
        action_url: None,
        organizer: None,
        is_recurring: None,
        availability: None,
        is_ongoing: None,
        urgency: None,
        what_needed: None,
        goal: None,
        severity: None,
        category: None,
        effective_date: None,
        source_authority: None,
        what_would_help: None,
    };

    let payload = event.to_payload();

    // These derived values must NOT be in the event payload
    assert!(payload.get("embedding").is_none(), "embedding is a computed artifact, not a fact");
    assert!(payload.get("source_diversity").is_none(), "source_diversity is derived");
    assert!(payload.get("channel_diversity").is_none(), "channel_diversity is derived");
    assert!(payload.get("cause_heat").is_none(), "cause_heat is derived");
    assert!(payload.get("freshness_score").is_none(), "freshness_score is derived");
    assert!(payload.get("external_ratio").is_none(), "external_ratio is derived");
}

#[test]
fn signal_corroborated_has_no_derived_counts() {
    let event = Event::SignalCorroborated {
        signal_id: Uuid::new_v4(),
        node_type: NodeType::Gathering,
        new_source_url: "https://source2.com".into(),
        similarity: 0.92,
        new_corroboration_count: 3,
        summary: None,
    };

    let payload = event.to_payload();

    // new_corroboration_count IS a fact (absolute value computed by producer before append)
    assert!(payload.get("new_corroboration_count").is_some());

    // But diversity counts are NOT — they're enrichment
    assert!(payload.get("source_diversity").is_none());
    assert!(payload.get("channel_diversity").is_none());
}

// =========================================================================
// Usage pattern: scout pipeline emitting events
// =========================================================================

#[test]
fn scout_pipeline_event_chain_is_expressible() {
    // Verify the API supports the intended usage pattern:
    // scrape → extraction → signal_discovered + citation_recorded

    let scrape = Event::UrlScraped {
        url: "https://lakestreetstories.com/events".into(),
        strategy: "web_page".into(),
        success: true,
        content_bytes: 45_000,
    };

    let extraction = Event::LlmExtractionCompleted {
        source_url: "https://lakestreetstories.com/events".into(),
        content_chars: 12_000,
        signals_extracted: 2,
        implied_queries: 1,
    };

    let signal = Event::SignalDiscovered {
        signal_id: Uuid::new_v4(),
        node_type: NodeType::Gathering,
        title: "Lake Street Block Party".into(),
        summary: "Annual community gathering on Lake Street".into(),
        sensitivity: SensitivityLevel::General,
        confidence: 0.88,
        source_url: "https://lakestreetstories.com/events".into(),
        extracted_at: Utc::now(),
        content_date: Some(Utc::now()),
        about_location: Some(GeoPoint {
            lat: 44.9488,
            lng: -93.2683,
            precision: GeoPrecision::Neighborhood,
        }),
        about_location_name: Some("Lake Street, Minneapolis".into()),
        from_location: None,
        implied_queries: vec!["lake street events minneapolis".into()],
        mentioned_actors: vec!["Lake Street Council".into()],
        author_actor: None,
        starts_at: Some(Utc::now()),
        ends_at: None,
        action_url: Some("https://lakestreetstories.com/events/block-party".into()),
        organizer: Some("Lake Street Council".into()),
        is_recurring: Some(true),
        availability: None,
        is_ongoing: None,
        urgency: None,
        what_needed: None,
        goal: None,
        severity: None,
        category: None,
        effective_date: None,
        source_authority: None,
        what_would_help: None,
    };

    let citation = Event::CitationRecorded {
        citation_id: Uuid::new_v4(),
        signal_id: Uuid::new_v4(),
        url: "https://lakestreetstories.com/events".into(),
        content_hash: "abc123".into(),
        snippet: Some("Join us for the annual block party...".into()),
        relevance: Some("primary".into()),
        channel_type: Some(ChannelType::CommunityMedia),
        evidence_confidence: Some(0.95),
    };

    // All serialize cleanly
    for event in [&scrape, &extraction, &signal, &citation] {
        let payload = event.to_payload();
        assert!(payload.is_object());
        assert!(payload["type"].is_string());
    }
}

// =========================================================================
// Usage pattern: admin/supervisor operations
// =========================================================================

#[test]
fn review_verdict_event_captures_status_change() {
    let event = Event::ReviewVerdictReached {
        signal_id: Uuid::new_v4(),
        old_status: "staged".into(),
        new_status: "live".into(),
        reason: "Verified against source content".into(),
    };

    let payload = event.to_payload();
    assert_eq!(payload["old_status"], "staged");
    assert_eq!(payload["new_status"], "live");
}

#[test]
fn bulk_operation_decomposes_into_individual_events() {
    // reap_expired produces one signal_expired event per signal
    let expired_signals: Vec<Event> = (0..5)
        .map(|_| Event::SignalExpired {
            signal_id: Uuid::new_v4(),
            node_type: NodeType::Gathering,
            reason: "gathering_past_end_date".into(),
        })
        .collect();

    assert_eq!(expired_signals.len(), 5);
    assert!(expired_signals.iter().all(|e| e.event_type() == "signal_expired"));
}

// =========================================================================
// Usage pattern: reducer determines action from event type
// =========================================================================

#[test]
fn reducer_can_match_on_deserialized_event() {
    let payload = json!({
        "type": "signal_discovered",
        "signal_id": "550e8400-e29b-41d4-a716-446655440000",
        "node_type": "gathering",
        "title": "Test",
        "summary": "Test",
        "sensitivity": "general",
        "confidence": 0.8,
        "source_url": "https://example.com",
        "extracted_at": "2026-01-01T00:00:00Z",
        "implied_queries": [],
        "mentioned_actors": [],
    });

    let event = Event::from_payload(&payload).unwrap();

    // The reducer uses pattern matching — this must be ergonomic
    let should_write = match &event {
        Event::SignalDiscovered { signal_id: _, node_type: _, title, confidence, .. } => {
            assert!(!title.is_empty());
            assert!(*confidence > 0.0);
            true // → MERGE node
        }
        Event::UrlScraped { .. } => false, // → no-op
        _ => false,
    };

    assert!(should_write);
}

#[test]
fn reducer_skips_observability_events() {
    let observability_events = vec![
        Event::UrlScraped { url: "x".into(), strategy: "y".into(), success: true, content_bytes: 0 },
        Event::FeedScraped { url: "x".into(), items: 5 },
        Event::BudgetCheckpoint { spent_cents: 100, remaining_cents: 900 },
        Event::LlmExtractionCompleted { source_url: "x".into(), content_chars: 0, signals_extracted: 0, implied_queries: 0 },
    ];

    for event in &observability_events {
        let produces_graph_change = !matches!(
            event,
            Event::UrlScraped { .. }
                | Event::FeedScraped { .. }
                | Event::SocialScraped { .. }
                | Event::SocialTopicsSearched { .. }
                | Event::SearchPerformed { .. }
                | Event::LlmExtractionCompleted { .. }
                | Event::BudgetCheckpoint { .. }
                | Event::BootstrapCompleted { .. }
                | Event::AgentWebSearched { .. }
                | Event::AgentPageRead { .. }
                | Event::AgentFutureQuery { .. }
                | Event::LintBatchCompleted { .. }
                | Event::SignalDroppedNoDate { .. }
        );
        assert!(
            !produces_graph_change,
            "Event '{}' should be a reducer no-op",
            event.event_type()
        );
    }
}

// =========================================================================
// Helper: build one instance of every Event variant
// =========================================================================

fn build_all_events() -> Vec<Event> {
    let id = Uuid::new_v4();
    let now = Utc::now();

    vec![
        // Observability
        Event::UrlScraped { url: "x".into(), strategy: "y".into(), success: true, content_bytes: 0 },
        Event::FeedScraped { url: "x".into(), items: 0 },
        Event::SocialScraped { platform: "x".into(), identifier: "y".into(), post_count: 0 },
        Event::SocialTopicsSearched { platform: "x".into(), topics: vec![], posts_found: 0 },
        Event::SearchPerformed { query: "x".into(), provider: "y".into(), result_count: 0, canonical_key: "z".into() },
        Event::LlmExtractionCompleted { source_url: "x".into(), content_chars: 0, signals_extracted: 0, implied_queries: 0 },
        Event::BudgetCheckpoint { spent_cents: 0, remaining_cents: 0 },
        Event::BootstrapCompleted { sources_created: 0 },
        Event::AgentWebSearched { provider: "x".into(), query: "y".into(), result_count: 0, title: "z".into() },
        Event::AgentPageRead { provider: "x".into(), url: "y".into(), content_chars: 0, title: "z".into() },
        Event::AgentFutureQuery { provider: "x".into(), query: "y".into(), title: "z".into() },
        // Signals
        Event::SignalDiscovered {
            signal_id: id, node_type: NodeType::Gathering, title: "x".into(), summary: "y".into(),
            sensitivity: SensitivityLevel::General, confidence: 0.0, source_url: "z".into(),
            extracted_at: now, content_date: None, about_location: None, about_location_name: None,
            from_location: None, implied_queries: vec![], mentioned_actors: vec![],
            author_actor: None, starts_at: None, ends_at: None, action_url: None,
            organizer: None, is_recurring: None, availability: None, is_ongoing: None,
            urgency: None, what_needed: None, goal: None, severity: None, category: None,
            effective_date: None, source_authority: None, what_would_help: None,
        },
        Event::SignalCorroborated { signal_id: id, node_type: NodeType::Aid, new_source_url: "x".into(), similarity: 0.9, new_corroboration_count: 2, summary: None },
        Event::SignalRefreshed { signal_ids: vec![id], node_type: NodeType::Need, new_last_confirmed_active: now },
        Event::SignalConfidenceScored { signal_id: id, old_confidence: 0.5, new_confidence: 0.8 },
        Event::SignalFieldsCorrected { signal_id: id, corrections: vec![FieldCorrection { field: "title".into(), old_value: json!("old"), new_value: json!("new"), reason: "typo".into() }] },
        Event::SignalRejected { signal_id: Some(id), title: "x".into(), source_url: "y".into(), reason: "z".into() },
        Event::SignalExpired { signal_id: id, node_type: NodeType::Gathering, reason: "past_end_date".into() },
        Event::SignalPurged { signal_id: id, node_type: NodeType::Tension, reason: "admin".into(), context: None },
        Event::SignalDeduplicated { signal_type: NodeType::Gathering, title: "x".into(), matched_id: id, similarity: 0.95, action: "merge".into(), source_url: "y".into(), summary: None },
        Event::SignalDroppedNoDate { title: "x".into(), source_url: "y".into() },
        Event::ReviewVerdictReached { signal_id: id, old_status: "staged".into(), new_status: "live".into(), reason: "ok".into() },
        Event::ImpliedQueriesConsumed { signal_ids: vec![id] },
        // Citations
        Event::CitationRecorded { citation_id: id, signal_id: id, url: "x".into(), content_hash: "y".into(), snippet: None, relevance: None, channel_type: None, evidence_confidence: None },
        Event::OrphanedCitationsCleaned { citation_ids: vec![id] },
        // Sources
        Event::SourceRegistered { source_id: id, canonical_key: "x".into(), canonical_value: "y".into(), url: None, discovery_method: DiscoveryMethod::Curated, weight: 0.5, source_role: SourceRole::Mixed, gap_context: None },
        Event::SourceUpdated { source_id: id, canonical_key: "x".into(), changes: json!({"weight": 0.8}) },
        Event::SourceDeactivated { source_ids: vec![id], reason: "empty".into() },
        Event::SourceRemoved { source_id: id, canonical_key: "x".into() },
        Event::SourceScrapeRecorded { canonical_key: "x".into(), signals_produced: 0, scrape_count: 1, consecutive_empty_runs: 0 },
        Event::SourceLinkDiscovered { child_id: id, parent_canonical_key: "x".into() },
        Event::ExpansionQueryCollected { query: "x".into(), source_url: "y".into() },
        Event::ExpansionSourceCreated { canonical_key: "x".into(), query: "y".into(), source_url: "z".into() },
        // Actors
        Event::ActorIdentified { actor_id: id, name: "x".into(), actor_type: ActorType::Organization, entity_id: "y".into(), domains: vec![], social_urls: vec![], description: "z".into() },
        Event::ActorLinkedToSignal { actor_id: id, signal_id: id, role: "organizer".into() },
        Event::ActorLinkedToSource { actor_id: id, source_id: id },
        Event::ActorStatsUpdated { actor_id: id, signal_count: 5, last_active: now },
        Event::ActorLocationIdentified { actor_id: id, location_lat: 44.9, location_lng: -93.2, location_name: Some("Minneapolis".into()) },
        Event::DuplicateActorsMerged { kept_id: id, merged_ids: vec![Uuid::new_v4()] },
        Event::OrphanedActorsCleaned { actor_ids: vec![id] },
        // Relationships
        Event::RelationshipEstablished { from_id: id, to_id: Uuid::new_v4(), relationship_type: "responds_to".into(), properties: Some(json!({"match_strength": 0.8})) },
        // Situations
        Event::SituationIdentified { situation_id: id, headline: "x".into(), lede: "y".into(), arc: SituationArc::Emerging, temperature: 0.5, centroid_lat: None, centroid_lng: None, location_name: None, sensitivity: SensitivityLevel::General, category: None, structured_state: "{}".into() },
        Event::SituationEvolved { situation_id: id, changes: json!({"temperature": 0.7}) },
        Event::SituationPromoted { situation_ids: vec![id] },
        Event::DispatchCreated { dispatch_id: id, situation_id: id, body: "x".into(), signal_ids: vec![], dispatch_type: DispatchType::Emergence, supersedes: None, fidelity_score: Some(0.9) },
        // Tags
        Event::TagsAggregated { situation_id: id, tags: vec![TagFact { slug: "housing".into(), name: "Housing".into(), weight: 0.8 }] },
        Event::TagSuppressed { situation_id: id, tag_slug: "generic".into() },
        Event::TagsMerged { source_slug: "old".into(), target_slug: "new".into() },
        // Quality
        Event::LintBatchCompleted { source_url: "x".into(), signal_count: 5, passed: 3, corrected: 1, rejected: 1 },
        Event::LintCorrectionApplied { node_id: id, signal_type: NodeType::Gathering, title: "x".into(), field: "title".into(), old_value: "old".into(), new_value: "new".into(), reason: "typo".into() },
        Event::LintRejectionIssued { node_id: id, signal_type: NodeType::Aid, title: "x".into(), reason: "spam".into() },
        Event::EmptySignalsCleaned { signal_ids: vec![id] },
        Event::FakeCoordinatesNulled { signal_ids: vec![id], old_coords: vec![(0.0, 0.0)] },
        // Schedule
        Event::ScheduleRecorded { signal_id: id, rrule: "FREQ=WEEKLY;BYDAY=SA".into(), dtstart: now, label: Some("Saturday cleanup".into()) },
        // Pins / demand / submissions
        Event::PinCreated { pin_id: id, location_lat: 44.9, location_lng: -93.2, source_id: id, created_by: "scout".into() },
        Event::PinsRemoved { pin_ids: vec![id] },
        Event::DemandSignalReceived { demand_id: id, query: "food shelf".into(), center_lat: 44.9, center_lng: -93.2, radius_km: 10.0 },
        Event::DemandAggregated { created_task_ids: vec![id], consumed_demand_ids: vec![id] },
        Event::SubmissionReceived { submission_id: id, url: "https://example.com".into(), reason: Some("good source".into()), source_canonical_key: Some("example.com".into()) },
    ]
}
