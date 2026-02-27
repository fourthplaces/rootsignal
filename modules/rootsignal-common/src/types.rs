use anyhow::Result;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::safety::SensitivityLevel;

// --- Re-exports from rootsignal-world ---
pub use rootsignal_world::types::{
    haversine_km, ActorType, ChannelType, DiscoveryMethod, EdgeType, GeoPoint, GeoPrecision,
    NodeType, Severity, SocialPlatform, SourceRole, Urgency,
};
pub use rootsignal_world::values::{Location, Schedule};

// --- Situation Types (system clustering model, not world facts) ---

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

// --- Actor Types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorNode {
    pub id: Uuid,
    pub name: String,
    pub actor_type: ActorType,
    pub canonical_key: String,
    pub domains: Vec<String>,
    pub social_urls: Vec<String>,
    pub description: String,
    pub signal_count: u32,
    pub first_seen: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
    pub typical_roles: Vec<String>,
    // --- Actor profile fields (populated when actor has linked social accounts) ---
    /// Actor bio / description for LLM context.
    pub bio: Option<String>,
    /// Pinned location latitude.
    pub location_lat: Option<f64>,
    /// Pinned location longitude.
    pub location_lng: Option<f64>,
    /// Pinned location display name (e.g. "Minneapolis, MN").
    pub location_name: Option<String>,
    /// How many hops from the bootstrap seed this actor was discovered at.
    /// 0 = bootstrap, 1 = discovered from a bootstrap source, etc.
    pub discovery_depth: u32,
}

/// Context passed from a known actor to the signal extractor.
/// Provides location fallback when posts don't mention geography.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorContext {
    pub actor_name: String,
    pub bio: Option<String>,
    pub location_name: Option<String>,
    pub location_lat: Option<f64>,
    pub location_lng: Option<f64>,
    pub discovery_depth: u32,
}

/// A social account mentioned in a post from a known actor.
/// Used for social graph discovery (Phase 3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MentionedAccount {
    pub platform: SocialPlatform,
    pub handle: String,
    /// How the account was referenced: "mentioned in post", "tagged", "retweeted"
    pub context: String,
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
    pub corroboration_count: u32,
    /// The canonical query/map field — where the content is ABOUT.
    /// At write time: content location if extracted, else falls back to from_location.
    pub about_location: Option<GeoPoint>,
    /// Human-readable content location name.
    pub about_location_name: Option<String>,
    /// Actor's location — provenance for where the signal was posted FROM.
    #[serde(default)]
    pub from_location: Option<GeoPoint>,
    pub source_url: String,
    pub extracted_at: DateTime<Utc>,
    /// When the content was actually published/updated (from LLM extraction, RSS pub_date, or social published_at).
    /// Falls back to `extracted_at` when unavailable.
    #[serde(default)]
    pub published_at: Option<DateTime<Utc>>,
    pub last_confirmed_active: DateTime<Utc>,
    /// Number of unique entity sources (orgs/domains) that have evidence for this signal.
    pub source_diversity: u32,
    /// Cause heat: how much independent community attention exists in this signal's
    /// semantic neighborhood (0.0–1.0). A food shelf Need rises when the housing crisis is trending.
    pub cause_heat: f64,
    /// Implied search queries from this signal for expansion discovery.
    /// Only populated during extraction; cleared after expansion processing.
    pub implied_queries: Vec<String>,
    /// Number of distinct channel types with external entity evidence (1-4).
    #[serde(default = "default_channel_diversity")]
    pub channel_diversity: u32,
    #[serde(default)]
    pub review_status: ReviewStatus,
    #[serde(default)]
    pub was_corrected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corrections: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejection_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    #[default]
    Staged,
    Accepted,
    Rejected,
    Corrected,
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
pub struct CitationNode {
    pub id: Uuid,
    pub source_url: String,
    pub retrieved_at: DateTime<Utc>,
    pub content_hash: String,
    pub snippet: Option<String>,
    pub relevance: Option<String>,
    /// Stored as `evidence_confidence` in Neo4j and event payloads (immutable schema).
    pub confidence: Option<f32>,
    #[serde(default)]
    pub channel_type: Option<ChannelType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleNode {
    pub id: Uuid,
    pub rrule: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rdates: Vec<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exdates: Vec<DateTime<Utc>>,
    pub dtstart: Option<DateTime<Utc>>,
    pub dtend: Option<DateTime<Utc>>,
    pub timezone: Option<String>,
    pub schedule_text: Option<String>,
    pub extracted_at: DateTime<Utc>,
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
    Citation(CitationNode),
}

impl Node {
    pub fn node_type(&self) -> NodeType {
        match self {
            Node::Gathering(_) => NodeType::Gathering,
            Node::Aid(_) => NodeType::Aid,
            Node::Need(_) => NodeType::Need,
            Node::Notice(_) => NodeType::Notice,
            Node::Tension(_) => NodeType::Tension,
            Node::Citation(_) => NodeType::Citation,
        }
    }

