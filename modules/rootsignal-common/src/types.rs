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

// --- Story Synthesis Types ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StoryArc {
    Emerging,
    Growing,
    Stable,
    Fading,
}

impl std::fmt::Display for StoryArc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoryArc::Emerging => write!(f, "emerging"),
            StoryArc::Growing => write!(f, "growing"),
            StoryArc::Stable => write!(f, "stable"),
            StoryArc::Fading => write!(f, "fading"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StoryCategory {
    Resource,
    Gathering,
    Crisis,
    Governance,
    Stewardship,
    Community,
}

impl std::fmt::Display for StoryCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoryCategory::Resource => write!(f, "resource"),
            StoryCategory::Gathering => write!(f, "gathering"),
            StoryCategory::Crisis => write!(f, "crisis"),
            StoryCategory::Governance => write!(f, "governance"),
            StoryCategory::Stewardship => write!(f, "stewardship"),
            StoryCategory::Community => write!(f, "community"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionGuidance {
    pub role: AudienceRole,
    pub guidance: String,
    pub action_urls: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorySynthesis {
    pub headline: String,
    pub lede: String,
    pub narrative: String,
    pub action_guidance: Vec<ActionGuidance>,
    pub key_entities: Vec<String>,
    pub category: StoryCategory,
    pub arc: StoryArc,
}

// --- Actor Types ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActorType {
    Organization,
    Individual,
    GovernmentBody,
    Coalition,
}

impl std::fmt::Display for ActorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActorType::Organization => write!(f, "organization"),
            ActorType::Individual => write!(f, "individual"),
            ActorType::GovernmentBody => write!(f, "government_body"),
            ActorType::Coalition => write!(f, "coalition"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorNode {
    pub id: Uuid,
    pub name: String,
    pub actor_type: ActorType,
    pub entity_id: String,
    pub domains: Vec<String>,
    pub social_urls: Vec<String>,
    pub city: String,
    pub description: String,
    pub signal_count: u32,
    pub first_seen: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
    pub typical_roles: Vec<String>,
}

// --- Edition Types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditionNode {
    pub id: Uuid,
    pub city: String,
    pub period: String,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub generated_at: DateTime<Utc>,
    pub story_count: u32,
    pub new_signal_count: u32,
    pub editorial_summary: String,
}

// --- Response Mapping Types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleActionPlan {
    pub role: AudienceRole,
    pub urgent_asks: Vec<Node>,
    pub opportunities: Vec<Node>,
    pub active_tensions: Vec<(Node, Vec<Node>)>,
}

// --- Node Metadata (shared across all signal types) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeMeta {
    pub id: Uuid,
    pub title: String,
    pub summary: String,
    pub sensitivity: SensitivityLevel,
    pub confidence: f32,
    pub freshness_score: f32,
    pub corroboration_count: u32,
    pub location: Option<GeoPoint>,
    pub location_name: Option<String>,
    pub source_url: String,
    pub extracted_at: DateTime<Utc>,
    pub last_confirmed_active: DateTime<Utc>,
    pub audience_roles: Vec<AudienceRole>,
    /// Number of unique entity sources (orgs/domains) that have evidence for this signal.
    pub source_diversity: u32,
    /// Fraction of evidence from sources other than the signal's originating entity (0.0-1.0).
    pub external_ratio: f32,
    /// Organizations/groups mentioned in this signal (extracted by LLM, used for Actor resolution)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mentioned_actors: Vec<String>,
}

// --- Signal Node Types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventNode {
    pub meta: NodeMeta,
    pub starts_at: Option<DateTime<Utc>>,
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
    pub relevance: Option<String>,
    pub evidence_confidence: Option<f32>,
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
    pub entity_count: u32,
    pub type_diversity: u32,
    pub source_domains: Vec<String>,
    pub corroboration_depth: u32,
    pub status: String,  // "emerging" or "confirmed"
    // M2: Story synthesis fields
    pub arc: Option<String>,
    pub category: Option<String>,
    pub lede: Option<String>,
    pub narrative: Option<String>,
    pub action_guidance: Option<String>, // JSON string of Vec<ActionGuidance>
}

