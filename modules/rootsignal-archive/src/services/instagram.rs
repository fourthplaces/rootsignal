// Instagram service: posts, stories, short videos (reels), topic search.
// Wraps ApifyClient, returns universal content types.

use anyhow::Result;
use apify_client::ApifyClient;
use rootsignal_common::ProfileSnapshot;
use tracing::info;
use uuid::Uuid;

use crate::store::{InsertFile, InsertPost, InsertShortVideo, InsertStory};
use crate::text_extract;

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

        let raw = self
            .client
            .scrape_instagram_posts(identifier, limit)
            .await?;

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

                let mentions = p.mentions.unwrap_or_default();
                let hashtags = text_extract::extract_hashtags(text.as_deref().unwrap_or(""));
                let media_type = p.post_type;
                let platform_id = p.short_code;

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
                        mentions,
                        hashtags,
                        media_type,
                        platform_id,
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
                let mentions = text_extract::extract_mentions(&p.content);
                let hashtags = text_extract::extract_hashtags(&p.content);

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
                        mentions,
                        hashtags,
                        media_type: None,
                        platform_id: None,
                    },
                    files: Vec::new(),
                }
            })
            .collect();

        Ok(posts)
    }

    /// Fetch stories from an Instagram profile.
    pub(crate) async fn fetch_stories(
        &self,
        identifier: &str,
        source_id: Uuid,
    ) -> Result<Vec<FetchedStory>> {
        info!(identifier, "instagram: fetching stories");

        let profile_url = if identifier.starts_with("http") {
            identifier.to_string()
        } else {
            format!("https://www.instagram.com/{}/", identifier)
        };

        let raw = self.client.scrape_instagram_stories(&profile_url).await?;

        let stories = raw
            .into_iter()
            .filter_map(|s| {
                let media_url = s.media_url()?;
                let content_hash =
                    rootsignal_common::content_hash(media_url).to_string();

                let files = vec![InsertFile {
                    url: media_url.to_string(),
                    content_hash: content_hash.clone(),
                    title: None,
                    mime_type: s.mime_type().to_string(),
                    duration: None,
                    page_count: None,
                    text: None,
                    text_language: None,
                }];

                Some(FetchedStory {
                    story: InsertStory {
                        source_id,
                        content_hash,
                        text: s.caption.filter(|c| !c.is_empty()),
                        location: s.location_name,
                        expires_at: s.expiring_at,
                        permalink: s.url,
                    },
                    files,
                })
            })
            .collect();

        Ok(stories)
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

    /// Fetch profile metadata (bio, external URL) via Apify profile scraper.
    pub(crate) async fn fetch_profile(
        &self,
        identifier: &str,
    ) -> Result<Option<ProfileSnapshot>> {
        let username = extract_username(identifier);
        info!(username, "instagram: fetching profile");

        let profile = self.client.scrape_instagram_profile(&username).await?;
        Ok(profile.map(|p| ProfileSnapshot {
            bio: p.biography.filter(|b| !b.is_empty()),
            external_url: p.external_url.filter(|u| !u.is_empty()),
            display_name: p.full_name.filter(|n| !n.is_empty()),
            follower_count: p.followers_count,
        }))
    }
}

/// Extract a bare username from a URL or identifier string.
fn extract_username(identifier: &str) -> String {
    if identifier.contains("instagram.com") {
        identifier
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(identifier)
            .to_string()
    } else {
        identifier.trim_start_matches('@').to_string()
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
                                first.to_uppercase().to_string() + &chars.as_str().to_lowercase()
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

    #[test]
    fn extract_username_from_url() {
        assert_eq!(extract_username("https://www.instagram.com/sanctuarysupply/"), "sanctuarysupply");
        assert_eq!(extract_username("https://instagram.com/cafe_latte"), "cafe_latte");
    }

    #[test]
    fn extract_username_from_bare_handle() {
        assert_eq!(extract_username("@sanctuarysupply"), "sanctuarysupply");
        assert_eq!(extract_username("sanctuarysupply"), "sanctuarysupply");
    }
}
