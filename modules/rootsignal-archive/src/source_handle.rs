// SourceHandle: the public API for interacting with a source.
// Callers get a SourceHandle from Archive::source(url) and call content-type
// methods on it. Each method returns a request builder that implements
// IntoFuture for ergonomic .await.

use std::collections::{HashSet, VecDeque};
use std::future::{Future, IntoFuture};
use std::pin::Pin;
use std::sync::Arc;

use chrono::Utc;
use rootsignal_common::types::{
    ArchivedFeed, ArchivedPage, ArchivedSearchResults, FeedItem, LongVideo, Post,
    SearchResult, ShortVideo, Source, Story,
};
use tracing::{info, warn};
use uuid::Uuid;

use crate::error::{ArchiveError, Result};
use crate::router::Platform;
use crate::store::Store;

use crate::services::bluesky::BlueskyService;
use crate::services::facebook::FacebookService;
use crate::services::feed::FeedService;
use crate::services::instagram::InstagramService;
use crate::services::page::{BrowserlessPageService, ChromePageService};
use crate::services::reddit::RedditService;
use crate::services::search::SearchService;
use crate::services::tiktok::TikTokService;
use crate::services::twitter::TwitterService;

/// Internal shared state for the archive. Holds services + store.
pub(crate) struct ArchiveInner {
    pub store: Store,
    pub instagram: Option<InstagramService>,
    pub twitter: Option<TwitterService>,
    pub reddit: Option<RedditService>,
    pub facebook: Option<FacebookService>,
    pub tiktok: Option<TikTokService>,
    pub bluesky: Option<BlueskyService>,
    pub chrome_page: Option<ChromePageService>,
    pub browserless_page: Option<BrowserlessPageService>,
    pub feed: FeedService,
    pub search: Option<SearchService>,
}

/// A handle to a source. Returned by `Archive::source(url)`.
/// All content-type methods are available directly — call what you need,
/// handle `Err(Unsupported)` for content types this source doesn't support.
pub struct SourceHandle {
    pub(crate) source: Source,
    pub(crate) platform: Platform,
    pub(crate) identifier: String,
    pub(crate) inner: Arc<ArchiveInner>,
}

impl SourceHandle {
    pub fn id(&self) -> Uuid {
        self.source.id
    }

    pub fn url(&self) -> &str {
        &self.source.url
    }

    pub fn platform(&self) -> Platform {
        self.platform
    }

    // --- Content type methods ---

    pub fn posts(&self, limit: u32) -> PostsRequest {
        PostsRequest {
            inner: self.inner.clone(),
            source: self.source.clone(),
            platform: self.platform,
            identifier: self.identifier.clone(),
            limit,
            text_analysis: false,
        }
    }

    pub fn stories(&self) -> StoriesRequest {
        StoriesRequest {
            inner: self.inner.clone(),
            source: self.source.clone(),
            platform: self.platform,
            identifier: self.identifier.clone(),
            text_analysis: false,
        }
    }

    pub fn short_videos(&self, limit: u32) -> ShortVideoRequest {
        ShortVideoRequest {
            inner: self.inner.clone(),
            source: self.source.clone(),
            platform: self.platform,
            identifier: self.identifier.clone(),
            limit,
            text_analysis: false,
        }
    }

    pub fn videos(&self, limit: u32) -> VideoRequest {
        VideoRequest {
            inner: self.inner.clone(),
            source: self.source.clone(),
            platform: self.platform,
            identifier: self.identifier.clone(),
            limit,
            text_analysis: false,
        }
    }

    pub fn page(&self) -> PageRequest {
        PageRequest {
            inner: self.inner.clone(),
            source: self.source.clone(),
        }
    }

    pub fn feed(&self) -> FeedRequest {
        FeedRequest {
            inner: self.inner.clone(),
            source: self.source.clone(),
        }
    }

    pub fn search(&self, query: &str) -> SearchRequest {
        SearchRequest {
            inner: self.inner.clone(),
            source: self.source.clone(),
            query: query.to_string(),
            max_results: 5,
        }
    }

    pub fn crawl(&self) -> CrawlRequest {
        CrawlRequest {
            inner: self.inner.clone(),
            seed_url: self.source.url.clone(),
            max_depth: 2,
            limit: 20,
            include_patterns: Vec::new(),
            exclude_patterns: Vec::new(),
        }
    }

    pub fn search_topics(&self, topics: &[&str], limit: u32) -> TopicSearchRequest {
        TopicSearchRequest {
            inner: self.inner.clone(),
            source: self.source.clone(),
            platform: self.platform,
            topics: topics.iter().map(|s| s.to_string()).collect(),
            limit,
        }
    }
}

