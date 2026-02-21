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
    Gathering,
    Aid,
    Need,
    Notice,
    Tension,
    Evidence,
}

impl std::fmt::Display for NodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeType::Gathering => write!(f, "Gathering"),
            NodeType::Aid => write!(f, "Aid"),
            NodeType::Need => write!(f, "Need"),
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
    Environment,
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
            StoryCategory::Environment => write!(f, "environment"),
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
    pub description: String,
    pub signal_count: u32,
    pub first_seen: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
    pub typical_roles: Vec<String>,
}

// --- Response Mapping Types ---

// RoleActionPlan removed: audience roles no longer drive action routing.
// Use signal type (Need/Aid/Gathering) and geography for discovery instead.

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
    /// semantic neighborhood (0.0–1.0). A food shelf Need rises when the housing crisis is trending.
    pub cause_heat: f64,
    /// Implied search queries from this signal for expansion discovery.
    /// Only populated during extraction; cleared after expansion processing.
    pub implied_queries: Vec<String>,
    /// Number of distinct channel types with external entity evidence (1-4).
    #[serde(default = "default_channel_diversity")]
    pub channel_diversity: u32,
    /// Organizations/groups mentioned in this signal (extracted by LLM, used for Actor resolution)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mentioned_actors: Vec<String>,
}

// --- Signal Node Types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatheringNode {
    pub meta: NodeMeta,
    pub starts_at: Option<DateTime<Utc>>,
    pub ends_at: Option<DateTime<Utc>>,
    pub action_url: String,
    pub organizer: Option<String>,
    pub is_recurring: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AidNode {
    pub meta: NodeMeta,
    pub action_url: String,
    pub availability: Option<String>,
    pub is_ongoing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeedNode {
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
    #[serde(default)]
    pub channel_type: Option<ChannelType>,
}

// --- Sum type ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "node_type")]
pub enum Node {
    Gathering(GatheringNode),
    Aid(AidNode),
    Need(NeedNode),
    Notice(NoticeNode),
    Tension(TensionNode),
    Evidence(EvidenceNode),
}

impl Node {
    pub fn node_type(&self) -> NodeType {
        match self {
            Node::Gathering(_) => NodeType::Gathering,
            Node::Aid(_) => NodeType::Aid,
            Node::Need(_) => NodeType::Need,
            Node::Notice(_) => NodeType::Notice,
            Node::Tension(_) => NodeType::Tension,
            Node::Evidence(_) => NodeType::Evidence,
        }
    }

    pub fn id(&self) -> Uuid {
        match self {
            Node::Gathering(n) => n.meta.id,
            Node::Aid(n) => n.meta.id,
            Node::Need(n) => n.meta.id,
            Node::Notice(n) => n.meta.id,
            Node::Tension(n) => n.meta.id,
            Node::Evidence(n) => n.id,
        }
    }

    pub fn meta(&self) -> Option<&NodeMeta> {
        match self {
            Node::Gathering(n) => Some(&n.meta),
            Node::Aid(n) => Some(&n.meta),
            Node::Need(n) => Some(&n.meta),
            Node::Notice(n) => Some(&n.meta),
            Node::Tension(n) => Some(&n.meta),
            Node::Evidence(_) => None,
        }
    }

    pub fn meta_mut(&mut self) -> Option<&mut NodeMeta> {
        match self {
            Node::Gathering(n) => Some(&mut n.meta),
            Node::Aid(n) => Some(&mut n.meta),
            Node::Need(n) => Some(&mut n.meta),
            Node::Notice(n) => Some(&mut n.meta),
            Node::Tension(n) => Some(&mut n.meta),
            Node::Evidence(_) => None,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Node::Gathering(n) => &n.meta.title,
            Node::Aid(n) => &n.meta.title,
            Node::Need(n) => &n.meta.title,
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
    // Story pipeline consolidation fields
    pub cause_heat: f64,
    pub ask_count: u32,
    pub give_count: u32,
    pub event_count: u32,
    pub drawn_to_count: u32,
    pub gap_score: i32,
    pub gap_velocity: f64,
    pub channel_diversity: u32,
}

/// A snapshot of a story's signal and entity counts at a point in time, used for velocity tracking.
/// Velocity is driven by entity_count growth (not raw signal count) to resist single-source flooding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterSnapshot {
    pub id: Uuid,
    pub story_id: Uuid,
    pub signal_count: u32,
    pub entity_count: u32,
    pub ask_count: u32,
    pub give_count: u32,
    pub run_at: DateTime<Utc>,
}

// --- Scout Scope (geographic context for a scout run) ---

/// The geographic context passed through the scout pipeline.
/// Defines where scout looks — center point, radius, and search terms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoutScope {
    pub center_lat: f64,
    pub center_lng: f64,
    pub radius_km: f64,
    pub name: String,
    pub geo_terms: Vec<String>,
}

impl ScoutScope {
    /// Compute bounding box from center + radius.
    pub fn bounding_box(&self) -> (f64, f64, f64, f64) {
        let lat_delta = self.radius_km / 111.0;
        let lng_delta = self.radius_km / (111.0 * self.center_lat.to_radians().cos());
        (
            self.center_lat - lat_delta,
            self.center_lat + lat_delta,
            self.center_lng - lng_delta,
            self.center_lng + lng_delta,
        )
    }
}

/// Temporary aliases — will be removed once all callers migrate.
pub type RegionNode = ScoutScope;
pub type CityNode = ScoutScope;

// --- Scout Task (ephemeral unit of work for the scout swarm) ---

/// How a scout task was created.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ScoutTaskSource {
    /// Seeded from config/env vars (replaces old RegionNode)
    Manual,
    /// Created by signal clustering (feedback loop)
    Beacon,
    /// Created by Driver A (user search demand aggregation)
    DriverA,
    /// Created by Driver B (global news scanning)
    DriverB,
}

