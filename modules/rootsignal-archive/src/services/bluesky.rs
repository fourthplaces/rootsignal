// Bluesky service: posts and topic search via the AT Protocol public API.
// getAuthorFeed is public (no auth). searchPosts requires authentication.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tracing::info;
use uuid::Uuid;

use crate::store::InsertPost;
use crate::text_extract;

const PUBLIC_API: &str = "https://public.api.bsky.app";

/// Raw fetched post before persistence.
pub(crate) struct FetchedPost {
    pub post: InsertPost,
}

pub(crate) struct BlueskyService {
    client: reqwest::Client,
}

impl BlueskyService {
    pub(crate) fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// Fetch posts from a Bluesky profile. Uses the public API (no auth needed).
    pub(crate) async fn fetch_posts(
        &self,
        identifier: &str,
        source_id: Uuid,
        limit: u32,
    ) -> Result<Vec<FetchedPost>> {
        info!(identifier, limit, "bluesky: fetching posts");

        let resp = self
            .client
            .get(format!("{PUBLIC_API}/xrpc/app.bsky.feed.getAuthorFeed"))
            .query(&[
                ("actor", identifier),
                ("limit", &limit.min(100).to_string()),
            ])
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .context("Bluesky getAuthorFeed request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Bluesky API error {status}: {body}");
        }

        let feed: AuthorFeedResponse = resp.json().await.context("Failed to parse Bluesky feed")?;

        let posts = feed
            .feed
            .into_iter()
            .filter_map(|item| convert_post(item.post, source_id))
            .collect();

        Ok(posts)
    }

    /// Search Bluesky posts by keywords.
    /// Requires the public API to support unauthenticated search.
    /// Falls back to error if search is not available.
    pub(crate) async fn search_topics(
        &self,
        topics: &[&str],
        source_id: Uuid,
        limit: u32,
    ) -> Result<Vec<FetchedPost>> {
        info!(?topics, limit, "bluesky: searching topics");

        let query = topics.join(" ");
        let resp = self
            .client
            .get(format!("{PUBLIC_API}/xrpc/app.bsky.feed.searchPosts"))
            .query(&[
                ("q", query.as_str()),
                ("limit", &limit.min(100).to_string()),
            ])
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .context("Bluesky searchPosts request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Bluesky search API error {status}: {body}");
        }

        let search: SearchPostsResponse = resp
            .json()
            .await
            .context("Failed to parse Bluesky search")?;

        let posts = search
            .posts
            .into_iter()
            .filter_map(|post| convert_post(post, source_id))
            .collect();

        Ok(posts)
    }
}

fn convert_post(post: BskyPostView, source_id: Uuid) -> Option<FetchedPost> {
    let text = post.record.text.filter(|t| !t.is_empty())?;
    let content_hash = rootsignal_common::content_hash(&text).to_string();

    let engagement = serde_json::json!({
        "likes": post.like_count,
        "comments": post.reply_count,
        "shares": post.repost_count,
    });

    // Extract mentions from facets
    let mut mentions: Vec<String> = post
        .record
        .facets
        .unwrap_or_default()
        .into_iter()
        .flat_map(|f| f.features)
        .filter_map(|feat| {
            if feat.r#type == "app.bsky.richtext.facet#mention" {
                feat.did
            } else {
                None
            }
        })
        .collect();
    // Also pick up @mentions from text as fallback (facets use DIDs, text has handles)
    let text_mentions = text_extract::extract_mentions(&text);
    for m in text_mentions {
        if !mentions.contains(&m) {
            mentions.push(m);
        }
    }

    let hashtags = text_extract::extract_hashtags(&text);

    let published_at = post
        .record
        .created_at
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|dt| dt.with_timezone(&Utc));

    // Build permalink from URI: at://did/app.bsky.feed.post/rkey → https://bsky.app/profile/handle/post/rkey
    let permalink = build_permalink(&post.uri, &post.author.handle);

    // Extract rkey from URI as platform_id
    let platform_id = post.uri.rsplit('/').next().map(|s| s.to_string());

    Some(FetchedPost {
        post: InsertPost {
            source_id,
            content_hash,
            text: Some(text),
            author: Some(post.author.handle),
            location: None,
            engagement: Some(engagement),
            published_at,
            permalink,
            mentions,
            hashtags,
            media_type: None,
            platform_id,
        },
    })
}

fn build_permalink(uri: &str, handle: &str) -> Option<String> {
    // URI format: at://did:plc:xxx/app.bsky.feed.post/rkey
    let rkey = uri.rsplit('/').next()?;
    Some(format!("https://bsky.app/profile/{handle}/post/{rkey}"))
}

// --- AT Protocol response types ---

#[derive(Deserialize)]
struct AuthorFeedResponse {
    feed: Vec<FeedItem>,
}

#[derive(Deserialize)]
struct FeedItem {
    post: BskyPostView,
}

#[derive(Deserialize)]
struct SearchPostsResponse {
    posts: Vec<BskyPostView>,
}

#[derive(Deserialize)]
struct BskyPostView {
    uri: String,
    author: BskyAuthor,
    record: BskyRecord,
    #[serde(rename = "likeCount")]
    like_count: Option<i64>,
    #[serde(rename = "replyCount")]
    reply_count: Option<i64>,
    #[serde(rename = "repostCount")]
    repost_count: Option<i64>,
}

#[derive(Deserialize)]
struct BskyAuthor {
    handle: String,
}

#[derive(Deserialize)]
struct BskyRecord {
    text: Option<String>,
    #[serde(rename = "createdAt")]
    created_at: Option<String>,
    facets: Option<Vec<BskyFacet>>,
}

#[derive(Deserialize)]
struct BskyFacet {
    features: Vec<BskyFacetFeature>,
}

#[derive(Deserialize)]
struct BskyFacetFeature {
    #[serde(rename = "$type")]
    r#type: String,
    /// DID for mention facets
    did: Option<String>,
}