// ---------------------------------------------------------------------------
// Request builders + IntoFuture
// ---------------------------------------------------------------------------

pub struct PostsRequest {
    inner: Arc<ArchiveInner>,
    source: Source,
    platform: Platform,
    identifier: String,
    limit: u32,
    text_analysis: bool,
}

impl PostsRequest {
    pub fn with_text_analysis(mut self) -> Self {
        self.text_analysis = true;
        self
    }

    pub async fn send(self) -> Result<Vec<Post>> {
        let source_id = self.source.id;

        let fetched = match self.platform {
            Platform::Instagram => {
                let svc = self.inner.instagram.as_ref()
                    .ok_or_else(|| ArchiveError::Unsupported("Instagram service not configured".into()))?;
                svc.fetch_posts(&self.identifier, source_id, self.limit)
                    .await
                    .map_err(ArchiveError::Other)?
                    .into_iter()
                    .map(|f| (f.post, f.files))
                    .collect::<Vec<_>>()
            }
            Platform::Twitter => {
                let svc = self.inner.twitter.as_ref()
                    .ok_or_else(|| ArchiveError::Unsupported("Twitter service not configured".into()))?;
                svc.fetch_posts(&self.identifier, source_id, self.limit)
                    .await
                    .map_err(ArchiveError::Other)?
                    .into_iter()
                    .map(|f| (f.post, Vec::new()))
                    .collect()
            }
            Platform::Reddit => {
                let svc = self.inner.reddit.as_ref()
                    .ok_or_else(|| ArchiveError::Unsupported("Reddit service not configured".into()))?;
                svc.fetch_posts(&self.identifier, source_id, self.limit)
                    .await
                    .map_err(ArchiveError::Other)?
                    .into_iter()
                    .map(|f| (f.post, Vec::new()))
                    .collect()
            }
            Platform::Facebook => {
                let svc = self.inner.facebook.as_ref()
                    .ok_or_else(|| ArchiveError::Unsupported("Facebook service not configured".into()))?;
                svc.fetch_posts(&self.identifier, source_id, self.limit)
                    .await
                    .map_err(ArchiveError::Other)?
                    .into_iter()
                    .map(|f| (f.post, Vec::new()))
                    .collect()
            }
            Platform::TikTok => {
                let svc = self.inner.tiktok.as_ref()
                    .ok_or_else(|| ArchiveError::Unsupported("TikTok service not configured".into()))?;
                svc.fetch_posts(&self.identifier, source_id, self.limit)
                    .await
                    .map_err(ArchiveError::Other)?
                    .into_iter()
                    .map(|f| (f.post, Vec::new()))
                    .collect()
            }
            Platform::Bluesky => {
                let svc = self.inner.bluesky.as_ref()
                    .ok_or_else(|| ArchiveError::Unsupported("Bluesky service not configured".into()))?;
                svc.fetch_posts(&self.identifier, source_id, self.limit)
                    .await
                    .map_err(ArchiveError::Other)?
                    .into_iter()
                    .map(|f| (f.post, Vec::new()))
                    .collect()
            }
            Platform::Web => {
                return Err(ArchiveError::Unsupported("Web sources don't have posts".into()));
            }
        };

        // Persist and build result
        let mut posts = Vec::with_capacity(fetched.len());
        for (insert_post, insert_files) in fetched {
            let post_id = self.inner.store.insert_post(&insert_post).await?;

            // Persist files and create attachments
            let mut attachments = Vec::new();
            for insert_file in &insert_files {
                let file = self.inner.store.upsert_file(insert_file).await?;
                attachments.push(file);
            }
            let file_positions: Vec<(Uuid, i32)> = attachments.iter().enumerate().map(|(i, f)| (f.id, i as i32)).collect();
            if !file_positions.is_empty() {
                self.inner.store.insert_attachments("posts", post_id, &file_positions).await?;
            }

            posts.push(Post {
                id: post_id,
                source_id,
                fetched_at: Utc::now(),
                content_hash: insert_post.content_hash,
                text: insert_post.text,
                author: insert_post.author,
                location: insert_post.location,
                engagement: insert_post.engagement,
                published_at: insert_post.published_at,
                permalink: insert_post.permalink,
                mentions: insert_post.mentions,
                hashtags: insert_post.hashtags,
                media_type: insert_post.media_type,
                platform_id: insert_post.platform_id,
                attachments,
            });
        }

        self.inner.store.update_last_scraped(source_id, "posts").await?;
        Ok(posts)
    }
}

