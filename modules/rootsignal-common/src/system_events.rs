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
    AidCorrection, GatheringCorrection, NeedCorrection, NoticeCorrection, SituationChange,
    SourceChange, SystemSourceChange, TensionCorrection,
};
use crate::safety::SensitivityLevel;
use crate::types::{DiscoveryMethod, DispatchType, NodeType, SituationArc, SourceRole};

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

    ImpliedQueriesExtracted {
        signal_id: Uuid,
        queries: Vec<String>,
    },

    // -----------------------------------------------------------------------
    // Correction decisions
    // -----------------------------------------------------------------------
    GatheringCorrected {
        signal_id: Uuid,
        correction: GatheringCorrection,
        reason: String,
    },

    AidCorrected {
        signal_id: Uuid,
        correction: AidCorrection,
        reason: String,
    },

    NeedCorrected {
        signal_id: Uuid,
        correction: NeedCorrection,
        reason: String,
    },

    NoticeCorrected {
        signal_id: Uuid,
        correction: NoticeCorrection,
        reason: String,
    },

    TensionCorrected {
        signal_id: Uuid,
        correction: TensionCorrection,
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

    SourceLinkDiscovered {
        child_id: Uuid,
        parent_canonical_key: String,
    },

    // -----------------------------------------------------------------------
    // Actor-source links (links actor to a system entity)
    // -----------------------------------------------------------------------
    ActorLinkedToSource {
        actor_id: Uuid,
        source_id: Uuid,
    },

    SignalLinkedToSource {
        signal_id: Uuid,
        source_id: Uuid,
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
            SystemEvent::ImpliedQueriesExtracted { .. } => "implied_queries_extracted",
            SystemEvent::GatheringCorrected { .. } => "gathering_corrected",
            SystemEvent::AidCorrected { .. } => "aid_corrected",
            SystemEvent::NeedCorrected { .. } => "need_corrected",
            SystemEvent::NoticeCorrected { .. } => "notice_corrected",
            SystemEvent::TensionCorrected { .. } => "tension_corrected",
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
            SystemEvent::SourceLinkDiscovered { .. } => "source_link_discovered",
            SystemEvent::ActorLinkedToSource { .. } => "actor_linked_to_source",
            SystemEvent::SignalLinkedToSource { .. } => "signal_linked_to_source",
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
