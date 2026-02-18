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
}

// --- Source Types (for emergent source discovery) ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    Web,
    Instagram,
    Facebook,
    Reddit,
    TavilyQuery,
    TikTok,
    Twitter,
    GoFundMe,
    EventbriteSearch,
    Bluesky,
}

impl std::fmt::Display for SourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceType::Web => write!(f, "web"),
            SourceType::Instagram => write!(f, "instagram"),
            SourceType::Facebook => write!(f, "facebook"),
            SourceType::Reddit => write!(f, "reddit"),
            SourceType::TavilyQuery => write!(f, "tavily_query"),
            SourceType::TikTok => write!(f, "tiktok"),
            SourceType::Twitter => write!(f, "twitter"),
            SourceType::GoFundMe => write!(f, "gofundme"),
            SourceType::EventbriteSearch => write!(f, "eventbrite_search"),
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
            "tavily_query" => Self::TavilyQuery,
            "tiktok" => Self::TikTok,
            "twitter" => Self::Twitter,
            "gofundme" => Self::GoFundMe,
            "eventbrite_search" => Self::EventbriteSearch,
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
        } else {
            Self::Web
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
    /// URL for web/social sources. None for query-type sources (TavilyQuery).
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
    /// Cumulative API cost in cents.
    pub total_cost_cents: u64,
    /// Cost of last scrape in cents.
    pub last_cost_cents: u64,
    /// JSON string of signal type breakdown: `{"tension": N, "ask": N, ...}`.
    pub taxonomy_stats: Option<String>,
    /// Quality penalty from supervisor (0.0-1.0, default 1.0). Multiplied with weight.
    pub quality_penalty: f64,
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

// --- Edge Types ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    /// Any signal node -> Evidence (provenance)
    SourcedFrom,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use crate::safety::SensitivityLevel;

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
        assert_eq!(t.what_would_help.as_deref(), Some("affordable housing policy"));
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
        assert!((dist - 13.0).abs() < 2.0, "SF to Oakland should be ~13km, got {dist}");
    }

    #[test]
    fn haversine_sf_to_la() {
        // SF to LA is ~559km
        let dist = haversine_km(37.7749, -122.4194, 34.0522, -118.2437);
        assert!((dist - 559.0).abs() < 10.0, "SF to LA should be ~559km, got {dist}");
    }

    #[test]
    fn haversine_same_point_is_zero() {
        let dist = haversine_km(44.9778, -93.265, 44.9778, -93.265);
        assert!(dist < 0.001, "Same point should be 0km, got {dist}");
    }
}
