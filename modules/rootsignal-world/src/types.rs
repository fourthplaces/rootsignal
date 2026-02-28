use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// --- Geo Types ---

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GeoPrecision {
    Exact,
    Neighborhood,
    Approximate,
    Region,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GeoPoint {
    pub lat: f64,
    pub lng: f64,
    pub precision: GeoPrecision,
}

/// Haversine great-circle distance between two lat/lng points in kilometers.
pub fn haversine_km(lat1: f64, lng1: f64, lat2: f64, lng2: f64) -> f64 {
    const EARTH_RADIUS_KM: f64 = 6371.0;
    let d_lat = (lat2 - lat1).to_radians();
    let d_lng = (lng2 - lng1).to_radians();
    let lat1_r = lat1.to_radians();
    let lat2_r = lat2.to_radians();

    let a = (d_lat / 2.0).sin().powi(2) + lat1_r.cos() * lat2_r.cos() * (d_lng / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    EARTH_RADIUS_KM * c
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

impl std::fmt::Display for Urgency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Urgency::Low => write!(f, "low"),
            Urgency::Medium => write!(f, "medium"),
            Urgency::High => write!(f, "high"),
            Urgency::Critical => write!(f, "critical"),
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Low => write!(f, "low"),
            Severity::Medium => write!(f, "medium"),
            Severity::High => write!(f, "high"),
            Severity::Critical => write!(f, "critical"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    Gathering,
    Aid,
    Need,
    Notice,
    Tension,
    Condition,
    Incident,
    Citation,
}

impl std::fmt::Display for NodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeType::Gathering => write!(f, "Gathering"),
            NodeType::Aid => write!(f, "Aid"),
            NodeType::Need => write!(f, "Need"),
            NodeType::Notice => write!(f, "Notice"),
            NodeType::Tension => write!(f, "Tension"),
            NodeType::Condition => write!(f, "Condition"),
            NodeType::Incident => write!(f, "Incident"),
            NodeType::Citation => write!(f, "Citation"),
        }
    }
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

// --- Channel Types ---

/// The type of channel a piece of evidence came through.
/// Used for channel diversity scoring — cross-channel corroboration is
/// epistemologically stronger than same-channel repetition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChannelType {
    Press,
    Social,
    DirectAction,
    CommunityMedia,
}

impl ChannelType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChannelType::Press => "press",
            ChannelType::Social => "social",
            ChannelType::DirectAction => "direct_action",
            ChannelType::CommunityMedia => "community_media",
        }
    }
}

impl std::fmt::Display for ChannelType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// --- Social Platform ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SocialPlatform {
    Instagram,
    Facebook,
    Reddit,
    Twitter,
    TikTok,
    Bluesky,
}

// --- Discovery & Source ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryMethod {
    /// From initial seed list
    Curated,
    /// LLM gap analysis identified a gap and suggested this
    GapAnalysis,
    /// Extracted from signal content (org mentioned but not tracked)
    SignalReference,
    /// Discovered via topic/hashtag search on social platforms
    HashtagDiscovery,
    /// Generated during cold start bootstrap
    ColdStart,
    /// Discovered from tension-seeded follow-up queries
    TensionSeed,
    /// Submitted by a human via the submission endpoint
    HumanSubmission,
    /// Expanded from implied queries on extracted signals
    SignalExpansion,
    /// Social account linked to a known actor
    ActorAccount,
    /// Discovered via a known actor's social graph
    SocialGraphFollow,
    /// Discovered as an outbound link on a scraped page
    LinkedFrom,
}

impl std::fmt::Display for DiscoveryMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiscoveryMethod::Curated => write!(f, "curated"),
            DiscoveryMethod::GapAnalysis => write!(f, "gap_analysis"),
            DiscoveryMethod::SignalReference => write!(f, "signal_reference"),
            DiscoveryMethod::HashtagDiscovery => write!(f, "hashtag_discovery"),
            DiscoveryMethod::ColdStart => write!(f, "cold_start"),
            DiscoveryMethod::TensionSeed => write!(f, "tension_seed"),
            DiscoveryMethod::HumanSubmission => write!(f, "human_submission"),
            DiscoveryMethod::SignalExpansion => write!(f, "signal_expansion"),
            DiscoveryMethod::ActorAccount => write!(f, "actor_account"),
            DiscoveryMethod::SocialGraphFollow => write!(f, "social_graph_follow"),
            DiscoveryMethod::LinkedFrom => write!(f, "linked_from"),
        }
    }
}

/// What kind of signals a source tends to surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum SourceRole {
    /// Surfaces problems (forums, complaint boards, news).
    Tension,
    /// Surfaces responses (nonprofits, service directories, event calendars).
    Response,
    /// Produces both tensions and responses (general community pages, social media).
    #[default]
    Mixed,
}

