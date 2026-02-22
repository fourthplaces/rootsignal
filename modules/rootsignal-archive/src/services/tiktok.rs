// TikTok service: posts, short videos, topic search.
// Wraps ApifyClient, returns universal content types.

use anyhow::Result;
use apify_client::ApifyClient;
use chrono::{DateTime, Utc};
use tracing::info;
use uuid::Uuid;

use crate::store::{InsertFile, InsertPost, InsertShortVideo};
use crate::text_extract;

/// Raw fetched post before persistence.
pub(crate) struct FetchedPost {
    pub post: InsertPost,
}

pub(crate) struct FetchedShortVideo {
    pub video: InsertShortVideo,
    pub files: Vec<InsertFile>,
}

pub(crate) struct TikTokService {
    client: ApifyClient,
}

impl TikTokService {
    pub(crate) fn new(client: ApifyClient) -> Self {
        Self { client }
    }

    /// Fetch posts from a TikTok profile. Filters to text-heavy posts (>= 20 chars).
    pub(crate) async fn fetch_posts(
        &self,
        identifier: &str,
        source_id: Uuid,
        limit: u32,
    ) -> Result<Vec<FetchedPost>> {
        info!(identifier, limit, "tiktok: fetching posts");

        let raw = self.client.scrape_tiktok_posts(identifier, limit).await?;

        let posts = raw
            .into_iter()
            .filter_map(|p| {
                let text = p.text.filter(|t| t.len() >= 20)?;
                let content_hash = rootsignal_common::content_hash(&text).to_string();

                let engagement = serde_json::json!({
                    "likes": p.digg_count,
                    "comments": p.comment_count,
                    "shares": p.share_count,
                    "plays": p.play_count,
                });

                let mentions = text_extract::extract_mentions(&text);
                let hashtags = p.hashtags
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|h| h.name.map(|n| n.to_lowercase()))
                    .collect();

                Some(FetchedPost {
                    post: InsertPost {
                        source_id,
                        content_hash,
                        text: Some(text),
                        author: p.author_meta.and_then(|a| a.name),
                        location: None,
                        engagement: Some(engagement),
                        published_at: p.create_time_iso.as_deref()
                            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                            .map(|dt| dt.with_timezone(&Utc)),
                        permalink: p.web_video_url,
                        mentions,
                        hashtags,
                        media_type: Some("video".to_string()),
                        platform_id: p.id,
                    },
                })
            })
            .collect();

        Ok(posts)
    }

    /// Fetch short videos from a TikTok profile. All TikTok posts are short videos.
    pub(crate) async fn fetch_short_videos(
        &self,
        identifier: &str,
        source_id: Uuid,
        limit: u32,
    ) -> Result<Vec<FetchedShortVideo>> {
        info!(identifier, limit, "tiktok: fetching short videos");

        let raw = self.client.scrape_tiktok_posts(identifier, limit).await?;

        let videos = raw
            .into_iter()
            .filter_map(|p| {
                let text = p.text;
                let content_for_hash = text.as_deref().unwrap_or("");
                let content_hash = rootsignal_common::content_hash(content_for_hash).to_string();

                let engagement = serde_json::json!({
                    "likes": p.digg_count,
                    "comments": p.comment_count,
                    "shares": p.share_count,
                    "plays": p.play_count,
                });

                let mut files = Vec::new();
                if let Some(ref video_url) = p.web_video_url {
                    files.push(InsertFile {
                        url: video_url.clone(),
                        content_hash: content_hash.clone(),
                        title: None,
                        mime_type: "video/mp4".to_string(),
                        duration: None,
                        page_count: None,
                        text: None,
                        text_language: None,
                    });
                }

                Some(FetchedShortVideo {
                    video: InsertShortVideo {
                        source_id,
                        content_hash,
                        text,
                        location: None,
                        engagement: Some(engagement),
                        published_at: p.create_time_iso.as_deref()
                            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                            .map(|dt| dt.with_timezone(&Utc)),
                        permalink: p.web_video_url,
                    },
                    files,
                })
            })
            .collect();

        Ok(videos)
    }

    /// Search TikTok by keywords (topic search).
    pub(crate) async fn search_topics(
        &self,
        topics: &[&str],
        source_id: Uuid,
        limit: u32,
    ) -> Result<Vec<FetchedPost>> {
        info!(?topics, limit, "tiktok: searching topics");

        let raw = self.client.search_tiktok_keywords(topics, limit).await?;

        let posts = raw
            .into_iter()
            .filter_map(|p| {
                let text = p.text.filter(|t| t.len() >= 20)?;
                let content_hash = rootsignal_common::content_hash(&text).to_string();

                let engagement = serde_json::json!({
                    "likes": p.digg_count,
                    "comments": p.comment_count,
                    "shares": p.share_count,
                    "plays": p.play_count,
                });

                let mentions = text_extract::extract_mentions(&text);
                let hashtags = p.hashtags
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|h| h.name.map(|n| n.to_lowercase()))
                    .collect();

                Some(FetchedPost {
                    post: InsertPost {
                        source_id,
                        content_hash,
                        text: Some(text),
                        author: p.author_meta.and_then(|a| a.name),
                        location: None,
                        engagement: Some(engagement),
                        published_at: p.create_time_iso.as_deref()
                            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                            .map(|dt| dt.with_timezone(&Utc)),
                        permalink: p.web_video_url,
                        mentions,
                        hashtags,
                        media_type: Some("video".to_string()),
                        platform_id: p.id,
                    },
                })
            })
            .collect();

        Ok(posts)
    }
}
