//! Projector contract tests.
//!
//! These verify the projector's classification of events (no-op vs applied)
//! and its structural guarantees without requiring a Neo4j instance.
//! Integration tests with Neo4j would live in a separate file using testcontainers.

use chrono::Utc;
use rootsignal_common::events::{
    Event, GatheringCorrection, SituationChange, SystemEvent, SystemSourceChange, TelemetryEvent,
    WorldEvent,
};
use rootsignal_common::safety::SensitivityLevel;
use rootsignal_common::types::*;
use causal::types::PersistedEvent;
use serde_json::json;
use uuid::Uuid;

/// Build a minimal PersistedEvent from an Event for testing.
fn stored_event(event: &Event) -> PersistedEvent {
    PersistedEvent {
        position: 1,
        event_id: Uuid::new_v4(),
        parent_id: None,
        correlation_id: Uuid::new_v4(),
        event_type: event.event_type().to_string(),
        payload: event.to_payload(),
        created_at: Utc::now(),
        aggregate_type: None,
        aggregate_id: None,
        version: None,
        metadata: {
            let mut m = serde_json::Map::new();
            m.insert("run_id".into(), json!("test-run"));
            m.insert("schema_v".into(), json!(1));
            m.insert("actor".into(), json!("test-actor"));
            m
        },
        ephemeral: None,
        persistent: true,
    }
}

// =========================================================================
// Classification: which events are no-ops?
// =========================================================================

/// All telemetry + informational events produce no Cypher.
const NOOP_EVENT_TYPES: &[&str] = &[
    // Telemetry
    "telemetry:url_scraped",
    "telemetry:feed_scraped",
    "telemetry:social_scraped",
    "telemetry:social_topics_searched",
    "telemetry:search_performed",
    "telemetry:llm_extraction_completed",
    "telemetry:budget_checkpoint",
    "telemetry:bootstrap_completed",
    "telemetry:agent_web_searched",
    "telemetry:agent_page_read",
    "telemetry:agent_future_query",
    "telemetry:pins_removed",
    "telemetry:demand_aggregated",
    // System informational — no graph mutation
    "system:expansion_query_collected",
    "world:source_link_discovered",
    // System informational — no graph mutation
    "system:observation_rejected",
    "system:extraction_dropped_no_date",
    "system:duplicate_detected",
    "system:dispatch_created",
];