/// A snapshot of a story's signal and entity counts at a point in time, used for velocity tracking.
/// Velocity is driven by entity_count growth (not raw signal count) to resist single-source flooding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterSnapshot {
    pub id: Uuid,
    pub story_id: Uuid,
    pub signal_count: u32,
    pub entity_count: u32,
    pub run_at: DateTime<Utc>,
}

// --- Source Types (for emergent source discovery) ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    Web,
    Instagram,
    Facebook,
    Reddit,
}

impl std::fmt::Display for SourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceType::Web => write!(f, "web"),
            SourceType::Instagram => write!(f, "instagram"),
            SourceType::Facebook => write!(f, "facebook"),
            SourceType::Reddit => write!(f, "reddit"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryMethod {
    /// From CityProfile seed list
    Curated,
    /// LLM gap analysis identified a gap and suggested this
    GapAnalysis,
    /// Extracted from signal content (org mentioned but not tracked)
    SignalReference,
    /// Discovered via topic/hashtag search on social platforms
    HashtagDiscovery,
}

impl std::fmt::Display for DiscoveryMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiscoveryMethod::Curated => write!(f, "curated"),
            DiscoveryMethod::GapAnalysis => write!(f, "gap_analysis"),
            DiscoveryMethod::SignalReference => write!(f, "signal_reference"),
            DiscoveryMethod::HashtagDiscovery => write!(f, "hashtag_discovery"),
        }
    }
}

/// A tracked source in the graph — either curated (from seed list) or discovered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceNode {
    pub id: Uuid,
    pub url: String,
    pub source_type: SourceType,
    pub discovery_method: DiscoveryMethod,
    pub city: String,
    pub created_at: DateTime<Utc>,
    pub last_scraped: Option<DateTime<Utc>>,
    pub last_produced_signal: Option<DateTime<Utc>>,
    pub signals_produced: u32,
    pub signals_corroborated: u32,
    pub consecutive_empty_runs: u32,
    pub active: bool,
    pub gap_context: Option<String>,
}

/// A blocked source URL pattern that should never be re-discovered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockedSource {
    pub url_pattern: String,
    pub blocked_at: DateTime<Utc>,
    pub reason: String,
}

// --- Entity Resolution ---

/// Owned entity mapping for resolving source URLs to parent entities.
/// Used across scout (corroboration) and graph (clustering) crates.
#[derive(Debug, Clone)]
pub struct EntityMappingOwned {
    pub entity_id: String,
    pub domains: Vec<String>,
    pub instagram: Vec<String>,
    pub facebook: Vec<String>,
    pub reddit: Vec<String>,
}

/// Resolve a source URL to its parent entity ID using entity mappings.
/// Returns the entity_id if matched, otherwise extracts the domain as a fallback entity.
pub fn resolve_entity(url: &str, mappings: &[EntityMappingOwned]) -> String {
    let domain = extract_domain(url);

    for mapping in mappings {
        for d in &mapping.domains {
            if domain.contains(d.as_str()) {
                return mapping.entity_id.clone();
            }
        }
        for ig in &mapping.instagram {
            if url.contains(&format!("instagram.com/{ig}")) {
                return mapping.entity_id.clone();
            }
        }
        for fb in &mapping.facebook {
            if url.contains(fb.as_str()) {
                return mapping.entity_id.clone();
            }
        }
        for r in &mapping.reddit {
            if url.contains(&format!("reddit.com/user/{r}"))
                || url.contains(&format!("reddit.com/u/{r}"))
            {
                return mapping.entity_id.clone();
            }
        }
    }

    // Fallback: use the domain itself as the entity
    domain
}

/// Extract the domain from a URL (e.g., "https://www.example.com/path" -> "www.example.com").
pub fn extract_domain(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("")
        .to_lowercase()
}

// --- Edge Types ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    /// Any signal node -> Evidence (provenance)
    SourcedFrom,
}
