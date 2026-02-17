use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::safety::SensitivityLevel;

// --- Geo Types ---

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GeoPrecision {
    Exact,
    Neighborhood,
    City,
    Region,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GeoPoint {
    pub lat: f64,
    pub lng: f64,
    pub precision: GeoPrecision,
}

// --- Enums ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Urgency {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    Event,
    Give,
    Ask,
    Notice,
    Tension,
    Evidence,
}

impl std::fmt::Display for NodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeType::Event => write!(f, "Event"),
            NodeType::Give => write!(f, "Give"),
            NodeType::Ask => write!(f, "Ask"),
            NodeType::Notice => write!(f, "Notice"),
            NodeType::Tension => write!(f, "Tension"),
            NodeType::Evidence => write!(f, "Evidence"),
        }
    }
}

/// Controlled vocabulary for audience roles.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AudienceRole {
    Volunteer,
    Donor,
    Neighbor,
    Parent,
    Youth,
    Senior,
    Immigrant,
    Steward,
    CivicParticipant,
    SkillProvider,
}

impl std::fmt::Display for AudienceRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AudienceRole::Volunteer => write!(f, "Volunteer"),
            AudienceRole::Donor => write!(f, "Donor"),
            AudienceRole::Neighbor => write!(f, "Neighbor"),
            AudienceRole::Parent => write!(f, "Parent"),
            AudienceRole::Youth => write!(f, "Youth"),
            AudienceRole::Senior => write!(f, "Senior"),
            AudienceRole::Immigrant => write!(f, "Immigrant"),
            AudienceRole::Steward => write!(f, "Steward"),
            AudienceRole::CivicParticipant => write!(f, "Civic Participant"),
            AudienceRole::SkillProvider => write!(f, "Skill Provider"),
        }
    }
}

// --- Node Metadata (shared across all signal types) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeMeta {
    pub id: Uuid,
    pub title: String,
    pub summary: String,
    pub sensitivity: SensitivityLevel,
    pub confidence: f32,
    pub source_trust: f32,
    pub freshness_score: f32,
    pub corroboration_count: u32,
    pub location: Option<GeoPoint>,
    pub location_name: Option<String>,
    pub source_url: String,
    pub extracted_at: DateTime<Utc>,
    pub last_confirmed_active: DateTime<Utc>,
    pub audience_roles: Vec<AudienceRole>,
}

// --- Signal Node Types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventNode {
    pub meta: NodeMeta,
    pub starts_at: DateTime<Utc>,
    pub ends_at: Option<DateTime<Utc>>,
    pub action_url: String,
    pub organizer: Option<String>,
    pub is_recurring: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GiveNode {
    pub meta: NodeMeta,
    pub action_url: String,
    pub availability: String,
    pub is_ongoing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskNode {
    pub meta: NodeMeta,
    pub urgency: Urgency,
    pub what_needed: String,
    pub action_url: Option<String>,
    pub goal: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoticeNode {
    pub meta: NodeMeta,
    pub severity: Severity,
    pub category: Option<String>,
    pub effective_date: Option<DateTime<Utc>>,
    pub source_authority: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TensionNode {
    pub meta: NodeMeta,
    pub severity: Severity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceNode {
    pub id: Uuid,
    pub source_url: String,
    pub retrieved_at: DateTime<Utc>,
    pub content_hash: String,
    pub snippet: Option<String>,
}

// --- Sum type ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "node_type")]
pub enum Node {
    Event(EventNode),
    Give(GiveNode),
    Ask(AskNode),
    Notice(NoticeNode),
    Tension(TensionNode),
    Evidence(EvidenceNode),
}

impl Node {
    pub fn node_type(&self) -> NodeType {
        match self {
            Node::Event(_) => NodeType::Event,
            Node::Give(_) => NodeType::Give,
            Node::Ask(_) => NodeType::Ask,
            Node::Notice(_) => NodeType::Notice,
            Node::Tension(_) => NodeType::Tension,
            Node::Evidence(_) => NodeType::Evidence,
        }
    }

    pub fn id(&self) -> Uuid {
        match self {
            Node::Event(n) => n.meta.id,
            Node::Give(n) => n.meta.id,
            Node::Ask(n) => n.meta.id,
            Node::Notice(n) => n.meta.id,
            Node::Tension(n) => n.meta.id,
            Node::Evidence(n) => n.id,
        }
    }

    pub fn meta(&self) -> Option<&NodeMeta> {
        match self {
            Node::Event(n) => Some(&n.meta),
            Node::Give(n) => Some(&n.meta),
            Node::Ask(n) => Some(&n.meta),
            Node::Notice(n) => Some(&n.meta),
            Node::Tension(n) => Some(&n.meta),
            Node::Evidence(_) => None,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Node::Event(n) => &n.meta.title,
            Node::Give(n) => &n.meta.title,
            Node::Ask(n) => &n.meta.title,
            Node::Notice(n) => &n.meta.title,
            Node::Tension(n) => &n.meta.title,
            Node::Evidence(n) => &n.source_url,
        }
    }
}

// --- Story Node ---

/// A cluster of related signals that form an emergent story.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryNode {
    pub id: Uuid,
    pub headline: String,
    pub summary: String,
    pub signal_count: u32,
    pub first_seen: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
    pub velocity: f64,
    pub energy: f64,
    pub centroid_lat: Option<f64>,
    pub centroid_lng: Option<f64>,
    pub dominant_type: String,
    pub audience_roles: Vec<String>,
    pub sensitivity: String,
    pub source_count: u32,
    pub org_count: u32,
    pub source_domains: Vec<String>,
    pub corroboration_depth: u32,
    pub status: String,  // "emerging" or "confirmed"
}

/// A snapshot of a story's signal count at a point in time, used for velocity tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterSnapshot {
    pub id: Uuid,
    pub story_id: Uuid,
    pub signal_count: u32,
    pub run_at: DateTime<Utc>,
}

// --- Edge Types ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    /// Any signal node -> Evidence (provenance)
    SourcedFrom,
}