/// All graph-mutating events produce Cypher.
const APPLIED_EVENT_TYPES: &[&str] = &[
    // World: Discovery (6 typed variants)
    "world:gathering_announced",
    "world:resource_offered",
    "world:help_requested",
    "world:announcement_shared",
    "world:concern_raised",
    "world:condition_observed",
    // System: Corroboration fact
    "system:observation_corroborated",
    // World: Citations
    "world:citation_published",
    // System: Actors
    "system:actor_identified",
    "system:actor_linked_to_signal",
    "system:actor_location_identified",
    // World: Relationship edges
    "world:resource_linked",
    "system:response_linked",
    "system:concern_linked",
    // World: Lifecycle
    "world:gathering_cancelled",
    "world:resource_depleted",
    "world:announcement_retracted",
    "world:citation_retracted",
    "world:details_changed",
    // World: Resource identification
    "world:resource_identified",
    // World: Signal-source links
    "world:signal_linked_to_source",
    // System: Observation lifecycle
    "system:freshness_confirmed",
    "system:confidence_scored",
    "system:corroboration_scored",
    "system:signals_expired",
    "system:entity_purged",
    "system:review_verdict_reached",
    "system:implied_queries_consumed",
    // System: Classifications
    "system:sensitivity_classified",
    "system:tone_classified",
    "system:severity_classified",
    "system:urgency_classified",
    "system:category_classified",
    "system:implied_queries_extracted",
    // System: Corrections (5 typed variants)
    "system:gathering_corrected",
    "system:resource_corrected",
    "system:help_request_corrected",
    "system:announcement_corrected",
    "system:concern_corrected",
    // System: Actors
    "system:duplicate_actors_merged",
    "system:orphaned_actors_cleaned",
    // System: Situations
    "system:situation_identified",
    "system:situation_changed",
    "system:situation_promoted",
    "system:signal_assigned_to_situation",
    "system:situation_tags_aggregated",
    "system:dispatch_flagged_for_review",
    "system:signals_pending_weaving",
    // System: Tags
    "system:signal_tagged",
    "system:tag_suppressed",
    "system:tags_merged",
    // System: Quality / lint
    "system:empty_entities_cleaned",
    "system:fake_coordinates_nulled",
    "system:orphaned_citations_cleaned",
    // System: Source editorial
    "system:source_system_changed",
    // System: Source registry
    "system:sources_registered",
    "system:source_changed",
    "system:source_deactivated",
    // World: Actor-source links
    "world:actor_linked_to_source",
    // System: App user actions
    "system:pin_created",
    "system:pins_consumed",
    "system:demand_received",
    "system:submission_received",
    // System: Source scrape telemetry
    "system:source_scraped",
    // System: Synthesis telemetry
    "system:response_scouted",
    "system:query_embedding_stored",
    "system:curiosity_triggered",
    "system:signal_investigated",
    "system:exhausted_retries_promoted",
    "system:concern_linker_outcome_recorded",
    "system:gathering_scouted",
    // System: Place & gathering geography
    "system:place_discovered",
    "system:gathers_at_place_linked",
    // System: Concern deduplication
    "system:duplicate_concern_merged",
    // System: Source weight adjustments
    "system:sources_boosted_for_situation",
    // System: Supervisor analytics
    "system:echo_scored",
    "system:cause_heat_computed",
    "system:signal_diversity_computed",
    "system:actor_stats_computed",
    "system:similarity_edges_rebuilt",
    // System: Admin actions
    "system:validation_issue_dismissed",
    // System: Location geocoding
    "system:location_geocoded",
    // System: Signal groups (coalescing)
    "system:group_created",
    "system:signal_added_to_group",
    "system:group_queries_refined",
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
            "Event type '{}' is not classified as noop or applied in projector tests",
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
    let source = include_str!("../src/projector.rs");

    assert!(
        source.contains("MERGE (n:{label} {{id: $id}})"),
        "Discovery handlers must use MERGE, not CREATE"
    );
}

#[test]
fn projector_source_has_no_utc_now() {
    let source = include_str!("../src/projector.rs");

    assert!(
        !source.contains("Utc::now()"),
        "Projector must not use Utc::now() — all timestamps come from event payloads"
    );
}

#[test]
fn projector_source_has_no_uuid_new() {
    let source = include_str!("../src/projector.rs");

    assert!(
        !source.contains("Uuid::new_v4()"),
        "Projector must not generate UUIDs — all IDs come from event payloads"
    );
}

// Embedding writes are now legitimate in the projector for:
// - SituationIdentified: narrative_embedding, causal_embedding
// - QueryEmbeddingStored: source query embedding
// Signal-level embeddings (n.embedding) remain an enrichment-pass concern.

// Diversity writes are now event-sourced via SignalDiversityComputed — the projector legitimately
// writes source_diversity, channel_diversity, and external_ratio.

// cause_heat is now event-sourced via CauseHeatComputed — the projector legitimately writes it.

#[test]
fn projector_source_has_no_freshness_score_writes() {
    let source = include_str!("../src/projector.rs");

    assert!(
        !source.contains("n.freshness_score =") && !source.contains("freshness_score: $"),
        "Projector must not write freshness_score — that's a derived metric"
    );
}

