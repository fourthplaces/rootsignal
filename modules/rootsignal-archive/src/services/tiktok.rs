// TikTok service: posts, short videos, topic search.
// Wraps ApifyClient, returns universal content types.

use anyhow::Result;
use apify_client::ApifyClient;
use chrono::{DateTime, Utc};
use tracing::info;
use uuid::Uuid;

use crate::store::{InsertFile, InsertPost, InsertShortVideo};
use crate::text_extract;

/// Build a TikTok permalink from available fields.
/// Prefers `web_video_url` when present; falls back to constructing from author + id.
fn tiktok_permalink(
    web_video_url: Option<String>,
    id: Option<&str>,
    author_name: Option<&str>,
) -> Option<String> {
    if web_video_url.is_some() {
        return web_video_url;
    }
    let id = id?;
    let author = author_name?;
    Some(format!("https://www.tiktok.com/@{author}/video/{id}"))
}

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

                let permalink = tiktok_permalink(
                    p.web_video_url,
                    p.id.as_deref(),
                    p.author_meta.as_ref().and_then(|a| a.name.as_deref()),
                );

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
                        permalink,
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

                let permalink = tiktok_permalink(
                    p.web_video_url.clone(),
                    p.id.as_deref(),
                    p.author_meta.as_ref().and_then(|a| a.name.as_deref()),
                );

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
                        permalink,
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

                let permalink = tiktok_permalink(
                    p.web_video_url,
                    p.id.as_deref(),
                    p.author_meta.as_ref().and_then(|a| a.name.as_deref()),
                );

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
                        permalink,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permalink_uses_web_video_url_when_present() {
        let url = "https://www.tiktok.com/@user/video/123".to_string();
        let result = tiktok_permalink(Some(url.clone()), Some("123"), Some("user"));
        assert_eq!(result, Some(url));
    }

    #[test]
    fn permalink_constructed_from_author_and_id() {
        let result = tiktok_permalink(None, Some("7890"), Some("cooluser"));
        assert_eq!(
            result,
            Some("https://www.tiktok.com/@cooluser/video/7890".to_string())
        );
    }

    #[test]
    fn permalink_none_when_id_missing() {
        let result = tiktok_permalink(None, None, Some("cooluser"));
        assert_eq!(result, None);
    }

    #[test]
    fn permalink_none_when_author_missing() {
        let result = tiktok_permalink(None, Some("7890"), None);
        assert_eq!(result, None);
    }
}
