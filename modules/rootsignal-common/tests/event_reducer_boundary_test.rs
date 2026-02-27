//! Reducer boundary tests.
//!
//! These verify the contract between the Event enum and the reducer pattern:
//! - Every event has a deterministic event_type string
//! - Events cleanly serialize to JSONB and back
//! - Observability events are distinguishable from graph-mutating events
//! - The event_type tag matches the serde tag exactly
//! - Schema evolution works (old payloads missing new fields still deserialize)

use chrono::Utc;
use rootsignal_common::events::{
    Event, WorldEvent, SystemDecision, TelemetryEvent,
    GatheringCorrection, SituationChange, SystemSourceChange,
};
use rootsignal_common::types::*;
use rootsignal_common::safety::SensitivityLevel;
use serde_json::json;
use uuid::Uuid;

// =========================================================================
// Reducer classification — which events produce graph changes?
// =========================================================================

/// Events that the reducer should act on (produce graph mutations).
const GRAPH_MUTATING_TYPES: &[&str] = &[
    // World: Discovery (5 typed)
    "gathering_discovered",
    "aid_discovered",
    "need_discovered",
    "notice_discovered",
    "tension_discovered",
    // World: Corroboration fact
    "observation_corroborated",
    // World: Citations
    "citation_recorded",
    // World: Sources
    "source_registered",
    "source_changed",
    "source_deactivated",
    // World: Actors
    "actor_identified",
    "actor_linked_to_entity",
    "actor_linked_to_source",
    "actor_location_identified",
    // World: Community input
    "pin_created",
    "demand_received",
    "submission_received",
    // World: Relationship edges
    "resource_edge_created",
    "response_linked",
    "gravity_linked",
    // System: Observation lifecycle
    "freshness_confirmed",
    "confidence_scored",
    "corroboration_scored",
    "entity_expired",
    "entity_purged",
    "review_verdict_reached",
    "implied_queries_consumed",
    // System: Classifications
    "sensitivity_classified",
    "implied_queries_extracted",
    // System: Corrections (5 typed)
    "gathering_corrected",
    "aid_corrected",
    "need_corrected",
    "notice_corrected",
    "tension_corrected",
    // System: Actors
    "duplicate_actors_merged",
    "orphaned_actors_cleaned",
    // System: Situations
    "situation_identified",
    "situation_changed",
    "situation_promoted",
    // System: Tags
    "tag_suppressed",
    "tags_merged",
    // System: Quality / lint
    "empty_entities_cleaned",
    "fake_coordinates_nulled",
    "orphaned_citations_cleaned",
    // System: Source editorial
    "source_system_changed",
];

/// Events that the reducer should ignore (telemetry / informational / no-ops).
const OBSERVABILITY_TYPES: &[&str] = &[
    // Telemetry
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
    "pins_removed",
    "demand_aggregated",
    // World informational — no graph mutation
    "expansion_query_collected",
    "source_link_discovered",
    // System informational — no graph mutation
    "observation_rejected",
    "extraction_dropped_no_date",
    "duplicate_detected",
    "dispatch_created",
];

#[test]
fn observability_events_are_distinct_from_graph_events() {
    for obs in OBSERVABILITY_TYPES {
        assert!(
            !GRAPH_MUTATING_TYPES.contains(obs),
            "{obs} appears in both observability and graph-mutating lists"
        );
    }
}