#[test]
fn malformed_payload_returns_deserialize_error() {
    let stored = PersistedEvent {
        position: 1,
        event_id: Uuid::new_v4(),
        parent_id: None,
        correlation_id: Uuid::new_v4(),
        event_type: "gathering_announced".to_string(),
        payload: json!({"type": "gathering_announced", "bogus": true}),
        created_at: Utc::now(),
        aggregate_type: None,
        aggregate_id: None,
        version: None,
        metadata: serde_json::Map::new(),
        ephemeral: None,
        persistent: true,
    };

    let result = Event::from_payload(&stored.payload);
    assert!(
        result.is_err(),
        "Malformed GatheringAnnounced payload should fail deserialization"
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
    assert_eq!(parsed.event_type(), "telemetry:url_scraped");
}

#[test]
fn discovery_payload_has_no_enrichment_fields() {
    let event = Event::World(WorldEvent::GatheringAnnounced {
        id: Uuid::new_v4(),
        title: "Test".into(),
        summary: "Test".into(),
        url: "https://example.com".into(),
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
        // World (21 variants)
        // =====================================================================
        // Discovery (6 typed variants) — no sensitivity or implied_queries
        Event::World(WorldEvent::GatheringAnnounced {
            id,
            title: "".into(),
            summary: "".into(),
            url: "".into(),
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
            title: "".into(),
            summary: "".into(),
            url: "".into(),
            published_at: None,
            extraction_id: None,
            locations: vec![],
            mentioned_entities: vec![],
            references: vec![],
            schedule: None,
            action_url: None,
            availability: None,
            eligibility: None,
        }),
        Event::World(WorldEvent::HelpRequested {
            id,
            title: "".into(),
            summary: "".into(),
            url: "".into(),
            published_at: None,
            extraction_id: None,
            locations: vec![],
            mentioned_entities: vec![],
            references: vec![],
            schedule: None,
            what_needed: None,
            stated_goal: None,
        }),
        Event::World(WorldEvent::AnnouncementShared {
            id,
            title: "".into(),
            summary: "".into(),
            url: "".into(),
            published_at: None,
            extraction_id: None,
            locations: vec![],
            mentioned_entities: vec![],
            references: vec![],
            schedule: None,
            subject: None,
            effective_date: None,
        }),
        Event::World(WorldEvent::ConcernRaised {
            id,
            title: "".into(),
            summary: "".into(),
            url: "".into(),
            published_at: None,
            extraction_id: None,
            locations: vec![],
            mentioned_entities: vec![],
            references: vec![],
            schedule: None,
            subject: None,
            opposing: None,
        }),
        Event::World(WorldEvent::ConditionObserved {
            id,
            title: "".into(),
            summary: "".into(),
            url: "".into(),
            published_at: None,
            extraction_id: None,
            locations: vec![],
            mentioned_entities: vec![],
            references: vec![],
            schedule: None,
            subject: None,
            observed_by: None,
            measurement: None,
            affected_scope: None,
        }),
        // Corroboration (world fact only — no similarity or count)
        Event::System(SystemEvent::ObservationCorroborated {
            signal_id: id,
            node_type: NodeType::Gathering,
            new_url: "".into(),
            summary: None,
        }),
        // Citations
        Event::World(WorldEvent::CitationPublished {
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
        Event::System(SystemEvent::ActorIdentified {
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
        Event::System(SystemEvent::ActorLinkedToSignal {
            actor_id: id,
            signal_id: id,
            role: "".into(),
        }),
        Event::System(SystemEvent::ActorLocationIdentified {
            actor_id: id,
            location_lat: 0.0,
            location_lng: 0.0,
            location_name: None,
        }),
        // Relationship edges
        Event::World(WorldEvent::ResourceLinked {
            signal_id: id,
            resource_slug: "".into(),
            role: "requires".into(),
            confidence: 0.8,
            quantity: None,
            notes: None,
            capacity: None,
        }),
        Event::System(SystemEvent::ResponseLinked {
            signal_id: id,
            concern_id: id,
            strength: 0.7,
            explanation: "".into(),
            source_url: None,
        }),
        Event::System(SystemEvent::ConcernLinked {
            signal_id: id,
            concern_id: id,
            strength: 0.6,
            explanation: "".into(),
            source_url: None,
        }),
        // Lifecycle events
        Event::World(WorldEvent::GatheringCancelled {
            signal_id: id,
            reason: "".into(),
            url: "".into(),
        }),
        Event::World(WorldEvent::ResourceDepleted {
            signal_id: id,
            reason: "".into(),
            url: "".into(),
        }),
        Event::World(WorldEvent::AnnouncementRetracted {
            signal_id: id,
            reason: "".into(),
            url: "".into(),
        }),
        Event::World(WorldEvent::CitationRetracted {
            citation_id: id,
            reason: "".into(),
            url: "".into(),
        }),
        Event::World(WorldEvent::DetailsChanged {
            signal_id: id,
            node_type: NodeType::Concern,
            title: "".into(),
            summary: "".into(),
            url: "".into(),
        }),
        // Resource identification
        Event::World(WorldEvent::ResourceIdentified {
            resource_id: id,
            name: "".into(),
            slug: "".into(),
            description: "".into(),
        }),
        // Signal-source links
        Event::World(WorldEvent::SignalLinkedToSource {
            signal_id: id,
            source_id: id,
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
        Event::System(SystemEvent::SignalsExpired {
            signals: vec![rootsignal_common::system_events::StaleSignal {
                signal_id: id,
                node_type: NodeType::Gathering,
                reason: "".into(),
            }],
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
            new_status: "accepted".into(),
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
        Event::System(SystemEvent::ResourceCorrected {
            signal_id: id,
            correction: rootsignal_common::events::ResourceCorrection::Title {
                old: "".into(),
                new: "".into(),
            },
            reason: "".into(),
        }),
        Event::System(SystemEvent::HelpRequestCorrected {
            signal_id: id,
            correction: rootsignal_common::events::HelpRequestCorrection::Title {
                old: "".into(),
                new: "".into(),
            },
            reason: "".into(),
        }),
        Event::System(SystemEvent::AnnouncementCorrected {
            signal_id: id,
            correction: rootsignal_common::events::AnnouncementCorrection::Title {
                old: "".into(),
                new: "".into(),
            },
            reason: "".into(),
        }),
        Event::System(SystemEvent::ConcernCorrected {
            signal_id: id,
            correction: rootsignal_common::events::ConcernCorrection::Title {
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
            tension_heat: None,
            clarity: None,
            signal_count: None,
            narrative_embedding: None,
            causal_embedding: None,
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
            flagged_for_review: None,
            flag_reason: None,
        }),
        // Tags
        Event::System(SystemEvent::SignalTagged {
            signal_id: id,
            tag_slugs: vec!["test-tag".into()],
        }),
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
        Event::System(SystemEvent::SourcesRegistered {
            sources: vec![SourceNode::new(
                "".into(), "".into(), None,
                DiscoveryMethod::Curated, 0.5, SourceRole::Mixed, None,
            )],
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
        Event::World(WorldEvent::SourceLinkDiscovered {
            child_id: id,
            parent_canonical_key: "".into(),
        }),
        // Actor-source links
        Event::World(WorldEvent::ActorLinkedToSource {
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
        Event::System(SystemEvent::PinsConsumed {
            pin_ids: vec![id],
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
        // Source scrape telemetry
        Event::System(SystemEvent::SourceScraped {
            canonical_key: "".into(),
            signals_produced: 0,
            scraped_at: now,
        }),
        // System curiosity
        Event::System(SystemEvent::ExpansionQueryCollected {
            query: "".into(),
            source_url: "".into(),
        }),
        // Classifications (cont.)
        Event::System(SystemEvent::CategoryClassified {
            signal_id: id,
            category: "housing".into(),
        }),
        // Situations (cont.)
        Event::System(SystemEvent::SignalAssignedToSituation {
            signal_id: id,
            situation_id: id,
            signal_label: "".into(),
            confidence: 0.8,
            reasoning: "".into(),
        }),
        Event::System(SystemEvent::SituationTagsAggregated {
            situation_id: id,
            tag_slugs: vec![],
        }),
        Event::System(SystemEvent::DispatchFlaggedForReview {
            dispatch_id: id,
            reason: "".into(),
        }),
        Event::System(SystemEvent::SignalsPendingWeaving {
            signal_ids: vec![id],
            scout_run_id: "".into(),
        }),
        // Synthesis telemetry
        Event::System(SystemEvent::ResponseScouted {
            concern_id: id,
            scouted_at: now,
        }),
        Event::System(SystemEvent::QueryEmbeddingStored {
            canonical_key: "".into(),
            embedding: vec![],
        }),
        Event::System(SystemEvent::CuriosityTriggered {
            situation_id: id,
            signal_ids: vec![],
        }),
        Event::System(SystemEvent::SignalInvestigated {
            signal_id: id,
            node_type: NodeType::Gathering,
            investigated_at: now,
        }),
        Event::System(SystemEvent::ExhaustedRetriesPromoted {
            promoted_at: now,
        }),
        Event::System(SystemEvent::ConcernLinkerOutcomeRecorded {
            signal_id: id,
            label: "".into(),
            outcome: "".into(),
            increment_retry: false,
        }),
        Event::System(SystemEvent::GatheringScouted {
            concern_id: id,
            found_gatherings: false,
            scouted_at: now,
        }),
        // Place & gathering geography
        Event::System(SystemEvent::PlaceDiscovered {
            place_id: id,
            name: "".into(),
            slug: "".into(),
            lat: 0.0,
            lng: 0.0,
            discovered_at: now,
        }),
        Event::System(SystemEvent::GathersAtPlaceLinked {
            signal_id: id,
            place_slug: "".into(),
        }),
        // Concern deduplication
        Event::System(SystemEvent::DuplicateConcernMerged {
            survivor_id: id,
            duplicate_id: id,
        }),
        // Source weight adjustments
        Event::System(SystemEvent::SourcesBoostedForSituation {
            headline: "".into(),
            factor: 1.0,
        }),
        // Supervisor analytics
        Event::System(SystemEvent::EchoScored {
            situation_id: id,
            echo_score: 0.5,
        }),
        Event::System(SystemEvent::CauseHeatComputed {
            scores: vec![],
        }),
        Event::System(SystemEvent::SignalDiversityComputed {
            metrics: vec![],
        }),
        Event::System(SystemEvent::ActorStatsComputed {
            stats: vec![],
        }),
        Event::System(SystemEvent::SimilarityEdgesRebuilt {
            edges: vec![],
        }),
        // Admin actions
        Event::System(SystemEvent::ValidationIssueDismissed {
            issue_id: "".into(),
        }),
        // Location geocoding
        Event::System(SystemEvent::LocationGeocoded {
            signal_id: id,
            location_name: "Minneapolis".into(),
            lat: 44.9778,
            lng: -93.2650,
            address: Some("Minneapolis, Minnesota, United States".into()),
            precision: "approximate".into(),
            timezone: Some("America/Chicago".into()),
            city: Some("Minneapolis".into()),
            state: Some("Minnesota".into()),
            country_code: Some("US".into()),
        }),
        // Signal groups (coalescing)
        Event::System(SystemEvent::GroupCreated {
            group_id: id,
            label: "Housing affordability cluster".into(),
            queries: vec!["rent increase".into()],
            seed_signal_id: Some(id),
        }),
        Event::System(SystemEvent::SignalAddedToGroup {
            signal_id: id,
            group_id: id,
            confidence: 0.85,
        }),
        Event::System(SystemEvent::GroupQueriesRefined {
            group_id: id,
            queries: vec!["rent increase".into(), "housing cost".into()],
        }),
    ]
}

// =========================================================================
// Coalescing Cypher structural guarantees
// =========================================================================

#[test]
fn group_created_cypher_uses_merge() {
    let source = include_str!("../src/projector.rs");
    assert!(
        source.contains("MERGE (g:SignalGroup {id: $id})"),
        "GroupCreated must MERGE, not CREATE — idempotent replay"
    );
}

#[test]
fn group_created_seed_uses_merge_member_of() {
    let source = include_str!("../src/projector.rs");
    assert!(
        source.contains("MERGE (sig)-[r:MEMBER_OF]->(g)"),
        "Seed signal edge must use MERGE for idempotent replay"
    );
}

#[test]
fn signal_added_to_group_uses_multi_label_match() {
    let source = include_str!("../src/projector.rs");
    // The MEMBER_OF edge query must match all signal labels
    let signal_match = "sig:Gathering OR sig:Resource OR sig:HelpRequest OR sig:Announcement OR sig:Concern OR sig:Condition";
    assert!(
        source.contains(signal_match),
        "SignalAddedToGroup must match all 6 signal labels for multi-label coalesce"
    );
}

#[test]
fn group_queries_refined_uses_match_not_merge() {
    let source = include_str!("../src/projector.rs");
    // Refining queries on an existing group — MATCH, not MERGE
    assert!(
        source.contains("MATCH (g:SignalGroup {id: $id})\n                     SET g.queries"),
        "GroupQueriesRefined should MATCH existing group, not MERGE a new one"
    );
}

#[test]
fn classification_lists_cover_expected_count() {
    let total = NOOP_EVENT_TYPES.len() + APPLIED_EVENT_TYPES.len();
    let built = build_all_events().len();
    assert_eq!(
        total, built,
        "Classification lists ({total}) don't match build_all_events ({built}). \
         Did you add a new event variant without updating both?"
    );
}

