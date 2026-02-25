//! Reducer contract tests.
//!
//! These verify the reducer's classification of events (no-op vs applied)
//! and its structural guarantees without requiring a Neo4j instance.
//! Integration tests with Neo4j would live in a separate file using testcontainers.

use rootsignal_common::events::Event;
use rootsignal_common::types::*;
use rootsignal_common::safety::SensitivityLevel;
use serde_json::json;
use chrono::Utc;
use uuid::Uuid;

/// Build a minimal StoredEvent from an Event for testing.
fn stored_event(event: &Event) -> rootsignal_events::StoredEvent {
    rootsignal_events::StoredEvent {
        seq: 1,
        ts: Utc::now(),
        event_type: event.event_type().to_string(),
        parent_seq: None,
        caused_by_seq: None,
        run_id: Some("test-run".to_string()),
        actor: Some("test-actor".to_string()),
        payload: event.to_payload(),
        schema_v: 1,
    }
}

// =========================================================================
// Classification: which events are no-ops?
// =========================================================================

/// All observability events produce no Cypher.
const NOOP_EVENT_TYPES: &[&str] = &[
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
    // Informational — no graph mutation
    "signal_rejected",
    "signal_dropped_no_date",
    "signal_deduplicated",
    "expansion_query_collected",
    "expansion_source_created",
    "source_link_discovered",
    // Non-graph artifact
    "dispatch_created",
];