impl IntoFuture for PostsRequest {
    type Output = Result<Vec<Post>>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.send())
    }
}

pub struct StoriesRequest {
    inner: Arc<ArchiveInner>,
    source: Source,
    platform: Platform,
    identifier: String,
    text_analysis: bool,
}

impl StoriesRequest {
    pub fn with_text_analysis(mut self) -> Self {
        self.text_analysis = true;
        self
    }

    pub async fn send(self) -> Result<Vec<Story>> {
        let source_id = self.source.id;

        let fetched = match self.platform {
            Platform::Instagram => {
                let svc = self.inner.instagram.as_ref()
                    .ok_or_else(|| ArchiveError::Unsupported("Instagram service not configured".into()))?;
                svc.fetch_stories(&self.identifier, source_id)
                    .await
                    .map_err(ArchiveError::Other)?
            }
            _ => {
                return Err(ArchiveError::Unsupported(
                    format!("{:?} doesn't support stories", self.platform),
                ));
            }
        };

        let mut stories = Vec::with_capacity(fetched.len());
        for f in fetched {
            let story_id = self.inner.store.insert_story(&f.story).await?;
            let mut attachments = Vec::new();
            for insert_file in &f.files {
                let file = self.inner.store.upsert_file(insert_file).await?;
                attachments.push(file);
            }
            let file_positions: Vec<(Uuid, i32)> = attachments.iter().enumerate().map(|(i, f)| (f.id, i as i32)).collect();
            if !file_positions.is_empty() {
                self.inner.store.insert_attachments("stories", story_id, &file_positions).await?;
            }
            stories.push(Story {
                id: story_id,
                source_id,
                fetched_at: Utc::now(),
                content_hash: f.story.content_hash,
                text: f.story.text,
                location: f.story.location,
                expires_at: f.story.expires_at,
                permalink: f.story.permalink,
                attachments,
            });
        }

        self.inner.store.update_last_scraped(source_id, "stories").await?;
        Ok(stories)
    }
}

impl IntoFuture for StoriesRequest {
    type Output = Result<Vec<Story>>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.send())
    }
}

pub struct ShortVideoRequest {
    inner: Arc<ArchiveInner>,
    source: Source,
    platform: Platform,
    identifier: String,
    limit: u32,
    text_analysis: bool,
}

impl ShortVideoRequest {
    pub fn with_text_analysis(mut self) -> Self {
        self.text_analysis = true;
        self
    }

    pub async fn send(self) -> Result<Vec<ShortVideo>> {
        let source_id = self.source.id;

        let fetched = match self.platform {
            Platform::Instagram => {
                let svc = self.inner.instagram.as_ref()
                    .ok_or_else(|| ArchiveError::Unsupported("Instagram service not configured".into()))?;
                svc.fetch_short_videos(&self.identifier, source_id, self.limit)
                    .await
                    .map_err(ArchiveError::Other)?
                    .into_iter()
                    .map(|f| (f.video, f.files))
                    .collect::<Vec<_>>()
            }
            Platform::TikTok => {
                let svc = self.inner.tiktok.as_ref()
                    .ok_or_else(|| ArchiveError::Unsupported("TikTok service not configured".into()))?;
                svc.fetch_short_videos(&self.identifier, source_id, self.limit)
                    .await
                    .map_err(ArchiveError::Other)?
                    .into_iter()
                    .map(|f| (f.video, f.files))
                    .collect()
            }
            _ => {
                return Err(ArchiveError::Unsupported(
                    format!("{:?} doesn't support short videos", self.platform),
                ));
            }
        };

        let mut videos = Vec::with_capacity(fetched.len());
        for (insert_video, insert_files) in fetched {
            let video_id = self.inner.store.insert_short_video(&insert_video).await?;
            let mut attachments = Vec::new();
            for insert_file in &insert_files {
                let file = self.inner.store.upsert_file(insert_file).await?;
                attachments.push(file);
            }
            let file_positions: Vec<(Uuid, i32)> = attachments.iter().enumerate().map(|(i, f)| (f.id, i as i32)).collect();
            if !file_positions.is_empty() {
                self.inner.store.insert_attachments("short_videos", video_id, &file_positions).await?;
            }
            videos.push(ShortVideo {
                id: video_id,
                source_id,
                fetched_at: Utc::now(),
                content_hash: insert_video.content_hash,
                text: insert_video.text,
                location: insert_video.location,
                engagement: insert_video.engagement,
                published_at: insert_video.published_at,
                permalink: insert_video.permalink,
                attachments,
            });
        }

        self.inner.store.update_last_scraped(source_id, "short_videos").await?;
        Ok(videos)
    }
}

