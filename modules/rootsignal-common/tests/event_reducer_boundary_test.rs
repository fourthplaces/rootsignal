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
    Event, GatheringCorrection, SituationChange, SystemEvent, SystemSourceChange, TelemetryEvent,
    WorldEvent,
};
use rootsignal_common::safety::SensitivityLevel;
use rootsignal_common::types::{
ActorType, ChannelType, DiscoveryMethod, DispatchType, Entity, EntityType, GeoPoint,
GeoPrecision, NodeType, SituationArc, SourceRole,
};
use serde_json::json;
use uuid::Uuid;
use rootsignal_common::events::{Location, Schedule};

// =========================================================================
// Reducer classification — which events produce graph changes?
// =========================================================================

/// Events that the reducer should act on (produce graph mutations).
const GRAPH_MUTATING_TYPES: &[&str] = &[
    // World: Signal types (7)
    "gathering_announced",
    "resource_offered",
    "help_requested",
    "announcement_shared",
    "concern_raised",
    "condition_observed",
    "incident_reported",
    // World: Corroboration fact
    "observation_corroborated",
    // World: Citations
    "citation_published",
    // World: Actors
    "actor_identified",
    "actor_linked_to_signal",
    "actor_location_identified",
    // World: Relationship edges
    "resource_linked",
    "response_linked",
    "tension_linked",
    // World: Lifecycle
    "gathering_cancelled",
    "resource_depleted",
    "announcement_retracted",
    "citation_retracted",
    "details_changed",
    // World: Resource identification
    "resource_identified",
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
    "tone_classified",
    "severity_classified",
    "urgency_classified",
    "implied_queries_extracted",
    // System: Corrections (5 typed)
    "gathering_corrected",
    "resource_corrected",
    "help_request_corrected",
    "announcement_corrected",
    "concern_corrected",
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
    // System: Source registry
    "source_registered",
    "source_changed",
    "source_deactivated",
    // System: Actor-source links
    "actor_linked_to_source",
    // World: Signal-source links
    "signal_linked_to_source",
    // System: Tags
    "signal_tagged",
    // System: App user actions
    "pin_created",
    "demand_received",
    "submission_received",
    // System: Source scrape telemetry
    "pins_consumed",
    "source_scraped",
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
    // System informational — no graph mutation
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
fn gathering_announced_missing_optional_fields_deserializes() {
    let old_payload = json!({
        "type": "gathering_announced",
        "id": "550e8400-e29b-41d4-a716-446655440000",
        "title": "Old Gathering",
        "summary": "From before we added optional fields",
        "source_url": "https://example.com"
        // NOTE: no schedule, action_url, locations, etc. (optional/defaulted fields)
    });

    let event = Event::from_payload(&old_payload).unwrap();
    match event {
        Event::World(WorldEvent::GatheringAnnounced {
            schedule,
            action_url,
            locations,
            ..
        }) => {
            assert!(schedule.is_none());
            assert!(action_url.is_none());
            assert!(locations.is_empty(), "Missing vec fields should default to empty");
        }
        _ => panic!("Expected GatheringAnnounced"),
    }
}

#[test]
fn observation_corroborated_missing_summary_deserializes() {
    let old_payload = json!({
        "type": "observation_corroborated",
        "signal_id": "550e8400-e29b-41d4-a716-446655440000",
        "node_type": "gathering",
        "new_source_url": "https://example.com/source2"
        // NOTE: no summary field
    });

    let event = Event::from_payload(&old_payload).unwrap();
    match event {
        Event::System(SystemEvent::ObservationCorroborated { summary, .. }) => {
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
    let event = Event::World(WorldEvent::GatheringAnnounced {
        id: Uuid::new_v4(),
        title: "Test".into(),
        summary: "Test".into(),
        source_url: "https://example.com".into(),
        published_at: None,
        extraction_id: None,
        locations: vec![],
        mentioned_entities: vec![],
        references: vec![],
        schedule: None,
        action_url: None,
    });

    let payload = event.to_payload();
    assert!(
        payload.get("embedding").is_none(),
        "embedding is a computed artifact, not a fact"
    );
    assert!(
        payload.get("source_diversity").is_none(),
        "source_diversity is derived"
    );
    assert!(
        payload.get("channel_diversity").is_none(),
        "channel_diversity is derived"
    );
    assert!(payload.get("cause_heat").is_none(), "cause_heat is derived");
    assert!(
        payload.get("freshness_score").is_none(),
        "freshness_score is derived"
    );
    assert!(
        payload.get("external_ratio").is_none(),
        "external_ratio is derived"
    );
    assert!(
        payload.get("sensitivity").is_none(),
        "sensitivity is a system classification, not a world fact"
    );
    assert!(
        payload.get("implied_queries").is_none(),
        "implied_queries is a system artifact, not a world fact"
    );
}

#[test]
fn corroboration_world_fact_has_no_scoring_fields() {
    let event = Event::System(SystemEvent::ObservationCorroborated {
        signal_id: Uuid::new_v4(),
        node_type: NodeType::Gathering,
        new_source_url: "https://source2.com".into(),
        summary: None,
    });

    let payload = event.to_payload();

    // World fact only carries the observation — no scoring
    assert!(
        payload.get("similarity").is_none(),
        "similarity is a system score, not a world fact"
    );
    assert!(
        payload.get("new_corroboration_count").is_none(),
        "corroboration_count is a system score"
    );

    // Diversity counts are enrichment
    assert!(payload.get("source_diversity").is_none());
    assert!(payload.get("channel_diversity").is_none());
}

// =========================================================================
// Usage pattern: scout pipeline emitting events
// =========================================================================

#[test]
fn scout_pipeline_event_chain_is_expressible() {

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

    let gathering = Event::World(WorldEvent::GatheringAnnounced {
        id: Uuid::new_v4(),
        title: "Lake Street Block Party".into(),
        summary: "Annual community gathering on Lake Street".into(),
        source_url: "https://lakestreetstories.com/events".into(),
        published_at: Some(Utc::now()),
        extraction_id: None,
        locations: vec![Location {
            point: Some(GeoPoint {
                lat: 44.9488,
                lng: -93.2683,
                precision: GeoPrecision::Neighborhood,
            }),
            name: Some("Lake Street, Minneapolis".into()),
            address: None,
            role: None,
        }],
        mentioned_entities: vec![Entity {
            name: "Lake Street Council".into(),
            entity_type: EntityType::Organization,
            role: Some("organizer".into()),
        }],
        references: vec![],
        schedule: Some(Schedule {
            starts_at: Some(Utc::now()),
            ends_at: None,
            all_day: false,
            rrule: Some("FREQ=YEARLY".into()),
            timezone: Some("America/Chicago".into()),
        }),
        action_url: Some("https://lakestreetstories.com/events/block-party".into()),
    });

    let citation = Event::World(WorldEvent::CitationPublished {
        citation_id: Uuid::new_v4(),
        signal_id: Uuid::new_v4(),
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
    let event = Event::System(SystemEvent::ReviewVerdictReached {
        signal_id: Uuid::new_v4(),
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
        .map(|_| {
            Event::System(SystemEvent::EntityExpired {
                signal_id: Uuid::new_v4(),
                node_type: NodeType::Gathering,
                reason: "gathering_past_end_date".into(),
            })
        })
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
        "type": "gathering_announced",
        "id": "550e8400-e29b-41d4-a716-446655440000",
        "title": "Test",
        "summary": "Test",
        "source_url": "https://example.com",
    });

    let event = Event::from_payload(&payload).unwrap();

    let should_write = match &event {
        Event::World(WorldEvent::GatheringAnnounced {
            title, ..
        }) => {
            assert!(!title.is_empty());
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
        Event::Telemetry(TelemetryEvent::UrlScraped {
            url: "x".into(),
            strategy: "y".into(),
            success: true,
            content_bytes: 0,
        }),
        Event::Telemetry(TelemetryEvent::FeedScraped {
            url: "x".into(),
            items: 5,
        }),
        Event::Telemetry(TelemetryEvent::BudgetCheckpoint {
            spent_cents: 100,
            remaining_cents: 900,
        }),
        Event::Telemetry(TelemetryEvent::LlmExtractionCompleted {
            source_url: "x".into(),
            content_chars: 0,
            entities_extracted: 0,
            implied_queries: 0,
        }),
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
        Event::Telemetry(TelemetryEvent::UrlScraped {
            url: "x".into(),
            strategy: "y".into(),
            success: true,
            content_bytes: 0,
        }),
        Event::Telemetry(TelemetryEvent::FeedScraped {
            url: "x".into(),
            items: 0,
        }),
        Event::Telemetry(TelemetryEvent::SocialScraped {
            platform: "x".into(),
            identifier: "y".into(),
            post_count: 0,
        }),
        Event::Telemetry(TelemetryEvent::SocialTopicsSearched {
            platform: "x".into(),
            topics: vec![],
            posts_found: 0,
        }),
        Event::Telemetry(TelemetryEvent::SearchPerformed {
            query: "x".into(),
            provider: "y".into(),
            result_count: 0,
            canonical_key: "z".into(),
        }),
        Event::Telemetry(TelemetryEvent::LlmExtractionCompleted {
            source_url: "x".into(),
            content_chars: 0,
            entities_extracted: 0,
            implied_queries: 0,
        }),
        Event::Telemetry(TelemetryEvent::BudgetCheckpoint {
            spent_cents: 0,
            remaining_cents: 0,
        }),
        Event::Telemetry(TelemetryEvent::BootstrapCompleted { sources_created: 0 }),
        Event::Telemetry(TelemetryEvent::AgentWebSearched {
            provider: "x".into(),
            query: "y".into(),
            result_count: 0,
            title: "z".into(),
        }),
        Event::Telemetry(TelemetryEvent::AgentPageRead {
            provider: "x".into(),
            url: "y".into(),
            content_chars: 0,
            title: "z".into(),
        }),
        Event::Telemetry(TelemetryEvent::AgentFutureQuery {
            provider: "x".into(),
            query: "y".into(),
            title: "z".into(),
        }),
        Event::Telemetry(TelemetryEvent::PinsRemoved { pin_ids: vec![id] }),
        Event::Telemetry(TelemetryEvent::DemandAggregated {
            created_task_ids: vec![id],
            consumed_demand_ids: vec![id],
        }),
        // =====================================================================
        // World (22 variants)
        // =====================================================================
        // Signal types (7 variants)
        Event::World(WorldEvent::GatheringAnnounced {
            id,
            title: "x".into(),
            summary: "y".into(),
            source_url: "z".into(),
            published_at: None,
            extraction_id: None,
            locations: vec![],
            mentioned_entities: vec![],
            references: vec![],
            schedule: None,
            action_url: None,
        }),
        Event::World(WorldEvent::ResourceOffered {
            id,
            title: "x".into(),
            summary: "y".into(),
            source_url: "z".into(),
            published_at: None,
            extraction_id: None,
            locations: vec![],
            mentioned_entities: vec![],
            references: vec![],
            schedule: None,
            action_url: None,
            availability: None,
        }),
        Event::World(WorldEvent::HelpRequested {
            id,
            title: "x".into(),
            summary: "y".into(),
            source_url: "z".into(),
            published_at: None,
            extraction_id: None,
            locations: vec![],
            mentioned_entities: vec![],
            references: vec![],
            schedule: None,
            what_needed: None,
            goal: None,
        }),
        Event::World(WorldEvent::AnnouncementShared {
            id,
            title: "x".into(),
            summary: "y".into(),
            source_url: "z".into(),
            published_at: None,
            extraction_id: None,
            locations: vec![],
            mentioned_entities: vec![],
            references: vec![],
            schedule: None,
            category: None,
            effective_date: None,
        }),
        Event::World(WorldEvent::ConcernRaised {
            id,
            title: "x".into(),
            summary: "y".into(),
            source_url: "z".into(),
            published_at: None,
            extraction_id: None,
            locations: vec![],
            mentioned_entities: vec![],
            references: vec![],
            schedule: None,
            what_would_help: None,
        }),
        Event::World(WorldEvent::ConditionObserved {
            id,
            title: "x".into(),
            summary: "y".into(),
            source_url: "z".into(),
            published_at: None,
            extraction_id: None,
            locations: vec![],
            mentioned_entities: vec![],
            references: vec![],
            schedule: None,
        }),
        Event::World(WorldEvent::IncidentReported {
            id,
            title: "x".into(),
            summary: "y".into(),
            source_url: "z".into(),
            published_at: None,
            extraction_id: None,
            locations: vec![],
            mentioned_entities: vec![],
            references: vec![],
            schedule: None,
        }),
        // Corroboration (world fact only)
        Event::System(SystemEvent::ObservationCorroborated {
            signal_id: id,
            node_type: NodeType::Aid,
            new_source_url: "x".into(),
            summary: None,
        }),
        // Citations
        Event::World(WorldEvent::CitationPublished {
            citation_id: id,
            signal_id: id,
            url: "x".into(),
            content_hash: "y".into(),
            snippet: None,
            relevance: None,
            channel_type: None,
            evidence_confidence: None,
        }),
        // Actors (no discovery_depth)
        Event::System(SystemEvent::ActorIdentified {
            actor_id: id,
            name: "x".into(),
            actor_type: ActorType::Organization,
            canonical_key: "y".into(),
            domains: vec![],
            social_urls: vec![],
            description: "z".into(),
            bio: None,
            location_lat: None,
            location_lng: None,
            location_name: None,
        }),
        Event::System(SystemEvent::ActorLinkedToSignal {
            actor_id: id,
            signal_id: id,
            role: "organizer".into(),
        }),
        Event::System(SystemEvent::ActorLocationIdentified {
            actor_id: id,
            location_lat: 44.9,
            location_lng: -93.2,
            location_name: Some("Minneapolis".into()),
        }),
        // Relationship edges
        Event::World(WorldEvent::ResourceLinked {
            signal_id: id,
            resource_slug: "test-resource".into(),
            role: "requires".into(),
            confidence: 0.8,
            quantity: None,
            notes: None,
            capacity: None,
        }),
        Event::System(SystemEvent::ResponseLinked {
            signal_id: id,
            tension_id: id,
            strength: 0.7,
            explanation: "x".into(),
            source_url: None,
        }),
        Event::System(SystemEvent::TensionLinked {
            signal_id: id,
            tension_id: id,
            strength: 0.6,
            explanation: "x".into(),
            source_url: None,
        }),
        // Lifecycle events
        Event::World(WorldEvent::GatheringCancelled {
            signal_id: id,
            reason: "cancelled".into(),
            source_url: "x".into(),
        }),
        Event::World(WorldEvent::ResourceDepleted {
            signal_id: id,
            reason: "exhausted".into(),
            source_url: "x".into(),
        }),
        Event::World(WorldEvent::AnnouncementRetracted {
            signal_id: id,
            reason: "retracted".into(),
            source_url: "x".into(),
        }),
        Event::World(WorldEvent::CitationRetracted {
            citation_id: id,
            reason: "retracted".into(),
            source_url: "x".into(),
        }),
        Event::World(WorldEvent::DetailsChanged {
            signal_id: id,
            summary: "updated details".into(),
            source_url: "x".into(),
        }),
        // Resource identification
        Event::World(WorldEvent::ResourceIdentified {
            resource_id: id,
            name: "x".into(),
            slug: "x".into(),
            description: "y".into(),
        }),
        // =====================================================================
        // System (38 variants)
        // =====================================================================
        // Observation lifecycle
        Event::System(SystemEvent::FreshnessConfirmed {
            signal_ids: vec![id],
            node_type: NodeType::Need,
            confirmed_at: now,
        }),
        Event::System(SystemEvent::ConfidenceScored {
            signal_id: id,
            old_confidence: 0.5,
            new_confidence: 0.8,
        }),
        Event::System(SystemEvent::CorroborationScored {
            signal_id: id,
            similarity: 0.9,
            new_corroboration_count: 2,
        }),
        Event::System(SystemEvent::ObservationRejected {
            signal_id: Some(id),
            title: "x".into(),
            source_url: "y".into(),
            reason: "z".into(),
        }),
        Event::System(SystemEvent::EntityExpired {
            signal_id: id,
            node_type: NodeType::Gathering,
            reason: "past_end_date".into(),
        }),
        Event::System(SystemEvent::EntityPurged {
            signal_id: id,
            node_type: NodeType::Tension,
            reason: "admin".into(),
            context: None,
        }),
        Event::System(SystemEvent::DuplicateDetected {
            node_type: NodeType::Gathering,
            title: "x".into(),
            matched_id: id,
            similarity: 0.95,
            action: "merge".into(),
            source_url: "y".into(),
            summary: None,
        }),
        Event::System(SystemEvent::ExtractionDroppedNoDate {
            title: "x".into(),
            source_url: "y".into(),
        }),
        Event::System(SystemEvent::ReviewVerdictReached {
            signal_id: id,
            old_status: "staged".into(),
            new_status: "live".into(),
            reason: "ok".into(),
        }),
        Event::System(SystemEvent::ImpliedQueriesConsumed {
            signal_ids: vec![id],
        }),
        // Classifications
        Event::System(SystemEvent::SensitivityClassified {
            signal_id: id,
            level: SensitivityLevel::General,
        }),
        Event::System(SystemEvent::ToneClassified {
            signal_id: id,
            tone: rootsignal_common::types::Tone::Hopeful,
        }),
        Event::System(SystemEvent::SeverityClassified {
            signal_id: id,
            severity: rootsignal_common::types::Severity::Medium,
        }),
        Event::System(SystemEvent::UrgencyClassified {
            signal_id: id,
            urgency: rootsignal_common::types::Urgency::Low,
        }),
        Event::System(SystemEvent::ImpliedQueriesExtracted {
            signal_id: id,
            queries: vec!["test query".into()],
        }),
        // Corrections (5 typed variants)
        Event::System(SystemEvent::GatheringCorrected {
            signal_id: id,
            correction: GatheringCorrection::Title {
                old: "old".into(),
                new: "new".into(),
            },
            reason: "typo".into(),
        }),
        Event::System(SystemEvent::ResourceCorrected {
            signal_id: id,
            correction: rootsignal_common::events::ResourceCorrection::Title {
                old: "old".into(),
                new: "new".into(),
            },
            reason: "typo".into(),
        }),
        Event::System(SystemEvent::HelpRequestCorrected {
            signal_id: id,
            correction: rootsignal_common::events::HelpRequestCorrection::Title {
                old: "old".into(),
                new: "new".into(),
            },
            reason: "typo".into(),
        }),
        Event::System(SystemEvent::AnnouncementCorrected {
            signal_id: id,
            correction: rootsignal_common::events::AnnouncementCorrection::Title {
                old: "old".into(),
                new: "new".into(),
            },
            reason: "typo".into(),
        }),
        Event::System(SystemEvent::ConcernCorrected {
            signal_id: id,
            correction: rootsignal_common::events::ConcernCorrection::Title {
                old: "old".into(),
                new: "new".into(),
            },
            reason: "typo".into(),
        }),
        // Actors
        Event::System(SystemEvent::DuplicateActorsMerged {
            kept_id: id,
            merged_ids: vec![Uuid::new_v4()],
        }),
        Event::System(SystemEvent::OrphanedActorsCleaned {
            actor_ids: vec![id],
        }),
        // Situations
        Event::System(SystemEvent::SituationIdentified {
            situation_id: id,
            headline: "x".into(),
            lede: "y".into(),
            arc: SituationArc::Emerging,
            temperature: 0.5,
            centroid_lat: None,
            centroid_lng: None,
            location_name: None,
            sensitivity: SensitivityLevel::General,
            category: None,
            structured_state: "{}".into(),
        }),
        Event::System(SystemEvent::SituationChanged {
            situation_id: id,
            change: SituationChange::Headline {
                old: "old".into(),
                new: "new".into(),
            },
        }),
        Event::System(SystemEvent::SituationPromoted {
            situation_ids: vec![id],
        }),
        Event::System(SystemEvent::DispatchCreated {
            dispatch_id: id,
            situation_id: id,
            body: "x".into(),
            signal_ids: vec![],
            dispatch_type: DispatchType::Emergence,
            supersedes: None,
            fidelity_score: Some(0.9),
        }),
        // Tags
        Event::System(SystemEvent::TagSuppressed {
            situation_id: id,
            tag_slug: "generic".into(),
        }),
        Event::System(SystemEvent::TagsMerged {
            source_slug: "old".into(),
            target_slug: "new".into(),
        }),
        // Quality
        Event::System(SystemEvent::EmptyEntitiesCleaned {
            signal_ids: vec![id],
        }),
        Event::System(SystemEvent::FakeCoordinatesNulled {
            signal_ids: vec![id],
            old_coords: vec![(0.0, 0.0)],
        }),
        Event::System(SystemEvent::OrphanedCitationsCleaned {
            citation_ids: vec![id],
        }),
        // Source editorial
        Event::System(SystemEvent::SourceSystemChanged {
            source_id: id,
            canonical_key: "x".into(),
            change: SystemSourceChange::QualityPenalty { old: 0.0, new: 0.5 },
        }),
        // Source registry
        Event::System(SystemEvent::SourceRegistered {
            source_id: id,
            canonical_key: "x".into(),
            canonical_value: "y".into(),
            url: None,
            discovery_method: DiscoveryMethod::Curated,
            weight: 0.5,
            source_role: SourceRole::Mixed,
            gap_context: None,
        }),
        Event::System(SystemEvent::SourceChanged {
            source_id: id,
            canonical_key: "x".into(),
            change: rootsignal_common::events::SourceChange::Weight { old: 0.5, new: 0.8 },
        }),
        Event::System(SystemEvent::SourceDeactivated {
            source_ids: vec![id],
            reason: "empty".into(),
        }),
        Event::World(WorldEvent::SourceLinkDiscovered {
            child_id: id,
            parent_canonical_key: "x".into(),
        }),
        // Actor-source links
        Event::World(WorldEvent::ActorLinkedToSource {
            actor_id: id,
            source_id: id,
        }),
        // Signal-source links
        Event::World(WorldEvent::SignalLinkedToSource {
            signal_id: id,
            source_id: id,
        }),
        // Tags
        Event::System(SystemEvent::SignalTagged {
            signal_id: id,
            tag_slugs: vec!["test-tag".into()],
        }),
        // App user actions
        Event::System(SystemEvent::PinCreated {
            pin_id: id,
            location_lat: 44.9,
            location_lng: -93.2,
            source_id: id,
            created_by: "scout".into(),
        }),
        Event::System(SystemEvent::DemandReceived {
            demand_id: id,
            query: "food shelf".into(),
            center_lat: 44.9,
            center_lng: -93.2,
            radius_km: 10.0,
        }),
        Event::System(SystemEvent::SubmissionReceived {
            submission_id: id,
            url: "https://example.com".into(),
            reason: Some("good source".into()),
            source_canonical_key: Some("example.com".into()),
        }),
        // Pins consumed
        Event::System(SystemEvent::PinsConsumed {
            pin_ids: vec![id],
        }),
        // Source scrape telemetry
        Event::System(SystemEvent::SourceScraped {
            canonical_key: "x".into(),
            signals_produced: 3,
            scraped_at: now,
        }),
        // System curiosity
        Event::System(SystemEvent::ExpansionQueryCollected {
            query: "x".into(),
            source_url: "y".into(),
        }),
    ]
}

#[test]
fn classification_lists_cover_expected_count() {
    let total = GRAPH_MUTATING_TYPES.len() + OBSERVABILITY_TYPES.len();
    let built = build_all_events().len();
    assert_eq!(
        total, built,
        "Classification lists ({total}) don't match build_all_events ({built}). \
         Did you add a new event variant without updating both?"
    );
}

