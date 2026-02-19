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

// --- Story Synthesis Types ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StoryArc {
    Emerging,
    Growing,
    Stable,
    Fading,
    Resurgent,
}

impl std::fmt::Display for StoryArc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoryArc::Emerging => write!(f, "emerging"),
            StoryArc::Growing => write!(f, "growing"),
            StoryArc::Stable => write!(f, "stable"),
            StoryArc::Fading => write!(f, "fading"),
            StoryArc::Resurgent => write!(f, "resurgent"),
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

// --- Response Mapping Types ---

// RoleActionPlan removed: audience roles no longer drive action routing.
// Use signal type (Ask/Give/Event) and geography for discovery instead.

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
    /// Number of unique entity sources (orgs/domains) that have evidence for this signal.
    pub source_diversity: u32,
    /// Fraction of evidence from sources other than the signal's originating entity (0.0-1.0).
    pub external_ratio: f32,
    /// Cross-story cause heat: how much independent community attention exists in this signal's
    /// semantic neighborhood (0.0–1.0). A food shelf Ask rises when the housing crisis is trending.
    pub cause_heat: f64,
    /// Implied search queries from this signal for expansion discovery.
    /// Only populated during extraction; cleared after expansion processing.
    pub implied_queries: Vec<String>,
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
    pub availability: Option<String>,
    pub is_ongoing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskNode {
    pub meta: NodeMeta,
    pub urgency: Urgency,
    pub what_needed: Option<String>,
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
    pub category: Option<String>,
    pub what_would_help: Option<String>,
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
    pub sensitivity: String,
    pub source_count: u32,
    pub entity_count: u32,
    pub type_diversity: u32,
    pub source_domains: Vec<String>,
    pub corroboration_depth: u32,
    pub status: String, // "emerging" or "confirmed"
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

// --- City Node (graph-backed city configuration) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CityNode {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub center_lat: f64,
    pub center_lng: f64,
    pub radius_km: f64,
    pub geo_terms: Vec<String>,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub last_scout_completed_at: Option<DateTime<Utc>>,
}

// --- Place Node (fourth places — venues that attract gatherings) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceNode {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub city: String,
    pub lat: f64,
    pub lng: f64,
    pub geocoded: bool,
    pub created_at: DateTime<Utc>,
}

// --- Resource Node (capability/resource matching) ---

/// A capability or resource type that signals can require, prefer, or offer.
/// Global taxonomy — not city-scoped. Geographic filtering happens through signal edges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceNode {
    pub id: Uuid,
    /// Canonical label (e.g. "vehicle", "bilingual-spanish", "legal-expertise")
    pub name: String,
    /// Machine-readable slug, used as MERGE key
    pub slug: String,
    /// Optional LLM-generated description
    pub description: String,
    pub created_at: DateTime<Utc>,
    /// Updated when any edge is created to this resource
    pub last_seen: DateTime<Utc>,
    /// Number of signals connected to this resource
    pub signal_count: u32,
}

// --- Source Types (for emergent source discovery) ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    Web,
    Instagram,
    Facebook,
    Reddit,
    WebQuery,
    TikTok,
    Twitter,
    GoFundMeQuery,
    EventbriteQuery,
    VolunteerMatchQuery,
    Bluesky,
}

impl std::fmt::Display for SourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceType::Web => write!(f, "web"),
            SourceType::Instagram => write!(f, "instagram"),
            SourceType::Facebook => write!(f, "facebook"),
            SourceType::Reddit => write!(f, "reddit"),
            SourceType::WebQuery => write!(f, "web_query"),
            SourceType::TikTok => write!(f, "tiktok"),
            SourceType::Twitter => write!(f, "twitter"),
            SourceType::GoFundMeQuery => write!(f, "gofundme_query"),
            SourceType::EventbriteQuery => write!(f, "eventbrite_query"),
            SourceType::VolunteerMatchQuery => write!(f, "volunteermatch_query"),
            SourceType::Bluesky => write!(f, "bluesky"),
        }
    }
}

