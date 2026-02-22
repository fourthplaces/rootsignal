use std::fmt;
use std::time::Instant;

use ai_client::Claude;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tracing::warn;
use uuid::Uuid;

use rootsignal_common::{ContentSemantics, SocialPlatform, CONTENT_SEMANTICS_VERSION};

use crate::error::{ArchiveError, Result};
use crate::fetchers::feed::RssFetcher;
use crate::fetchers::page::{BrowserlessFetcher, ChromeFetcher};
use crate::fetchers::search::SerperFetcher;
use crate::fetchers::social::SocialFetcher;
use crate::router::{detect_content_kind, detect_target, ContentKind, TargetKind};
use crate::semantics;
use crate::store::{ArchiveStore, InsertInteraction};

/// Default search result limit for web queries.
const DEFAULT_SEARCH_LIMIT: usize = 5;
/// Default post limit for social profile fetches.
const DEFAULT_SOCIAL_LIMIT: u32 = 10;
/// Default post limit for Reddit (tends to have more relevant content).
const DEFAULT_REDDIT_LIMIT: u32 = 20;

// ---------------------------------------------------------------------------
// FetchBackend trait + FetchedContent + FetchRequest
// ---------------------------------------------------------------------------

/// Trait for fetching and processing web content.
///
/// Production uses `Archive`; tests can implement this with mock data.
/// The default `fetch()` method returns a `FetchRequest` handle — call
/// `.text()`, `.content()`, or `.semantics()` to execute.
#[async_trait]
pub trait FetchBackend: Send + Sync {
    /// Fetch content and return intermediate result for further processing.
    async fn fetch_content(&self, target: &str) -> Result<FetchedContent>;

    /// Resolve semantics for already-fetched content.
    async fn resolve_semantics(&self, content: &FetchedContent) -> Result<ContentSemantics>;

    /// Run database migrations. Default is a no-op for backends without storage.
    async fn migrate(&self) -> Result<()> {
        Ok(())
    }
}

/// Extension trait that adds `.fetch(target)` to any `FetchBackend`.
///
/// This exists because a default method on `FetchBackend` would require
/// `Self: Sized` for the `&Self → &dyn FetchBackend` coercion, making it
/// uncallable through trait objects.
pub trait FetchBackendExt {
    fn fetch(&self, target: &str) -> FetchRequest<'_>;
}

// Blanket impl for all Sized implementors (Archive, MockArchive, etc.)
impl<T: FetchBackend> FetchBackendExt for T {
    fn fetch(&self, target: &str) -> FetchRequest<'_> {
        FetchRequest::new(self, target)
    }
}

// Explicit impl for trait objects (`dyn FetchBackend`)
impl FetchBackendExt for dyn FetchBackend {
    fn fetch(&self, target: &str) -> FetchRequest<'_> {
        FetchRequest::new(self, target)
    }
}

// Also cover `dyn FetchBackend + Send + Sync` (used via Arc<dyn FetchBackend>)
impl FetchBackendExt for dyn FetchBackend + Send + Sync {
    fn fetch(&self, target: &str) -> FetchRequest<'_> {
        FetchRequest::new(self, target)
    }
}

/// Intermediate result from a fetch.
pub struct FetchedContent {
    pub target: String,
    pub content: Content,
    pub content_hash: String,
    pub fetched_at: DateTime<Utc>,
    pub duration_ms: u32,
    pub text: String,
}

/// Handle to a pending fetch. No work until a terminal method is called.
pub struct FetchRequest<'a> {
    backend: &'a (dyn FetchBackend + 'a),
    target: String,
}

impl<'a> FetchRequest<'a> {
    pub fn new(backend: &'a (dyn FetchBackend + 'a), target: &str) -> Self {
        Self {
            backend,
            target: target.to_string(),
        }
    }

    /// Fetch content, return text representation.
    pub async fn text(self) -> Result<String> {
        let fetched = self.backend.fetch_content(&self.target).await?;
        Ok(fetched.text)
    }

    /// Fetch content, extract semantics (cached by content_hash).
    pub async fn semantics(self) -> Result<ContentSemantics> {
        let fetched = self.backend.fetch_content(&self.target).await?;
        self.backend.resolve_semantics(&fetched).await
    }

    /// Fetch content, return the typed Content enum.
    pub async fn content(self) -> Result<Content> {
        let fetched = self.backend.fetch_content(&self.target).await?;
        Ok(fetched.content)
    }
}