    pub fn id(&self) -> Uuid {
        match self {
            Node::Gathering(n) => n.meta.id,
            Node::Aid(n) => n.meta.id,
            Node::Need(n) => n.meta.id,
            Node::Notice(n) => n.meta.id,
            Node::Tension(n) => n.meta.id,
            Node::Citation(n) => n.id,
        }
    }

    pub fn meta(&self) -> Option<&NodeMeta> {
        match self {
            Node::Gathering(n) => Some(&n.meta),
            Node::Aid(n) => Some(&n.meta),
            Node::Need(n) => Some(&n.meta),
            Node::Notice(n) => Some(&n.meta),
            Node::Tension(n) => Some(&n.meta),
            Node::Citation(_) => None,
        }
    }

    pub fn meta_mut(&mut self) -> Option<&mut NodeMeta> {
        match self {
            Node::Gathering(n) => Some(&mut n.meta),
            Node::Aid(n) => Some(&mut n.meta),
            Node::Need(n) => Some(&mut n.meta),
            Node::Notice(n) => Some(&mut n.meta),
            Node::Tension(n) => Some(&mut n.meta),
            Node::Citation(_) => None,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Node::Gathering(n) => &n.meta.title,
            Node::Aid(n) => &n.meta.title,
            Node::Need(n) => &n.meta.title,
            Node::Notice(n) => &n.meta.title,
            Node::Tension(n) => &n.meta.title,
            Node::Citation(n) => &n.source_url,
        }
    }
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

// --- Scout Task (ephemeral unit of work for the scout swarm) ---

/// How a scout task was created.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ScoutTaskSource {
    /// Seeded from config/env vars
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
/// Each task owns its own phase_status (idle → running_bootstrap → ... → complete).
/// Tasks are one-shot and append-only — if a run fails, create a new task rather than retrying.
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
    /// Workflow phase status: "idle", "running_bootstrap", "bootstrap_complete", etc.
    #[serde(default = "default_phase_status")]
    pub phase_status: String,
}

fn default_phase_status() -> String {
    "idle".to_string()
}

impl From<&ScoutTask> for ScoutScope {
    fn from(task: &ScoutTask) -> Self {
        ScoutScope {
            center_lat: task.center_lat,
            center_lng: task.center_lng,
            radius_km: task.radius_km,
            name: task.context.clone(),
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
    pub lat: f64,
    pub lng: f64,
    pub geocoded: bool,
    pub created_at: DateTime<Utc>,
}

// --- Resource Node (capability/resource matching) ---

/// A capability or resource type that signals can require, prefer, or offer.
/// Global taxonomy — not region-scoped. Geographic filtering happens through signal edges.
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
/// Global taxonomy — not region-scoped. Tags are lowercased, slugified.
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

/// A web search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub url: String,
    pub title: String,
    pub snippet: String,
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

// --- Archive Content Types (v2: trait-based content type API) ---

/// A normalized source record. Identity is the normalized URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub id: Uuid,
    pub url: String,
    pub created_at: DateTime<Utc>,
}

/// Universal media record. All media (images, videos, audio, documents) lives here.
/// Text extraction (PDF parsing, transcription, OCR) is stored in the `text` field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveFile {
    pub id: Uuid,
    pub url: String,
    pub content_hash: String,
    pub fetched_at: DateTime<Utc>,
    pub title: Option<String>,
    pub mime_type: String,
    pub duration: Option<f64>,
    pub page_count: Option<i32>,
    pub text: Option<String>,
    pub text_language: Option<String>,
}

/// A social media post (Instagram, Twitter, Reddit, Facebook, TikTok, Bluesky).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Post {
    pub id: Uuid,
    pub source_id: Uuid,
    pub fetched_at: DateTime<Utc>,
    pub content_hash: String,
    pub text: Option<String>,
    pub author: Option<String>,
    pub location: Option<String>,
    pub engagement: Option<serde_json::Value>,
    pub published_at: Option<DateTime<Utc>>,
    pub permalink: Option<String>,
    pub mentions: Vec<String>,
    pub hashtags: Vec<String>,
    pub media_type: Option<String>,
    pub platform_id: Option<String>,
    pub attachments: Vec<ArchiveFile>,
}

