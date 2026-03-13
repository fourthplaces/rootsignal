//! Layer 2: System Events — the editorial layer.
//!
//! Every variant describes a decision Root Signal made about world facts:
//! scoring, correcting, classifying, expiring, clustering. These can evolve
//! rapidly without changing the archival world record.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::events::{
    ActorStatScore, AnnouncementCorrection, CauseHeatScore, ConditionCorrection, ConcernCorrection,
    GatheringCorrection, HelpRequestCorrection, ResourceCorrection, SignalDiversityScore,
    SimilarityEdge, SituationChange, SourceChange, SystemSourceChange,
};
use crate::safety::SensitivityLevel;
use crate::types::{
    ActorType, DispatchType, NodeType, Severity, SituationArc, SourceNode, Tone,
    Urgency,
};

/// A signal found to be stale — expired by age or staleness rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaleSignal {
    pub signal_id: Uuid,
    pub node_type: NodeType,
    pub reason: String,
}

/// A system event — an editorial judgment Root Signal made about world facts.
#[causal_core_macros::event(prefix = "system")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SystemEvent {
    // -----------------------------------------------------------------------
    // Signal lifecycle decisions
    // -----------------------------------------------------------------------
    FreshnessConfirmed {
        signal_ids: Vec<Uuid>,
        node_type: NodeType,
        confirmed_at: DateTime<Utc>,
    },

    ConfidenceScored {
        signal_id: Uuid,
        old_confidence: f32,
        new_confidence: f32,
    },

    /// Split from ObservationCorroborated — the system's assessment of corroboration.
    CorroborationScored {
        signal_id: Uuid,
        similarity: f64,
        new_corroboration_count: u32,
    },

    ObservationRejected {
        signal_id: Option<Uuid>,
        title: String,
        source_url: String,
        reason: String,
    },

    /// Batch of signals marked stale — sets `expired = true` on each node.
    /// One event carries all stale signals found in a single run.
    SignalsExpired {
        signals: Vec<StaleSignal>,
    },

    EntityPurged {
        signal_id: Uuid,
        node_type: NodeType,
        reason: String,
        context: Option<String>,
    },

    DuplicateDetected {
        node_type: NodeType,
        title: String,
        matched_id: Uuid,
        similarity: f64,
        action: String,
        source_url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },

    ExtractionDroppedNoDate {
        title: String,
        source_url: String,
    },

    ReviewVerdictReached {
        signal_id: Uuid,
        old_status: String,
        new_status: String,
        reason: String,
    },

    ImpliedQueriesConsumed {
        signal_ids: Vec<Uuid>,
    },

    // -----------------------------------------------------------------------
    // Sensitivity classification (NEW — moved from discovery events)
    // -----------------------------------------------------------------------
    SensitivityClassified {
        signal_id: Uuid,
        level: SensitivityLevel,
    },

    // TODO: wire producer when extraction pipeline supports tone classification
    ToneClassified {
        signal_id: Uuid,
        tone: Tone,
    },

    // TODO: wire producer when extraction pipeline supports severity classification
    SeverityClassified {
        signal_id: Uuid,
        severity: Severity,
    },

    // TODO: wire producer when extraction pipeline supports urgency classification
    UrgencyClassified {
        signal_id: Uuid,
        urgency: Urgency,
    },

    /// Thematic domain classification for any signal type.
    CategoryClassified {
        signal_id: Uuid,
        category: String,
    },

    ImpliedQueriesExtracted {
        signal_id: Uuid,
        queries: Vec<String>,
    },

    // -----------------------------------------------------------------------
    // Corroboration — system judgment that two sources are about the same thing
    // -----------------------------------------------------------------------
    ObservationCorroborated {
        signal_id: Uuid,
        node_type: NodeType,
        #[serde(alias = "new_source_url")]
        new_url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },

    // -----------------------------------------------------------------------
    // Actor identification — system extraction, not a world event
    // -----------------------------------------------------------------------
    ActorIdentified {
        actor_id: Uuid,
        name: String,
        actor_type: ActorType,
        canonical_key: String,
        domains: Vec<String>,
        social_urls: Vec<String>,
        description: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bio: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        location_lat: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        location_lng: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        location_name: Option<String>,
    },

    ActorLinkedToSignal {
        actor_id: Uuid,
        signal_id: Uuid,
        role: String,
    },

    ActorLocationIdentified {
        actor_id: Uuid,
        location_lat: f64,
        location_lng: f64,
        location_name: Option<String>,
    },

    ActorProfileEnriched {
        actor_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bio: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        external_url: Option<String>,
    },

    // -----------------------------------------------------------------------
    // Relationship linking — system judgments about signal relationships
    // -----------------------------------------------------------------------
    ResponseLinked {
        signal_id: Uuid,
        concern_id: Uuid,
        strength: f64,
        explanation: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_url: Option<String>,
    },

    ConcernLinked {
        signal_id: Uuid,
        concern_id: Uuid,
        strength: f64,
        explanation: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_url: Option<String>,
    },

    // -----------------------------------------------------------------------
    // Correction decisions
    // -----------------------------------------------------------------------
    GatheringCorrected {
        signal_id: Uuid,
        correction: GatheringCorrection,
        reason: String,
    },

    ResourceCorrected {
        signal_id: Uuid,
        correction: ResourceCorrection,
        reason: String,
    },

    HelpRequestCorrected {
        signal_id: Uuid,
        correction: HelpRequestCorrection,
        reason: String,
    },

    AnnouncementCorrected {
        signal_id: Uuid,
        correction: AnnouncementCorrection,
        reason: String,
    },

    ConcernCorrected {
        signal_id: Uuid,
        correction: ConcernCorrection,
        reason: String,
    },

    ConditionCorrected {
        signal_id: Uuid,
        correction: ConditionCorrection,
        reason: String,
    },

    // -----------------------------------------------------------------------
    // Actor decisions
    // -----------------------------------------------------------------------
    DuplicateActorsMerged {
        kept_id: Uuid,
        merged_ids: Vec<Uuid>,
    },

    OrphanedActorsCleaned {
        actor_ids: Vec<Uuid>,
    },

    // -----------------------------------------------------------------------
    // Situation decisions
    // -----------------------------------------------------------------------
    SituationIdentified {
        situation_id: Uuid,
        headline: String,
        lede: String,
        arc: SituationArc,
        temperature: f64,
        centroid_lat: Option<f64>,
        centroid_lng: Option<f64>,
        location_name: Option<String>,
        sensitivity: SensitivityLevel,
        category: Option<String>,
        structured_state: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tension_heat: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        clarity: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signal_count: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        narrative_embedding: Option<Vec<f32>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        causal_embedding: Option<Vec<f32>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        briefing_body: Option<String>,
    },

    SituationChanged {
        situation_id: Uuid,
        change: SituationChange,
    },

    SituationPromoted {
        situation_ids: Vec<Uuid>,
    },

    DispatchCreated {
        dispatch_id: Uuid,
        situation_id: Uuid,
        body: String,
        signal_ids: Vec<Uuid>,
        dispatch_type: DispatchType,
        supersedes: Option<Uuid>,
        fidelity_score: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flagged_for_review: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flag_reason: Option<String>,
    },

    SignalAssignedToSituation {
        signal_id: Uuid,
        situation_id: Uuid,
        signal_label: String,
        confidence: f64,
        reasoning: String,
    },

    SituationTagsAggregated {
        situation_id: Uuid,
        tag_slugs: Vec<String>,
    },

    DispatchFlaggedForReview {
        dispatch_id: Uuid,
        reason: String,
    },

    SignalsPendingWeaving {
        signal_ids: Vec<Uuid>,
        scout_run_id: String,
    },

    GroupCreated {
        group_id: Uuid,
        label: String,
        queries: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seed_signal_id: Option<Uuid>,
    },

    SignalAddedToGroup {
        signal_id: Uuid,
        group_id: Uuid,
        confidence: f64,
    },

    GroupQueriesRefined {
        group_id: Uuid,
        queries: Vec<String>,
    },

    GroupWovenIntoSituation {
        group_id: Uuid,
        situation_id: Uuid,
    },

    // -----------------------------------------------------------------------
    // Tag decisions
    // -----------------------------------------------------------------------
    SignalTagged {
        signal_id: Uuid,
        tag_slugs: Vec<String>,
    },

    TagSuppressed {
        situation_id: Uuid,
        tag_slug: String,
    },

    TagsMerged {
        source_slug: String,
        target_slug: String,
    },

    // -----------------------------------------------------------------------
    // Quality / lint decisions
    // -----------------------------------------------------------------------
    EmptyEntitiesCleaned {
        signal_ids: Vec<Uuid>,
    },

    FakeCoordinatesNulled {
        signal_ids: Vec<Uuid>,
        old_coords: Vec<(f64, f64)>,
    },

    OrphanedCitationsCleaned {
        citation_ids: Vec<Uuid>,
    },

    // -----------------------------------------------------------------------
    // Source system changes (editorial, not world fact)
    // -----------------------------------------------------------------------
    SourceSystemChanged {
        source_id: Uuid,
        canonical_key: String,
        change: SystemSourceChange,
    },

    // -----------------------------------------------------------------------
    // Source registry — Root Signal's source management
    // -----------------------------------------------------------------------
    SourcesRegistered {
        sources: Vec<SourceNode>,
    },

    SourceChanged {
        source_id: Uuid,
        canonical_key: String,
        change: SourceChange,
    },

    SourceDeactivated {
        source_ids: Vec<Uuid>,
        reason: String,
    },

    /// Admin-initiated removal of all signals produced by a source.
    SourceSignalsCleared {
        source_id: Uuid,
        canonical_key: String,
    },

    /// Admin-initiated source deletion — removes the source and all edges.
    SourceDeleted {
        source_id: Uuid,
        canonical_key: String,
    },

    // -----------------------------------------------------------------------
    // App user actions
    // -----------------------------------------------------------------------
    PinCreated {
        pin_id: Uuid,
        location_lat: f64,
        location_lng: f64,
        source_id: Uuid,
        created_by: String,
    },

    PinsConsumed {
        pin_ids: Vec<Uuid>,
    },

    DemandReceived {
        demand_id: Uuid,
        query: String,
        center_lat: f64,
        center_lng: f64,
        radius_km: f64,
    },

    SubmissionReceived {
        submission_id: Uuid,
        url: String,
        reason: Option<String>,
        source_canonical_key: Option<String>,
    },

    // -----------------------------------------------------------------------
    // Response scouting
    // -----------------------------------------------------------------------
    ResponseScouted {
        concern_id: Uuid,
        scouted_at: DateTime<Utc>,
    },

    // -----------------------------------------------------------------------
    // Query embedding storage
    // -----------------------------------------------------------------------
    QueryEmbeddingStored {
        canonical_key: String,
        embedding: Vec<f32>,
    },

    // -----------------------------------------------------------------------
    // Situation curiosity
    // -----------------------------------------------------------------------
    CuriosityTriggered {
        situation_id: Uuid,
        signal_ids: Vec<Uuid>,
    },

    // -----------------------------------------------------------------------
    // System curiosity
    // -----------------------------------------------------------------------
    ExpansionQueryCollected {
        query: String,
        source_url: String,
    },

    // -----------------------------------------------------------------------
    // Source scrape telemetry (event-sourced, not a direct GraphStore write)
    // -----------------------------------------------------------------------
    SourceScraped {
        canonical_key: String,
        signals_produced: u32,
        scraped_at: DateTime<Utc>,
    },

    /// Credit a source for discovering child sources via link promotion.
    SourceDiscoveryCredit {
        canonical_key: String,
        sources_discovered: u32,
    },

    // -----------------------------------------------------------------------
    // Investigation & curiosity bookkeeping
    // -----------------------------------------------------------------------
    SignalInvestigated {
        signal_id: Uuid,
        node_type: NodeType,
        investigated_at: DateTime<Utc>,
    },

    ExhaustedRetriesPromoted {
        promoted_at: DateTime<Utc>,
    },

    ConcernLinkerOutcomeRecorded {
        signal_id: Uuid,
        label: String,
        outcome: String,
        increment_retry: bool,
    },

    GatheringScouted {
        concern_id: Uuid,
        found_gatherings: bool,
        scouted_at: DateTime<Utc>,
    },

    // -----------------------------------------------------------------------
    // Location geocoding — deterministic coordinates from Mapbox
    // -----------------------------------------------------------------------
    LocationGeocoded {
        signal_id: Uuid,
        location_name: String,
        lat: f64,
        lng: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        address: Option<String>,
        precision: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timezone: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        city: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        country_code: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        country_name: Option<String>,
    },

    // -----------------------------------------------------------------------
    // Region auto-discovery — created from Mapbox geographic hierarchy
    // -----------------------------------------------------------------------
    RegionDiscovered {
        region_id: Uuid,
        name: String,
        center_lat: f64,
        center_lng: f64,
        radius_km: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        city: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        country_code: Option<String>,
        /// Geographic scale: "city", "state", or "country"
        scale: String,
        /// ID of the parent region (state for city, country for state)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_region_id: Option<Uuid>,
    },

    // -----------------------------------------------------------------------
    // Place & gathering geography
    // -----------------------------------------------------------------------
    PlaceDiscovered {
        place_id: Uuid,
        name: String,
        slug: String,
        lat: f64,
        lng: f64,
        discovered_at: DateTime<Utc>,
    },

    GathersAtPlaceLinked {
        signal_id: Uuid,
        place_slug: String,
    },

    // -----------------------------------------------------------------------
    // Tension deduplication
    // -----------------------------------------------------------------------
    DuplicateConcernMerged {
        survivor_id: Uuid,
        duplicate_id: Uuid,
    },

    // -----------------------------------------------------------------------
    // Source weight adjustments
    // -----------------------------------------------------------------------
    SourcesBoostedForSituation {
        headline: String,
        factor: f64,
    },

    // -----------------------------------------------------------------------
    // Supervisor analytics — computed scores and detected patterns
    // -----------------------------------------------------------------------
    EchoScored {
        situation_id: Uuid,
        echo_score: f64,
    },

    CauseHeatComputed {
        scores: Vec<CauseHeatScore>,
    },

    SignalDiversityComputed {
        metrics: Vec<SignalDiversityScore>,
    },

    ActorStatsComputed {
        stats: Vec<ActorStatScore>,
    },

    SimilarityEdgesRebuilt {
        edges: Vec<SimilarityEdge>,
    },

    // -----------------------------------------------------------------------
    // Admin actions
    // -----------------------------------------------------------------------
    ValidationIssueDismissed {
        issue_id: String,
    },
}