// ---------------------------------------------------------------------------
// Content enum
// ---------------------------------------------------------------------------

/// The detected content type and its parsed data.
#[derive(Debug, Clone)]
pub enum Content {
    Page(rootsignal_common::ScrapedPage),
    Feed(Vec<rootsignal_common::FeedItem>),
    SearchResults(Vec<rootsignal_common::SearchResult>),
    SocialPosts(Vec<rootsignal_common::SocialPost>),
    Pdf(rootsignal_common::PdfContent),
    Raw(String),
}

// ---------------------------------------------------------------------------
// SocialSearch — convenience builder for social search URLs
// ---------------------------------------------------------------------------

/// Convenience type for constructing social search URLs.
/// Pass `social_search.to_string()` to `archive.fetch()`.
pub struct SocialSearch {
    pub platform: SocialPlatform,
    pub topics: Vec<String>,
    pub limit: u32,
}

impl fmt::Display for SocialSearch {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let q = self.topics.join("+");
        match self.platform {
            SocialPlatform::Instagram => {
                write!(f, "https://www.instagram.com/explore/tags/{}?limit={}", q, self.limit)
            }
            SocialPlatform::Reddit => {
                write!(f, "https://www.reddit.com/search/?q={}&limit={}", q, self.limit)
            }
            SocialPlatform::Twitter => {
                write!(f, "https://x.com/search?q={}&limit={}", q, self.limit)
            }
            SocialPlatform::TikTok => {
                write!(f, "https://www.tiktok.com/search?q={}&limit={}", q, self.limit)
            }
            SocialPlatform::Facebook => {
                write!(f, "https://www.facebook.com/search/posts/?q={}&limit={}", q, self.limit)
            }
            SocialPlatform::Bluesky => {
                write!(f, "https://bsky.app/search?q={}&limit={}", q, self.limit)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ArchiveConfig + Archive struct
// ---------------------------------------------------------------------------

/// Configuration for which concrete fetchers to use.
pub struct ArchiveConfig {
    pub page_backend: PageBackend,
    pub serper_api_key: String,
    pub apify_api_key: Option<String>,
    pub anthropic_api_key: Option<String>,
}

pub enum PageBackend {
    Chrome,
    Browserless { base_url: String, token: Option<String> },
}

/// The web, as seen by the scout.
/// Every request fetches fresh, every response is recorded to Postgres.
pub struct Archive {
    store: ArchiveStore,
    claude: Option<Claude>,
    page_fetcher: PageFetcherKind,
    search_fetcher: SerperFetcher,
    social_fetcher: Option<SocialFetcher>,
    feed_fetcher: RssFetcher,
    http_client: reqwest::Client,
    run_id: Uuid,
    region_slug: String,
}

enum PageFetcherKind {
    Chrome(ChromeFetcher),
    Browserless(BrowserlessFetcher),
}

impl Archive {
    pub fn new(
        pool: PgPool,
        config: ArchiveConfig,
        run_id: Uuid,
        region_slug: String,
    ) -> Self {
        let page_fetcher = match config.page_backend {
            PageBackend::Chrome => PageFetcherKind::Chrome(ChromeFetcher::new()),
            PageBackend::Browserless { base_url, token } => {
                PageFetcherKind::Browserless(BrowserlessFetcher::new(
                    &base_url,
                    token.as_deref(),
                ))
            }
        };

        let social_fetcher = config.apify_api_key.map(|key| {
            SocialFetcher::new(apify_client::ApifyClient::new(key))
        });

        let claude = config
            .anthropic_api_key
            .as_deref()
            .filter(|k| !k.is_empty())
            .map(|key| Claude::new(key, "claude-haiku-4-5-20251001"));

        Self {
            store: ArchiveStore::new(pool),
            claude,
            page_fetcher,
            search_fetcher: SerperFetcher::new(&config.serper_api_key),
            social_fetcher,
            feed_fetcher: RssFetcher::new(),
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to build HTTP client"),
            run_id,
            region_slug,
        }
    }

    /// Run database migrations. Call once at startup.
    pub async fn migrate(&self) -> Result<()> {
        self.store.migrate().await
    }

    /// The primary entry point. Pass a URL or query string.
    /// Returns a handle — call `.text()`, `.semantics()`, or `.content()` to execute.
    pub fn fetch(&self, target: &str) -> FetchRequest<'_> {
        FetchRequest::new(self, target)
    }

    // --- Internal fetch methods (used by FetchBackend impl) ---

    async fn do_fetch(&self, target: &str) -> Result<FetchedContent> {
        let start = Instant::now();
        let target_raw = target.to_string();

        match detect_target(target) {
            TargetKind::WebQuery(query) => {
                self.fetch_search(&query, &target_raw, DEFAULT_SEARCH_LIMIT, start).await
            }
            TargetKind::Social { platform, identifier } => {
                let limit = if platform == SocialPlatform::Reddit {
                    DEFAULT_REDDIT_LIMIT
                } else {
                    DEFAULT_SOCIAL_LIMIT
                };
                self.fetch_social_profile(&platform, &identifier, &target_raw, limit, start).await
            }
            TargetKind::SocialSearch { platform, topics, limit } => {
                self.fetch_social_topics(&platform, &topics, limit, &target_raw, start).await
            }
            TargetKind::Url(url) => {
                self.fetch_url(&url, &target_raw, start).await
            }
        }
    }

    async fn fetch_search(
        &self,
        query: &str,
        target_raw: &str,
        max_results: usize,
        start: Instant,
    ) -> Result<FetchedContent> {
        let result = self.search_fetcher.search(query, max_results).await;
        let duration_ms = start.elapsed().as_millis() as u32;
        let fetched_at = Utc::now();

        match result {
            Ok(results) => {
                let json = serde_json::to_value(&results).unwrap_or_default();
                let hash = rootsignal_common::content_hash(&json.to_string()).to_string();

                self.store.insert(InsertInteraction {
                    run_id: self.run_id,
                    region_slug: self.region_slug.clone(),
                    kind: "search".to_string(),
                    target: query.to_string(),
                    target_raw: target_raw.to_string(),
                    fetcher: "serper".to_string(),
                    raw_html: None,
                    markdown: None,
                    response_json: Some(json),
                    raw_bytes: None,
                    content_hash: hash.clone(),
                    duration_ms: duration_ms as i32,
                    error: None,
                    metadata: None,
                    semantics: None,
                }).await;

                let content = Content::SearchResults(results);
                let text = semantics::extractable_text(&content, target_raw);
                Ok(FetchedContent {
                    target: target_raw.to_string(),
                    content,
                    content_hash: hash,
                    fetched_at,
                    duration_ms,
                    text,
                })
            }
            Err(e) => {
                self.record_error("search", query, target_raw, "serper", &e, duration_ms).await;
                Err(ArchiveError::FetchFailed(e.to_string()))
            }
        }
    }

    async fn fetch_social_profile(
        &self,
        platform: &SocialPlatform,
        identifier: &str,
        target_raw: &str,
        limit: u32,
        start: Instant,
    ) -> Result<FetchedContent> {
        let social = self.social_fetcher.as_ref().ok_or_else(|| {
            ArchiveError::FetchFailed("No Apify API key configured".to_string())
        })?;

        let result = social.fetch_posts(platform, identifier, limit).await;
        let duration_ms = start.elapsed().as_millis() as u32;
        let fetched_at = Utc::now();
        let target_normalized = format!("{}:{}", platform_str(platform), identifier);

        match result {
            Ok(posts) => {
                let json = serde_json::to_value(&posts).unwrap_or_default();
                let hash = rootsignal_common::content_hash(&json.to_string()).to_string();

                self.store.insert(InsertInteraction {
                    run_id: self.run_id,
                    region_slug: self.region_slug.clone(),
                    kind: "social".to_string(),
                    target: target_normalized,
                    target_raw: target_raw.to_string(),
                    fetcher: "apify".to_string(),
                    raw_html: None,
                    markdown: None,
                    response_json: Some(json),
                    raw_bytes: None,
                    content_hash: hash.clone(),
                    duration_ms: duration_ms as i32,
                    error: None,
                    metadata: Some(serde_json::json!({
                        "platform": platform_str(platform),
                        "identifier": identifier,
                        "limit": limit,
                    })),
                    semantics: None,
                }).await;

                let content = Content::SocialPosts(posts);
                let text = semantics::extractable_text(&content, target_raw);
                Ok(FetchedContent {
                    target: target_raw.to_string(),
                    content,
                    content_hash: hash,
                    fetched_at,
                    duration_ms,
                    text,
                })
            }
            Err(e) => {
                self.record_error("social", &target_normalized, target_raw, "apify", &e, duration_ms).await;
                Err(ArchiveError::FetchFailed(e.to_string()))
            }
        }
    }

    async fn fetch_social_topics(
        &self,
        platform: &SocialPlatform,
        topics: &[String],
        limit: u32,
        target_raw: &str,
        start: Instant,
    ) -> Result<FetchedContent> {
        let social = self.social_fetcher.as_ref().ok_or_else(|| {
            ArchiveError::FetchFailed("No Apify API key configured".to_string())
        })?;

        let topic_refs: Vec<&str> = topics.iter().map(|s| s.as_str()).collect();
        let result = social.search_topics(platform, &topic_refs, limit).await;
        let duration_ms = start.elapsed().as_millis() as u32;
        let fetched_at = Utc::now();

        match result {
            Ok(posts) => {
                let json = serde_json::to_value(&posts).unwrap_or_default();
                let hash = rootsignal_common::content_hash(&json.to_string()).to_string();

                self.store.insert(InsertInteraction {
                    run_id: self.run_id,
                    region_slug: self.region_slug.clone(),
                    kind: "social".to_string(),
                    target: target_raw.to_string(),
                    target_raw: target_raw.to_string(),
                    fetcher: "apify".to_string(),
                    raw_html: None,
                    markdown: None,
                    response_json: Some(json),
                    raw_bytes: None,
                    content_hash: hash.clone(),
                    duration_ms: duration_ms as i32,
                    error: None,
                    metadata: Some(serde_json::json!({
                        "platform": platform_str(platform),
                        "topics": topics,
                        "limit": limit,
                        "search_type": "topics",
                    })),
                    semantics: None,
                }).await;

                let content = Content::SocialPosts(posts);
                let text = semantics::extractable_text(&content, target_raw);
                Ok(FetchedContent {
                    target: target_raw.to_string(),
                    content,
                    content_hash: hash,
                    fetched_at,
                    duration_ms,
                    text,
                })
            }
            Err(e) => {
                self.record_error("social", target_raw, target_raw, "apify", &e, duration_ms).await;
                Err(ArchiveError::FetchFailed(e.to_string()))
            }
        }
    }

    async fn fetch_url(
        &self,
        url: &str,
        target_raw: &str,
        start: Instant,
    ) -> Result<FetchedContent> {
        // HEAD request to determine content type
        let head_result = self.http_client
            .head(url)
            .header("User-Agent", "rootsignal-archive/0.1")
            .send()
            .await;

        let content_type = match &head_result {
            Ok(resp) => resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("text/html")
                .to_string(),
            Err(_) => "text/html".to_string(), // Default to HTML on HEAD failure
        };

        match detect_content_kind(&content_type, None) {
            ContentKind::Html => self.fetch_page(url, target_raw, start).await,
            ContentKind::Feed => self.fetch_feed(url, target_raw, start).await,
            ContentKind::Pdf => self.fetch_pdf(url, target_raw, start).await,
            ContentKind::Raw => self.fetch_raw(url, target_raw, start).await,
        }
    }

    async fn fetch_page(
        &self,
        url: &str,
        target_raw: &str,
        start: Instant,
    ) -> Result<FetchedContent> {
        let fetcher_name = match &self.page_fetcher {
            PageFetcherKind::Chrome(_) => "chrome",
            PageFetcherKind::Browserless(_) => "browserless",
        };

        let result = match &self.page_fetcher {
            PageFetcherKind::Chrome(f) => f.fetch(url).await,
            PageFetcherKind::Browserless(f) => f.fetch(url).await,
        };

        let duration_ms = start.elapsed().as_millis() as u32;
        let fetched_at = Utc::now();

        match result {
            Ok(page) => {
                let hash = page.content_hash.clone();

                self.store.insert(InsertInteraction {
                    run_id: self.run_id,
                    region_slug: self.region_slug.clone(),
                    kind: "page".to_string(),
                    target: url.to_string(),
                    target_raw: target_raw.to_string(),
                    fetcher: fetcher_name.to_string(),
                    raw_html: Some(page.raw_html.clone()),
                    markdown: Some(page.markdown.clone()),
                    response_json: None,
                    raw_bytes: None,
                    content_hash: hash.clone(),
                    duration_ms: duration_ms as i32,
                    error: None,
                    metadata: None,
                    semantics: None,
                }).await;

                let content = Content::Page(page);
                let text = semantics::extractable_text(&content, target_raw);
                Ok(FetchedContent {
                    target: target_raw.to_string(),
                    content,
                    content_hash: hash,
                    fetched_at,
                    duration_ms,
                    text,
                })
            }
            Err(e) => {
                self.record_error("page", url, target_raw, fetcher_name, &e, duration_ms).await;
                Err(ArchiveError::FetchFailed(e.to_string()))
            }
        }
    }

    async fn fetch_feed(
        &self,
        url: &str,
        target_raw: &str,
        start: Instant,
    ) -> Result<FetchedContent> {
        let result = self.feed_fetcher.fetch_items(url).await;
        let duration_ms = start.elapsed().as_millis() as u32;
        let fetched_at = Utc::now();

        match result {
            Ok(items) => {
                let json = serde_json::to_value(&items).unwrap_or_default();
                let hash = rootsignal_common::content_hash(&json.to_string()).to_string();

                self.store.insert(InsertInteraction {
                    run_id: self.run_id,
                    region_slug: self.region_slug.clone(),
                    kind: "feed".to_string(),
                    target: url.to_string(),
                    target_raw: target_raw.to_string(),
                    fetcher: "reqwest".to_string(),
                    raw_html: None,
                    markdown: None,
                    response_json: Some(json),
                    raw_bytes: None,
                    content_hash: hash.clone(),
                    duration_ms: duration_ms as i32,
                    error: None,
                    metadata: None,
                    semantics: None,
                }).await;

                let content = Content::Feed(items);
                let text = semantics::extractable_text(&content, target_raw);
                Ok(FetchedContent {
                    target: target_raw.to_string(),
                    content,
                    content_hash: hash,
                    fetched_at,
                    duration_ms,
                    text,
                })
            }
            Err(e) => {
                self.record_error("feed", url, target_raw, "reqwest", &e, duration_ms).await;
                Err(ArchiveError::FetchFailed(e.to_string()))
            }
        }
    }

    async fn fetch_pdf(
        &self,
        url: &str,
        target_raw: &str,
        start: Instant,
    ) -> Result<FetchedContent> {
        // Download raw bytes. PDF text extraction is a known gap — store raw bytes for now.
        let resp = self.http_client.get(url)
            .header("User-Agent", "rootsignal-archive/0.1")
            .send()
            .await
            .map_err(|e| ArchiveError::FetchFailed(e.to_string()))?;

        let bytes = resp.bytes().await
            .map_err(|e| ArchiveError::FetchFailed(e.to_string()))?;

        let duration_ms = start.elapsed().as_millis() as u32;
        let fetched_at = Utc::now();
        let hash = rootsignal_common::content_hash(&format!("{}", bytes.len())).to_string();

        let pdf = rootsignal_common::PdfContent {
            extracted_text: String::new(), // TODO: add PDF text extraction
        };

        self.store.insert(InsertInteraction {
            run_id: self.run_id,
            region_slug: self.region_slug.clone(),
            kind: "pdf".to_string(),
            target: url.to_string(),
            target_raw: target_raw.to_string(),
            fetcher: "reqwest".to_string(),
            raw_html: None,
            markdown: None,
            response_json: None,
            raw_bytes: Some(bytes.to_vec()),
            content_hash: hash.clone(),
            duration_ms: duration_ms as i32,
            error: None,
            metadata: None,
            semantics: None,
        }).await;

        let content = Content::Pdf(pdf);
        let text = semantics::extractable_text(&content, target_raw);
        Ok(FetchedContent {
            target: target_raw.to_string(),
            content,
            content_hash: hash,
            fetched_at,
            duration_ms,
            text,
        })
    }

    async fn fetch_raw(
        &self,
        url: &str,
        target_raw: &str,
        start: Instant,
    ) -> Result<FetchedContent> {
        let resp = self.http_client.get(url)
            .header("User-Agent", "rootsignal-archive/0.1")
            .send()
            .await
            .map_err(|e| ArchiveError::FetchFailed(e.to_string()))?;

        let body = resp.text().await
            .map_err(|e| ArchiveError::FetchFailed(e.to_string()))?;

        let duration_ms = start.elapsed().as_millis() as u32;
        let fetched_at = Utc::now();
        let hash = rootsignal_common::content_hash(&body).to_string();

        self.store.insert(InsertInteraction {
            run_id: self.run_id,
            region_slug: self.region_slug.clone(),
            kind: "raw".to_string(),
            target: url.to_string(),
            target_raw: target_raw.to_string(),
            fetcher: "reqwest".to_string(),
            raw_html: None,
            markdown: None,
            response_json: None,
            raw_bytes: None,
            content_hash: hash.clone(),
            duration_ms: duration_ms as i32,
            error: None,
            metadata: Some(serde_json::json!({ "body_preview": &body[..body.len().min(500)] })),
            semantics: None,
        }).await;

        let content = Content::Raw(body);
        let text = semantics::extractable_text(&content, target_raw);
        Ok(FetchedContent {
            target: target_raw.to_string(),
            content,
            content_hash: hash,
            fetched_at,
            duration_ms,
            text,
        })
    }

    async fn record_error(
        &self,
        kind: &str,
        target: &str,
        target_raw: &str,
        fetcher: &str,
        error: &anyhow::Error,
        duration_ms: u32,
    ) {
        warn!(kind, target, error = %error, "Fetch failed");
        self.store.insert(InsertInteraction {
            run_id: self.run_id,
            region_slug: self.region_slug.clone(),
            kind: kind.to_string(),
            target: target.to_string(),
            target_raw: target_raw.to_string(),
            fetcher: fetcher.to_string(),
            raw_html: None,
            markdown: None,
            response_json: None,
            raw_bytes: None,
            content_hash: String::new(),
            duration_ms: duration_ms as i32,
            error: Some(error.to_string()),
            metadata: None,
            semantics: None,
        }).await;
    }
}

// ---------------------------------------------------------------------------
// FetchBackend for Archive
// ---------------------------------------------------------------------------

#[async_trait]
impl FetchBackend for Archive {
    async fn fetch_content(&self, target: &str) -> Result<FetchedContent> {
        self.do_fetch(target).await
    }

    async fn migrate(&self) -> Result<()> {
        self.store.migrate().await
    }

    async fn resolve_semantics(&self, content: &FetchedContent) -> Result<ContentSemantics> {
        // 1. Check DB cache by content_hash
        if let Ok(Some(cached_json)) =
            self.store.semantics_by_content_hash(&content.content_hash).await
        {
            if let Ok(sem) = serde_json::from_value::<ContentSemantics>(cached_json) {
                if sem.version >= CONTENT_SEMANTICS_VERSION {
                    return Ok(sem);
                }
            }
        }

        // 2. Require Claude
        let claude = self.claude.as_ref().ok_or_else(|| {
            ArchiveError::FetchFailed("Claude not configured — cannot extract semantics".to_string())
        })?;

        // 3. Check minimum text length
        if content.text.len() < semantics::MIN_EXTRACT_CHARS {
            return Err(ArchiveError::FetchFailed(
                "Content too short for semantics extraction".to_string(),
            ));
        }

        // 4. LLM extraction
        let content_kind = semantics::content_kind_label(&content.content);
        let sem = semantics::extract_semantics(claude, &content.text, &content.target, content_kind)
            .await
            .map_err(|e| ArchiveError::FetchFailed(format!("Semantics extraction failed: {e}")))?;

        // 5. Persist to DB
        if let Ok(json) = serde_json::to_value(&sem) {
            self.store.update_semantics(&content.content_hash, &json).await;
        }

        Ok(sem)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn platform_str(platform: &SocialPlatform) -> &'static str {
    match platform {
        SocialPlatform::Instagram => "instagram",
        SocialPlatform::Facebook => "facebook",
        SocialPlatform::Reddit => "reddit",
        SocialPlatform::Twitter => "twitter",
        SocialPlatform::TikTok => "tiktok",
        SocialPlatform::Bluesky => "bluesky",
    }
}

// Re-export for use in lib.rs
pub use Content::*;
