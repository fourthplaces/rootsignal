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

/// What the AI extracts from content — a semantic signal.
///
/// schema.org alignment:
///   signal_type "ask"         → schema:Demand
///   signal_type "give"        → schema:Offer
///   signal_type "event"       → schema:Event
///   signal_type "informative" → schema:Report
///   about                     → schema:about (subject matter)
///   location_text             → schema:location (geocoded into locationables)
///   start_date / end_date     → schema:startDate / endDate (stored in schedules)
///   source_locale             → schema:inLanguage (BCP 47)
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExtractedSignal {
    /// Signal type: ask, give, event, informative
    pub signal_type: String,
    /// Natural language description of the signal
    pub content: String,
    /// schema.org: about — what's being asked/given/discussed
    pub about: Option<String>,
    /// Location mention (raw text — extraction activity geocodes into locationables)
    pub location_text: Option<String>,
    /// Address components (extraction activity creates Location record)
    pub address: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub postal_code: Option<String>,
    /// Temporal (ISO 8601 — extraction activity creates Schedule record)
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    /// Local start time at the venue (HH:MM, 24h format), e.g. "20:00"
    pub start_time: Option<String>,
    /// Local end time at the venue (HH:MM, 24h format), e.g. "22:00"
    pub end_time: Option<String>,
    pub is_recurring: Option<bool>,
    pub recurrence_description: Option<String>,
    /// When this signal was broadcast into the world (ISO 8601), if visible on the page
    pub broadcasted_at: Option<String>,
    /// Source URL
    pub source_url: Option<String>,
    /// Detected language (BCP 47)
    pub source_locale: Option<String>,
    /// Whether this signal hints at a deeper phenomenon worth investigating
    pub needs_investigation: Option<bool>,
    /// Brief reason why investigation is warranted
    pub investigation_reason: Option<String>,
    /// If this signal updates a previously known signal, set this to the
    /// alias provided in the prompt context (e.g. "signal_3").
    /// Leave null if this is a new signal.
    pub existing_signal_alias: Option<String>,
}

/// Wrapper for batch signal extraction response.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExtractedSignals {
    pub signals: Vec<ExtractedSignal>,
}

/// Polymorphic resource type discriminator used across the system
/// (taggables, locationables, noteables, embeddings, translations, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    Entity,
    Service,
    Signal,
    Finding,
}

impl ResourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Entity => "entity",
            Self::Service => "service",
            Self::Signal => "signal",
            Self::Finding => "finding",
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
            "entity" => Ok(Self::Entity),
            "service" => Ok(Self::Service),
            "signal" => Ok(Self::Signal),
            "finding" => Ok(Self::Finding),
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