impl IntoFuture for ShortVideoRequest {
    type Output = Result<Vec<ShortVideo>>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.send())
    }
}

pub struct VideoRequest {
    inner: Arc<ArchiveInner>,
    source: Source,
    platform: Platform,
    identifier: String,
    limit: u32,
    text_analysis: bool,
}

impl VideoRequest {
    pub fn with_text_analysis(mut self) -> Self {
        self.text_analysis = true;
        self
    }

    pub async fn send(self) -> Result<Vec<LongVideo>> {
        Err(ArchiveError::Unsupported(
            format!("{:?} long video fetching not yet implemented", self.platform),
        ))
    }
}

impl IntoFuture for VideoRequest {
    type Output = Result<Vec<LongVideo>>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.send())
    }
}

pub struct PageRequest {
    inner: Arc<ArchiveInner>,
    source: Source,
}

impl PageRequest {
    pub async fn send(self) -> Result<ArchivedPage> {
        let source_id = self.source.id;

        // Google Docs: fetch the HTML export directly instead of using Chrome
        if let Some(export_url) = google_docs_export_url(&self.source.url) {
            return self.fetch_google_doc(source_id, &export_url).await;
        }

        let fetched = if let Some(ref svc) = self.inner.browserless_page {
            svc.fetch(&self.source.url, source_id)
                .await
                .map_err(ArchiveError::Other)?
        } else if let Some(ref svc) = self.inner.chrome_page {
            svc.fetch(&self.source.url, source_id)
                .await
                .map_err(ArchiveError::Other)?
        } else {
            return Err(ArchiveError::Unsupported("No page fetcher configured".into()));
        };

        let links = crate::links::extract_all_links(&fetched.raw_html, &self.source.url);
        let page = crate::store::InsertPage {
            source_id,
            content_hash: fetched.page.content_hash,
            markdown: fetched.page.markdown,
            title: fetched.page.title,
            links: links.clone(),
        };
        let page_id = self.inner.store.insert_page(&page).await?;
        self.inner.store.update_last_scraped(source_id, "pages").await?;

        Ok(ArchivedPage {
            id: page_id,
            source_id,
            fetched_at: Utc::now(),
            content_hash: page.content_hash,
            raw_html: fetched.raw_html,
            markdown: page.markdown,
            title: page.title,
            links,
        })
    }
}

impl PageRequest {
    async fn fetch_google_doc(&self, source_id: Uuid, export_url: &str) -> Result<ArchivedPage> {
        info!(url = %self.source.url, export_url, "page: fetching Google Doc HTML export");

        let resp = reqwest::get(export_url)
            .await
            .map_err(|e| ArchiveError::Other(e.into()))?;

        if !resp.status().is_success() {
            return Err(ArchiveError::Other(anyhow::anyhow!(
                "Google Docs export returned HTTP {}",
                resp.status()
            )));
        }

        let html = resp
            .text()
            .await
            .map_err(|e| ArchiveError::Other(e.into()))?;

        let markdown = crate::readability::html_to_markdown(html.as_bytes(), Some(export_url));
        let hash = rootsignal_common::content_hash(&html).to_string();
        let title = crate::services::page::extract_title(&html);
        let links = crate::links::extract_all_links(&html, &self.source.url);

        let page = crate::store::InsertPage {
            source_id,
            content_hash: hash,
            markdown,
            title,
            links: links.clone(),
        };
        let page_id = self.inner.store.insert_page(&page).await?;
        self.inner.store.update_last_scraped(source_id, "pages").await?;

        Ok(ArchivedPage {
            id: page_id,
            source_id,
            fetched_at: Utc::now(),
            content_hash: page.content_hash,
            raw_html: html,
            markdown: page.markdown,
            title: page.title,
            links,
        })
    }
}

impl IntoFuture for PageRequest {
    type Output = Result<ArchivedPage>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.send())
    }
}

pub struct FeedRequest {
    inner: Arc<ArchiveInner>,
    source: Source,
}

