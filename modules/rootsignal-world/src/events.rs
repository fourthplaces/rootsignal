//! Layer 1: World Facts — the golden thread.
//!
//! Every variant describes something observed in the world. No system opinions,
//! no derived metrics, no operational telemetry. These events are the archival
//! record that can be replayed independently.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::{ChannelType, Entity, NodeType, Reference};
use crate::values::{Location, Schedule};

/// A world fact — something observed in reality, independent of Root Signal's
/// editorial decisions or operational metrics.
#[causal_core_macros::event(prefix = "world")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorldEvent {
    // -----------------------------------------------------------------------
    // 6 signal types — the shared base + type-specific fields
    // -----------------------------------------------------------------------

    /// People are coming together at a time and place.
    GatheringAnnounced {
        id: Uuid,
        title: String,
        summary: String,
        #[serde(alias = "source_url")]
        url: String,
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
    ResourceOffered {
        id: Uuid,
        title: String,
        summary: String,
        #[serde(alias = "source_url")]
        url: String,
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
        /// Who is eligible as explicitly stated in the content.
        /// Null if not stated — do not infer eligibility from context.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        eligibility: Option<String>,
    },

    /// Someone needs something.
    HelpRequested {
        id: Uuid,
        title: String,
        summary: String,
        #[serde(alias = "source_url")]
        url: String,
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
        /// The goal as explicitly stated in the content.
        /// Extract what the content says, not what you infer the goal to be.
        #[serde(default, skip_serializing_if = "Option::is_none", alias = "goal")]
        stated_goal: Option<String>,
    },

    /// Pure information broadcast — the category of last resort.
    /// If the content contains a gathering, resource, need, condition,
    /// or concern, classify as that type instead. AnnouncementShared is
    /// for content that doesn't embed any of the other five.
    AnnouncementShared {
        id: Uuid,
        title: String,
        summary: String,
        #[serde(alias = "source_url")]
        url: String,
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
        /// The core subject in plain terms, for search/retrieval.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subject: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effective_date: Option<DateTime<Utc>>,
    },

    /// Someone expressed opposition, filed a grievance, or pushed back
    /// against something. The act of opposing is the fact being recorded —
    /// not the systemic tension it may point to (that's intelligence layer).
    ///
    /// This is NOT a catch-all for complaints about conditions. If the
    /// content describes a state of the world (pothole, pollution, outage),
    /// that's ConditionObserved. ConcernRaised is for social friction:
    /// opposition to proposals, disputes between groups, protests,
    /// objections filed, community pushback.
    ConcernRaised {
        id: Uuid,
        title: String,
        summary: String,
        #[serde(alias = "source_url")]
        url: String,
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
        // -- type-specific (strict extraction only) --
        /// The core subject of the friction in plain terms.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subject: Option<String>,
        /// What is being opposed, as explicitly stated in the content.
        /// Null if the content doesn't clearly state what's being opposed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        opposing: Option<String>,
    },

    /// A state of the world being described — infrastructure, environment,
    /// emergencies, public health, safety. Severity and urgency fields on the
    /// resulting signal node distinguish routine observations from acute events
    /// (which are functionally "incidents" — use filters, not types).
    ///
    /// All type-specific fields are strict extraction only: they capture what
    /// the source content explicitly states, not what the LLM infers.
    ConditionObserved {
        id: Uuid,
        title: String,
        summary: String,
        #[serde(alias = "source_url")]
        url: String,
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
        // -- type-specific (strict extraction only) --
        /// The core subject in plain terms, for search/retrieval.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subject: Option<String>,
        /// Who or what reported/observed this? Null if not stated.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        observed_by: Option<String>,
        /// Quantitative reading if the content includes one. Null if none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        measurement: Option<String>,
        /// Scope of what's affected as stated in the content. Null if not stated.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        affected_scope: Option<String>,
    },

    // -----------------------------------------------------------------------
    // Citations
    // -----------------------------------------------------------------------
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
        #[serde(alias = "source_url")]
        url: String,
    },

    ResourceDepleted {
        signal_id: Uuid,
        reason: String,
        #[serde(alias = "source_url")]
        url: String,
    },

    AnnouncementRetracted {
        signal_id: Uuid,
        reason: String,
        #[serde(alias = "source_url")]
        url: String,
    },

    CitationRetracted {
        citation_id: Uuid,
        reason: String,
        #[serde(alias = "source_url")]
        url: String,
    },

    DetailsChanged {
        signal_id: Uuid,
        node_type: NodeType,
        title: String,
        summary: String,
        #[serde(alias = "source_url")]
        url: String,
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

impl WorldEvent {
    /// Deserialize a world event from a JSON payload.
    pub fn from_payload(payload: &serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(payload.clone())
    }

    pub fn signal_id(&self) -> Option<Uuid> {
        match self {
            Self::GatheringAnnounced { id, .. }
            | Self::ResourceOffered { id, .. }
            | Self::HelpRequested { id, .. }
            | Self::AnnouncementShared { id, .. }
            | Self::ConcernRaised { id, .. }
            | Self::ConditionObserved { id, .. } => Some(*id),
            _ => None,
        }
    }

    pub fn title(&self) -> Option<&str> {
        match self {
            Self::GatheringAnnounced { title, .. }
            | Self::ResourceOffered { title, .. }
            | Self::HelpRequested { title, .. }
            | Self::AnnouncementShared { title, .. }
            | Self::ConcernRaised { title, .. }
            | Self::ConditionObserved { title, .. } => Some(title),
            _ => None,
        }
    }

    pub fn summary(&self) -> Option<&str> {
        match self {
            Self::GatheringAnnounced { summary, .. }
            | Self::ResourceOffered { summary, .. }
            | Self::HelpRequested { summary, .. }
            | Self::AnnouncementShared { summary, .. }
            | Self::ConcernRaised { summary, .. }
            | Self::ConditionObserved { summary, .. } => Some(summary),
            _ => None,
        }
    }

    pub fn url(&self) -> Option<&str> {
        match self {
            Self::GatheringAnnounced { url, .. }
            | Self::ResourceOffered { url, .. }
            | Self::HelpRequested { url, .. }
            | Self::AnnouncementShared { url, .. }
            | Self::ConcernRaised { url, .. }
            | Self::ConditionObserved { url, .. } => Some(url),
            _ => None,
        }
    }

    pub fn opposing(&self) -> Option<&str> {
        match self {
            Self::ConcernRaised { opposing, .. } => opposing.as_deref(),
            _ => None,
        }
    }

    pub fn is_signal(&self) -> bool {
        self.signal_id().is_some()
    }

    pub fn node_type(&self) -> Option<NodeType> {
        match self {
            Self::GatheringAnnounced { .. } => Some(NodeType::Gathering),
            Self::ResourceOffered { .. } => Some(NodeType::Resource),
            Self::HelpRequested { .. } => Some(NodeType::HelpRequest),
            Self::AnnouncementShared { .. } => Some(NodeType::Announcement),
            Self::ConcernRaised { .. } => Some(NodeType::Concern),
            Self::ConditionObserved { .. } => Some(NodeType::Condition),
            _ => None,
        }
    }

    pub fn locations(&self) -> &[Location] {
        match self {
            Self::GatheringAnnounced { locations, .. }
            | Self::ResourceOffered { locations, .. }
            | Self::HelpRequested { locations, .. }
            | Self::AnnouncementShared { locations, .. }
            | Self::ConcernRaised { locations, .. }
            | Self::ConditionObserved { locations, .. } => locations,
            _ => &[],
        }
    }

    /// Label for the signal variant (e.g. "Concern", "Resource").
    pub fn node_type_label(&self) -> Option<&'static str> {
        match self {
            Self::GatheringAnnounced { .. } => Some("Gathering"),
            Self::ResourceOffered { .. } => Some("Resource"),
            Self::HelpRequested { .. } => Some("HelpRequest"),
            Self::AnnouncementShared { .. } => Some("Announcement"),
            Self::ConcernRaised { .. } => Some("Concern"),
            Self::ConditionObserved { .. } => Some("Condition"),
            _ => None,
        }
    }
}
