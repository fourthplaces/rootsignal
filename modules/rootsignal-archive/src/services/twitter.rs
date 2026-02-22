// Twitter/X service: posts, topic search.
// Wraps ApifyClient, returns universal content types.

use anyhow::Result;
use apify_client::ApifyClient;
use tracing::info;
use uuid::Uuid;

use crate::store::InsertPost;

/// Raw fetched post before persistence.
pub(crate) struct FetchedPost {
    pub post: InsertPost,
}

pub(crate) struct TwitterService {
    client: ApifyClient,
}

impl TwitterService {
    pub(crate) fn new(client: ApifyClient) -> Self {
        Self { client }
    }

    /// Fetch posts (tweets) from a Twitter/X profile.
    pub(crate) async fn fetch_posts(
        &self,
        identifier: &str,
        source_id: Uuid,
        limit: u32,
    ) -> Result<Vec<FetchedPost>> {
        info!(identifier, limit, "twitter: fetching posts");

        let raw = self.client.scrape_x_posts(identifier, limit).await?;

        let posts = raw
            .into_iter()
            .filter_map(|t| {
                let text = t.content()?.to_string();
                if text.is_empty() {
                    return None;
                }
                let content_hash = rootsignal_common::content_hash(&text).to_string();

                let engagement = serde_json::json!({
                    "likes": t.like_count,
                    "comments": t.reply_count,
                    "shares": t.retweet_count,
                });

                Some(FetchedPost {
                    post: InsertPost {
                        source_id,
                        content_hash,
                        text: Some(text),
                        author: t.author.as_ref().and_then(|a| a.user_name.clone()),
                        location: None,
                        engagement: Some(engagement),
                        published_at: None,
                        permalink: t.url,
                    },
                })
            })
            .collect();

        Ok(posts)
    }

    /// Search Twitter/X by keywords (topic search).
    pub(crate) async fn search_topics(
        &self,
        topics: &[&str],
        source_id: Uuid,
        limit: u32,
    ) -> Result<Vec<FetchedPost>> {
        info!(?topics, limit, "twitter: searching topics");

        let raw = self.client.search_x_keywords(topics, limit).await?;

        let posts = raw
            .into_iter()
            .filter_map(|t| {
                let text = t.content()?.to_string();
                if text.is_empty() {
                    return None;
                }
                let content_hash = rootsignal_common::content_hash(&text).to_string();

                let engagement = serde_json::json!({
                    "likes": t.like_count,
                    "comments": t.reply_count,
                    "shares": t.retweet_count,
                });

                Some(FetchedPost {
                    post: InsertPost {
                        source_id,
                        content_hash,
                        text: Some(text),
                        author: t.author.as_ref().and_then(|a| a.user_name.clone()),
                        location: None,
                        engagement: Some(engagement),
                        published_at: None,
                        permalink: t.url,
                    },
                })
            })
            .collect();

        Ok(posts)
    }
}