impl FeedRequest {
    pub async fn send(self) -> Result<ArchivedFeed> {
        let source_id = self.source.id;

        let fetched = self.inner.feed
            .fetch(&self.source.url, source_id)
            .await
            .map_err(ArchiveError::Other)?;

        let feed_id = self.inner.store.insert_feed(&fetched.feed).await?;
        self.inner.store.update_last_scraped(source_id, "feeds").await?;

        let items: Vec<FeedItem> = serde_json::from_value(fetched.feed.items).unwrap_or_default();
        Ok(ArchivedFeed {
            id: feed_id,
            source_id,
            fetched_at: Utc::now(),
            content_hash: fetched.feed.content_hash,
            items,
            title: fetched.feed.title,
        })
    }
}

impl IntoFuture for FeedRequest {
    type Output = Result<ArchivedFeed>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.send())
    }
}

pub struct SearchRequest {
    inner: Arc<ArchiveInner>,
    source: Source,
    query: String,
    max_results: usize,
}

impl SearchRequest {
    pub fn max_results(mut self, n: usize) -> Self {
        self.max_results = n;
        self
    }

    pub async fn send(self) -> Result<ArchivedSearchResults> {
        let source_id = self.source.id;

        let svc = self.inner.search.as_ref()
            .ok_or_else(|| ArchiveError::Unsupported("Search service not configured".into()))?;

        let fetched = svc
            .search(&self.query, source_id, self.max_results)
            .await
            .map_err(ArchiveError::Other)?;

        let results_id = self.inner.store.insert_search_results(&fetched.results).await?;
        self.inner.store.update_last_scraped(source_id, "search_results").await?;

        let results: Vec<SearchResult> =
            serde_json::from_value(fetched.results.results).unwrap_or_default();
        Ok(ArchivedSearchResults {
            id: results_id,
            source_id,
            fetched_at: Utc::now(),
            content_hash: fetched.results.content_hash,
            query: fetched.results.query,
            results,
        })
    }
}

impl IntoFuture for SearchRequest {
    type Output = Result<ArchivedSearchResults>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.send())
    }
}

pub struct TopicSearchRequest {
    inner: Arc<ArchiveInner>,
    source: Source,
    platform: Platform,
    topics: Vec<String>,
    limit: u32,
}

impl TopicSearchRequest {
    pub async fn send(self) -> Result<Vec<Post>> {
        let source_id = self.source.id;
        let topic_refs: Vec<&str> = self.topics.iter().map(|s| s.as_str()).collect();

        let fetched = match self.platform {
            Platform::Instagram => {
                let svc = self.inner.instagram.as_ref()
                    .ok_or_else(|| ArchiveError::Unsupported("Instagram service not configured".into()))?;
                svc.search_topics(&topic_refs, source_id, self.limit)
                    .await
                    .map_err(ArchiveError::Other)?
                    .into_iter()
                    .map(|f| (f.post, f.files))
                    .collect::<Vec<_>>()
            }
            Platform::Twitter => {
                let svc = self.inner.twitter.as_ref()
                    .ok_or_else(|| ArchiveError::Unsupported("Twitter service not configured".into()))?;
                svc.search_topics(&topic_refs, source_id, self.limit)
                    .await
                    .map_err(ArchiveError::Other)?
                    .into_iter()
                    .map(|f| (f.post, Vec::new()))
                    .collect()
            }
            Platform::Reddit => {
                let svc = self.inner.reddit.as_ref()
                    .ok_or_else(|| ArchiveError::Unsupported("Reddit service not configured".into()))?;
                svc.search_topics(&topic_refs, source_id, self.limit)
                    .await
                    .map_err(ArchiveError::Other)?
                    .into_iter()
                    .map(|f| (f.post, Vec::new()))
                    .collect()
            }
            Platform::TikTok => {
                let svc = self.inner.tiktok.as_ref()
                    .ok_or_else(|| ArchiveError::Unsupported("TikTok service not configured".into()))?;
                svc.search_topics(&topic_refs, source_id, self.limit)
                    .await
                    .map_err(ArchiveError::Other)?
                    .into_iter()
                    .map(|f| (f.post, Vec::new()))
                    .collect()
            }
            Platform::Bluesky => {
                let svc = self.inner.bluesky.as_ref()
                    .ok_or_else(|| ArchiveError::Unsupported("Bluesky service not configured".into()))?;
                svc.search_topics(&topic_refs, source_id, self.limit)
                    .await
                    .map_err(ArchiveError::Other)?
                    .into_iter()
                    .map(|f| (f.post, Vec::new()))
                    .collect()
            }
            _ => {
                return Err(ArchiveError::Unsupported(
                    format!("{:?} doesn't support topic search", self.platform),
                ));
            }
        };

        let mut posts = Vec::with_capacity(fetched.len());
        for (insert_post, insert_files) in fetched {
            let post_id = self.inner.store.insert_post(&insert_post).await?;
            let mut attachments = Vec::new();
            for insert_file in &insert_files {
                let file = self.inner.store.upsert_file(insert_file).await?;
                attachments.push(file);
            }
            let file_positions: Vec<(Uuid, i32)> = attachments.iter().enumerate().map(|(i, f)| (f.id, i as i32)).collect();
            if !file_positions.is_empty() {
                self.inner.store.insert_attachments("posts", post_id, &file_positions).await?;
            }
            posts.push(Post {
                id: post_id,
                source_id,
                fetched_at: Utc::now(),
                content_hash: insert_post.content_hash,
                text: insert_post.text,
                author: insert_post.author,
                location: insert_post.location,
                engagement: insert_post.engagement,
                published_at: insert_post.published_at,
                permalink: insert_post.permalink,
                mentions: insert_post.mentions,
                hashtags: insert_post.hashtags,
                media_type: insert_post.media_type,
                platform_id: insert_post.platform_id,
                attachments,
            });
        }

        self.inner.store.update_last_scraped(source_id, "posts").await?;
        Ok(posts)
    }
}

