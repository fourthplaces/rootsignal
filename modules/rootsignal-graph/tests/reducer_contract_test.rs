//! Reducer contract tests.
//!
//! These verify the reducer's classification of events (no-op vs applied)
//! and its structural guarantees without requiring a Neo4j instance.
//! Integration tests with Neo4j would live in a separate file using testcontainers.

use chrono::Utc;
use rootsignal_common::events::{
    Event, GatheringCorrection, SituationChange, SystemEvent, SystemSourceChange, TelemetryEvent,
    WorldEvent,
};
use rootsignal_common::safety::SensitivityLevel;
use rootsignal_common::types::*;
use serde_json::json;
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
        id: None,
        parent_id: None,
    }
}

// =========================================================================
// Classification: which events are no-ops?
// =========================================================================

/// All telemetry + informational events produce no Cypher.
const NOOP_EVENT_TYPES: &[&str] = &[
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

/// All graph-mutating events produce Cypher.
const APPLIED_EVENT_TYPES: &[&str] = &[
    // World: Discovery (5 typed variants)
    "gathering_discovered",
    "aid_discovered",
    "need_discovered",
    "notice_discovered",
    "tension_discovered",
    // World: Corroboration fact
    "observation_corroborated",
    // World: Citations
    "citation_recorded",
    // World: Actors
    "actor_identified",
    "actor_linked_to_signal",
    "actor_location_identified",
    // World: Relationship edges
    "resource_edge_created",
    "response_linked",
    "tension_linked",
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
    // System: Corrections (5 typed variants)
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
    // System: Source registry
    "source_registered",
    "source_changed",
    "source_deactivated",
    // System: Actor-source links
    "actor_linked_to_source",
    // System: App user actions
    "pin_created",
    "demand_received",
    "submission_received",
];

#[test]
fn every_event_type_is_classified_as_noop_or_applied() {
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
        total_events,
        classified,
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
fn discovery_cypher_uses_merge_not_create() {
    let source = include_str!("../src/reducer.rs");

    assert!(
        source.contains("MERGE (n:{label} {{id: $id}})"),
        "Discovery handlers must use MERGE, not CREATE"
    );
}

#[test]
fn reducer_source_has_no_utc_now() {
    let source = include_str!("../src/reducer.rs");

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
        event_type: "gathering_discovered".to_string(),
        parent_seq: None,
        caused_by_seq: None,
        run_id: None,
        actor: None,
        payload: json!({"type": "gathering_discovered", "bogus": true}),
        schema_v: 1,
        id: None,
        parent_id: None,
    };

    let result = Event::from_payload(&stored.payload);
    assert!(
        result.is_err(),
        "Malformed GatheringDiscovered payload should fail deserialization"
    );
}

#[test]
fn noop_event_stored_event_deserializes_cleanly() {
    let event = Event::Telemetry(TelemetryEvent::UrlScraped {
        url: "https://example.com".into(),
        strategy: "web_page".into(),
        success: true,
        content_bytes: 1024,
    });
    let stored = stored_event(&event);

    let parsed = Event::from_payload(&stored.payload).unwrap();
    assert_eq!(parsed.event_type(), "url_scraped");
}

#[test]
fn discovery_payload_has_no_enrichment_fields() {
    let event = Event::World(WorldEvent::GatheringDiscovered {
        id: Uuid::new_v4(),
        title: "Test".into(),
        summary: "Test".into(),
        confidence: 0.8,
        source_url: "https://example.com".into(),
        extracted_at: Utc::now(),
        published_at: None,
        location: None,
        from_location: None,
        mentioned_actors: vec![],
        author_actor: None,
        schedule: None,
        action_url: None,
        organizer: None,
    });

    let payload = event.to_payload();
    assert!(
        payload.get("embedding").is_none(),
        "Discovery must not carry an embedding field"
    );
    assert!(
        payload.get("freshness_score").is_none(),
        "Discovery must not carry freshness_score"
    );
    assert!(
        payload.get("source_diversity").is_none(),
        "Discovery must not carry source_diversity"
    );
    assert!(
        payload.get("sensitivity").is_none(),
        "Discovery must not carry sensitivity (system classification)"
    );
    assert!(
        payload.get("implied_queries").is_none(),
        "Discovery must not carry implied_queries (system artifact)"
    );
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
            url: "".into(),
            strategy: "".into(),
            success: true,
            content_bytes: 0,
        }),
        Event::Telemetry(TelemetryEvent::FeedScraped {
            url: "".into(),
            items: 0,
        }),
        Event::Telemetry(TelemetryEvent::SocialScraped {
            platform: "".into(),
            identifier: "".into(),
            post_count: 0,
        }),
        Event::Telemetry(TelemetryEvent::SocialTopicsSearched {
            platform: "".into(),
            topics: vec![],
            posts_found: 0,
        }),
        Event::Telemetry(TelemetryEvent::SearchPerformed {
            query: "".into(),
            provider: "".into(),
            result_count: 0,
            canonical_key: "".into(),
        }),
        Event::Telemetry(TelemetryEvent::LlmExtractionCompleted {
            source_url: "".into(),
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
            provider: "".into(),
            query: "".into(),
            result_count: 0,
            title: "".into(),
        }),
        Event::Telemetry(TelemetryEvent::AgentPageRead {
            provider: "".into(),
            url: "".into(),
            content_chars: 0,
            title: "".into(),
        }),
        Event::Telemetry(TelemetryEvent::AgentFutureQuery {
            provider: "".into(),
            query: "".into(),
            title: "".into(),
        }),
        Event::Telemetry(TelemetryEvent::PinsRemoved { pin_ids: vec![] }),
        Event::Telemetry(TelemetryEvent::DemandAggregated {
            created_task_ids: vec![],
            consumed_demand_ids: vec![],
        }),
        // =====================================================================
        // World (22 variants)
        // =====================================================================
        // Discovery (5 typed variants) — no sensitivity or implied_queries
        Event::World(WorldEvent::GatheringDiscovered {
            id,
            title: "".into(),
            summary: "".into(),
            confidence: 0.0,
            source_url: "".into(),
            extracted_at: now,
            published_at: None,
            location: None,
            from_location: None,
            mentioned_actors: vec![],
            author_actor: None,
            schedule: None,
            action_url: None,
            organizer: None,
        }),
        Event::World(WorldEvent::AidDiscovered {
            id,
            title: "".into(),
            summary: "".into(),
            confidence: 0.0,
            source_url: "".into(),
            extracted_at: now,
            published_at: None,
            location: None,
            from_location: None,
            mentioned_actors: vec![],
            author_actor: None,
            action_url: None,
            availability: None,
            is_ongoing: None,
        }),
        Event::World(WorldEvent::NeedDiscovered {
            id,
            title: "".into(),
            summary: "".into(),
            confidence: 0.0,
            source_url: "".into(),
            extracted_at: now,
            published_at: None,
            location: None,
            from_location: None,
            mentioned_actors: vec![],
            author_actor: None,
            urgency: None,
            what_needed: None,
            goal: None,
        }),
        Event::World(WorldEvent::NoticeDiscovered {
            id,
            title: "".into(),
            summary: "".into(),
            confidence: 0.0,
            source_url: "".into(),
            extracted_at: now,
            published_at: None,
            location: None,
            from_location: None,
            mentioned_actors: vec![],
            author_actor: None,
            severity: None,
            category: None,
            effective_date: None,
            source_authority: None,
        }),
        Event::World(WorldEvent::TensionDiscovered {
            id,
            title: "".into(),
            summary: "".into(),
            confidence: 0.0,
            source_url: "".into(),
            extracted_at: now,
            published_at: None,
            location: None,
            from_location: None,
            mentioned_actors: vec![],
            author_actor: None,
            severity: None,
            what_would_help: None,
        }),
        // Corroboration (world fact only — no similarity or count)
        Event::World(WorldEvent::ObservationCorroborated {
            signal_id: id,
            node_type: NodeType::Gathering,
            new_source_url: "".into(),
            summary: None,
        }),
        // Citations
        Event::World(WorldEvent::CitationRecorded {
            citation_id: id,
            signal_id: id,
            url: "".into(),
            content_hash: "".into(),
            snippet: None,
            relevance: None,
            channel_type: None,
            evidence_confidence: None,
        }),
        // Actors (no discovery_depth)
        Event::World(WorldEvent::ActorIdentified {
            actor_id: id,
            name: "".into(),
            actor_type: ActorType::Organization,
            canonical_key: "".into(),
            domains: vec![],
            social_urls: vec![],
            description: "".into(),
            bio: None,
            location_lat: None,
            location_lng: None,
            location_name: None,
        }),
        Event::World(WorldEvent::ActorLinkedToSignal {
            actor_id: id,
            signal_id: id,
            role: "".into(),
        }),
        Event::World(WorldEvent::ActorLocationIdentified {
            actor_id: id,
            location_lat: 0.0,
            location_lng: 0.0,
            location_name: None,
        }),
        // Relationship edges
        Event::World(WorldEvent::ResourceEdgeCreated {
            signal_id: id,
            resource_id: id,
            role: "requires".into(),
            confidence: 0.8,
            quantity: None,
            notes: None,
            capacity: None,
        }),
        Event::World(WorldEvent::ResponseLinked {
            signal_id: id,
            tension_id: id,
            strength: 0.7,
            explanation: "".into(),
            source_url: None,
        }),
        Event::World(WorldEvent::TensionLinked {
            signal_id: id,
            tension_id: id,
            strength: 0.6,
            explanation: "".into(),
            source_url: None,
        }),
        // =====================================================================
        // System (38 variants)
        // =====================================================================
        // Observation lifecycle
        Event::System(SystemEvent::FreshnessConfirmed {
            signal_ids: vec![id],
            node_type: NodeType::Gathering,
            confirmed_at: now,
        }),
        Event::System(SystemEvent::ConfidenceScored {
            signal_id: id,
            old_confidence: 0.5,
            new_confidence: 0.8,
        }),
        Event::System(SystemEvent::CorroborationScored {
            signal_id: id,
            similarity: 0.0,
            new_corroboration_count: 1,
        }),
        Event::System(SystemEvent::ObservationRejected {
            signal_id: None,
            title: "".into(),
            source_url: "".into(),
            reason: "".into(),
        }),
        Event::System(SystemEvent::EntityExpired {
            signal_id: id,
            node_type: NodeType::Gathering,
            reason: "".into(),
        }),
        Event::System(SystemEvent::EntityPurged {
            signal_id: id,
            node_type: NodeType::Gathering,
            reason: "".into(),
            context: None,
        }),
        Event::System(SystemEvent::DuplicateDetected {
            node_type: NodeType::Gathering,
            title: "".into(),
            matched_id: id,
            similarity: 0.0,
            action: "".into(),
            source_url: "".into(),
            summary: None,
        }),
        Event::System(SystemEvent::ExtractionDroppedNoDate {
            title: "".into(),
            source_url: "".into(),
        }),
        Event::System(SystemEvent::ReviewVerdictReached {
            signal_id: id,
            old_status: "staged".into(),
            new_status: "live".into(),
            reason: "".into(),
        }),
        Event::System(SystemEvent::ImpliedQueriesConsumed {
            signal_ids: vec![id],
        }),
        // Classifications
        Event::System(SystemEvent::SensitivityClassified {
            signal_id: id,
            level: SensitivityLevel::General,
        }),
        Event::System(SystemEvent::ImpliedQueriesExtracted {
            signal_id: id,
            queries: vec![],
        }),
        // Corrections (5 typed variants)
        Event::System(SystemEvent::GatheringCorrected {
            signal_id: id,
            correction: GatheringCorrection::Title {
                old: "".into(),
                new: "".into(),
            },
            reason: "".into(),
        }),
        Event::System(SystemEvent::AidCorrected {
            signal_id: id,
            correction: rootsignal_common::events::AidCorrection::Title {
                old: "".into(),
                new: "".into(),
            },
            reason: "".into(),
        }),
        Event::System(SystemEvent::NeedCorrected {
            signal_id: id,
            correction: rootsignal_common::events::NeedCorrection::Title {
                old: "".into(),
                new: "".into(),
            },
            reason: "".into(),
        }),
        Event::System(SystemEvent::NoticeCorrected {
            signal_id: id,
            correction: rootsignal_common::events::NoticeCorrection::Title {
                old: "".into(),
                new: "".into(),
            },
            reason: "".into(),
        }),
        Event::System(SystemEvent::TensionCorrected {
            signal_id: id,
            correction: rootsignal_common::events::TensionCorrection::Title {
                old: "".into(),
                new: "".into(),
            },
            reason: "".into(),
        }),
        // Actors
        Event::System(SystemEvent::DuplicateActorsMerged {
            kept_id: id,
            merged_ids: vec![],
        }),
        Event::System(SystemEvent::OrphanedActorsCleaned { actor_ids: vec![] }),
        // Situations
        Event::System(SystemEvent::SituationIdentified {
            situation_id: id,
            headline: "".into(),
            lede: "".into(),
            arc: SituationArc::Emerging,
            temperature: 0.0,
            centroid_lat: None,
            centroid_lng: None,
            location_name: None,
            sensitivity: SensitivityLevel::General,
            category: None,
            structured_state: "".into(),
        }),
        Event::System(SystemEvent::SituationChanged {
            situation_id: id,
            change: SituationChange::Headline {
                old: "".into(),
                new: "".into(),
            },
        }),
        Event::System(SystemEvent::SituationPromoted {
            situation_ids: vec![id],
        }),
        Event::System(SystemEvent::DispatchCreated {
            dispatch_id: id,
            situation_id: id,
            body: "".into(),
            signal_ids: vec![],
            dispatch_type: DispatchType::Update,
            supersedes: None,
            fidelity_score: None,
        }),
        // Tags
        Event::System(SystemEvent::TagSuppressed {
            situation_id: id,
            tag_slug: "".into(),
        }),
        Event::System(SystemEvent::TagsMerged {
            source_slug: "".into(),
            target_slug: "".into(),
        }),
        // Quality / lint
        Event::System(SystemEvent::EmptyEntitiesCleaned { signal_ids: vec![] }),
        Event::System(SystemEvent::FakeCoordinatesNulled {
            signal_ids: vec![],
            old_coords: vec![],
        }),
        Event::System(SystemEvent::OrphanedCitationsCleaned {
            citation_ids: vec![id],
        }),
        // Source editorial
        Event::System(SystemEvent::SourceSystemChanged {
            source_id: id,
            canonical_key: "".into(),
            change: SystemSourceChange::QualityPenalty { old: 0.0, new: 0.0 },
        }),
        // Source registry
        Event::System(SystemEvent::SourceRegistered {
            source_id: id,
            canonical_key: "".into(),
            canonical_value: "".into(),
            url: None,
            discovery_method: DiscoveryMethod::Curated,
            weight: 0.5,
            source_role: SourceRole::Mixed,
            gap_context: None,
        }),
        Event::System(SystemEvent::SourceChanged {
            source_id: id,
            canonical_key: "".into(),
            change: rootsignal_common::events::SourceChange::Weight { old: 0.0, new: 0.0 },
        }),
        Event::System(SystemEvent::SourceDeactivated {
            source_ids: vec![id],
            reason: "".into(),
        }),
        Event::System(SystemEvent::SourceLinkDiscovered {
            child_id: id,
            parent_canonical_key: "".into(),
        }),
        // Actor-source links
        Event::System(SystemEvent::ActorLinkedToSource {
            actor_id: id,
            source_id: id,
        }),
        // App user actions
        Event::System(SystemEvent::PinCreated {
            pin_id: id,
            location_lat: 0.0,
            location_lng: 0.0,
            source_id: id,
            created_by: "".into(),
        }),
        Event::System(SystemEvent::DemandReceived {
            demand_id: id,
            query: "".into(),
            center_lat: 0.0,
            center_lng: 0.0,
            radius_km: 0.0,
        }),
        Event::System(SystemEvent::SubmissionReceived {
            submission_id: id,
            url: "".into(),
            reason: None,
            source_canonical_key: None,
        }),
        // System curiosity
        Event::System(SystemEvent::ExpansionQueryCollected {
            query: "".into(),
            source_url: "".into(),
        }),
    ]
}