#[test]
fn all_event_types_are_classified() {
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
            "event_type() = '{}' but serde tag = '{}' for event '{}'",
            method_type, serde_type, method_type
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
fn gathering_discovered_missing_optional_fields_deserializes() {
    let old_payload = json!({
        "type": "gathering_discovered",
        "id": "550e8400-e29b-41d4-a716-446655440000",
        "title": "Old Gathering",
        "summary": "From before we added organizer field",
        "confidence": 0.8,
        "source_url": "https://example.com",
        "extracted_at": "2026-01-01T00:00:00Z",
        "mentioned_actors": []
        // NOTE: no schedule, action_url, organizer (optional fields)
    });

    let event = Event::from_payload(&old_payload).unwrap();
    match event {
        Event::World(WorldEvent::GatheringDiscovered { organizer, schedule, action_url, .. }) => {
            assert!(organizer.is_none(), "Missing optional fields should be None");
            assert!(schedule.is_none());
            assert!(action_url.is_none());
        }
        _ => panic!("Expected GatheringDiscovered"),
    }
}

#[test]
fn observation_corroborated_missing_summary_deserializes() {
    let old_payload = json!({
        "type": "observation_corroborated",
        "entity_id": "550e8400-e29b-41d4-a716-446655440000",
        "node_type": "gathering",
        "new_source_url": "https://example.com/source2"
        // NOTE: no summary field
    });

    let event = Event::from_payload(&old_payload).unwrap();
    match event {
        Event::World(WorldEvent::ObservationCorroborated { summary, .. }) => {
            assert!(summary.is_none());
        }
        _ => panic!("Expected ObservationCorroborated"),
    }
}

#[test]
fn extra_fields_in_payload_are_ignored() {
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
fn discovery_has_no_enrichment_fields() {
    let event = Event::World(WorldEvent::GatheringDiscovered {
        id: Uuid::new_v4(),
        title: "Test".into(),
        summary: "Test".into(),
        confidence: 0.8,
        source_url: "https://example.com".into(),
        extracted_at: Utc::now(),
        content_date: None,
        location: None,
        from_location: None,
        mentioned_actors: vec![],
        author_actor: None,
        schedule: None,
        action_url: None,
        organizer: None,
    });

    let payload = event.to_payload();
    assert!(payload.get("embedding").is_none(), "embedding is a computed artifact, not a fact");
    assert!(payload.get("source_diversity").is_none(), "source_diversity is derived");
    assert!(payload.get("channel_diversity").is_none(), "channel_diversity is derived");
    assert!(payload.get("cause_heat").is_none(), "cause_heat is derived");
    assert!(payload.get("freshness_score").is_none(), "freshness_score is derived");
    assert!(payload.get("external_ratio").is_none(), "external_ratio is derived");
    assert!(payload.get("sensitivity").is_none(), "sensitivity is a system classification, not a world fact");
    assert!(payload.get("implied_queries").is_none(), "implied_queries is a system artifact, not a world fact");
}

#[test]
fn corroboration_world_fact_has_no_scoring_fields() {
    let event = Event::World(WorldEvent::ObservationCorroborated {
        entity_id: Uuid::new_v4(),
        node_type: NodeType::Gathering,
        new_source_url: "https://source2.com".into(),
        summary: None,
    });

    let payload = event.to_payload();

    // World fact only carries the observation — no scoring
    assert!(payload.get("similarity").is_none(), "similarity is a system score, not a world fact");
    assert!(payload.get("new_corroboration_count").is_none(), "corroboration_count is a system score");

    // Diversity counts are enrichment
    assert!(payload.get("source_diversity").is_none());
    assert!(payload.get("channel_diversity").is_none());
}

// =========================================================================
// Usage pattern: scout pipeline emitting events
// =========================================================================

#[test]
fn scout_pipeline_event_chain_is_expressible() {
    use rootsignal_common::events::{Location, Schedule};

    let scrape = Event::Telemetry(TelemetryEvent::UrlScraped {
        url: "https://lakestreetstories.com/events".into(),
        strategy: "web_page".into(),
        success: true,
        content_bytes: 45_000,
    });

    let extraction = Event::Telemetry(TelemetryEvent::LlmExtractionCompleted {
        source_url: "https://lakestreetstories.com/events".into(),
        content_chars: 12_000,
        entities_extracted: 2,
        implied_queries: 1,
    });

    let gathering = Event::World(WorldEvent::GatheringDiscovered {
        id: Uuid::new_v4(),
        title: "Lake Street Block Party".into(),
        summary: "Annual community gathering on Lake Street".into(),
        confidence: 0.88,
        source_url: "https://lakestreetstories.com/events".into(),
        extracted_at: Utc::now(),
        content_date: Some(Utc::now()),
        location: Some(Location {
            point: Some(GeoPoint { lat: 44.9488, lng: -93.2683, precision: GeoPrecision::Neighborhood }),
            name: Some("Lake Street, Minneapolis".into()),
            address: None,
        }),
        from_location: None,
        mentioned_actors: vec!["Lake Street Council".into()],
        author_actor: None,
        schedule: Some(Schedule {
            starts_at: Some(Utc::now()),
            ends_at: None,
            all_day: false,
            rrule: Some("FREQ=YEARLY".into()),
            timezone: Some("America/Chicago".into()),
        }),
        action_url: Some("https://lakestreetstories.com/events/block-party".into()),
        organizer: Some("Lake Street Council".into()),
    });

    let citation = Event::World(WorldEvent::CitationRecorded {
        citation_id: Uuid::new_v4(),
        entity_id: Uuid::new_v4(),
        url: "https://lakestreetstories.com/events".into(),
        content_hash: "abc123".into(),
        snippet: Some("Join us for the annual block party...".into()),
        relevance: Some("primary".into()),
        channel_type: Some(ChannelType::CommunityMedia),
        evidence_confidence: Some(0.95),
    });

    for event in [&scrape, &extraction, &gathering, &citation] {
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
    let event = Event::System(SystemDecision::ReviewVerdictReached {
        entity_id: Uuid::new_v4(),
        old_status: "staged".into(),
        new_status: "live".into(),
        reason: "Verified against source content".into(),
    });

    let payload = event.to_payload();
    assert_eq!(payload["old_status"], "staged");
    assert_eq!(payload["new_status"], "live");
}

#[test]
fn bulk_operation_decomposes_into_individual_events() {
    let expired: Vec<Event> = (0..5)
        .map(|_| Event::System(SystemDecision::EntityExpired {
            entity_id: Uuid::new_v4(),
            node_type: NodeType::Gathering,
            reason: "gathering_past_end_date".into(),
        }))
        .collect();

    assert_eq!(expired.len(), 5);
    assert!(expired.iter().all(|e| e.event_type() == "entity_expired"));
}

// =========================================================================
// Usage pattern: reducer determines action from event type
// =========================================================================

#[test]
fn reducer_can_match_on_deserialized_event() {
    let payload = json!({
        "type": "gathering_discovered",
        "id": "550e8400-e29b-41d4-a716-446655440000",
        "title": "Test",
        "summary": "Test",
        "confidence": 0.8,
        "source_url": "https://example.com",
        "extracted_at": "2026-01-01T00:00:00Z",
        "mentioned_actors": [],
    });

    let event = Event::from_payload(&payload).unwrap();

    let should_write = match &event {
        Event::World(WorldEvent::GatheringDiscovered { title, confidence, .. }) => {
            assert!(!title.is_empty());
            assert!(*confidence > 0.0);
            true // → MERGE node
        }
        Event::Telemetry(TelemetryEvent::UrlScraped { .. }) => false, // → no-op
        _ => false,
    };

    assert!(should_write);
}

#[test]
fn reducer_skips_telemetry_events() {
    let telemetry_events = vec![
        Event::Telemetry(TelemetryEvent::UrlScraped { url: "x".into(), strategy: "y".into(), success: true, content_bytes: 0 }),
        Event::Telemetry(TelemetryEvent::FeedScraped { url: "x".into(), items: 5 }),
        Event::Telemetry(TelemetryEvent::BudgetCheckpoint { spent_cents: 100, remaining_cents: 900 }),
        Event::Telemetry(TelemetryEvent::LlmExtractionCompleted { source_url: "x".into(), content_chars: 0, entities_extracted: 0, implied_queries: 0 }),
    ];

    for event in &telemetry_events {
        let is_telemetry = matches!(event, Event::Telemetry(_));
        assert!(
            is_telemetry,
            "Event '{}' should be a telemetry event (reducer no-op)",
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
        // =====================================================================
        // Telemetry (13 variants)
        // =====================================================================
        Event::Telemetry(TelemetryEvent::UrlScraped { url: "x".into(), strategy: "y".into(), success: true, content_bytes: 0 }),
        Event::Telemetry(TelemetryEvent::FeedScraped { url: "x".into(), items: 0 }),
        Event::Telemetry(TelemetryEvent::SocialScraped { platform: "x".into(), identifier: "y".into(), post_count: 0 }),
        Event::Telemetry(TelemetryEvent::SocialTopicsSearched { platform: "x".into(), topics: vec![], posts_found: 0 }),
        Event::Telemetry(TelemetryEvent::SearchPerformed { query: "x".into(), provider: "y".into(), result_count: 0, canonical_key: "z".into() }),
        Event::Telemetry(TelemetryEvent::LlmExtractionCompleted { source_url: "x".into(), content_chars: 0, entities_extracted: 0, implied_queries: 0 }),
        Event::Telemetry(TelemetryEvent::BudgetCheckpoint { spent_cents: 0, remaining_cents: 0 }),
        Event::Telemetry(TelemetryEvent::BootstrapCompleted { sources_created: 0 }),
        Event::Telemetry(TelemetryEvent::AgentWebSearched { provider: "x".into(), query: "y".into(), result_count: 0, title: "z".into() }),
        Event::Telemetry(TelemetryEvent::AgentPageRead { provider: "x".into(), url: "y".into(), content_chars: 0, title: "z".into() }),
        Event::Telemetry(TelemetryEvent::AgentFutureQuery { provider: "x".into(), query: "y".into(), title: "z".into() }),
        Event::Telemetry(TelemetryEvent::PinsRemoved { pin_ids: vec![id] }),
        Event::Telemetry(TelemetryEvent::DemandAggregated { created_task_ids: vec![id], consumed_demand_ids: vec![id] }),
        // =====================================================================
        // World (22 variants)
        // =====================================================================
        // Discovery (5 typed variants) — no sensitivity or implied_queries
        Event::World(WorldEvent::GatheringDiscovered {
            id, title: "x".into(), summary: "y".into(),
            confidence: 0.0, source_url: "z".into(), extracted_at: now, content_date: None,
            location: None, from_location: None,
            mentioned_actors: vec![], author_actor: None, schedule: None, action_url: None, organizer: None,
        }),
        Event::World(WorldEvent::AidDiscovered {
            id, title: "x".into(), summary: "y".into(),
            confidence: 0.0, source_url: "z".into(), extracted_at: now, content_date: None,
            location: None, from_location: None,
            mentioned_actors: vec![], author_actor: None, action_url: None, availability: None, is_ongoing: None,
        }),
        Event::World(WorldEvent::NeedDiscovered {
            id, title: "x".into(), summary: "y".into(),
            confidence: 0.0, source_url: "z".into(), extracted_at: now, content_date: None,
            location: None, from_location: None,
            mentioned_actors: vec![], author_actor: None, urgency: None, what_needed: None, goal: None,
        }),
        Event::World(WorldEvent::NoticeDiscovered {
            id, title: "x".into(), summary: "y".into(),
            confidence: 0.0, source_url: "z".into(), extracted_at: now, content_date: None,
            location: None, from_location: None,
            mentioned_actors: vec![], author_actor: None, severity: None, category: None,
            effective_date: None, source_authority: None,
        }),
        Event::World(WorldEvent::TensionDiscovered {
            id, title: "x".into(), summary: "y".into(),
            confidence: 0.0, source_url: "z".into(), extracted_at: now, content_date: None,
            location: None, from_location: None,
            mentioned_actors: vec![], author_actor: None, severity: None, what_would_help: None,
        }),
        // Corroboration (world fact only)
        Event::World(WorldEvent::ObservationCorroborated { entity_id: id, node_type: NodeType::Aid, new_source_url: "x".into(), summary: None }),
        // Citations
        Event::World(WorldEvent::CitationRecorded { citation_id: id, entity_id: id, url: "x".into(), content_hash: "y".into(), snippet: None, relevance: None, channel_type: None, evidence_confidence: None }),
        // Sources
        Event::World(WorldEvent::SourceRegistered { source_id: id, canonical_key: "x".into(), canonical_value: "y".into(), url: None, discovery_method: DiscoveryMethod::Curated, weight: 0.5, source_role: SourceRole::Mixed, gap_context: None }),
        Event::World(WorldEvent::SourceChanged { source_id: id, canonical_key: "x".into(), change: rootsignal_world::values::WorldSourceChange::Weight { old: 0.5, new: 0.8 } }),
        Event::World(WorldEvent::SourceDeactivated { source_ids: vec![id], reason: "empty".into() }),
        Event::World(WorldEvent::SourceLinkDiscovered { child_id: id, parent_canonical_key: "x".into() }),
        // Actors (no discovery_depth)
        Event::World(WorldEvent::ActorIdentified { actor_id: id, name: "x".into(), actor_type: ActorType::Organization, entity_id: "y".into(), domains: vec![], social_urls: vec![], description: "z".into(), bio: None, location_lat: None, location_lng: None, location_name: None }),
        Event::World(WorldEvent::ActorLinkedToEntity { actor_id: id, entity_id: id, role: "organizer".into() }),
        Event::World(WorldEvent::ActorLinkedToSource { actor_id: id, source_id: id }),
        Event::World(WorldEvent::ActorLocationIdentified { actor_id: id, location_lat: 44.9, location_lng: -93.2, location_name: Some("Minneapolis".into()) }),
        // Community input
        Event::World(WorldEvent::PinCreated { pin_id: id, location_lat: 44.9, location_lng: -93.2, source_id: id, created_by: "scout".into() }),
        Event::World(WorldEvent::DemandReceived { demand_id: id, query: "food shelf".into(), center_lat: 44.9, center_lng: -93.2, radius_km: 10.0 }),
        Event::World(WorldEvent::SubmissionReceived { submission_id: id, url: "https://example.com".into(), reason: Some("good source".into()), source_canonical_key: Some("example.com".into()) }),
        // Relationship edges
        Event::World(WorldEvent::ResourceEdgeCreated { signal_id: id, resource_id: id, role: "requires".into(), confidence: 0.8, quantity: None, notes: None, capacity: None }),
        Event::World(WorldEvent::ResponseLinked { signal_id: id, tension_id: id, strength: 0.7, explanation: "x".into() }),
        Event::World(WorldEvent::GravityLinked { signal_id: id, tension_id: id, strength: 0.6, explanation: "x".into(), gathering_type: "community".into() }),
        // Expansion provenance
        Event::World(WorldEvent::ExpansionQueryCollected { query: "x".into(), source_url: "y".into() }),
        // =====================================================================
        // System (29 variants)
        // =====================================================================
        // Observation lifecycle
        Event::System(SystemDecision::FreshnessConfirmed { entity_ids: vec![id], node_type: NodeType::Need, confirmed_at: now }),
        Event::System(SystemDecision::ConfidenceScored { entity_id: id, old_confidence: 0.5, new_confidence: 0.8 }),
        Event::System(SystemDecision::CorroborationScored { entity_id: id, similarity: 0.9, new_corroboration_count: 2 }),
        Event::System(SystemDecision::ObservationRejected { entity_id: Some(id), title: "x".into(), source_url: "y".into(), reason: "z".into() }),
        Event::System(SystemDecision::EntityExpired { entity_id: id, node_type: NodeType::Gathering, reason: "past_end_date".into() }),
        Event::System(SystemDecision::EntityPurged { entity_id: id, node_type: NodeType::Tension, reason: "admin".into(), context: None }),
        Event::System(SystemDecision::DuplicateDetected { node_type: NodeType::Gathering, title: "x".into(), matched_id: id, similarity: 0.95, action: "merge".into(), source_url: "y".into(), summary: None }),
        Event::System(SystemDecision::ExtractionDroppedNoDate { title: "x".into(), source_url: "y".into() }),
        Event::System(SystemDecision::ReviewVerdictReached { entity_id: id, old_status: "staged".into(), new_status: "live".into(), reason: "ok".into() }),
        Event::System(SystemDecision::ImpliedQueriesConsumed { entity_ids: vec![id] }),
        // Classifications
        Event::System(SystemDecision::SensitivityClassified { entity_id: id, level: SensitivityLevel::General }),
        Event::System(SystemDecision::ImpliedQueriesExtracted { entity_id: id, queries: vec!["test query".into()] }),
        // Corrections (5 typed variants)
        Event::System(SystemDecision::GatheringCorrected { entity_id: id, correction: GatheringCorrection::Title { old: "old".into(), new: "new".into() }, reason: "typo".into() }),
        Event::System(SystemDecision::AidCorrected { entity_id: id, correction: rootsignal_common::events::AidCorrection::Title { old: "old".into(), new: "new".into() }, reason: "typo".into() }),
        Event::System(SystemDecision::NeedCorrected { entity_id: id, correction: rootsignal_common::events::NeedCorrection::Title { old: "old".into(), new: "new".into() }, reason: "typo".into() }),
        Event::System(SystemDecision::NoticeCorrected { entity_id: id, correction: rootsignal_common::events::NoticeCorrection::Title { old: "old".into(), new: "new".into() }, reason: "typo".into() }),
        Event::System(SystemDecision::TensionCorrected { entity_id: id, correction: rootsignal_common::events::TensionCorrection::Title { old: "old".into(), new: "new".into() }, reason: "typo".into() }),
        // Actors
        Event::System(SystemDecision::DuplicateActorsMerged { kept_id: id, merged_ids: vec![Uuid::new_v4()] }),
        Event::System(SystemDecision::OrphanedActorsCleaned { actor_ids: vec![id] }),
        // Situations
        Event::System(SystemDecision::SituationIdentified { situation_id: id, headline: "x".into(), lede: "y".into(), arc: SituationArc::Emerging, temperature: 0.5, centroid_lat: None, centroid_lng: None, location_name: None, sensitivity: SensitivityLevel::General, category: None, structured_state: "{}".into() }),
        Event::System(SystemDecision::SituationChanged { situation_id: id, change: SituationChange::Headline { old: "old".into(), new: "new".into() } }),
        Event::System(SystemDecision::SituationPromoted { situation_ids: vec![id] }),
        Event::System(SystemDecision::DispatchCreated { dispatch_id: id, situation_id: id, body: "x".into(), entity_ids: vec![], dispatch_type: DispatchType::Emergence, supersedes: None, fidelity_score: Some(0.9) }),
        // Tags
        Event::System(SystemDecision::TagSuppressed { situation_id: id, tag_slug: "generic".into() }),
        Event::System(SystemDecision::TagsMerged { source_slug: "old".into(), target_slug: "new".into() }),
        // Quality
        Event::System(SystemDecision::EmptyEntitiesCleaned { entity_ids: vec![id] }),
        Event::System(SystemDecision::FakeCoordinatesNulled { entity_ids: vec![id], old_coords: vec![(0.0, 0.0)] }),
        Event::System(SystemDecision::OrphanedCitationsCleaned { citation_ids: vec![id] }),
        // Source editorial
        Event::System(SystemDecision::SourceSystemChanged { source_id: id, canonical_key: "x".into(), change: SystemSourceChange::QualityPenalty { old: 0.0, new: 0.5 } }),
    ]
}
