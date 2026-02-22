// SourceHandle: the public API for interacting with a source.
// Callers get a SourceHandle from Archive::source(url) and call content-type
// methods on it. Each method returns a request builder that implements
// IntoFuture for ergonomic .await.

use std::future::{Future, IntoFuture};
use std::pin::Pin;
use std::sync::Arc;

use chrono::Utc;
use rootsignal_common::types::{
    ArchivedFeed, ArchivedPage, ArchivedSearchResults, FeedItem, LongVideo, Post,
    SearchResult, ShortVideo, Source, Story,
};
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
                return Err(ArchiveError::Unsupported("Bluesky posts not yet supported".into()));
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
            let file_ids: Vec<Uuid> = attachments.iter().map(|f| f.id).collect();
            if !file_ids.is_empty() {
                self.inner.store.insert_attachments("posts", post_id, &file_ids).await?;
            }

            posts.push(Post {
                id: post_id,
                source_id,
                fetched_at: Utc::now(),
                content_hash: insert_post.content_hash,
                text: insert_post.text,
                location: insert_post.location,
                engagement: insert_post.engagement,
                published_at: insert_post.published_at,
                permalink: insert_post.permalink,
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
            let file_ids: Vec<Uuid> = attachments.iter().map(|f| f.id).collect();
            if !file_ids.is_empty() {
                self.inner.store.insert_attachments("stories", story_id, &file_ids).await?;
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
            let file_ids: Vec<Uuid> = attachments.iter().map(|f| f.id).collect();
            if !file_ids.is_empty() {
                self.inner.store.insert_attachments("short_videos", video_id, &file_ids).await?;
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

        let page_id = self.inner.store.insert_page(&fetched.page).await?;
        self.inner.store.update_last_scraped(source_id, "pages").await?;

        Ok(ArchivedPage {
            id: page_id,
            source_id,
            fetched_at: Utc::now(),
            content_hash: fetched.page.content_hash,
            markdown: fetched.page.markdown,
            title: fetched.page.title,
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
            let file_ids: Vec<Uuid> = attachments.iter().map(|f| f.id).collect();
            if !file_ids.is_empty() {
                self.inner.store.insert_attachments("posts", post_id, &file_ids).await?;
            }
            posts.push(Post {
                id: post_id,
                source_id,
                fetched_at: Utc::now(),
                content_hash: insert_post.content_hash,
                text: insert_post.text,
                location: insert_post.location,
                engagement: insert_post.engagement,
                published_at: insert_post.published_at,
                permalink: insert_post.permalink,
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