impl std::fmt::Display for ScoutTaskSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manual => write!(f, "manual"),
            Self::Beacon => write!(f, "beacon"),
            Self::DriverA => write!(f, "driver_a"),
            Self::DriverB => write!(f, "driver_b"),
        }
    }
}

impl std::str::FromStr for ScoutTaskSource {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "manual" => Ok(Self::Manual),
            "beacon" => Ok(Self::Beacon),
            "driver_a" => Ok(Self::DriverA),
            "driver_b" => Ok(Self::DriverB),
            other => Err(format!("unknown ScoutTaskSource: {other}")),
        }
    }
}

/// Status of a scout task in the queue.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ScoutTaskStatus {
    Pending,
    Running,
    Completed,
    Cancelled,
}

impl std::fmt::Display for ScoutTaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Running => write!(f, "running"),
            Self::Completed => write!(f, "completed"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::str::FromStr for ScoutTaskStatus {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(format!("unknown ScoutTaskStatus: {other}")),
        }
    }
}

/// An ephemeral unit of work for the scout swarm.
/// Replaces RegionNode as the thing that tells scout where to look.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoutTask {
    pub id: Uuid,
    pub center_lat: f64,
    pub center_lng: f64,
    pub radius_km: f64,
    pub context: String,
    pub geo_terms: Vec<String>,
    pub priority: f64,
    pub source: ScoutTaskSource,
    pub status: ScoutTaskStatus,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl From<&ScoutTask> for ScoutScope {
    fn from(task: &ScoutTask) -> Self {
        ScoutScope {
            center_lat: task.center_lat,
            center_lng: task.center_lng,
            radius_km: task.radius_km,
            name: task.context.clone(),
            geo_terms: task.geo_terms.clone(),
        }
    }
}

// --- Demand Signal (raw user search telemetry for Driver A) ---

/// A raw demand signal recorded from a user search.
/// Aggregated into ScoutTasks by the interval loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandSignal {
    pub id: Uuid,
    pub query: String,
    pub center_lat: f64,
    pub center_lng: f64,
    pub radius_km: f64,
    pub created_at: DateTime<Utc>,
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

// --- Tag Node (thematic tagging for signals and stories) ---

/// A thematic tag that can be applied to signals and stories.
/// Global taxonomy — not city-scoped. Tags are lowercased, slugified.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagNode {
    pub id: Uuid,
    /// Canonical slug (e.g. "ice-enforcement", "housing-displacement")
    pub slug: String,
    /// Human-readable display name (e.g. "ICE Enforcement")
    pub name: String,
    pub created_at: DateTime<Utc>,
}