/// An ephemeral story (Instagram stories, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Story {
    pub id: Uuid,
    pub source_id: Uuid,
    pub fetched_at: DateTime<Utc>,
    pub content_hash: String,
    pub text: Option<String>,
    pub location: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub permalink: Option<String>,
    pub attachments: Vec<ArchiveFile>,
}

/// A short-form video (Instagram Reels, YouTube Shorts, TikToks).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortVideo {
    pub id: Uuid,
    pub source_id: Uuid,
    pub fetched_at: DateTime<Utc>,
    pub content_hash: String,
    pub text: Option<String>,
    pub location: Option<String>,
    pub engagement: Option<serde_json::Value>,
    pub published_at: Option<DateTime<Utc>>,
    pub permalink: Option<String>,
    pub attachments: Vec<ArchiveFile>,
}

/// A long-form video (YouTube videos, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LongVideo {
    pub id: Uuid,
    pub source_id: Uuid,
    pub fetched_at: DateTime<Utc>,
    pub content_hash: String,
    pub text: Option<String>,
    pub engagement: Option<serde_json::Value>,
    pub published_at: Option<DateTime<Utc>>,
    pub permalink: Option<String>,
    pub attachments: Vec<ArchiveFile>,
}

// --- Channels (declarative content channel selection) ---

/// Declarative selection of which content channels to fetch from a source.
/// Each flag corresponds to a content type; unsupported channels for a given
/// platform are silently skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Channels {
    pub page: bool,
    pub feed: bool,
    pub media: bool,
    pub discussion: bool,
    pub events: bool,
}

impl Channels {
    /// All channels enabled.
    pub fn everything() -> Self {
        Self {
            page: true,
            feed: true,
            media: true,
            discussion: true,
            events: true,
        }
    }

    /// Only the page channel.
    pub fn page() -> Self {
        Self {
            page: true,
            ..Default::default()
        }
    }

    /// Only the feed channel.
    pub fn feed() -> Self {
        Self {
            feed: true,
            ..Default::default()
        }
    }

    /// Only the media channel.
    pub fn media() -> Self {
        Self {
            media: true,
            ..Default::default()
        }
    }

    pub fn with_page(mut self) -> Self {
        self.page = true;
        self
    }

    pub fn with_feed(mut self) -> Self {
        self.feed = true;
        self
    }

    pub fn with_media(mut self) -> Self {
        self.media = true;
        self
    }

    pub fn is_empty(&self) -> bool {
        !self.page && !self.feed && !self.media && !self.discussion && !self.events
    }
}

/// A unified result item from a multi-channel fetch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArchiveItem {
    Page(ArchivedPage),
    Feed(ArchivedFeed),
    Posts(Vec<Post>),
    Stories(Vec<Story>),
    ShortVideos(Vec<ShortVideo>),
}

/// A scraped web page (v2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedPage {
    pub id: Uuid,
    pub source_id: Uuid,
    pub fetched_at: DateTime<Utc>,
    pub content_hash: String,
    pub raw_html: String,
    pub markdown: String,
    pub title: Option<String>,
    pub links: Vec<String>,
    #[serde(default)]
    pub published_at: Option<DateTime<Utc>>,
}

/// A fetched RSS/Atom feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedFeed {
    pub id: Uuid,
    pub source_id: Uuid,
    pub fetched_at: DateTime<Utc>,
    pub content_hash: String,
    pub items: Vec<FeedItem>,
    pub title: Option<String>,
}

