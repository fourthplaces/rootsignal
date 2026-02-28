//! Layer 1: World Facts — the golden thread.
//!
//! Every variant describes something observed in the world. No system opinions,
//! no derived metrics, no operational telemetry. These events are the archival
//! record that can be replayed independently.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::eventlike::Eventlike;
use crate::types::{ChannelType, Entity, Reference};
use crate::values::{Location, Schedule};

/// A world fact — something observed in reality, independent of Root Signal's
/// editorial decisions or operational metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorldEvent {
    // -----------------------------------------------------------------------
    // 7 signal types — the shared base + type-specific fields
    // -----------------------------------------------------------------------

    /// People are coming together at a time and place.
    #[serde(alias = "gathering_discovered")]
    GatheringAnnounced {
        id: Uuid,
        title: String,
        summary: String,
        source_url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        published_at: Option<DateTime<Utc>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extraction_id: Option<Uuid>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        locations: Vec<Location>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        mentioned_entities: Vec<Entity>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        references: Vec<Reference>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        schedule: Option<Schedule>,
        // -- type-specific --
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action_url: Option<String>,
    },

    /// Something is being made available to the community.
    #[serde(alias = "aid_discovered")]
    ResourceOffered {
        id: Uuid,
        title: String,
        summary: String,
        source_url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        published_at: Option<DateTime<Utc>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extraction_id: Option<Uuid>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        locations: Vec<Location>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        mentioned_entities: Vec<Entity>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        references: Vec<Reference>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        schedule: Option<Schedule>,
        // -- type-specific --
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action_url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        availability: Option<String>,
    },

    /// Someone needs something.
    #[serde(alias = "need_discovered")]
    HelpRequested {
        id: Uuid,
        title: String,
        summary: String,
        source_url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        published_at: Option<DateTime<Utc>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extraction_id: Option<Uuid>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        locations: Vec<Location>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        mentioned_entities: Vec<Entity>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        references: Vec<Reference>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        schedule: Option<Schedule>,
        // -- type-specific --
        #[serde(default, skip_serializing_if = "Option::is_none")]
        what_needed: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        goal: Option<String>,
    },

    /// Information was shared with the community.
    #[serde(alias = "notice_discovered")]
    AnnouncementShared {
        id: Uuid,
        title: String,
        summary: String,
        source_url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        published_at: Option<DateTime<Utc>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extraction_id: Option<Uuid>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        locations: Vec<Location>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        mentioned_entities: Vec<Entity>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        references: Vec<Reference>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        schedule: Option<Schedule>,
        // -- type-specific --
        #[serde(default, skip_serializing_if = "Option::is_none")]
        category: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effective_date: Option<DateTime<Utc>>,
    },

    /// Someone voiced a concern. Always a human act.
    #[serde(alias = "tension_discovered")]
    ConcernRaised {
        id: Uuid,
        title: String,
        summary: String,
        source_url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        published_at: Option<DateTime<Utc>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extraction_id: Option<Uuid>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        locations: Vec<Location>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        mentioned_entities: Vec<Entity>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        references: Vec<Reference>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        schedule: Option<Schedule>,
        // -- type-specific --
        #[serde(default, skip_serializing_if = "Option::is_none")]
        what_would_help: Option<String>,
    },

    /// A state of the world was measured or recorded.
    /// Reducer and graph wiring complete — awaiting extractor support to produce these.
    ConditionObserved {
        id: Uuid,
        title: String,
        summary: String,
        source_url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        published_at: Option<DateTime<Utc>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extraction_id: Option<Uuid>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        locations: Vec<Location>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        mentioned_entities: Vec<Entity>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        references: Vec<Reference>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        schedule: Option<Schedule>,
    },

    /// A discrete event occurred in the world. States persist, incidents happen.
    /// Reducer and graph wiring complete — awaiting extractor support to produce these.
    IncidentReported {
        id: Uuid,
        title: String,
        summary: String,
        source_url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        published_at: Option<DateTime<Utc>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extraction_id: Option<Uuid>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        locations: Vec<Location>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        mentioned_entities: Vec<Entity>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        references: Vec<Reference>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        schedule: Option<Schedule>,
    },

    // -----------------------------------------------------------------------
    // Citations
    // -----------------------------------------------------------------------
    #[serde(alias = "citation_recorded")]
    CitationPublished {
        citation_id: Uuid,
        signal_id: Uuid,
        url: String,
        content_hash: String,
        snippet: Option<String>,
        relevance: Option<String>,
        channel_type: Option<ChannelType>,
        evidence_confidence: Option<f32>,
    },

    // -----------------------------------------------------------------------
    // Resource edges — real-world resource relationships
    // -----------------------------------------------------------------------
    #[serde(alias = "resource_edge_created")]
    ResourceLinked {
        signal_id: Uuid,
        resource_slug: String,
        role: String,
        confidence: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        quantity: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notes: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capacity: Option<String>,
    },

    // -----------------------------------------------------------------------
    // Lifecycle events — the world changed, superseding a previous fact
    // -----------------------------------------------------------------------

    GatheringCancelled {
        signal_id: Uuid,
        reason: String,
        source_url: String,
    },

    ResourceDepleted {
        signal_id: Uuid,
        reason: String,
        source_url: String,
    },

    AnnouncementRetracted {
        signal_id: Uuid,
        reason: String,
        source_url: String,
    },

    CitationRetracted {
        citation_id: Uuid,
        reason: String,
        source_url: String,
    },

    DetailsChanged {
        signal_id: Uuid,
        summary: String,
        source_url: String,
    },

    // -----------------------------------------------------------------------
    // Resource identification — replay-safe resource creation
    // -----------------------------------------------------------------------
    ResourceIdentified {
        resource_id: Uuid,
        name: String,
        slug: String,
        description: String,
    },

    // -----------------------------------------------------------------------
    // Provenance edges — real-world relationships between entities and sources
    // -----------------------------------------------------------------------
    ActorLinkedToSource {
        actor_id: Uuid,
        source_id: Uuid,
    },

    SignalLinkedToSource {
        signal_id: Uuid,
        source_id: Uuid,
    },

    SourceLinkDiscovered {
        child_id: Uuid,
        parent_canonical_key: String,
    },
}

