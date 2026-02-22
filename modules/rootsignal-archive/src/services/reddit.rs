// Reddit service: posts, topic search.
// Wraps ApifyClient, returns universal content types.

use anyhow::Result;
use apify_client::ApifyClient;
use chrono::{DateTime, Utc};
use tracing::info;
use uuid::Uuid;

use crate::store::InsertPost;
use crate::text_extract;

/// Raw fetched post before persistence.
pub(crate) struct FetchedPost {
    pub post: InsertPost,
}

pub(crate) struct RedditService {
    client: ApifyClient,
}

impl RedditService {
    pub(crate) fn new(client: ApifyClient) -> Self {
        Self { client }
    }

    /// Fetch posts from a subreddit.
    pub(crate) async fn fetch_posts(
        &self,
        identifier: &str,
        source_id: Uuid,
        limit: u32,
    ) -> Result<Vec<FetchedPost>> {
        info!(identifier, limit, "reddit: fetching posts");

        let raw = self.client.scrape_reddit_posts(identifier, limit).await?;

        let posts = raw
            .into_iter()
            .filter_map(|p| {
                if p.data_type.as_deref() != Some("post") {
                    return None;
                }
                let title = p.title.unwrap_or_default();
                let body = p.body.unwrap_or_default();
                let text = format!("{}\n\n{}", title, body).trim().to_string();
                if text.is_empty() {
                    return None;
                }
                let content_hash = rootsignal_common::content_hash(&text).to_string();

                let engagement = serde_json::json!({
                    "likes": p.up_votes,
                    "comments": p.number_of_comments,
                });

                let mentions = text_extract::extract_mentions(&text);
                let hashtags = text_extract::extract_hashtags(&text);

                Some(FetchedPost {
                    post: InsertPost {
                        source_id,
                        content_hash,
                        text: Some(text),
                        author: None,
                        location: None,
                        engagement: Some(engagement),
                        published_at: p.created_at.as_deref()
                            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                            .map(|dt| dt.with_timezone(&Utc)),
                        permalink: p.url,
                        mentions,
                        hashtags,
                        media_type: Some("text".to_string()),
                        platform_id: None,
                    },
                })
            })
            .collect();

        Ok(posts)
    }

    /// Search Reddit by keywords (topic search).
    pub(crate) async fn search_topics(
        &self,
        topics: &[&str],
        source_id: Uuid,
        limit: u32,
    ) -> Result<Vec<FetchedPost>> {
        info!(?topics, limit, "reddit: searching topics");

        let raw = self.client.search_reddit_keywords(topics, limit).await?;

        let posts = raw
            .into_iter()
            .filter_map(|p| {
                if p.data_type.as_deref() != Some("post") {
                    return None;
                }
                let title = p.title.unwrap_or_default();
                let body = p.body.unwrap_or_default();
                let text = format!("{}\n\n{}", title, body).trim().to_string();
                if text.is_empty() {
                    return None;
                }
                let content_hash = rootsignal_common::content_hash(&text).to_string();

                let engagement = serde_json::json!({
                    "likes": p.up_votes,
                    "comments": p.number_of_comments,
                });

                let mentions = text_extract::extract_mentions(&text);
                let hashtags = text_extract::extract_hashtags(&text);

                Some(FetchedPost {
                    post: InsertPost {
                        source_id,
                        content_hash,
                        text: Some(text),
                        author: None,
                        location: None,
                        engagement: Some(engagement),
                        published_at: p.created_at.as_deref()
                            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                            .map(|dt| dt.with_timezone(&Utc)),
                        permalink: p.url,
                        mentions,
                        hashtags,
                        media_type: Some("text".to_string()),
                        platform_id: None,
                    },
                })
            })
            .collect();

        Ok(posts)
    }
}

/// Extract a Reddit username from a URL like "https://www.reddit.com/user/NAME/..."
fn extract_reddit_username(url: &str) -> Option<String> {
    let parts: Vec<&str> = url.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        if (*part == "user" || *part == "u") && i + 1 < parts.len() {
            let name = parts[i + 1];
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_reddit_user_from_url() {
        assert_eq!(
            extract_reddit_username("https://www.reddit.com/user/someuser/comments/abc"),
            Some("someuser".to_string())
        );
    }

    #[test]
    fn extract_reddit_user_short_form() {
        assert_eq!(
            extract_reddit_username("https://reddit.com/u/testuser"),
            Some("testuser".to_string())
        );
    }
}