impl SystemEvent {
    /// Deserialize a system event from a JSON payload.
    pub fn from_payload(payload: &serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(payload.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn serde_round_trip(event: &SystemEvent) -> SystemEvent {
        let json = serde_json::to_value(event).expect("serialize");
        serde_json::from_value(json).expect("deserialize")
    }

    #[test]
    fn group_created_round_trips_with_seed() {
        let event = SystemEvent::GroupCreated {
            group_id: Uuid::new_v4(),
            label: "Housing affordability".into(),
            queries: vec!["rent increase".into(), "eviction notice".into()],
            seed_signal_id: Some(Uuid::new_v4()),
        };
        let parsed = serde_round_trip(&event);
        match parsed {
            SystemEvent::GroupCreated { label, queries, seed_signal_id, .. } => {
                assert_eq!(label, "Housing affordability");
                assert_eq!(queries.len(), 2);
                assert!(seed_signal_id.is_some());
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn group_created_round_trips_without_seed() {
        let event = SystemEvent::GroupCreated {
            group_id: Uuid::new_v4(),
            label: "Transit disruptions".into(),
            queries: vec!["bus route change".into()],
            seed_signal_id: None,
        };
        let parsed = serde_round_trip(&event);
        match parsed {
            SystemEvent::GroupCreated { seed_signal_id, .. } => {
                assert!(seed_signal_id.is_none());
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn group_created_omits_seed_signal_id_when_none() {
        let event = SystemEvent::GroupCreated {
            group_id: Uuid::new_v4(),
            label: "test".into(),
            queries: vec![],
            seed_signal_id: None,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert!(
            json.get("seed_signal_id").is_none(),
            "seed_signal_id should be omitted when None (skip_serializing_if)"
        );
    }

    #[test]
    fn group_created_with_empty_queries_round_trips() {
        let event = SystemEvent::GroupCreated {
            group_id: Uuid::new_v4(),
            label: "".into(),
            queries: vec![],
            seed_signal_id: None,
        };
        let parsed = serde_round_trip(&event);
        match parsed {
            SystemEvent::GroupCreated { queries, .. } => {
                assert!(queries.is_empty());
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn signal_added_to_group_round_trips() {
        let event = SystemEvent::SignalAddedToGroup {
            signal_id: Uuid::new_v4(),
            group_id: Uuid::new_v4(),
            confidence: 0.92,
        };
        let parsed = serde_round_trip(&event);
        match parsed {
            SystemEvent::SignalAddedToGroup { confidence, .. } => {
                assert!((confidence - 0.92).abs() < f64::EPSILON);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn signal_added_to_group_boundary_confidences() {
        for conf in [0.0, 1.0, 0.5] {
            let event = SystemEvent::SignalAddedToGroup {
                signal_id: Uuid::new_v4(),
                group_id: Uuid::new_v4(),
                confidence: conf,
            };
            let parsed = serde_round_trip(&event);
            match parsed {
                SystemEvent::SignalAddedToGroup { confidence, .. } => {
                    assert!((confidence - conf).abs() < f64::EPSILON);
                }
                _ => panic!("Wrong variant"),
            }
        }
    }

    #[test]
    fn group_queries_refined_round_trips() {
        let event = SystemEvent::GroupQueriesRefined {
            group_id: Uuid::new_v4(),
            queries: vec!["updated query".into(), "second query".into()],
        };
        let parsed = serde_round_trip(&event);
        match parsed {
            SystemEvent::GroupQueriesRefined { queries, .. } => {
                assert_eq!(queries, vec!["updated query", "second query"]);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn group_queries_refined_empty_queries_round_trips() {
        let event = SystemEvent::GroupQueriesRefined {
            group_id: Uuid::new_v4(),
            queries: vec![],
        };
        let parsed = serde_round_trip(&event);
        match parsed {
            SystemEvent::GroupQueriesRefined { queries, .. } => {
                assert!(queries.is_empty());
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn group_created_deserializes_from_legacy_without_seed() {
        let json = serde_json::json!({
            "type": "group_created",
            "group_id": Uuid::new_v4().to_string(),
            "label": "test",
            "queries": ["q1"]
        });
        let parsed: SystemEvent = serde_json::from_value(json).unwrap();
        match parsed {
            SystemEvent::GroupCreated { seed_signal_id, .. } => {
                assert!(seed_signal_id.is_none(), "Missing seed_signal_id should default to None");
            }
            _ => panic!("Wrong variant"),
        }
    }
}

