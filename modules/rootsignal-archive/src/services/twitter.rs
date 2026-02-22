// Twitter/X service: posts, topic search.
// Wraps ApifyClient, returns universal content types.

use anyhow::Result;
use apify_client::ApifyClient;
use chrono::{DateTime, Utc};
use tracing::info;
use uuid::Uuid;

use crate::store::InsertPost;
use crate::text_extract;

/// Parse Twitter's created_at format: "Wed Oct 10 20:19:24 +0000 2018"
fn parse_twitter_date(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_str(s, "%a %b %d %H:%M:%S %z %Y")
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

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

                let mentions = text_extract::extract_mentions(&text);
                let hashtags = text_extract::extract_hashtags(&text);

                Some(FetchedPost {
                    post: InsertPost {
                        source_id,
                        content_hash,
                        text: Some(text),
                        author: t.author.as_ref().and_then(|a| a.user_name.clone()),
                        location: None,
                        engagement: Some(engagement),
                        published_at: t.created_at.as_deref().and_then(parse_twitter_date),
                        permalink: t.url,
                        mentions,
                        hashtags,
                        media_type: None,
                        platform_id: t.id,
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

                let mentions = text_extract::extract_mentions(&text);
                let hashtags = text_extract::extract_hashtags(&text);

                Some(FetchedPost {
                    post: InsertPost {
                        source_id,
                        content_hash,
                        text: Some(text),
                        author: t.author.as_ref().and_then(|a| a.user_name.clone()),
                        location: None,
                        engagement: Some(engagement),
                        published_at: t.created_at.as_deref().and_then(parse_twitter_date),
                        permalink: t.url,
                        mentions,
                        hashtags,
                        media_type: None,
                        platform_id: t.id,
                    },
                })
            })
            .collect();

        Ok(posts)
    }
}
