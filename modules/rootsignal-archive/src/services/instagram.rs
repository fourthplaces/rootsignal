// Instagram service: posts, stories, short videos (reels), topic search.
// Wraps ApifyClient, returns universal content types.

use anyhow::Result;
use apify_client::ApifyClient;
use tracing::info;
use uuid::Uuid;

use crate::store::{InsertFile, InsertPost, InsertShortVideo, InsertStory};

/// Raw fetched post with its media, before persistence.
pub(crate) struct FetchedPost {
    pub post: InsertPost,
    pub files: Vec<InsertFile>,
}

pub(crate) struct FetchedShortVideo {
    pub video: InsertShortVideo,
    pub files: Vec<InsertFile>,
}

pub(crate) struct FetchedStory {
    pub story: InsertStory,
    pub files: Vec<InsertFile>,
}

pub(crate) struct InstagramService {
    client: ApifyClient,
}

impl InstagramService {
    pub(crate) fn new(client: ApifyClient) -> Self {
        Self { client }
    }

    /// Fetch posts from an Instagram profile.
    pub(crate) async fn fetch_posts(
        &self,
        identifier: &str,
        source_id: Uuid,
        limit: u32,
    ) -> Result<Vec<FetchedPost>> {
        info!(identifier, limit, "instagram: fetching posts");

        let raw = self.client.scrape_instagram_posts(identifier, limit).await?;

        let posts = raw
            .into_iter()
            .filter_map(|p| {
                let text = p.caption.filter(|c| !c.is_empty());
                let content_for_hash = text.as_deref().unwrap_or("");
                let content_hash = rootsignal_common::content_hash(content_for_hash).to_string();

                let engagement = serde_json::json!({
                    "likes": p.likes_count,
                    "comments": p.comments_count,
                });

                let mut files = Vec::new();
                if let Some(ref display_url) = p.display_url {
                    files.push(InsertFile {
                        url: display_url.clone(),
                        content_hash: content_hash.clone(),
                        title: None,
                        mime_type: "image/jpeg".to_string(),
                        duration: None,
                        page_count: None,
                        text: None,
                        text_language: None,
                    });
                }

                Some(FetchedPost {
                    post: InsertPost {
                        source_id,
                        content_hash,
                        text,
                        author: p.owner_username,
                        location: p.location_name,
                        engagement: Some(engagement),
                        published_at: p.timestamp,
                        permalink: Some(p.url),
                    },
                    files,
                })
            })
            .collect();

        Ok(posts)
    }

    /// Search Instagram by hashtags (topic search).
    pub(crate) async fn search_topics(
        &self,
        topics: &[&str],
        source_id: Uuid,
        limit: u32,
    ) -> Result<Vec<FetchedPost>> {
        info!(?topics, limit, "instagram: searching topics");

        let sanitized = sanitize_topics_to_hashtags(topics);
        let refs: Vec<&str> = sanitized.iter().map(|s| s.as_str()).collect();
        let raw = self.client.search_instagram_hashtags(&refs, limit).await?;

        let posts = raw
            .into_iter()
            .map(|p| {
                let content_hash = rootsignal_common::content_hash(&p.content).to_string();
                FetchedPost {
                    post: InsertPost {
                        source_id,
                        content_hash,
                        text: Some(p.content),
                        author: Some(p.author_username),
                        location: None,
                        engagement: None,
                        published_at: p.timestamp,
                        permalink: Some(p.post_url),
                    },
                    files: Vec::new(),
                }
            })
            .collect();

        Ok(posts)
    }

    /// Fetch stories from an Instagram profile.
    /// NOTE: Apify does not yet have a stories endpoint. This is a stub
    /// that returns an empty vec. Will be wired when the Apify actor is available.
    pub(crate) async fn fetch_stories(
        &self,
        _identifier: &str,
        _source_id: Uuid,
    ) -> Result<Vec<FetchedStory>> {
        info!("instagram: stories not yet supported by Apify");
        Ok(Vec::new())
    }

    /// Fetch reels (short videos) from an Instagram profile.
    /// NOTE: Apify does not yet have a dedicated reels endpoint. This is a stub.
    pub(crate) async fn fetch_short_videos(
        &self,
        _identifier: &str,
        _source_id: Uuid,
        _limit: u32,
    ) -> Result<Vec<FetchedShortVideo>> {
        info!("instagram: reels not yet supported by Apify");
        Ok(Vec::new())
    }
}

/// Convert multi-word topic strings into valid Instagram hashtags (camelCase,
/// alphanumeric only). The Instagram hashtag API rejects values containing
/// spaces, punctuation, or other special characters.
pub(crate) fn sanitize_topics_to_hashtags(topics: &[&str]) -> Vec<String> {
    topics
        .iter()
        .map(|t| {
            t.split_whitespace()
                .enumerate()
                .map(|(i, w)| {
                    let w: String = w.chars().filter(|c| c.is_alphanumeric()).collect();
                    if i == 0 {
                        w.to_lowercase()
                    } else {
                        let mut chars = w.chars();
                        match chars.next() {
                            Some(first) => {
                                first.to_uppercase().to_string()
                                    + &chars.as_str().to_lowercase()
                            }
                            None => String::new(),
                        }
                    }
                })
                .collect::<String>()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_multi_word_topics() {
        let topics = &[
            "Minneapolis immigration legal aid volunteer Minnesota",
            "Minnesota teacher sanctuary movement",
        ];
        let result = sanitize_topics_to_hashtags(topics);
        assert_eq!(
            result,
            vec![
                "minneapolisImmigrationLegalAidVolunteerMinnesota",
                "minnesotaTeacherSanctuaryMovement",
            ]
        );
    }

    #[test]
    fn sanitize_single_word_topic() {
        let result = sanitize_topics_to_hashtags(&["MNimmigration"]);
        assert_eq!(result, vec!["mnimmigration"]);
    }

    #[test]
    fn sanitize_strips_special_chars() {
        let result = sanitize_topics_to_hashtags(&["Minneapolis: ICE raids — 2026!"]);
        assert_eq!(result, vec!["minneapolisIceRaids2026"]);
    }

    #[test]
    fn sanitize_filters_empty() {
        let result = sanitize_topics_to_hashtags(&["", "   ", "valid topic"]);
        assert_eq!(result, vec!["validTopic"]);
    }
}