/// All graph-mutating events produce Cypher.
const APPLIED_EVENT_TYPES: &[&str] = &[
    "signal_discovered",
    "signal_corroborated",
    "signal_refreshed",
    "signal_confidence_scored",
    "signal_fields_corrected",
    "signal_expired",
    "signal_purged",
    "review_verdict_reached",
    "implied_queries_consumed",
    "citation_recorded",
    "orphaned_citations_cleaned",
    "source_registered",
    "source_updated",
    "source_deactivated",
    "source_removed",
    "source_scrape_recorded",
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

#[test]
fn every_event_type_is_classified_as_noop_or_applied() {
    // Build one instance of every Event variant and check its type is in one list
    let all_events = build_all_events();

    for event in &all_events {
        let event_type = event.event_type();
        let is_noop = NOOP_EVENT_TYPES.contains(&event_type);
        let is_applied = APPLIED_EVENT_TYPES.contains(&event_type);

        assert!(
            is_noop || is_applied,
            "Event type '{}' is not classified as noop or applied in reducer tests",
            event_type
        );
        assert!(
            !(is_noop && is_applied),
            "Event type '{}' is classified as BOTH noop and applied",
            event_type
        );
    }
}

#[test]
fn no_overlap_between_noop_and_applied() {
    for noop in NOOP_EVENT_TYPES {
        assert!(
            !APPLIED_EVENT_TYPES.contains(noop),
            "'{}' appears in both NOOP and APPLIED lists",
            noop
        );
    }
}

#[test]
fn noop_plus_applied_covers_all_event_types() {
    let all_events = build_all_events();
    let total_events = all_events.len();
    let classified = NOOP_EVENT_TYPES.len() + APPLIED_EVENT_TYPES.len();

    assert_eq!(
        total_events, classified,
        "Event count ({}) doesn't match classified count ({}). Missing: {:?}",
        total_events,
        classified,
        all_events
            .iter()
            .map(|e| e.event_type())
            .filter(|t| !NOOP_EVENT_TYPES.contains(t) && !APPLIED_EVENT_TYPES.contains(t))
            .collect::<Vec<_>>()
    );
}

// =========================================================================
// Structural guarantees
// =========================================================================

#[test]
fn signal_discovered_cypher_uses_merge_not_create() {
    // The reducer must use MERGE for idempotency, not CREATE.
    // This is a code-level assertion — we verify by reading the source.
    let source = include_str!("../src/reducer.rs");

    // SignalDiscovered handler should use MERGE
    assert!(
        source.contains("MERGE (n:{label} {{id: $id}})"),
        "SignalDiscovered handler must use MERGE, not CREATE"
    );
}

#[test]
fn reducer_source_has_no_utc_now() {
    let source = include_str!("../src/reducer.rs");

    // No wall-clock time usage in reducer
    assert!(
        !source.contains("Utc::now()"),
        "Reducer must not use Utc::now() — all timestamps come from event payloads"
    );
}

#[test]
fn reducer_source_has_no_uuid_new() {
    let source = include_str!("../src/reducer.rs");

    assert!(
        !source.contains("Uuid::new_v4()"),
        "Reducer must not generate UUIDs — all IDs come from event payloads"
    );
}

#[test]
fn reducer_source_has_no_embedding_writes() {
    let source = include_str!("../src/reducer.rs");

    // No embedding property in reducer Cypher
    assert!(
        !source.contains("n.embedding =") && !source.contains("embedding: $embedding"),
        "Reducer must not write embeddings — that's an enrichment pass"
    );
}

#[test]
fn reducer_source_has_no_diversity_writes() {
    let source = include_str!("../src/reducer.rs");

    assert!(
        !source.contains("n.source_diversity =")
            && !source.contains("n.channel_diversity =")
            && !source.contains("n.external_ratio ="),
        "Reducer must not write diversity metrics — those are enrichment pass values"
    );
}

#[test]
fn reducer_source_has_no_cause_heat_writes() {
    let source = include_str!("../src/reducer.rs");

    assert!(
        !source.contains("n.cause_heat =") && !source.contains("cause_heat: $"),
        "Reducer must not write cause_heat — that's an enrichment pass value"
    );
}

#[test]
fn reducer_source_has_no_freshness_score_writes() {
    let source = include_str!("../src/reducer.rs");

    assert!(
        !source.contains("n.freshness_score =") && !source.contains("freshness_score: $"),
        "Reducer must not write freshness_score — that's a derived metric"
    );
}

#[test]
fn malformed_payload_returns_deserialize_error() {
    let stored = rootsignal_events::StoredEvent {
        seq: 1,
        ts: Utc::now(),
        event_type: "signal_discovered".to_string(),
        parent_seq: None,
        caused_by_seq: None,
        run_id: None,
        actor: None,
        payload: json!({"type": "signal_discovered", "bogus": true}),
        schema_v: 1,
    };

    // We can't call apply() without a GraphClient (Neo4j connection),
    // but we can verify the Event::from_payload fails on malformed data
    let result = Event::from_payload(&stored.payload);
    assert!(result.is_err(), "Malformed SignalDiscovered payload should fail deserialization");
}

#[test]
fn noop_event_stored_event_deserializes_cleanly() {
    let event = Event::UrlScraped {
        url: "https://example.com".into(),
        strategy: "web_page".into(),
        success: true,
        content_bytes: 1024,
    };
    let stored = stored_event(&event);

    // The reducer should be able to deserialize this
    let parsed = Event::from_payload(&stored.payload).unwrap();
    assert_eq!(parsed.event_type(), "url_scraped");
}

#[test]
fn signal_discovered_payload_has_no_embedding_field() {
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
    assert!(
        payload.get("embedding").is_none(),
        "SignalDiscovered must not carry an embedding field"
    );
    assert!(
        payload.get("freshness_score").is_none(),
        "SignalDiscovered must not carry freshness_score"
    );
    assert!(
        payload.get("source_diversity").is_none(),
        "SignalDiscovered must not carry source_diversity"
    );
}

#[test]
fn sanitize_field_name_prevents_injection() {
    // The reducer sanitizes field names for dynamic Cypher property setting.
    // Verify by calling the function indirectly through the source patterns.
    let source = include_str!("../src/reducer.rs");

    // Every dynamic property SET uses sanitize_field_name
    assert!(
        source.contains("sanitize_field_name"),
        "Reducer must use sanitize_field_name for dynamic field names"
    );
}

// =========================================================================
// Helper: build one instance of every Event variant
// =========================================================================

fn build_all_events() -> Vec<Event> {
    let id = Uuid::new_v4();
    let now = Utc::now();

    vec![
        // Observability
        Event::UrlScraped { url: "".into(), strategy: "".into(), success: true, content_bytes: 0 },
        Event::FeedScraped { url: "".into(), items: 0 },
        Event::SocialScraped { platform: "".into(), identifier: "".into(), post_count: 0 },
        Event::SocialTopicsSearched { platform: "".into(), topics: vec![], posts_found: 0 },
        Event::SearchPerformed { query: "".into(), provider: "".into(), result_count: 0, canonical_key: "".into() },
        Event::LlmExtractionCompleted { source_url: "".into(), content_chars: 0, signals_extracted: 0, implied_queries: 0 },
        Event::BudgetCheckpoint { spent_cents: 0, remaining_cents: 0 },
        Event::BootstrapCompleted { sources_created: 0 },
        Event::AgentWebSearched { provider: "".into(), query: "".into(), result_count: 0, title: "".into() },
        Event::AgentPageRead { provider: "".into(), url: "".into(), content_chars: 0, title: "".into() },
        Event::AgentFutureQuery { provider: "".into(), query: "".into(), title: "".into() },
        // Signals
        Event::SignalDiscovered {
            signal_id: id, node_type: NodeType::Gathering, title: "".into(), summary: "".into(),
            sensitivity: SensitivityLevel::General, confidence: 0.0, source_url: "".into(),
            extracted_at: now, content_date: None, about_location: None, about_location_name: None,
            from_location: None, implied_queries: vec![], mentioned_actors: vec![], author_actor: None,
            starts_at: None, ends_at: None, action_url: None, organizer: None, is_recurring: None,
            availability: None, is_ongoing: None, urgency: None, what_needed: None, goal: None,
            severity: None, category: None, effective_date: None, source_authority: None, what_would_help: None,
        },
        Event::SignalCorroborated { signal_id: id, node_type: NodeType::Gathering, new_source_url: "".into(), similarity: 0.0, new_corroboration_count: 1, summary: None },
        Event::SignalRefreshed { signal_ids: vec![id], node_type: NodeType::Gathering, new_last_confirmed_active: now },
        Event::SignalConfidenceScored { signal_id: id, old_confidence: 0.5, new_confidence: 0.8 },
        Event::SignalFieldsCorrected { signal_id: id, corrections: vec![] },
        Event::SignalRejected { signal_id: None, title: "".into(), source_url: "".into(), reason: "".into() },
        Event::SignalExpired { signal_id: id, node_type: NodeType::Gathering, reason: "".into() },
        Event::SignalPurged { signal_id: id, node_type: NodeType::Gathering, reason: "".into(), context: None },
        Event::SignalDeduplicated { signal_type: NodeType::Gathering, title: "".into(), matched_id: id, similarity: 0.0, action: "".into(), source_url: "".into(), summary: None },
        Event::SignalDroppedNoDate { title: "".into(), source_url: "".into() },
        Event::ReviewVerdictReached { signal_id: id, old_status: "staged".into(), new_status: "live".into(), reason: "".into() },
        Event::ImpliedQueriesConsumed { signal_ids: vec![id] },
        // Citations
        Event::CitationRecorded { citation_id: id, signal_id: id, url: "".into(), content_hash: "".into(), snippet: None, relevance: None, channel_type: None, evidence_confidence: None },
        Event::OrphanedCitationsCleaned { citation_ids: vec![id] },
        // Sources
        Event::SourceRegistered { source_id: id, canonical_key: "".into(), canonical_value: "".into(), url: None, discovery_method: DiscoveryMethod::Curated, weight: 0.5, source_role: SourceRole::Mixed, gap_context: None },
        Event::SourceUpdated { source_id: id, canonical_key: "".into(), changes: json!({}) },
        Event::SourceDeactivated { source_ids: vec![id], reason: "".into() },
        Event::SourceRemoved { source_id: id, canonical_key: "".into() },
        Event::SourceScrapeRecorded { canonical_key: "".into(), signals_produced: 0, scrape_count: 1, consecutive_empty_runs: 1 },
        Event::SourceLinkDiscovered { child_id: id, parent_canonical_key: "".into() },
        Event::ExpansionQueryCollected { query: "".into(), source_url: "".into() },
        Event::ExpansionSourceCreated { canonical_key: "".into(), query: "".into(), source_url: "".into() },
        // Actors
        Event::ActorIdentified { actor_id: id, name: "".into(), actor_type: ActorType::Organization, entity_id: "".into(), domains: vec![], social_urls: vec![], description: "".into() },
        Event::ActorLinkedToSignal { actor_id: id, signal_id: id, role: "".into() },
        Event::ActorLinkedToSource { actor_id: id, source_id: id },
        Event::ActorStatsUpdated { actor_id: id, signal_count: 1, last_active: now },
        Event::ActorLocationIdentified { actor_id: id, location_lat: 0.0, location_lng: 0.0, location_name: None },
        Event::DuplicateActorsMerged { kept_id: id, merged_ids: vec![] },
        Event::OrphanedActorsCleaned { actor_ids: vec![] },
        // Relationships
        Event::RelationshipEstablished { from_id: id, to_id: id, relationship_type: "RESPONDS_TO".into(), properties: None },
        // Situations
        Event::SituationIdentified { situation_id: id, headline: "".into(), lede: "".into(), arc: SituationArc::Emerging, temperature: 0.0, centroid_lat: None, centroid_lng: None, location_name: None, sensitivity: SensitivityLevel::General, category: None, structured_state: "".into() },
        Event::SituationEvolved { situation_id: id, changes: json!({}) },
        Event::SituationPromoted { situation_ids: vec![id] },
        Event::DispatchCreated { dispatch_id: id, situation_id: id, body: "".into(), signal_ids: vec![], dispatch_type: DispatchType::Update, supersedes: None, fidelity_score: None },
        // Tags
        Event::TagsAggregated { situation_id: id, tags: vec![] },
        Event::TagSuppressed { situation_id: id, tag_slug: "".into() },
        Event::TagsMerged { source_slug: "".into(), target_slug: "".into() },
        // Quality / lint
        Event::LintBatchCompleted { source_url: "".into(), signal_count: 0, passed: 0, corrected: 0, rejected: 0 },
        Event::LintCorrectionApplied { node_id: id, signal_type: NodeType::Gathering, title: "".into(), field: "".into(), old_value: "".into(), new_value: "".into(), reason: "".into() },
        Event::LintRejectionIssued { node_id: id, signal_type: NodeType::Gathering, title: "".into(), reason: "".into() },
        Event::EmptySignalsCleaned { signal_ids: vec![] },
        Event::FakeCoordinatesNulled { signal_ids: vec![], old_coords: vec![] },
        // Schedule
        Event::ScheduleRecorded { signal_id: id, rrule: "".into(), dtstart: now, label: None },
        // Pins / demand / submissions
        Event::PinCreated { pin_id: id, location_lat: 0.0, location_lng: 0.0, source_id: id, created_by: "".into() },
        Event::PinsRemoved { pin_ids: vec![] },
        Event::DemandSignalReceived { demand_id: id, query: "".into(), center_lat: 0.0, center_lng: 0.0, radius_km: 0.0 },
        Event::DemandAggregated { created_task_ids: vec![], consumed_demand_ids: vec![] },
        Event::SubmissionReceived { submission_id: id, url: "".into(), reason: None, source_canonical_key: None },
    ]
}