/// A set of web search results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedSearchResults {
    pub id: Uuid,
    pub source_id: Uuid,
    pub fetched_at: DateTime<Utc>,
    pub content_hash: String,
    pub query: String,
    pub results: Vec<SearchResult>,
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
    if lower.contains("facebook.com") && is_facebook_page_url(value) {
        return ScrapingStrategy::Social(SocialPlatform::Facebook);
    }
    if lower.contains("facebook.com") {
        return ScrapingStrategy::WebPage;
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

impl std::fmt::Display for ScrapingStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScrapingStrategy::WebQuery => write!(f, "web_query"),
            ScrapingStrategy::WebPage => write!(f, "web_page"),
            ScrapingStrategy::Rss => write!(f, "rss"),
            ScrapingStrategy::Social(p) => {
                let label = match p {
                    SocialPlatform::Instagram => "instagram",
                    SocialPlatform::Facebook => "facebook",
                    SocialPlatform::Reddit => "reddit",
                    SocialPlatform::Twitter => "twitter",
                    SocialPlatform::TikTok => "tiktok",
                    SocialPlatform::Bluesky => "bluesky",
                };
                write!(f, "social:{label}")
            }
            ScrapingStrategy::HtmlListing { link_pattern } => {
                write!(f, "html_listing:{link_pattern}")
            }
        }
    }
}

/// Returns true if a Facebook URL points to a page or group homepage
/// (not an individual post, photo, event, etc.).
pub fn is_facebook_page_url(url: &str) -> bool {
    let path = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .skip(1) // skip domain
        .collect::<Vec<_>>();

    match path.as_slice() {
        // facebook.com/<slug> or facebook.com/<slug>/
        [slug] | [slug, ""] if is_valid_fb_slug(slug) => true,
        // facebook.com/groups/<name> or facebook.com/groups/<name>/
        ["groups", name] | ["groups", name, ""] if !name.is_empty() => true,
        // facebook.com/pg/<slug> (legacy page URL)
        ["pg", slug] | ["pg", slug, ""] if is_valid_fb_slug(slug) => true,
        _ => false,
    }
}

/// A valid Facebook slug is non-empty, doesn't start with reserved path
/// segments, and doesn't look like a numeric post ID.
fn is_valid_fb_slug(slug: &str) -> bool {
    let slug = slug.trim_end_matches('/');
    if slug.is_empty() {
        return false;
    }
    let reserved = [
        "photo",
        "photos",
        "video",
        "videos",
        "events",
        "posts",
        "story",
        "stories",
        "watch",
        "marketplace",
        "gaming",
        "login",
        "help",
        "settings",
        "messages",
        "notifications",
        "bookmarks",
        "pages",
        "groups",
        "profile.php",
        "permalink.php",
        "share",
    ];
    if reserved.contains(&slug.to_lowercase().as_str()) {
        return false;
    }
    // Pure numeric = post/object ID, not a page slug
    if slug.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    true
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

/// An ephemeral pin — a one-shot instruction to scrape a source at a location.
/// Created during bootstrap or mid-run discovery, consumed after the scout run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinNode {
    pub id: Uuid,
    pub location_lat: f64,
    pub location_lng: f64,
    pub source_id: Uuid,
    /// Who created this pin: scout run ID or "human".
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

/// A tracked source in the graph — either curated (from seed list) or discovered.
/// Identity is `canonical_key` = `canonical_value` (globally unique).
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
    /// Create a new SourceNode with sensible defaults for bookkeeping fields.
    pub fn new(
        canonical_key: String,
        canonical_value: String,
        url: Option<String>,
        discovery_method: DiscoveryMethod,
        weight: f64,
        source_role: SourceRole,
        gap_context: Option<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            canonical_key,
            canonical_value,
            url,
            discovery_method,
            created_at: Utc::now(),
            last_scraped: None,
            last_produced_signal: None,
            signals_produced: 0,
            signals_corroborated: 0,
            consecutive_empty_runs: 0,
            active: true,
            gap_context,
            weight,
            cadence_hours: None,
            avg_signals_per_scrape: 0.0,
            quality_penalty: 1.0,
            source_role,
            scrape_count: 0,
        }
    }

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
    pub canonical_key: String,
    pub domains: Vec<String>,
    pub instagram: Vec<String>,
    pub facebook: Vec<String>,
    pub reddit: Vec<String>,
}

