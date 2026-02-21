use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use rootsignal_common::SocialPlatform;

use crate::error::Result;

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
    #[allow(dead_code)]
    pool: PgPool,
    #[allow(dead_code)]
    run_id: Uuid,
    #[allow(dead_code)]
    city_slug: String,
}

impl Archive {
    pub fn new(
        pool: PgPool,
        _config: ArchiveConfig,
        run_id: Uuid,
        city_slug: String,
    ) -> Self {
        Self {
            pool,
            run_id,
            city_slug,
        }
    }

    /// The primary entry point. Pass a URL or query string.
    /// The archive detects what it is, fetches it, records it, returns typed content.
    pub async fn fetch(&self, _target: &str) -> Result<FetchResponse> {
        todo!("Phase 5: implement fetch routing + recording")
    }

    /// Social topic/hashtag search — requires structured input that can't be
    /// encoded as a single URL or query string.
    pub async fn search_social(
        &self,
        _platform: &SocialPlatform,
        _topics: &[&str],
        _limit: u32,
    ) -> Result<FetchResponse> {
        todo!("Phase 5: implement search_social")
    }
}