impl IntoFuture for TopicSearchRequest {
    type Output = Result<Vec<Post>>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.send())
    }
}

pub struct CrawlRequest {
    inner: Arc<ArchiveInner>,
    seed_url: String,
    max_depth: usize,
    limit: usize,
    include_patterns: Vec<String>,
    exclude_patterns: Vec<String>,
}

impl CrawlRequest {
    pub fn max_depth(mut self, n: usize) -> Self {
        self.max_depth = n;
        self
    }

    pub fn limit(mut self, n: usize) -> Self {
        self.limit = n;
        self
    }

    pub fn include(mut self, pattern: &str) -> Self {
        self.include_patterns.push(pattern.to_string());
        self
    }

    pub fn exclude(mut self, pattern: &str) -> Self {
        self.exclude_patterns.push(pattern.to_string());
        self
    }

    pub async fn send(self) -> Result<Vec<ArchivedPage>> {
        if self.limit == 0 {
            return Ok(Vec::new());
        }

        let seed_full_url = ensure_scheme(&self.seed_url);
        let seed_host = extract_host(&seed_full_url);

        info!(
            url = %seed_full_url, max_depth = self.max_depth, limit = self.limit,
            "crawl: starting BFS"
        );

        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<(String, usize)> = VecDeque::new();
        let mut pages: Vec<ArchivedPage> = Vec::new();

        queue.push_back((seed_full_url.clone(), 0));
        visited.insert(normalize_crawl_url(&seed_full_url));

        while let Some((url, depth)) = queue.pop_front() {
            if pages.len() >= self.limit {
                break;
            }

            // Fetch page through archive's normal pipeline (browserless/Chrome)
            let page = match self.fetch_page(&url).await {
                Ok(p) => p,
                Err(e) => {
                    // Seed failure is a hard error
                    if pages.is_empty() && depth == 0 {
                        return Err(e);
                    }
                    warn!(url = %url, error = %e, "crawl: skipping failed page");
                    continue;
                }
            };

            // Extract child links if we haven't reached max depth
            if depth < self.max_depth {
                for link in &page.links {
                    let normalized = normalize_crawl_url(link);
                    if visited.contains(&normalized) {
                        continue;
                    }
                    if !same_host(link, &seed_host) {
                        continue;
                    }
                    if !matches_patterns(link, &self.include_patterns, &self.exclude_patterns) {
                        continue;
                    }
                    visited.insert(normalized);
                    queue.push_back((link.clone(), depth + 1));
                }
            }

            pages.push(page);
        }

        info!(
            seed = %self.seed_url, pages_crawled = pages.len(),
            urls_visited = visited.len(), "crawl: BFS complete"
        );

        Ok(pages)
    }

    /// Fetch a single page through the archive page pipeline.
    async fn fetch_page(&self, url: &str) -> Result<ArchivedPage> {
        let normalized = crate::router::normalize_url(url);
        let source = self.inner.store.upsert_source(&normalized).await?;
        let source_id = source.id;

        let fetched = if let Some(ref svc) = self.inner.browserless_page {
            svc.fetch(url, source_id)
                .await
                .map_err(ArchiveError::Other)?
        } else if let Some(ref svc) = self.inner.chrome_page {
            svc.fetch(url, source_id)
                .await
                .map_err(ArchiveError::Other)?
        } else {
            return Err(ArchiveError::Unsupported("No page fetcher configured".into()));
        };

        let links = crate::links::extract_all_links(&fetched.raw_html, url);
        let page = crate::store::InsertPage {
            source_id,
            content_hash: fetched.page.content_hash,
            markdown: fetched.page.markdown,
            title: fetched.page.title,
            links: links.clone(),
        };
        let page_id = self.inner.store.insert_page(&page).await?;
        self.inner.store.update_last_scraped(source_id, "pages").await?;

        Ok(ArchivedPage {
            id: page_id,
            source_id,
            fetched_at: Utc::now(),
            content_hash: page.content_hash,
            raw_html: fetched.raw_html,
            markdown: page.markdown,
            title: page.title,
            links,
        })
    }
}