impl SourceType {
    pub fn from_str_loose(s: &str) -> Self {
        match s {
            "instagram" => Self::Instagram,
            "facebook" => Self::Facebook,
            "reddit" => Self::Reddit,
            "web_query" | "tavily_query" => Self::WebQuery,
            "tiktok" => Self::TikTok,
            "twitter" => Self::Twitter,
            "gofundme" | "gofundme_query" => Self::GoFundMeQuery,
            "eventbrite_search" | "eventbrite_query" => Self::EventbriteQuery,
            "volunteermatch_query" => Self::VolunteerMatchQuery,
            "bluesky" => Self::Bluesky,
            _ => Self::Web,
        }
    }

    /// Infer SourceType from a URL based on known platform domains.
    pub fn from_url(url: &str) -> Self {
        if url.contains("instagram.com") {
            Self::Instagram
        } else if url.contains("facebook.com") {
            Self::Facebook
        } else if url.contains("reddit.com") {
            Self::Reddit
        } else if url.contains("tiktok.com") {
            Self::TikTok
        } else if url.contains("twitter.com") || url.contains("x.com") {
            Self::Twitter
        } else if url.contains("bsky.app") {
            Self::Bluesky
        } else if url.contains("eventbrite.com") {
            Self::EventbriteQuery
        } else if url.contains("gofundme.com") {
            Self::GoFundMeQuery
        } else if url.contains("volunteermatch.org") {
            Self::VolunteerMatchQuery
        } else {
            Self::Web
        }
    }

    /// Returns true if this source type produces URLs (queries) rather than content (pages).
    pub fn is_query(&self) -> bool {
        matches!(
            self,
            Self::WebQuery
                | Self::EventbriteQuery
                | Self::GoFundMeQuery
                | Self::VolunteerMatchQuery
        )
    }

    /// For HTML-based query sources, returns the URL pattern that identifies individual item pages.
    pub fn link_pattern(&self) -> Option<&'static str> {
        match self {
            Self::EventbriteQuery => Some("eventbrite.com/e/"),
            Self::GoFundMeQuery => Some("gofundme.com/f/"),
            Self::VolunteerMatchQuery => Some("volunteermatch.org/search/opp"),
            _ => None,
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
    /// Generated during cold start bootstrap
    ColdStart,
    /// Discovered from tension-seeded follow-up queries
    TensionSeed,
    /// Submitted by a human via the submission endpoint
    HumanSubmission,
    /// Expanded from implied queries on extracted signals
    SignalExpansion,
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

/// A tracked source in the graph — either curated (from seed list) or discovered.
/// Identity is `canonical_key` = `city_slug:source_type:canonical_value`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceNode {
    pub id: Uuid,
    /// Unique identity: `city_slug:source_type:canonical_value`.
    pub canonical_key: String,
    /// The handle/query/URL that identifies this source within its type.
    pub canonical_value: String,
    /// URL for web/social sources. None for query-type sources (WebQuery).
    pub url: Option<String>,
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
    /// Source weight (0.0-1.0), drives scrape priority. Default 0.5.
    pub weight: f64,
    /// Learned scrape interval in hours.
    pub cadence_hours: Option<u32>,
    /// Rolling average signals per scrape.
    pub avg_signals_per_scrape: f64,
    /// Quality penalty from supervisor (0.0-1.0, default 1.0). Multiplied with weight.
    pub quality_penalty: f64,
    /// What kind of signals this source tends to surface.
    pub source_role: SourceRole,
    /// Number of times this source has been scraped (independent of signal count).
    pub scrape_count: u32,
}

/// A human-submitted link with an optional reason for investigation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmissionNode {
    pub id: Uuid,
    pub url: String,
    pub reason: Option<String>,
    pub city: String,
    pub submitted_at: DateTime<Utc>,
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

// --- Response Mapping Result ---

/// A signal that responds to a Tension, with edge metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TensionResponse {
    pub node: Node,
    pub match_strength: f64,
    pub explanation: String,
}

