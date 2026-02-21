use std::time::Instant;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tracing::warn;
use uuid::Uuid;

use rootsignal_common::SocialPlatform;

use crate::error::{ArchiveError, Result};
use crate::fetchers::feed::RssFetcher;
use crate::fetchers::page::{BrowserlessFetcher, ChromeFetcher};
use crate::fetchers::search::SerperFetcher;
use crate::fetchers::social::SocialFetcher;
use crate::router::{detect_content_kind, detect_target, ContentKind, TargetKind};
use crate::store::{ArchiveStore, InsertInteraction};

/// Default search result limit for web queries.
const DEFAULT_SEARCH_LIMIT: usize = 5;
/// Default post limit for social profile fetches.
const DEFAULT_SOCIAL_LIMIT: u32 = 10;
/// Default post limit for Reddit (tends to have more relevant content).
const DEFAULT_REDDIT_LIMIT: u32 = 20;

/// Response from any archive fetch operation.
#[derive(Debug, Clone)]
pub struct FetchResponse {
    pub target: String,
    pub content: Content,
    pub content_hash: String,
    pub fetched_at: DateTime<Utc>,
    pub duration_ms: u32,
}

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

/// Configuration for which concrete fetchers to use.
pub struct ArchiveConfig {
    pub page_backend: PageBackend,
    pub serper_api_key: String,
    pub apify_api_key: Option<String>,
}

pub enum PageBackend {
    Chrome,
    Browserless { base_url: String, token: Option<String> },
}

/// The web, as seen by the scout.
/// Every request fetches fresh, every response is recorded to Postgres.
pub struct Archive {
    store: ArchiveStore,
    page_fetcher: PageFetcherKind,
    search_fetcher: SerperFetcher,
    social_fetcher: Option<SocialFetcher>,
    feed_fetcher: RssFetcher,
    http_client: reqwest::Client,
    run_id: Uuid,
    city_slug: String,
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
        city_slug: String,
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