impl IntoFuture for CrawlRequest {
    type Output = Result<Vec<ArchivedPage>>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.send())
    }
}

// ---------------------------------------------------------------------------
// Google Docs helper
// ---------------------------------------------------------------------------

/// If this is a Google Docs URL, return the HTML export URL.
/// Works on normalized URLs (no scheme) since source.url is normalized.
fn google_docs_export_url(url: &str) -> Option<String> {
    if !url.contains("docs.google.com/document/d/") {
        return None;
    }
    let d_idx = url.find("/d/")? + 3;
    let rest = &url[d_idx..];
    let doc_id = rest.split(&['/', '?', '#'][..]).next()?;
    if doc_id.is_empty() {
        return None;
    }
    Some(format!(
        "https://docs.google.com/document/d/{}/export?format=html",
        doc_id
    ))
}

// ---------------------------------------------------------------------------
// Crawl helpers
// ---------------------------------------------------------------------------

/// Ensure a URL has an https:// scheme. Archive's normalize_url strips the
/// scheme, so we need to add it back for fetching.
fn ensure_scheme(url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else {
        format!("https://{url}")
    }
}

/// Extract the host from a URL for same-host comparison.
fn extract_host(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_lowercase()))
        .unwrap_or_default()
}

/// Normalize a URL for the crawl visited set.
/// Strips fragments and trailing slashes, lowercases host.
pub(crate) fn normalize_crawl_url(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut parsed) => {
            parsed.set_fragment(None);
            let mut s = parsed.to_string();
            if s.ends_with('/') && s.len() > parsed.scheme().len() + 3 {
                // Don't strip trailing slash from bare domain (https://example.com/)
                let path = parsed.path();
                if path != "/" {
                    s.pop();
                }
            }
            s
        }
        Err(_) => url.to_string(),
    }
}

/// Check if a URL belongs to the same host.
fn same_host(url: &str, seed_host: &str) -> bool {
    extract_host(url) == *seed_host
}

/// Check if a URL passes include/exclude pattern filters.
/// Include patterns are OR'd (any match passes). Exclude patterns reject on any match.
/// Empty include = allow all. Patterns are substring matches on the URL path.
fn matches_patterns(url: &str, include: &[String], exclude: &[String]) -> bool {
    let path = url::Url::parse(url)
        .map(|u| u.path().to_string())
        .unwrap_or_default();

    // Exclude takes priority
    if exclude.iter().any(|p| path.contains(p.as_str())) {
        return false;
    }

    // Empty include = allow all
    if include.is_empty() {
        return true;
    }

    include.iter().any(|p| path.contains(p.as_str()))
}

#[cfg(test)]
mod crawl_tests {
    use super::*;

    #[test]
    fn normalize_strips_fragment() {
        assert_eq!(
            normalize_crawl_url("https://example.com/page#section"),
            "https://example.com/page"
        );
    }

    #[test]
    fn normalize_strips_trailing_slash_on_path() {
        assert_eq!(
            normalize_crawl_url("https://example.com/about/"),
            "https://example.com/about"
        );
    }

    #[test]
    fn normalize_keeps_trailing_slash_on_bare_domain() {
        assert_eq!(
            normalize_crawl_url("https://example.com/"),
            "https://example.com/"
        );
    }

    #[test]
    fn normalize_keeps_query_strings() {
        assert_eq!(
            normalize_crawl_url("https://example.com/news?page=2"),
            "https://example.com/news?page=2"
        );
    }

    #[test]
    fn normalize_strips_fragment_and_trailing_slash() {
        assert_eq!(
            normalize_crawl_url("https://example.com/about/#team"),
            "https://example.com/about"
        );
    }

    #[test]
    fn same_host_matches() {
        assert!(same_host("https://example.com/about", "example.com"));
    }

    #[test]
    fn same_host_rejects_subdomain() {
        assert!(!same_host("https://blog.example.com/post", "example.com"));
    }