// --- Edge Types ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    /// Signal -> Evidence (provenance)
    SourcedFrom,
    /// Story -> Signal (membership)
    Contains,
    /// Give/Event/Ask -> Tension (feedback loop). Properties: match_strength, explanation
    RespondsTo,
    /// Actor -> Signal (participation). Properties: role
    ActedIn,
    /// Story -> Story (evolution)
    EvolvedFrom,
    /// Signal <-> Signal (clustering). Properties: weight
    SimilarTo,
    /// Submission -> Source (human submission)
    SubmittedFor,
    /// Give/Event/Ask -> Tension (community formation / gathering). Properties: match_strength, explanation, gathering_type
    DrawnTo,
    /// Signal -> Place (gathering venue)
    GathersAt,
    /// Ask/Event -> Resource (must have this capability to help). Properties: confidence, quantity, notes
    Requires,
    /// Ask/Event -> Resource (better if you have it, not required). Properties: confidence
    Prefers,
    /// Give -> Resource (this is what we provide). Properties: confidence, capacity
    Offers,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety::SensitivityLevel;
    use chrono::Utc;

    fn test_meta() -> NodeMeta {
        NodeMeta {
            id: Uuid::new_v4(),
            title: "Test".to_string(),
            summary: "Test summary".to_string(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.8,
            freshness_score: 1.0,
            corroboration_count: 0,
            location: None,
            location_name: None,
            source_url: "https://example.com".to_string(),
            extracted_at: Utc::now(),
            last_confirmed_active: Utc::now(),
            source_diversity: 1,
            external_ratio: 0.0,
            cause_heat: 0.0,
            mentioned_actors: vec![],
            implied_queries: vec![],
        }
    }

    #[test]
    fn tension_node_has_all_fields() {
        let t = TensionNode {
            meta: test_meta(),
            severity: Severity::High,
            category: Some("housing".to_string()),
            what_would_help: Some("affordable housing policy".to_string()),
        };
        assert_eq!(t.severity, Severity::High);
        assert_eq!(t.category.as_deref(), Some("housing"));
        assert_eq!(
            t.what_would_help.as_deref(),
            Some("affordable housing policy")
        );
    }

    #[test]
    fn tension_node_optional_fields_default_none() {
        let t = TensionNode {
            meta: test_meta(),
            severity: Severity::Medium,
            category: None,
            what_would_help: None,
        };
        assert_eq!(t.severity, Severity::Medium);
        assert!(t.category.is_none());
        assert!(t.what_would_help.is_none());
    }

    #[test]
    fn haversine_sf_to_oakland() {
        // SF to Oakland is ~13km
        let dist = haversine_km(37.7749, -122.4194, 37.8044, -122.2712);
        assert!(
            (dist - 13.0).abs() < 2.0,
            "SF to Oakland should be ~13km, got {dist}"
        );
    }

    #[test]
    fn haversine_sf_to_la() {
        // SF to LA is ~559km
        let dist = haversine_km(37.7749, -122.4194, 34.0522, -118.2437);
        assert!(
            (dist - 559.0).abs() < 10.0,
            "SF to LA should be ~559km, got {dist}"
        );
    }

    #[test]
    fn haversine_same_point_is_zero() {
        let dist = haversine_km(44.9778, -93.265, 44.9778, -93.265);
        assert!(dist < 0.001, "Same point should be 0km, got {dist}");
    }

    #[test]
    fn resource_node_has_all_fields() {
        let now = Utc::now();
        let r = ResourceNode {
            id: Uuid::new_v4(),
            name: "vehicle".to_string(),
            slug: "vehicle".to_string(),
            description: "Car, truck, or other motor vehicle".to_string(),
            created_at: now,
            last_seen: now,
            signal_count: 3,
        };
        assert_eq!(r.name, "vehicle");
        assert_eq!(r.slug, "vehicle");
        assert_eq!(r.signal_count, 3);
    }

    #[test]
    fn edge_type_has_resource_variants() {
        let requires = EdgeType::Requires;
        let prefers = EdgeType::Prefers;
        let offers = EdgeType::Offers;
        let req_json = serde_json::to_string(&requires).unwrap();
        let pref_json = serde_json::to_string(&prefers).unwrap();
        let off_json = serde_json::to_string(&offers).unwrap();
        assert_eq!(req_json, "\"requires\"");
        assert_eq!(pref_json, "\"prefers\"");
        assert_eq!(off_json, "\"offers\"");
    }
}
