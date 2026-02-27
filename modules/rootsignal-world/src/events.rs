//! Layer 1: World Facts — the golden thread.
//!
//! Every variant describes something observed in the world. No system opinions,
//! no derived metrics, no operational telemetry. These events are the archival
//! record that can be replayed independently.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::eventlike::Eventlike;
use crate::types::{ActorType, ChannelType, NodeType, Severity, Urgency};
use crate::values::{Location, Schedule};

/// A world fact — something observed in reality, independent of Root Signal's
/// editorial decisions or operational metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorldEvent {
    // -----------------------------------------------------------------------
    // Discovery facts — 5 typed variants
    // No `sensitivity` (that's a system classification) or `implied_queries`
    // (that's a system expansion artifact).
    // -----------------------------------------------------------------------
    GatheringDiscovered {
        id: Uuid,
        title: String,
        summary: String,
        confidence: f32,
        source_url: String,
        extracted_at: DateTime<Utc>,
        published_at: Option<DateTime<Utc>>,
        location: Option<Location>,
        from_location: Option<Location>,
        mentioned_actors: Vec<String>,
        author_actor: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        schedule: Option<Schedule>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action_url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        organizer: Option<String>,
    },

    AidDiscovered {
        id: Uuid,
        title: String,
        summary: String,
        confidence: f32,
        source_url: String,
        extracted_at: DateTime<Utc>,
        published_at: Option<DateTime<Utc>>,
        location: Option<Location>,
        from_location: Option<Location>,
        mentioned_actors: Vec<String>,
        author_actor: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action_url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        availability: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_ongoing: Option<bool>,
    },

    NeedDiscovered {
        id: Uuid,
        title: String,
        summary: String,
        confidence: f32,
        source_url: String,
        extracted_at: DateTime<Utc>,
        published_at: Option<DateTime<Utc>>,
        location: Option<Location>,
        from_location: Option<Location>,
        mentioned_actors: Vec<String>,
        author_actor: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        urgency: Option<Urgency>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        what_needed: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        goal: Option<String>,
    },

    NoticeDiscovered {
        id: Uuid,
        title: String,
        summary: String,
        confidence: f32,
        source_url: String,
        extracted_at: DateTime<Utc>,
        published_at: Option<DateTime<Utc>>,
        location: Option<Location>,
        from_location: Option<Location>,
        mentioned_actors: Vec<String>,
        author_actor: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        severity: Option<Severity>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        category: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effective_date: Option<DateTime<Utc>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_authority: Option<String>,
    },

    TensionDiscovered {
        id: Uuid,
        title: String,
        summary: String,
        confidence: f32,
        source_url: String,
        extracted_at: DateTime<Utc>,
        published_at: Option<DateTime<Utc>>,
        location: Option<Location>,
        from_location: Option<Location>,
        mentioned_actors: Vec<String>,
        author_actor: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        severity: Option<Severity>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        what_would_help: Option<String>,
    },

    // -----------------------------------------------------------------------
    // Corroboration — the world fact only (no similarity score or count)
    // -----------------------------------------------------------------------
    ObservationCorroborated {
        signal_id: Uuid,
        node_type: NodeType,
        new_source_url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },

    // -----------------------------------------------------------------------
    // Citations
    // -----------------------------------------------------------------------
    CitationRecorded {
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
    // Actors — no `discovery_depth` (system metric)
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
    // Relationship edges
    // -----------------------------------------------------------------------
    ResourceEdgeCreated {
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
    // Resource identification — replay-safe resource creation
    // -----------------------------------------------------------------------
    ResourceIdentified {
        resource_id: Uuid,
        name: String,
        slug: String,
        description: String,
    },
}

impl Eventlike for WorldEvent {
    fn event_type(&self) -> &'static str {
        match self {
            WorldEvent::GatheringDiscovered { .. } => "gathering_discovered",
            WorldEvent::AidDiscovered { .. } => "aid_discovered",
            WorldEvent::NeedDiscovered { .. } => "need_discovered",
            WorldEvent::NoticeDiscovered { .. } => "notice_discovered",
            WorldEvent::TensionDiscovered { .. } => "tension_discovered",
            WorldEvent::ObservationCorroborated { .. } => "observation_corroborated",
            WorldEvent::CitationRecorded { .. } => "citation_recorded",
            WorldEvent::ActorIdentified { .. } => "actor_identified",
            WorldEvent::ActorLinkedToSignal { .. } => "actor_linked_to_signal",
            WorldEvent::ActorLocationIdentified { .. } => "actor_location_identified",
            WorldEvent::ResourceEdgeCreated { .. } => "resource_edge_created",
            WorldEvent::ResponseLinked { .. } => "response_linked",
            WorldEvent::TensionLinked { .. } => "tension_linked",
            WorldEvent::ResourceIdentified { .. } => "resource_identified",
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