        Self {
            store: ArchiveStore::new(pool),
            page_fetcher,
            search_fetcher: SerperFetcher::new(&config.serper_api_key),
            social_fetcher,
            feed_fetcher: RssFetcher::new(),
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to build HTTP client"),
            run_id,
            city_slug,
        }
    }

    /// Run database migrations. Call once at startup.
    pub async fn migrate(&self) -> Result<()> {
        self.store.migrate().await
    }

    /// The primary entry point. Pass a URL or query string.
    /// The archive detects what it is, fetches it, records it, returns typed content.
    pub async fn fetch(&self, target: &str) -> Result<FetchResponse> {
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
            TargetKind::Url(url) => {
                self.fetch_url(&url, &target_raw, start).await
            }
        }
    }

    /// Social topic/hashtag search — requires structured input that can't be
    /// encoded as a single URL or query string.
    pub async fn search_social(
        &self,
        platform: &SocialPlatform,
        topics: &[&str],
        limit: u32,
    ) -> Result<FetchResponse> {
        let start = Instant::now();
        let target_raw = format!("social_topics:{}:{}", platform_str(platform), topics.join(","));

        let social = self.social_fetcher.as_ref().ok_or_else(|| {
            ArchiveError::FetchFailed("No Apify API key configured".to_string())
        })?;

        let result = social.search_topics(platform, topics, limit).await;
        let duration_ms = start.elapsed().as_millis() as u32;
        let fetched_at = Utc::now();

        match result {
            Ok(posts) => {
                let json = serde_json::to_value(&posts).unwrap_or_default();
                let hash = rootsignal_common::content_hash(&json.to_string()).to_string();

                self.store.insert(InsertInteraction {
                    run_id: self.run_id,
                    city_slug: self.city_slug.clone(),
                    kind: "social".to_string(),
                    target: target_raw.clone(),
                    target_raw: target_raw.clone(),
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
                }).await;

                Ok(FetchResponse {
                    target: target_raw,
                    content: Content::SocialPosts(posts),
                    content_hash: hash,
                    fetched_at,
                    duration_ms,
                })
            }
            Err(e) => {
                self.store.insert(InsertInteraction {
                    run_id: self.run_id,
                    city_slug: self.city_slug.clone(),
                    kind: "social".to_string(),
                    target: target_raw.clone(),
                    target_raw: target_raw.clone(),
                    fetcher: "apify".to_string(),
                    raw_html: None,
                    markdown: None,
                    response_json: None,
                    raw_bytes: None,
                    content_hash: String::new(),
                    duration_ms: duration_ms as i32,
                    error: Some(e.to_string()),
                    metadata: Some(serde_json::json!({
                        "platform": platform_str(platform),
                        "topics": topics,
                        "search_type": "topics",
                    })),
                }).await;

                Err(ArchiveError::FetchFailed(e.to_string()))
            }
        }
    }

    async fn fetch_search(
        &self,
        query: &str,
        target_raw: &str,
        max_results: usize,
        start: Instant,
    ) -> Result<FetchResponse> {
        let result = self.search_fetcher.search(query, max_results).await;
        let duration_ms = start.elapsed().as_millis() as u32;
        let fetched_at = Utc::now();

        match result {
            Ok(results) => {
                let json = serde_json::to_value(&results).unwrap_or_default();
                let hash = rootsignal_common::content_hash(&json.to_string()).to_string();

                self.store.insert(InsertInteraction {
                    run_id: self.run_id,
                    city_slug: self.city_slug.clone(),
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
                }).await;

                Ok(FetchResponse {
                    target: target_raw.to_string(),
                    content: Content::SearchResults(results),
                    content_hash: hash,
                    fetched_at,
                    duration_ms,
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
    ) -> Result<FetchResponse> {
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
                    city_slug: self.city_slug.clone(),
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
                }).await;

                Ok(FetchResponse {
                    target: target_raw.to_string(),
                    content: Content::SocialPosts(posts),
                    content_hash: hash,
                    fetched_at,
                    duration_ms,
                })
            }
            Err(e) => {
                self.record_error("social", &target_normalized, target_raw, "apify", &e, duration_ms).await;
                Err(ArchiveError::FetchFailed(e.to_string()))
            }
        }
    }

    async fn fetch_url(
        &self,
        url: &str,
        target_raw: &str,
        start: Instant,
    ) -> Result<FetchResponse> {
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
    ) -> Result<FetchResponse> {
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
                    city_slug: self.city_slug.clone(),
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
                }).await;

                Ok(FetchResponse {
                    target: target_raw.to_string(),
                    content: Content::Page(page),
                    content_hash: hash,
                    fetched_at,
                    duration_ms,
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
    ) -> Result<FetchResponse> {
        let result = self.feed_fetcher.fetch_items(url).await;
        let duration_ms = start.elapsed().as_millis() as u32;
        let fetched_at = Utc::now();

        match result {
            Ok(items) => {
                let json = serde_json::to_value(&items).unwrap_or_default();
                let hash = rootsignal_common::content_hash(&json.to_string()).to_string();

                self.store.insert(InsertInteraction {
                    run_id: self.run_id,
                    city_slug: self.city_slug.clone(),
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
                }).await;

                Ok(FetchResponse {
                    target: target_raw.to_string(),
                    content: Content::Feed(items),
                    content_hash: hash,
                    fetched_at,
                    duration_ms,
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
    ) -> Result<FetchResponse> {
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
            city_slug: self.city_slug.clone(),
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
        }).await;

        Ok(FetchResponse {
            target: target_raw.to_string(),
            content: Content::Pdf(pdf),
            content_hash: hash,
            fetched_at,
            duration_ms,
        })
    }

    async fn fetch_raw(
        &self,
        url: &str,
        target_raw: &str,
        start: Instant,
    ) -> Result<FetchResponse> {
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
            city_slug: self.city_slug.clone(),
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
        }).await;

        Ok(FetchResponse {
            target: target_raw.to_string(),
            content: Content::Raw(body),
            content_hash: hash,
            fetched_at,
            duration_ms,
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
            city_slug: self.city_slug.clone(),
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
        }).await;
    }
}

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
