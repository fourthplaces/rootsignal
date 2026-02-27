// Archive: the public entry point for the content-type API.
// Callers use `archive.source(url)` to get a SourceHandle, then call
// content-type methods on it (.posts(), .stories(), .page(), etc.).

use std::sync::Arc;

use sqlx::PgPool;

use crate::enrichment::WorkflowDispatcher;
use crate::error::Result;
use crate::router::{detect_platform, extract_identifier, normalize_url};
use crate::services::bluesky::BlueskyService;
use crate::services::facebook::FacebookService;
use crate::services::feed::FeedService;
use crate::services::instagram::InstagramService;
use crate::services::page::{BrowserlessPageService, ChromePageService};
use crate::services::reddit::RedditService;
use crate::services::search::SearchService;
use crate::services::tiktok::TikTokService;
use crate::services::twitter::TwitterService;
use crate::source_handle::{ArchiveInner, SourceHandle};
use crate::store::Store;

/// Configuration for which concrete fetchers to use.
pub struct ArchiveConfig {
    pub page_backend: PageBackend,
    pub serper_api_key: String,
    pub apify_api_key: Option<String>,
}

pub enum PageBackend {
    Chrome,
    Browserless {
        base_url: String,
        token: Option<String>,
    },
}

/// The archive: fetch, store, and serve content from the web.
/// Use `archive.source(url)` to get a handle, then call content-type methods.
pub struct Archive {
    inner: Arc<ArchiveInner>,
}

impl Archive {
    pub fn new(
        pool: PgPool,
        config: ArchiveConfig,
        dispatcher: Option<Arc<dyn WorkflowDispatcher>>,
    ) -> Self {
        let store = Store::new(pool);

        // Page fetcher
        let (chrome_page, browserless_page) = match config.page_backend {
            PageBackend::Chrome => (Some(ChromePageService::new()), None),
            PageBackend::Browserless { base_url, token } => (
                None,
                Some(BrowserlessPageService::new(&base_url, token.as_deref())),
            ),
        };

        // Social services (all require Apify)
        let (instagram, twitter, reddit, facebook, tiktok, bluesky) =
            if let Some(ref api_key) = config.apify_api_key {
                (
                    Some(InstagramService::new(apify_client::ApifyClient::new(
                        api_key.clone(),
                    ))),
                    Some(TwitterService::new(apify_client::ApifyClient::new(
                        api_key.clone(),
                    ))),
                    Some(RedditService::new(apify_client::ApifyClient::new(
                        api_key.clone(),
                    ))),
                    Some(FacebookService::new(apify_client::ApifyClient::new(
                        api_key.clone(),
                    ))),
                    Some(TikTokService::new(apify_client::ApifyClient::new(
                        api_key.clone(),
                    ))),
                    Some(BlueskyService::new()),
                )
            } else {
                (None, None, None, None, None, None)
            };

        // Web search
        let search = if config.serper_api_key.is_empty() {
            None
        } else {
            Some(SearchService::new(&config.serper_api_key))
        };

        let inner = ArchiveInner {
            store,
            instagram,
            twitter,
            reddit,
            facebook,
            tiktok,
            bluesky,
            chrome_page,
            browserless_page,
            feed: FeedService::new(),
            search,
            dispatcher,
        };

        Self {
            inner: Arc::new(inner),
        }
    }

    /// Get a source handle for a URL. Upserts the source in the database.
    pub async fn source(&self, url: &str) -> Result<SourceHandle> {
        let normalized = normalize_url(url);
        let platform = detect_platform(&normalized);
        let identifier = extract_identifier(&normalized, platform);
        let source = self.inner.store.upsert_source(&normalized).await?;

        Ok(SourceHandle {
            source,
            platform,
            identifier,
            inner: self.inner.clone(),
        })
    }

    // --- Shorthand methods (skip the two-step source().await?.method().await pattern) ---

    /// Fetch posts from a social media URL.
    pub async fn posts(
        &self,
        url: &str,
        limit: u32,
    ) -> Result<Vec<rootsignal_common::types::Post>> {
        self.source(url).await?.posts(limit).await
    }

    /// Fetch and archive a web page.
    pub async fn page(&self, url: &str) -> Result<rootsignal_common::types::ArchivedPage> {
        self.source(url).await?.page().await
    }

    /// Fetch an RSS/Atom feed.
    pub async fn feed(&self, url: &str) -> Result<rootsignal_common::types::ArchivedFeed> {
        self.source(url).await?.feed().await
    }

    /// Run a web search.
    pub async fn search(
        &self,
        query: &str,
    ) -> Result<rootsignal_common::types::ArchivedSearchResults> {
        self.source(query).await?.search(query).await
    }

    /// Crawl a website via BFS, following links from the seed URL.
    /// Uses sensible defaults: max_depth=2, limit=20.
    pub async fn crawl(&self, url: &str) -> Result<Vec<rootsignal_common::types::ArchivedPage>> {
        self.source(url).await?.crawl().await
    }
}
