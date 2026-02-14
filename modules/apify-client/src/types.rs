use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Input for the apify/instagram-post-scraper actor.
#[derive(Debug, Clone, Serialize)]
pub struct InstagramScraperInput {
    pub username: Vec<String>,
    #[serde(rename = "resultsLimit")]
    pub results_limit: u32,
}

/// A single Instagram post from the Apify dataset.
#[derive(Debug, Clone, Deserialize)]
pub struct InstagramPost {
    pub caption: Option<String>,
    #[serde(rename = "ownerUsername")]
    pub owner_username: Option<String>,
    #[serde(rename = "ownerFullName")]
    pub owner_full_name: Option<String>,
    pub url: String,
    #[serde(rename = "shortCode")]
    pub short_code: Option<String>,
    #[serde(rename = "displayUrl")]
    pub display_url: Option<String>,
    #[serde(rename = "likesCount")]
    pub likes_count: Option<i64>,
    #[serde(rename = "commentsCount")]
    pub comments_count: Option<i64>,
    pub timestamp: Option<DateTime<Utc>>,
    #[serde(rename = "type")]
    pub post_type: Option<String>,
    pub mentions: Option<Vec<String>>,
    #[serde(rename = "locationName")]
    pub location_name: Option<String>,
}

/// Wrapper for Apify API responses.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiResponse<T> {
    pub data: T,
}

/// Input for the apify/facebook-posts-scraper actor.
#[derive(Debug, Clone, Serialize)]
pub struct FacebookScraperInput {
    #[serde(rename = "startUrls")]
    pub start_urls: Vec<StartUrl>,
    #[serde(rename = "resultsLimit")]
    pub results_limit: u32,
}

/// A start URL entry for Facebook scraper input.
#[derive(Debug, Clone, Serialize)]
pub struct StartUrl {
    pub url: String,
}

/// A single Facebook post from the Apify dataset.
#[derive(Debug, Clone, Deserialize)]
pub struct FacebookPost {
    pub url: Option<String>,
    pub text: Option<String>,
    pub time: Option<String>,
    #[serde(rename = "pageName")]
    pub page_name: Option<String>,
    pub likes: Option<i64>,
    pub comments: Option<i64>,
    pub shares: Option<i64>,
}

/// Input for the apidojo/tweet-scraper actor.
#[derive(Debug, Clone, Serialize)]
pub struct TweetScraperInput {
    #[serde(rename = "twitterHandles")]
    pub twitter_handles: Vec<String>,
    #[serde(rename = "maxItems")]
    pub max_items: u32,
}

/// Author info nested inside a Tweet.
#[derive(Debug, Clone, Deserialize)]
pub struct TweetAuthor {
    #[serde(rename = "userName")]
    pub user_name: Option<String>,
    pub name: Option<String>,
}

/// A single tweet from the Apify dataset.
#[derive(Debug, Clone, Deserialize)]
pub struct Tweet {
    pub id: Option<String>,
    pub text: Option<String>,
    #[serde(rename = "full_text")]
    pub full_text: Option<String>,
    pub url: Option<String>,
    #[serde(rename = "created_at")]
    pub created_at: Option<String>,
    pub author: Option<TweetAuthor>,
    #[serde(rename = "likeCount")]
    pub like_count: Option<i64>,
    #[serde(rename = "retweetCount")]
    pub retweet_count: Option<i64>,
    #[serde(rename = "replyCount")]
    pub reply_count: Option<i64>,
}

impl Tweet {
    /// Returns whichever text field is populated, preferring `full_text`.
    pub fn content(&self) -> Option<&str> {
        self.full_text.as_deref().or(self.text.as_deref())
    }
}

/// Apify actor run metadata.
#[derive(Debug, Clone, Deserialize)]
pub struct RunData {
    pub id: String,
    pub status: String,
    #[serde(rename = "defaultDatasetId")]
    pub default_dataset_id: String,
    #[serde(rename = "startedAt")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(rename = "finishedAt")]
    pub finished_at: Option<DateTime<Utc>>,
}