/// Resolve a source URL to its parent entity ID using entity mappings.
/// Returns the canonical_key if matched, otherwise extracts the domain as a fallback entity.
pub fn resolve_entity(url: &str, mappings: &[EntityMappingOwned]) -> String {
    let domain = extract_domain(url);

    for mapping in mappings {
        for d in &mapping.domains {
            if domain.contains(d.as_str()) {
                return mapping.canonical_key.clone();
            }
        }
        for ig in &mapping.instagram {
            if url.contains(&format!("instagram.com/{ig}")) {
                return mapping.canonical_key.clone();
            }
        }
        for fb in &mapping.facebook {
            if url.contains(fb.as_str()) {
                return mapping.canonical_key.clone();
            }
        }
        for r in &mapping.reddit {
            if url.contains(&format!("reddit.com/user/{r}"))
                || url.contains(&format!("reddit.com/u/{r}"))
            {
                return mapping.canonical_key.clone();
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

// --- TextEmbedder trait (shared across crates) ---

#[async_trait::async_trait]
pub trait TextEmbedder: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
    async fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>>;
}

// --- EmbeddingLookup trait (get-or-compute cache interface) ---

#[async_trait::async_trait]
pub trait EmbeddingLookup: Send + Sync {
    /// Get an embedding for the given text. Cache hit = instant. Miss = compute + store.
    async fn get(&self, text: &str) -> Result<Vec<f32>>;
}

// --- Situation Types ---

/// A living situation: a root cause + affected population + place.
/// Organizational layer on top of the signal graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SituationNode {
    pub id: Uuid,
    pub headline: String,
    pub lede: String,
    pub arc: SituationArc,

    // Temperature components (all 0.0-1.0, derived from graph)
    pub temperature: f64,
    pub tension_heat: f64,
    pub entity_velocity: f64,
    pub amplification: f64,
    pub response_coverage: f64,
    pub clarity_need: f64,

    pub clarity: Clarity,

    pub centroid_lat: Option<f64>,
    pub centroid_lng: Option<f64>,
    pub location_name: Option<String>,

    /// LLM working memory (JSON blob). NOT exposed via public API.
    pub structured_state: String,

    pub signal_count: u32,
    pub tension_count: u32,
    pub dispatch_count: u32,
    pub first_seen: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
    pub sensitivity: SensitivityLevel,
    pub category: Option<String>,
}

/// An atomic dispatch in a situation's living narrative thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchNode {
    pub id: Uuid,
    pub situation_id: Uuid,
    pub body: String,
    pub signal_ids: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
    pub dispatch_type: DispatchType,
    pub supersedes: Option<Uuid>,
    pub flagged_for_review: bool,
    pub flag_reason: Option<String>,
    pub fidelity_score: Option<f64>,
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
            corroboration_count: 0,
            about_location: None,
            about_location_name: None,
            from_location: None,
            source_url: "https://example.com".to_string(),
            extracted_at: Utc::now(),
            published_at: None,
            last_confirmed_active: Utc::now(),
            source_diversity: 1,
            cause_heat: 0.0,
            channel_diversity: 1,
            implied_queries: vec![],
            review_status: ReviewStatus::Staged,
            was_corrected: false,
            corrections: None,
            rejection_reason: None,
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
        assert_eq!(
            channel_type("https://www.reddit.com/r/Minneapolis/comments/abc"),
            ChannelType::Social
        );
        assert_eq!(
            channel_type("https://facebook.com/lakestreetstories"),
            ChannelType::Social
        );
        assert_eq!(
            channel_type("https://www.instagram.com/p/abc123"),
            ChannelType::Social
        );
        assert_eq!(
            channel_type("https://x.com/user/status/123"),
            ChannelType::Social
        );
        assert_eq!(
            channel_type("https://nextdoor.com/post/123"),
            ChannelType::Social
        );
    }

    #[test]
    fn channel_type_direct_action() {
        assert_eq!(
            channel_type("https://www.gofundme.com/f/help-family"),
            ChannelType::DirectAction
        );
        assert_eq!(
            channel_type("https://www.eventbrite.com/e/community-event-123"),
            ChannelType::DirectAction
        );
        assert_eq!(
            channel_type("https://www.volunteermatch.org/search/opp123"),
            ChannelType::DirectAction
        );
        assert_eq!(
            channel_type("https://www.change.org/p/petition-name"),
            ChannelType::DirectAction
        );
    }

    #[test]
    fn channel_type_community_media() {
        assert_eq!(
            channel_type("https://example.com/feed"),
            ChannelType::CommunityMedia
        );
        assert_eq!(
            channel_type("https://example.com/rss"),
            ChannelType::CommunityMedia
        );
        assert_eq!(
            channel_type("https://patch.com/minnesota/minneapolis/story"),
            ChannelType::CommunityMedia
        );
        assert_eq!(
            channel_type("https://swnewsmedia.com/article/123"),
            ChannelType::CommunityMedia
        );
    }

    #[test]
    fn channel_type_press_default() {
        assert_eq!(
            channel_type("https://startribune.com/article/123"),
            ChannelType::Press
        );
        assert_eq!(
            channel_type("https://www.mprnews.org/story/abc"),
            ChannelType::Press
        );
        assert_eq!(
            channel_type("https://citycouncil.gov/minutes"),
            ChannelType::Press
        );
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

    // -----------------------------------------------------------------------
    // canonical_value() test battery
    // -----------------------------------------------------------------------

    // --- Instagram ---

    #[test]
    fn canonical_value_instagram_basic() {
        assert_eq!(
            canonical_value("https://instagram.com/mplsmutualaid"),
            "instagram.com/mplsmutualaid"
        );
    }

    #[test]
    fn canonical_value_instagram_www() {
        assert_eq!(
            canonical_value("https://www.instagram.com/mplsmutualaid"),
            "instagram.com/mplsmutualaid"
        );
    }

    #[test]
    fn canonical_value_instagram_trailing_slash() {
        assert_eq!(
            canonical_value("https://www.instagram.com/mplsmutualaid/"),
            "instagram.com/mplsmutualaid"
        );
    }

    #[test]
    fn canonical_value_instagram_dedup_www_and_trailing() {
        // www + trailing slash and bare URL must produce the same key
        let a = canonical_value("https://www.instagram.com/mplsmutualaid/");
        let b = canonical_value("https://instagram.com/mplsmutualaid");
        assert_eq!(a, b);
    }

    // --- Twitter / X ---

    #[test]
    fn canonical_value_twitter_normalizes_to_x() {
        assert_eq!(
            canonical_value("https://twitter.com/johndoe"),
            "x.com/johndoe"
        );
    }

    #[test]
    fn canonical_value_x_com() {
        assert_eq!(canonical_value("https://x.com/johndoe"), "x.com/johndoe");
    }

    #[test]
    fn canonical_value_twitter_and_x_dedup() {
        let a = canonical_value("https://twitter.com/handle");
        let b = canonical_value("https://x.com/handle");
        assert_eq!(a, b);
    }

    #[test]
    fn canonical_value_twitter_www_trailing() {
        assert_eq!(
            canonical_value("https://www.twitter.com/handle/"),
            "x.com/handle"
        );
    }

    #[test]
    fn canonical_value_twitter_strips_at() {
        // @ prefix should be stripped
        assert_eq!(canonical_value("https://x.com/@handle"), "x.com/handle");
    }

    // --- TikTok ---

    #[test]
    fn canonical_value_tiktok_with_at() {
        assert_eq!(
            canonical_value("https://tiktok.com/@dancer123"),
            "tiktok.com/dancer123"
        );
    }

    #[test]
    fn canonical_value_tiktok_www_trailing() {
        assert_eq!(
            canonical_value("https://www.tiktok.com/@handle/"),
            "tiktok.com/handle"
        );
    }

    #[test]
    fn canonical_value_tiktok_without_at() {
        assert_eq!(
            canonical_value("https://tiktok.com/dancer123"),
            "tiktok.com/dancer123"
        );
    }

    // --- Reddit ---

    #[test]
    fn canonical_value_reddit_subreddit() {
        assert_eq!(
            canonical_value("https://www.reddit.com/r/Minneapolis/"),
            "reddit.com/r/Minneapolis"
        );
    }

    #[test]
    fn canonical_value_reddit_preserves_case() {
        assert_eq!(
            canonical_value("https://reddit.com/r/MutualAid"),
            "reddit.com/r/MutualAid"
        );
    }

    #[test]
    fn canonical_value_reddit_strips_trailing_path() {
        // /r/Sub/comments/xyz should still extract just the subreddit
        assert_eq!(
            canonical_value("https://www.reddit.com/r/Minneapolis/comments/abc123"),
            "reddit.com/r/Minneapolis"
        );
    }

    #[test]
    fn canonical_value_reddit_no_subreddit_passthrough() {
        // Reddit URL without /r/ (e.g. user profile) — no special handling, passes through
        let url = "https://reddit.com/user/someone";
        assert_eq!(canonical_value(url), url);
    }

    // --- Facebook (no special handling — documenting current behavior) ---

    #[test]
    fn canonical_value_facebook_passthrough() {
        // Facebook URLs are NOT normalized — they pass through as-is
        let url = "https://facebook.com/local_org";
        assert_eq!(canonical_value(url), url);
    }

    #[test]
    fn canonical_value_facebook_www_not_stripped() {
        // www is NOT stripped for Facebook — different URLs produce different keys
        let a = canonical_value("https://www.facebook.com/local_org");
        let b = canonical_value("https://facebook.com/local_org");
        // Documenting the gap: these SHOULD be equal but currently aren't
        assert_ne!(a, b, "Facebook www stripping is a known gap");
    }

    // --- Bluesky (no special handling — documenting current behavior) ---

    #[test]
    fn canonical_value_bluesky_passthrough() {
        // Bluesky URLs pass through as-is — no normalization
        let url = "https://bsky.app/profile/user.bsky.social";
        assert_eq!(canonical_value(url), url);
    }

    // --- Web queries ---

    #[test]
    fn canonical_value_web_query_passthrough() {
        assert_eq!(
            canonical_value("site:linktr.ee mutual aid Minneapolis"),
            "site:linktr.ee mutual aid Minneapolis"
        );
    }

    #[test]
    fn canonical_value_web_query_plain_text() {
        assert_eq!(
            canonical_value("mutual aid Minneapolis food shelf"),
            "mutual aid Minneapolis food shelf"
        );
    }

    // --- Web URL edge cases (documenting current behavior) ---

    #[test]
    fn canonical_value_web_url_www_not_stripped() {
        // www is NOT stripped for generic web URLs
        let a = canonical_value("https://www.example.com/page");
        let b = canonical_value("https://example.com/page");
        assert_ne!(a, b, "www stripping for web URLs is a known gap");
    }

    #[test]
    fn canonical_value_web_url_fragment_not_stripped() {
        // Fragments are NOT stripped
        let a = canonical_value("https://example.com/page#section");
        let b = canonical_value("https://example.com/page");
        assert_ne!(a, b, "Fragment stripping is a known gap");
    }

    #[test]
    fn canonical_value_web_url_trailing_slash_not_stripped() {
        // Trailing slashes NOT stripped for generic web URLs
        let a = canonical_value("https://example.com/page/");
        let b = canonical_value("https://example.com/page");
        assert_ne!(a, b, "Trailing slash stripping for web URLs is a known gap");
    }

    #[test]
    fn canonical_value_web_url_case_preserved() {
        // Case is preserved for generic web URLs
        let a = canonical_value("https://Example.COM/Page");
        let b = canonical_value("https://example.com/page");
        assert_ne!(a, b, "Case normalization for web URLs is a known gap");
    }

    #[test]
    fn canonical_value_web_url_query_params_preserved() {
        // Query params are preserved (no tracking param stripping)
        let url = "https://example.com/page?utm_source=ig&important=yes";
        assert_eq!(canonical_value(url), url);
    }

    // --- Edge cases ---

    #[test]
    fn canonical_value_empty_string() {
        // Empty string is a web query (no http prefix) — passes through
        assert_eq!(canonical_value(""), "");
    }

    #[test]
    fn canonical_value_instagram_bare_domain() {
        // Just the domain, no handle path
        let result = canonical_value("https://instagram.com");
        assert_eq!(result, "instagram.com/instagram.com");
    }

    #[test]
    fn canonical_value_http_not_https() {
        // http:// should still be recognized as a URL (not a web query)
        let url = "http://example.com/page";
        assert_eq!(canonical_value(url), url);
    }

    #[test]
    fn canonical_value_is_web_query_check() {
        // Verify is_web_query aligns with canonical_value behavior
        assert!(is_web_query("site:linktr.ee mutual aid"));
        assert!(is_web_query("mutual aid Minneapolis"));
        assert!(!is_web_query("https://example.com"));
        assert!(!is_web_query("http://example.com"));
    }

    // --- Channels tests ---

    #[test]
    fn channels_default_is_empty() {
        let ch = Channels::default();
        assert!(ch.is_empty());
        assert!(!ch.page);
        assert!(!ch.feed);
        assert!(!ch.media);
        assert!(!ch.discussion);
        assert!(!ch.events);
    }

    #[test]
    fn channels_everything_enables_all() {
        let ch = Channels::everything();
        assert!(!ch.is_empty());
        assert!(ch.page);
        assert!(ch.feed);
        assert!(ch.media);
        assert!(ch.discussion);
        assert!(ch.events);
    }

    #[test]
    fn channels_named_constructors_enable_single_flag() {
        let p = Channels::page();
        assert!(p.page);
        assert!(!p.feed);
        assert!(!p.media);

        let f = Channels::feed();
        assert!(!f.page);
        assert!(f.feed);
        assert!(!f.media);

        let m = Channels::media();
        assert!(!m.page);
        assert!(!m.feed);
        assert!(m.media);
    }

    #[test]
    fn channels_builder_composes() {
        let ch = Channels::page().with_feed().with_media();
        assert!(ch.page);
        assert!(ch.feed);
        assert!(ch.media);
        assert!(!ch.discussion);
        assert!(!ch.events);
        assert!(!ch.is_empty());
    }

    #[test]
    fn channels_serde_roundtrip() {
        let ch = Channels {
            page: true,
            feed: false,
            media: true,
            discussion: false,
            events: true,
        };
        let json = serde_json::to_string(&ch).unwrap();
        let deserialized: Channels = serde_json::from_str(&json).unwrap();
        assert_eq!(ch, deserialized);
    }

    #[test]
    fn channels_literal_construction() {
        let ch = Channels {
            feed: true,
            media: true,
            ..Default::default()
        };
        assert!(!ch.page);
        assert!(ch.feed);
        assert!(ch.media);
        assert!(!ch.discussion);
        assert!(!ch.events);
    }

    // --- Facebook classification tests ---

    #[test]
    fn facebook_page_homepage_routes_to_social() {
        assert_eq!(
            scraping_strategy("https://www.facebook.com/lakestreetstories"),
            ScrapingStrategy::Social(SocialPlatform::Facebook)
        );
    }

    #[test]
    fn facebook_group_routes_to_social() {
        assert_eq!(
            scraping_strategy("https://www.facebook.com/groups/mpls-mutual-aid"),
            ScrapingStrategy::Social(SocialPlatform::Facebook)
        );
    }

    #[test]
    fn facebook_post_url_routes_to_webpage() {
        assert_eq!(
            scraping_strategy("https://www.facebook.com/lakestreetstories/posts/123456"),
            ScrapingStrategy::WebPage
        );
    }

    #[test]
    fn facebook_event_url_routes_to_webpage() {
        assert_eq!(
            scraping_strategy("https://www.facebook.com/events/123456"),
            ScrapingStrategy::WebPage
        );
    }

    #[test]
    fn facebook_photo_url_routes_to_webpage() {
        assert_eq!(
            scraping_strategy("https://www.facebook.com/photo/123456"),
            ScrapingStrategy::WebPage
        );
    }

    #[test]
    fn scraping_strategy_display_formats() {
        assert_eq!(format!("{}", ScrapingStrategy::WebPage), "web_page");
        assert_eq!(format!("{}", ScrapingStrategy::WebQuery), "web_query");
        assert_eq!(format!("{}", ScrapingStrategy::Rss), "rss");
    }
}