impl std::fmt::Display for SourceRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceRole::Tension => write!(f, "tension"),
            SourceRole::Response => write!(f, "response"),
            SourceRole::Mixed => write!(f, "mixed"),
        }
    }
}

impl SourceRole {
    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "tension" => SourceRole::Tension,
            "response" => SourceRole::Response,
            _ => SourceRole::Mixed,
        }
    }
}

// --- Entity Types ---

/// What kind of entity is referenced in a world event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    /// Individual humans.
    Person,
    /// Named, structured groups of people.
    Organization,
    /// Unnamed or loosely defined collections of people ("displaced families", "long-time renters").
    Group,
    /// Geographic — natural or built.
    Place,
    /// Everything else — species, legislation, infrastructure, programs.
    Thing,
}

impl std::fmt::Display for EntityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EntityType::Person => write!(f, "person"),
            EntityType::Organization => write!(f, "organization"),
            EntityType::Group => write!(f, "group"),
            EntityType::Place => write!(f, "place"),
            EntityType::Thing => write!(f, "thing"),
        }
    }
}

/// A reference to an entity mentioned in a world event.
///
/// Replaces the flat `mentioned_actors: Vec<String>` and `author_actor: Option<String>`.
/// The author is just an entity with `role: Some("author")`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Entity {
    pub name: String,
    pub entity_type: EntityType,
    /// Role this entity plays in the event: "author", "organizer", "subject", etc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

// --- Reference Types ---

/// The kind of stated relationship between world facts.
///
/// These are relationships as described by the source — world facts, not system inferences.
/// The system can match reference descriptions to existing signals, but the reference
/// itself records what the source stated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Relationship {
    /// "This cleanup was organized because of the spill."
    RespondsTo,
    /// "The encampment appeared after the shelter closed."
    CausedBy,
    /// "New information about the same situation."
    Updates,
    /// "This data says the opposite."
    Contradicts,
    /// "This report backs up the claim."
    Supports,
    /// "This replaces the previous announcement."
    Supersedes,
}

impl std::fmt::Display for Relationship {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Relationship::RespondsTo => write!(f, "responds_to"),
            Relationship::CausedBy => write!(f, "caused_by"),
            Relationship::Updates => write!(f, "updates"),
            Relationship::Contradicts => write!(f, "contradicts"),
            Relationship::Supports => write!(f, "supports"),
            Relationship::Supersedes => write!(f, "supersedes"),
        }
    }
}

/// A stated relationship to another world fact, as described by the source.
///
/// When a source says "this cleanup was organized because of the oil spill,"
/// that's a world fact captured here. The system can later match the description
/// to an existing signal, but the reference itself is what the source said.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Reference {
    /// What the source described: "the recent oil spill on Minnehaha Creek".
    pub description: String,
    /// The type of relationship stated.
    pub relationship: Relationship,
}

// --- Tone (used by SystemEvent for classification) ---

/// Emotional register classification — a system judgment, not a world fact.
///
/// Lives here as a shared type but only used in SystemEvent::ToneClassified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    Urgent,
    Distressed,
    Fearful,
    Grieving,
    Angry,
    Defiant,
    Hopeful,
    Supportive,
    Celebratory,
    Analytical,
    Neutral,
}

impl std::fmt::Display for Tone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tone::Urgent => write!(f, "urgent"),
            Tone::Distressed => write!(f, "distressed"),
            Tone::Fearful => write!(f, "fearful"),
            Tone::Grieving => write!(f, "grieving"),
            Tone::Angry => write!(f, "angry"),
            Tone::Defiant => write!(f, "defiant"),
            Tone::Hopeful => write!(f, "hopeful"),
            Tone::Supportive => write!(f, "supportive"),
            Tone::Celebratory => write!(f, "celebratory"),
            Tone::Analytical => write!(f, "analytical"),
            Tone::Neutral => write!(f, "neutral"),
        }
    }
}

// --- Edge Types ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    /// Signal -> Citation (provenance)
    SourcedFrom,
    /// Aid/Gathering/Need -> Tension (feedback loop). Properties: match_strength, explanation
    RespondsTo,
    /// Actor -> Signal (participation). Properties: role
    ActedIn,
    /// Submission -> Source (human submission)
    SubmittedFor,
    /// Aid/Gathering/Need -> Tension (drawn toward a tension). Properties: match_strength, explanation
    DrawnTo,
    /// Signal -> Place (gathering venue)
    GathersAt,
    /// Signal -> Tag (thematic tag)
    Tagged,
    /// Need/Gathering -> Resource (must have this capability to help). Properties: confidence, quantity, notes
    Requires,
    /// Need/Gathering -> Resource (better if you have it, not required). Properties: confidence
    Prefers,
    /// Aid -> Resource (this is what we provide). Properties: confidence, capacity
    Offers,
}