    #[test]
    fn same_host_rejects_different_domain() {
        assert!(!same_host("https://other.com/page", "example.com"));
    }

    #[test]
    fn patterns_empty_allows_all() {
        assert!(matches_patterns("https://example.com/anything", &[], &[]));
    }

    #[test]
    fn patterns_include_filters() {
        let include = vec!["/about".to_string(), "/contact".to_string()];
        assert!(matches_patterns("https://example.com/about", &include, &[]));
        assert!(matches_patterns("https://example.com/contact", &include, &[]));
        assert!(!matches_patterns("https://example.com/blog", &include, &[]));
    }

    #[test]
    fn patterns_exclude_rejects() {
        let exclude = vec!["/login".to_string()];
        assert!(!matches_patterns("https://example.com/login", &[], &exclude));
        assert!(matches_patterns("https://example.com/about", &[], &exclude));
    }

    #[test]
    fn patterns_exclude_takes_priority() {
        let include = vec!["/admin".to_string()];
        let exclude = vec!["/admin".to_string()];
        assert!(!matches_patterns("https://example.com/admin", &include, &exclude));
    }

    #[test]
    fn patterns_substring_match() {
        let include = vec!["/about".to_string()];
        assert!(matches_patterns("https://example.com/about-us", &include, &[]));
        assert!(matches_patterns("https://example.com/info/about/team", &include, &[]));
    }

    #[test]
    fn ensure_scheme_adds_https() {
        assert_eq!(ensure_scheme("example.com/page"), "https://example.com/page");
        assert_eq!(ensure_scheme("https://example.com"), "https://example.com");
        assert_eq!(ensure_scheme("http://example.com"), "http://example.com");
    }
}

#[cfg(test)]
mod google_docs_tests {
    use super::google_docs_export_url;

    const DOC_ID: &str = "1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgVE2upms";

    fn expected(id: &str) -> String {
        format!("https://docs.google.com/document/d/{id}/export?format=html")
    }

    #[test]
    fn normalized_edit() {
        let url = format!("docs.google.com/document/d/{DOC_ID}/edit");
        assert_eq!(google_docs_export_url(&url).unwrap(), expected(DOC_ID));
    }

    #[test]
    fn full_url_with_scheme() {
        let url = format!("https://docs.google.com/document/d/{DOC_ID}/edit");
        assert_eq!(google_docs_export_url(&url).unwrap(), expected(DOC_ID));
    }

    #[test]
    fn with_fragment() {
        let url = format!("docs.google.com/document/d/{DOC_ID}/edit#heading=h.abc");
        assert_eq!(google_docs_export_url(&url).unwrap(), expected(DOC_ID));
    }

    #[test]
    fn view_suffix() {
        let url = format!("docs.google.com/document/d/{DOC_ID}/view");
        assert_eq!(google_docs_export_url(&url).unwrap(), expected(DOC_ID));
    }

    #[test]
    fn pub_suffix() {
        let url = format!("docs.google.com/document/d/{DOC_ID}/pub");
        assert_eq!(google_docs_export_url(&url).unwrap(), expected(DOC_ID));
    }

    #[test]
    fn no_suffix() {
        let url = format!("docs.google.com/document/d/{DOC_ID}");
        assert_eq!(google_docs_export_url(&url).unwrap(), expected(DOC_ID));
    }

    #[test]
    fn trailing_slash() {
        let url = format!("docs.google.com/document/d/{DOC_ID}/");
        assert_eq!(google_docs_export_url(&url).unwrap(), expected(DOC_ID));
    }

    #[test]
    fn with_query_params() {
        let url = format!("docs.google.com/document/d/{DOC_ID}/edit?usp=sharing");
        assert_eq!(google_docs_export_url(&url).unwrap(), expected(DOC_ID));
    }

    #[test]
    fn already_export_url() {
        let url = format!("docs.google.com/document/d/{DOC_ID}/export?format=html");
        assert_eq!(google_docs_export_url(&url).unwrap(), expected(DOC_ID));
    }

    #[test]
    fn mobilebasic() {
        let url = format!("docs.google.com/document/d/{DOC_ID}/mobilebasic");
        assert_eq!(google_docs_export_url(&url).unwrap(), expected(DOC_ID));
    }

    #[test]
    fn non_google_docs_url() {
        assert!(google_docs_export_url("example.com/page").is_none());
    }

    #[test]
    fn malformed_missing_id() {
        assert!(google_docs_export_url("docs.google.com/document/d/").is_none());
    }
}
