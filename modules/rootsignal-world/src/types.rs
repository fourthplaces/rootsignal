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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
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
    Gathering,
    Aid,
    Need,
    Notice,
    Tension,
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
    /// Signal <-> Signal (clustering). Properties: weight
    SimilarTo,
    /// Submission -> Source (human submission)
    SubmittedFor,
    /// Aid/Gathering/Need -> Tension (community formation / gathering). Properties: match_strength, explanation, gathering_type
    DrawnTo,
    /// Signal -> Place (gathering venue)
    GathersAt,
    /// Signal -> Tag (thematic tag)
    Tagged,
    /// Signal -> Tag (admin suppressed an auto-aggregated tag)
    SuppressedTag,
    /// Need/Gathering -> Resource (must have this capability to help). Properties: confidence, quantity, notes
    Requires,
    /// Need/Gathering -> Resource (better if you have it, not required). Properties: confidence
    Prefers,
    /// Aid -> Resource (this is what we provide). Properties: confidence, capacity
    Offers,
}

// --- Situation Types ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SituationArc {
    Emerging,
    Developing,
    Active,
    Cooling,
    Cold,
}

impl std::fmt::Display for SituationArc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SituationArc::Emerging => write!(f, "emerging"),
            SituationArc::Developing => write!(f, "developing"),
            SituationArc::Active => write!(f, "active"),
            SituationArc::Cooling => write!(f, "cooling"),
            SituationArc::Cold => write!(f, "cold"),
        }
    }
}

impl std::str::FromStr for SituationArc {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "emerging" => Ok(Self::Emerging),
            "developing" => Ok(Self::Developing),
            "active" => Ok(Self::Active),
            "cooling" => Ok(Self::Cooling),
            "cold" => Ok(Self::Cold),
            other => Err(format!("unknown SituationArc: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Clarity {
    Fuzzy,
    Sharpening,
    Sharp,
}

impl std::fmt::Display for Clarity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Clarity::Fuzzy => write!(f, "fuzzy"),
            Clarity::Sharpening => write!(f, "sharpening"),
            Clarity::Sharp => write!(f, "sharp"),
        }
    }
}

impl std::str::FromStr for Clarity {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "fuzzy" => Ok(Self::Fuzzy),
            "sharpening" => Ok(Self::Sharpening),
            "sharp" => Ok(Self::Sharp),
            other => Err(format!("unknown Clarity: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DispatchType {
    Update,
    Emergence,
    Split,
    Merge,
    Reactivation,
    Correction,
}

impl std::fmt::Display for DispatchType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DispatchType::Update => write!(f, "update"),
            DispatchType::Emergence => write!(f, "emergence"),
            DispatchType::Split => write!(f, "split"),
            DispatchType::Merge => write!(f, "merge"),
            DispatchType::Reactivation => write!(f, "reactivation"),
            DispatchType::Correction => write!(f, "correction"),
        }
    }
}

impl std::str::FromStr for DispatchType {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "update" => Ok(Self::Update),
            "emergence" => Ok(Self::Emergence),
            "split" => Ok(Self::Split),
            "merge" => Ok(Self::Merge),
            "reactivation" => Ok(Self::Reactivation),
            "correction" => Ok(Self::Correction),
            other => Err(format!("unknown DispatchType: {other}")),
        }
    }
}
