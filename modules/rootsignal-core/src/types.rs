use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Raw page fetched from any adapter — the universal currency of scraping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawPage {
    pub url: String,
    pub content: String,
    pub title: Option<String>,
    pub html: Option<String>,
    pub content_type: Option<String>,
    pub fetched_at: DateTime<Utc>,
    pub metadata: HashMap<String, serde_json::Value>,
}

impl RawPage {
    pub fn new(url: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            content: content.into(),
            title: None,
            html: None,
            content_type: None,
            fetched_at: Utc::now(),
            metadata: HashMap::new(),
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_html(mut self, html: impl Into<String>) -> Self {
        self.html = Some(html.into());
        self
    }

    pub fn with_content_type(mut self, ct: impl Into<String>) -> Self {
        self.content_type = Some(ct.into());
        self
    }

    pub fn with_fetched_at(mut self, at: DateTime<Utc>) -> Self {
        self.fetched_at = at;
        self
    }

    pub fn with_metadata(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Check if this page has meaningful content.
    pub fn has_content(&self) -> bool {
        !self.content.trim().is_empty()
    }

    /// Extract the site URL (scheme + host) from this page's URL.
    pub fn site_url(&self) -> Option<String> {
        url::Url::parse(&self.url)
            .ok()
            .map(|u| format!("{}://{}", u.scheme(), u.host_str().unwrap_or("")))
    }

    /// SHA-256 hash of the content for dedup.
    pub fn content_hash(&self) -> Vec<u8> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(self.content.as_bytes());
        hasher.finalize().to_vec()
    }
}

/// Result of AI extraction from a page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    pub data: serde_json::Value,
    pub confidence_overall: f32,
    pub confidence_ai: f32,
    pub schema_version: i32,
    pub fingerprint: Vec<u8>,
}

/// What the AI extracts from a page — structured listing data.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExtractedListing {
    pub title: String,
    pub description: Option<String>,
    pub listing_type: String,
    pub categories: Vec<String>,
    pub audience_roles: Vec<String>,

    // Entity
    pub organization_name: Option<String>,
    pub organization_type: Option<String>,

    // Location
    pub location_text: Option<String>,
    pub address: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub postal_code: Option<String>,

    // Timing
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub is_recurring: Option<bool>,
    pub recurrence_description: Option<String>,

    // Contact
    pub contact_name: Option<String>,
    pub contact_email: Option<String>,
    pub contact_phone: Option<String>,

    // Service
    pub service_name: Option<String>,
    pub eligibility: Option<String>,
    pub fees: Option<String>,

    // Source
    pub source_url: Option<String>,

    // Urgency / capacity signals
    pub urgency: Option<String>,
    pub capacity_note: Option<String>,

    // Taxonomy dimensions (tag-based)
    pub signal_domain: Option<String>,
    pub capacity_status: Option<String>,
    pub confidence_hint: Option<String>,
    pub radius_relevant: Option<String>,
    pub populations: Option<Vec<String>>,

    // Temporal
    pub expires_at: Option<String>,

    /// Detected language of the source content (ISO 639-1: en, es, so, ht)
    pub source_locale: Option<String>,
}

/// Wrapper for batch extraction response.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExtractedListings {
    pub listings: Vec<ExtractedListing>,
}

/// Polymorphic resource type discriminator used across the system
/// (taggables, locationables, noteables, embeddings, translations, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    Listing,
    Entity,
    Service,
}

impl ResourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Listing => "listing",
            Self::Entity => "entity",
            Self::Service => "service",
        }
    }
}

impl std::fmt::Display for ResourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ResourceType {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "listing" => Ok(Self::Listing),
            "entity" => Ok(Self::Entity),
            "service" => Ok(Self::Service),
            _ => Err(anyhow::anyhow!("Unknown resource type: {}", s)),
        }
    }
}

/// Enum for source adapter types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterType {
    Firecrawl,
    Tavily,
    Http,
    Apify,
    Eventbrite,
}

impl std::fmt::Display for AdapterType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Firecrawl => write!(f, "firecrawl"),
            Self::Tavily => write!(f, "tavily"),
            Self::Http => write!(f, "http"),
            Self::Apify => write!(f, "apify"),
            Self::Eventbrite => write!(f, "eventbrite"),
        }
    }
}

impl std::str::FromStr for AdapterType {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "firecrawl" => Ok(Self::Firecrawl),
            "tavily" => Ok(Self::Tavily),
            "http" => Ok(Self::Http),
            "apify" => Ok(Self::Apify),
            "eventbrite" => Ok(Self::Eventbrite),
            _ => Err(anyhow::anyhow!("Unknown adapter type: {}", s)),
        }
    }
}

/// Extraction status for page_snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

impl std::fmt::Display for ExtractionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Processing => write!(f, "processing"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

impl std::str::FromStr for ExtractionStatus {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "processing" => Ok(Self::Processing),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            _ => Err(anyhow::anyhow!("Unknown extraction status: {}", s)),
        }
    }
}
