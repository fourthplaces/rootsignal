//! Layer 2: System Events — the editorial layer.
//!
//! Every variant describes a decision Root Signal made about world facts:
//! scoring, correcting, classifying, expiring, clustering. These can evolve
//! rapidly without changing the archival world record.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use rootsignal_world::Eventlike;

use crate::events::{
    AnnouncementCorrection, ConcernCorrection, GatheringCorrection, HelpRequestCorrection,
    ResourceCorrection, SituationChange, SourceChange, SystemSourceChange,
};
use crate::safety::SensitivityLevel;
use crate::types::{
    ActorType, DiscoveryMethod, DispatchType, NodeType, Severity, SituationArc, SourceRole, Tone,
    Urgency,
};

/// A system event — an editorial judgment Root Signal made about world facts.
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

    /// Soft-delete — sets `expired = true` on the node (no fact disappears).
    EntityExpired {
        signal_id: Uuid,
        node_type: NodeType,
        reason: String,
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
        new_source_url: String,
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

    // -----------------------------------------------------------------------
    // Relationship linking — system judgments about signal relationships
    // -----------------------------------------------------------------------
    ResponseLinked {
        signal_id: Uuid,
        tension_id: Uuid,
        strength: f64,
        explanation: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_url: Option<String>,
    },

    TensionLinked {
        signal_id: Uuid,
        tension_id: Uuid,
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

    #[serde(alias = "aid_corrected")]
    ResourceCorrected {
        signal_id: Uuid,
        correction: ResourceCorrection,
        reason: String,
    },

    #[serde(alias = "need_corrected")]
    HelpRequestCorrected {
        signal_id: Uuid,
        correction: HelpRequestCorrection,
        reason: String,
    },

    #[serde(alias = "notice_corrected")]
    AnnouncementCorrected {
        signal_id: Uuid,
        correction: AnnouncementCorrection,
        reason: String,
    },

    #[serde(alias = "tension_corrected")]
    ConcernCorrected {
        signal_id: Uuid,
        correction: ConcernCorrection,
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
    SourceRegistered {
        source_id: Uuid,
        canonical_key: String,
        canonical_value: String,
        url: Option<String>,
        discovery_method: DiscoveryMethod,
        weight: f64,
        source_role: SourceRole,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gap_context: Option<String>,
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
    // System curiosity
    // -----------------------------------------------------------------------
    ExpansionQueryCollected {
        query: String,
        source_url: String,
    },

    // -----------------------------------------------------------------------
    // Source scrape telemetry (event-sourced, not a direct GraphWriter write)
    // -----------------------------------------------------------------------
    SourceScraped {
        canonical_key: String,
        signals_produced: u32,
        scraped_at: DateTime<Utc>,
    },
}

impl Eventlike for SystemEvent {
    fn event_type(&self) -> &'static str {
        match self {
            SystemEvent::FreshnessConfirmed { .. } => "freshness_confirmed",
            SystemEvent::ConfidenceScored { .. } => "confidence_scored",
            SystemEvent::CorroborationScored { .. } => "corroboration_scored",
            SystemEvent::ObservationRejected { .. } => "observation_rejected",
            SystemEvent::EntityExpired { .. } => "entity_expired",
            SystemEvent::EntityPurged { .. } => "entity_purged",
            SystemEvent::DuplicateDetected { .. } => "duplicate_detected",
            SystemEvent::ExtractionDroppedNoDate { .. } => "extraction_dropped_no_date",
            SystemEvent::ReviewVerdictReached { .. } => "review_verdict_reached",
            SystemEvent::ImpliedQueriesConsumed { .. } => "implied_queries_consumed",
            SystemEvent::SensitivityClassified { .. } => "sensitivity_classified",
            SystemEvent::ToneClassified { .. } => "tone_classified",
            SystemEvent::SeverityClassified { .. } => "severity_classified",
            SystemEvent::UrgencyClassified { .. } => "urgency_classified",
            SystemEvent::ImpliedQueriesExtracted { .. } => "implied_queries_extracted",
            SystemEvent::ObservationCorroborated { .. } => "observation_corroborated",
            SystemEvent::ActorIdentified { .. } => "actor_identified",
            SystemEvent::ActorLinkedToSignal { .. } => "actor_linked_to_signal",
            SystemEvent::ActorLocationIdentified { .. } => "actor_location_identified",
            SystemEvent::ResponseLinked { .. } => "response_linked",
            SystemEvent::TensionLinked { .. } => "tension_linked",
            SystemEvent::GatheringCorrected { .. } => "gathering_corrected",
            SystemEvent::ResourceCorrected { .. } => "resource_corrected",
            SystemEvent::HelpRequestCorrected { .. } => "help_request_corrected",
            SystemEvent::AnnouncementCorrected { .. } => "announcement_corrected",
            SystemEvent::ConcernCorrected { .. } => "concern_corrected",
            SystemEvent::DuplicateActorsMerged { .. } => "duplicate_actors_merged",
            SystemEvent::OrphanedActorsCleaned { .. } => "orphaned_actors_cleaned",
            SystemEvent::SituationIdentified { .. } => "situation_identified",
            SystemEvent::SituationChanged { .. } => "situation_changed",
            SystemEvent::SituationPromoted { .. } => "situation_promoted",
            SystemEvent::DispatchCreated { .. } => "dispatch_created",
            SystemEvent::SignalTagged { .. } => "signal_tagged",
            SystemEvent::TagSuppressed { .. } => "tag_suppressed",
            SystemEvent::TagsMerged { .. } => "tags_merged",
            SystemEvent::EmptyEntitiesCleaned { .. } => "empty_entities_cleaned",
            SystemEvent::FakeCoordinatesNulled { .. } => "fake_coordinates_nulled",
            SystemEvent::OrphanedCitationsCleaned { .. } => "orphaned_citations_cleaned",
            SystemEvent::SourceSystemChanged { .. } => "source_system_changed",
            SystemEvent::SourceRegistered { .. } => "source_registered",
            SystemEvent::SourceChanged { .. } => "source_changed",
            SystemEvent::SourceDeactivated { .. } => "source_deactivated",
            SystemEvent::PinCreated { .. } => "pin_created",
            SystemEvent::PinsConsumed { .. } => "pins_consumed",
            SystemEvent::DemandReceived { .. } => "demand_received",
            SystemEvent::SubmissionReceived { .. } => "submission_received",
            SystemEvent::ExpansionQueryCollected { .. } => "expansion_query_collected",
            SystemEvent::SourceScraped { .. } => "source_scraped",
        }
    }

    fn to_payload(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("SystemEvent serialization should never fail")
    }
}

impl SystemEvent {
    /// Deserialize a system event from a JSON payload.
    pub fn from_payload(payload: &serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(payload.clone())
    }
}
