//! Layer 2: System Decisions — the editorial layer.
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
use crate::types::{DispatchType, NodeType, SituationArc};

/// A system decision — an editorial judgment Root Signal made about world facts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SystemDecision {
    // -----------------------------------------------------------------------
    // Signal lifecycle decisions
    // -----------------------------------------------------------------------
    FreshnessConfirmed {
        entity_ids: Vec<Uuid>,
        node_type: NodeType,
        confirmed_at: DateTime<Utc>,
    },

    ConfidenceScored {
        entity_id: Uuid,
        old_confidence: f32,
        new_confidence: f32,
    },

    /// Split from ObservationCorroborated — the system's assessment of corroboration.
    CorroborationScored {
        entity_id: Uuid,
        similarity: f64,
        new_corroboration_count: u32,
    },

    ObservationRejected {
        entity_id: Option<Uuid>,
        title: String,
        source_url: String,
        reason: String,
    },

    /// Soft-delete — sets `expired = true` on the node (no fact disappears).
    EntityExpired {
        entity_id: Uuid,
        node_type: NodeType,
        reason: String,
    },

    EntityPurged {
        entity_id: Uuid,
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
        entity_id: Uuid,
        old_status: String,
        new_status: String,
        reason: String,
    },

    ImpliedQueriesConsumed {
        entity_ids: Vec<Uuid>,
    },

    // -----------------------------------------------------------------------
    // Sensitivity classification (NEW — moved from discovery events)
    // -----------------------------------------------------------------------
    SensitivityClassified {
        entity_id: Uuid,
        level: SensitivityLevel,
    },

    // -----------------------------------------------------------------------
    // Correction decisions
    // -----------------------------------------------------------------------
    GatheringCorrected {
        entity_id: Uuid,
        correction: GatheringCorrection,
        reason: String,
    },

    AidCorrected {
        entity_id: Uuid,
        correction: AidCorrection,
        reason: String,
    },

    NeedCorrected {
        entity_id: Uuid,
        correction: NeedCorrection,
        reason: String,
    },

    NoticeCorrected {
        entity_id: Uuid,
        correction: NoticeCorrection,
        reason: String,
    },

    TensionCorrected {
        entity_id: Uuid,
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
        entity_ids: Vec<Uuid>,
        dispatch_type: DispatchType,
        supersedes: Option<Uuid>,
        fidelity_score: Option<f64>,
    },

    // -----------------------------------------------------------------------
    // Tag decisions
    // -----------------------------------------------------------------------
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
        entity_ids: Vec<Uuid>,
    },

    FakeCoordinatesNulled {
        entity_ids: Vec<Uuid>,
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
}

impl Eventlike for SystemDecision {
    fn event_type(&self) -> &'static str {
        match self {
            SystemDecision::FreshnessConfirmed { .. } => "freshness_confirmed",
            SystemDecision::ConfidenceScored { .. } => "confidence_scored",
            SystemDecision::CorroborationScored { .. } => "corroboration_scored",
            SystemDecision::ObservationRejected { .. } => "observation_rejected",
            SystemDecision::EntityExpired { .. } => "entity_expired",
            SystemDecision::EntityPurged { .. } => "entity_purged",
            SystemDecision::DuplicateDetected { .. } => "duplicate_detected",
            SystemDecision::ExtractionDroppedNoDate { .. } => "extraction_dropped_no_date",
            SystemDecision::ReviewVerdictReached { .. } => "review_verdict_reached",
            SystemDecision::ImpliedQueriesConsumed { .. } => "implied_queries_consumed",
            SystemDecision::SensitivityClassified { .. } => "sensitivity_classified",
            SystemDecision::GatheringCorrected { .. } => "gathering_corrected",
            SystemDecision::AidCorrected { .. } => "aid_corrected",
            SystemDecision::NeedCorrected { .. } => "need_corrected",
            SystemDecision::NoticeCorrected { .. } => "notice_corrected",
            SystemDecision::TensionCorrected { .. } => "tension_corrected",
            SystemDecision::DuplicateActorsMerged { .. } => "duplicate_actors_merged",
            SystemDecision::OrphanedActorsCleaned { .. } => "orphaned_actors_cleaned",
            SystemDecision::SituationIdentified { .. } => "situation_identified",
            SystemDecision::SituationChanged { .. } => "situation_changed",
            SystemDecision::SituationPromoted { .. } => "situation_promoted",
            SystemDecision::DispatchCreated { .. } => "dispatch_created",
            SystemDecision::TagSuppressed { .. } => "tag_suppressed",
            SystemDecision::TagsMerged { .. } => "tags_merged",
            SystemDecision::EmptyEntitiesCleaned { .. } => "empty_entities_cleaned",
            SystemDecision::FakeCoordinatesNulled { .. } => "fake_coordinates_nulled",
            SystemDecision::OrphanedCitationsCleaned { .. } => "orphaned_citations_cleaned",
            SystemDecision::SourceSystemChanged { .. } => "source_system_changed",
        }
    }

    fn to_payload(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("SystemDecision serialization should never fail")
    }
}

impl SystemDecision {
    /// Deserialize a system decision from a JSON payload.
    pub fn from_payload(payload: &serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(payload.clone())
    }
}