// --- Web Archive shared types ---

/// A scraped web page with both raw HTML and extracted markdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapedPage {
    pub url: String,
    pub raw_html: String,
    pub markdown: String,
    pub content_hash: String,
}

/// A web search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub url: String,
    pub title: String,
    pub snippet: String,
}

/// A social media post.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialPost {
    pub content: String,
    pub author: Option<String>,
    pub url: Option<String>,
}

/// A single item from an RSS/Atom feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedItem {
    pub url: String,
    pub title: Option<String>,
    pub pub_date: Option<DateTime<Utc>>,
}

/// Extracted content from a PDF document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfContent {
    pub extracted_text: String,
}

// --- Semantic Extraction Types ---

pub const CONTENT_SEMANTICS_VERSION: u32 = 1;

fn default_semantics_version() -> u32 {
    CONTENT_SEMANTICS_VERSION
}

/// Domain-neutral semantic extraction from any web content.
/// Populated lazily via `FetchResponse::semantics()`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContentSemantics {
    pub summary: String,
    pub entities: Vec<SemanticEntity>,
    pub locations: Vec<SemanticLocation>,
    pub contacts: Vec<SemanticContact>,
    pub schedules: Vec<SemanticSchedule>,
    pub claims: Vec<SemanticClaim>,
    #[serde(default)]
    pub temporal_markers: Vec<String>,
    #[serde(default)]
    pub topics: Vec<String>,
    pub provenance: Option<Provenance>,
    pub language: Option<String>,
    #[serde(default)]
    pub outbound_links: Vec<SemanticLink>,
    #[serde(default = "default_semantics_version")]
    pub version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SemanticEntity {
    pub name: String,
    /// "organization", "person", "government_body", "place", "event", "product"
    pub entity_type: String,
    pub description: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SemanticLocation {
    pub name: String,
    pub address: Option<String>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SemanticContact {
    pub name: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub address: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SemanticSchedule {
    pub label: String,
    pub starts_at: Option<String>,
    pub ends_at: Option<String>,
    /// "one-time", "weekly", "monthly", "daily", "irregular"
    pub recurrence: Option<String>,
    pub raw_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SemanticClaim {
    pub statement: String,
    pub attribution: Option<String>,
    /// "statistic", "quote", "policy", "assertion", "announcement"
    pub claim_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Provenance {
    pub author: Option<String>,
    pub published_at: Option<String>,
    pub source_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SemanticLink {
    pub url: String,
    pub label: Option<String>,
    /// "related", "source", "action", "reference", "follow-up"
    pub relationship: Option<String>,
}

/// Deterministic content hash for change detection (FNV-1a).
/// Must be stable across process restarts — `DefaultHasher` is NOT (HashDoS randomization).
pub fn content_hash(content: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325; // FNV offset basis
    for byte in content.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3); // FNV prime
    }
    hash
}

// --- Scraping Strategy (computed from URL, never stored) ---

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrapingStrategy {
    WebQuery,
    WebPage,
    Rss,
    Social(SocialPlatform),
    HtmlListing { link_pattern: &'static str },
}

/// Returns true if the value is a plain-text web query (not a URL).
pub fn is_web_query(value: &str) -> bool {
    !value.starts_with("http://") && !value.starts_with("https://")
}

/// Derive scraping strategy from a source's value (URL or query text).
pub fn scraping_strategy(value: &str) -> ScrapingStrategy {
    if is_web_query(value) {
        return ScrapingStrategy::WebQuery;
    }
    let lower = value.to_lowercase();
    if lower.contains("instagram.com") {
        return ScrapingStrategy::Social(SocialPlatform::Instagram);
    }
    if lower.contains("facebook.com") {
        return ScrapingStrategy::Social(SocialPlatform::Facebook);
    }
    if lower.contains("reddit.com") {
        return ScrapingStrategy::Social(SocialPlatform::Reddit);
    }
    if lower.contains("tiktok.com") {
        return ScrapingStrategy::Social(SocialPlatform::TikTok);
    }
    if lower.contains("twitter.com") || lower.contains("x.com/") {
        return ScrapingStrategy::Social(SocialPlatform::Twitter);
    }
    if lower.contains("bsky.app") {
        return ScrapingStrategy::Social(SocialPlatform::Bluesky);
    }
    if lower.contains("eventbrite.com") && lower.contains("/d/") {
        return ScrapingStrategy::HtmlListing {
            link_pattern: "eventbrite.com/e/",
        };
    }
    if lower.contains("volunteermatch.org") && lower.contains("/search") {
        return ScrapingStrategy::HtmlListing {
            link_pattern: "volunteermatch.org/search/opp",
        };
    }
    if lower.contains("/feed")
        || lower.contains("/rss")
        || lower.contains("/atom")
        || lower.ends_with(".rss")
        || lower.ends_with(".xml")
    {
        return ScrapingStrategy::Rss;
    }
    ScrapingStrategy::WebPage
}

/// Compute a canonical value from a source's raw value (URL or query text).
/// Includes the domain for social sources to prevent key collisions.
pub fn canonical_value(value: &str) -> String {
    if is_web_query(value) {
        return value.to_string();
    }
    let lower = value.to_lowercase();
    if lower.contains("instagram.com") {
        let handle = value
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(value);
        return format!("instagram.com/{}", handle);
    }
    if lower.contains("reddit.com") {
        if let Some(idx) = value.find("/r/") {
            let sub = value[idx + 3..]
                .trim_end_matches('/')
                .split('/')
                .next()
                .unwrap_or(value);
            return format!("reddit.com/r/{}", sub);
        }
    }
    if lower.contains("tiktok.com") {
        let handle = value
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(value)
            .trim_start_matches('@');
        return format!("tiktok.com/{}", handle);
    }
    if lower.contains("twitter.com") || lower.contains("x.com") {
        let handle = value
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(value)
            .trim_start_matches('@');
        return format!("x.com/{}", handle);
    }
    // Everything else: full URL as canonical value
    value.to_string()
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
/// Identity is `canonical_key` = `canonical_value` (region-independent).
/// Regions link to sources via `:SCOUTS` relationships in the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceNode {
    pub id: Uuid,
    /// Unique identity: the canonical_value (URL/content hash). Region-independent.
    pub canonical_key: String,
    /// The handle/query/URL that identifies this source within its type.
    pub canonical_value: String,
    /// URL for web/social sources. None for query-type sources (WebQuery).
    pub url: Option<String>,
    pub discovery_method: DiscoveryMethod,
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

impl SourceNode {
    /// Returns the source's primary value: the URL if present, otherwise the canonical_value.
    /// This is the single source of truth for "what is this source."
    pub fn value(&self) -> &str {
        self.url.as_deref().unwrap_or(&self.canonical_value)
    }
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

/// Classify a URL into a channel type based on domain patterns.
/// No LLM needed — pure pattern matching.
pub fn channel_type(url: &str) -> ChannelType {
    let lower = url.to_lowercase();
    let domain = extract_domain(&lower);

    // Social platforms
    let social_domains = [
        "reddit.com",
        "facebook.com",
        "instagram.com",
        "twitter.com",
        "x.com",
        "tiktok.com",
        "nextdoor.com",
        "threads.net",
        "mastodon.social",
        "bsky.app",
        "linkedin.com",
    ];
    if social_domains.iter().any(|d| domain.contains(d)) {
        return ChannelType::Social;
    }

    // Direct action platforms (fundraising, volunteering, petitions, event ticketing)
    let direct_action_domains = [
        "gofundme.com",
        "eventbrite.com",
        "volunteermatch.org",
        "change.org",
        "givemn.org",
        "givebutter.com",
        "actionnetwork.org",
        "mobilize.us",
        "signupgenius.com",
    ];
    if direct_action_domains.iter().any(|d| domain.contains(d)) {
        return ChannelType::DirectAction;
    }

    // Community media (RSS feeds, community radio/TV, neighborhood newsletters)
    if lower.contains("/feed") || lower.contains("/rss") || lower.contains(".rss") {
        return ChannelType::CommunityMedia;
    }
    let community_media_domains = [
        "patch.com",
        "swnewsmedia.com",
        "southwestjournal.com",
        "tcdailyplanet.net",
    ];
    if community_media_domains.iter().any(|d| domain.contains(d)) {
        return ChannelType::CommunityMedia;
    }

    // Default: press (news articles, org websites, government pages)
    ChannelType::Press
}

fn default_channel_diversity() -> u32 {
    1
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
    /// Aid/Gathering/Need -> Tension (feedback loop). Properties: match_strength, explanation
    RespondsTo,
    /// Actor -> Signal (participation). Properties: role
    ActedIn,
    /// Story -> Story (evolution)
    EvolvedFrom,
    /// Signal <-> Signal (clustering). Properties: weight
    SimilarTo,
    /// Submission -> Source (human submission)
    SubmittedFor,
    /// Aid/Gathering/Need -> Tension (community formation / gathering). Properties: match_strength, explanation, gathering_type
    DrawnTo,
    /// Signal -> Place (gathering venue)
    GathersAt,
    /// Signal/Story -> Tag (thematic tag)
    Tagged,
    /// Story -> Tag (admin suppressed an auto-aggregated tag)
    SuppressedTag,
    /// Need/Gathering -> Resource (must have this capability to help). Properties: confidence, quantity, notes
    Requires,
    /// Need/Gathering -> Resource (better if you have it, not required). Properties: confidence
    Prefers,
    /// Aid -> Resource (this is what we provide). Properties: confidence, capacity
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
            channel_diversity: 1,
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

    // --- channel_type tests ---

    #[test]
    fn channel_type_social_platforms() {
        assert_eq!(channel_type("https://www.reddit.com/r/Minneapolis/comments/abc"), ChannelType::Social);
        assert_eq!(channel_type("https://facebook.com/lakestreetstories"), ChannelType::Social);
        assert_eq!(channel_type("https://www.instagram.com/p/abc123"), ChannelType::Social);
        assert_eq!(channel_type("https://x.com/user/status/123"), ChannelType::Social);
        assert_eq!(channel_type("https://nextdoor.com/post/123"), ChannelType::Social);
    }

    #[test]
    fn channel_type_direct_action() {
        assert_eq!(channel_type("https://www.gofundme.com/f/help-family"), ChannelType::DirectAction);
        assert_eq!(channel_type("https://www.eventbrite.com/e/community-event-123"), ChannelType::DirectAction);
        assert_eq!(channel_type("https://www.volunteermatch.org/search/opp123"), ChannelType::DirectAction);
        assert_eq!(channel_type("https://www.change.org/p/petition-name"), ChannelType::DirectAction);
    }

    #[test]
    fn channel_type_community_media() {
        assert_eq!(channel_type("https://example.com/feed"), ChannelType::CommunityMedia);
        assert_eq!(channel_type("https://example.com/rss"), ChannelType::CommunityMedia);
        assert_eq!(channel_type("https://patch.com/minnesota/minneapolis/story"), ChannelType::CommunityMedia);
        assert_eq!(channel_type("https://swnewsmedia.com/article/123"), ChannelType::CommunityMedia);
    }

    #[test]
    fn channel_type_press_default() {
        assert_eq!(channel_type("https://startribune.com/article/123"), ChannelType::Press);
        assert_eq!(channel_type("https://www.mprnews.org/story/abc"), ChannelType::Press);
        assert_eq!(channel_type("https://citycouncil.gov/minutes"), ChannelType::Press);
    }

    #[test]
    fn channel_type_display_and_as_str() {
        assert_eq!(ChannelType::Press.as_str(), "press");
        assert_eq!(ChannelType::Social.as_str(), "social");
        assert_eq!(ChannelType::DirectAction.as_str(), "direct_action");
        assert_eq!(ChannelType::CommunityMedia.as_str(), "community_media");
        assert_eq!(format!("{}", ChannelType::Press), "press");
    }

    #[test]
    fn channel_type_serde_roundtrip() {
        let ct = ChannelType::DirectAction;
        let json = serde_json::to_string(&ct).unwrap();
        assert_eq!(json, "\"direct_action\"");
        let deserialized: ChannelType = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, ct);
    }
}
