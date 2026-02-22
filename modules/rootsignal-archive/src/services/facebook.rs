// Facebook service: posts only (no topic search support).
// Wraps ApifyClient, returns universal content types.

use anyhow::Result;
use apify_client::ApifyClient;
use chrono::{DateTime, NaiveDateTime, Utc};
use tracing::info;
use uuid::Uuid;

use crate::store::InsertPost;
use crate::text_extract;

/// Raw fetched post before persistence.
pub(crate) struct FetchedPost {
    pub post: InsertPost,
}

pub(crate) struct FacebookService {
    client: ApifyClient,
}

impl FacebookService {
    pub(crate) fn new(client: ApifyClient) -> Self {
        Self { client }
    }

    /// Fetch posts from a Facebook page.
    pub(crate) async fn fetch_posts(
        &self,
        identifier: &str,
        source_id: Uuid,
        limit: u32,
    ) -> Result<Vec<FetchedPost>> {
        info!(identifier, limit, "facebook: fetching posts");

        let raw = self.client.scrape_facebook_posts(identifier, limit).await?;

        let posts = raw
            .into_iter()
            .filter_map(|p| {
                let text = p.text.filter(|t| !t.is_empty())?;
                let content_hash = rootsignal_common::content_hash(&text).to_string();

                let engagement = serde_json::json!({
                    "likes": p.likes,
                    "comments": p.comments,
                    "shares": p.shares,
                });

                let mentions = text_extract::extract_mentions(&text);
                let hashtags = text_extract::extract_hashtags(&text);

                Some(FetchedPost {
                    post: InsertPost {
                        source_id,
                        content_hash,
                        text: Some(text),
                        author: p.page_name,
                        location: None,
                        engagement: Some(engagement),
                        published_at: p.time.as_deref().and_then(|s| {
                            DateTime::parse_from_rfc3339(s)
                                .map(|dt| dt.with_timezone(&Utc))
                                .ok()
                                .or_else(|| {
                                    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                                        .map(|ndt| ndt.and_utc())
                                        .ok()
                                })
                        }),
                        permalink: p.url,
                        mentions,
                        hashtags,
                        media_type: None,
                        platform_id: None,
                    },
                })
            })
            .collect();

        Ok(posts)
    }
}