impl Eventlike for WorldEvent {
    fn event_type(&self) -> &'static str {
        match self {
            WorldEvent::GatheringAnnounced { .. } => "gathering_announced",
            WorldEvent::ResourceOffered { .. } => "resource_offered",
            WorldEvent::HelpRequested { .. } => "help_requested",
            WorldEvent::AnnouncementShared { .. } => "announcement_shared",
            WorldEvent::ConcernRaised { .. } => "concern_raised",
            WorldEvent::ConditionObserved { .. } => "condition_observed",
            WorldEvent::IncidentReported { .. } => "incident_reported",
            WorldEvent::CitationPublished { .. } => "citation_published",
            WorldEvent::ResourceLinked { .. } => "resource_linked",
            WorldEvent::GatheringCancelled { .. } => "gathering_cancelled",
            WorldEvent::ResourceDepleted { .. } => "resource_depleted",
            WorldEvent::AnnouncementRetracted { .. } => "announcement_retracted",
            WorldEvent::CitationRetracted { .. } => "citation_retracted",
            WorldEvent::DetailsChanged { .. } => "details_changed",
            WorldEvent::ResourceIdentified { .. } => "resource_identified",
            WorldEvent::ActorLinkedToSource { .. } => "actor_linked_to_source",
            WorldEvent::SignalLinkedToSource { .. } => "signal_linked_to_source",
            WorldEvent::SourceLinkDiscovered { .. } => "source_link_discovered",
        }
    }

    fn to_payload(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("WorldEvent serialization should never fail")
    }
}

impl WorldEvent {
    /// Deserialize a world event from a JSON payload.
    pub fn from_payload(payload: &serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(payload.clone())
    }
}
