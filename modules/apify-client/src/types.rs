use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// --- Platform-agnostic discovery types ---

/// A normalized post from any social platform, used by the discovery pipeline.
/// Platform-specific scrapers convert their native post types into this.
#[derive(Debug, Clone)]
pub struct DiscoveredPost {
    pub content: String,
    pub author_username: String,
    pub author_display_name: Option<String>,
    pub post_url: String,
    pub timestamp: Option<DateTime<Utc>>,
    pub platform: String, // "instagram", "x", "tiktok", etc.
}

// --- Instagram hashtag scraper types ---

/// Input for the apify/instagram-hashtag-scraper actor.
#[derive(Debug, Clone, Serialize)]
pub struct InstagramHashtagInput {
    pub hashtags: Vec<String>,
    #[serde(rename = "resultsLimit")]
    pub results_limit: u32,
}

/// Input for the apify/instagram-post-scraper actor.
#[derive(Debug, Clone, Serialize)]
pub struct InstagramScraperInput {
    pub username: Vec<String>,
    #[serde(rename = "resultsLimit")]
    pub results_limit: u32,
}

/// A single Instagram post from the Apify dataset.
/// Also used as the output type for the hashtag scraper (same schema).
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

impl InstagramPost {
    /// Convert to a platform-agnostic DiscoveredPost for the discovery pipeline.
    pub fn into_discovered(self) -> Option<DiscoveredPost> {
        let content = self.caption?;
        let author_username = self.owner_username?;
        Some(DiscoveredPost {
            content,
            author_display_name: self.owner_full_name,
            author_username,
            post_url: self.url,
            timestamp: self.timestamp,
            platform: "instagram".to_string(),
        })
    }
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

/// Input for X/Twitter keyword search via apidojo/tweet-scraper.
#[derive(Debug, Clone, Serialize)]
pub struct TweetSearchInput {
    #[serde(rename = "searchTerms")]
    pub search_terms: Vec<String>,
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

/// Input for the clockworks/tiktok-scraper actor (profile-based).
#[derive(Debug, Clone, Serialize)]
pub struct TikTokScraperInput {
    pub profiles: Vec<String>,
    #[serde(rename = "resultsPerPage")]
    pub results_per_page: u32,
}

/// Input for TikTok keyword/hashtag search.
#[derive(Debug, Clone, Serialize)]
pub struct TikTokSearchInput {
    #[serde(rename = "searchQueries")]
    pub search_queries: Vec<String>,
    #[serde(rename = "resultsPerPage")]
    pub results_per_page: u32,
}

/// A single TikTok post from the Apify dataset.
#[derive(Debug, Clone, Deserialize)]
pub struct TikTokPost {
    pub id: Option<String>,
    pub text: Option<String>,
    #[serde(rename = "webVideoUrl")]
    pub web_video_url: Option<String>,
    #[serde(rename = "createTimeISO")]
    pub create_time_iso: Option<String>,
    #[serde(rename = "authorMeta")]
    pub author_meta: Option<TikTokAuthor>,
    #[serde(rename = "diggCount")]
    pub digg_count: Option<i64>,
    #[serde(rename = "shareCount")]
    pub share_count: Option<i64>,
    #[serde(rename = "playCount")]
    pub play_count: Option<i64>,
    #[serde(rename = "commentCount")]
    pub comment_count: Option<i64>,
    pub hashtags: Option<Vec<TikTokHashtag>>,
}

/// Author metadata from a TikTok post.
#[derive(Debug, Clone, Deserialize)]
pub struct TikTokAuthor {
    pub name: Option<String>,
    #[serde(rename = "nickName")]
    pub nick_name: Option<String>,
}

/// A hashtag reference in a TikTok post.
#[derive(Debug, Clone, Deserialize)]
pub struct TikTokHashtag {
    pub name: Option<String>,
}

/// Input for the jupri/gofundme scraper actor.
#[derive(Debug, Clone, Serialize)]
pub struct GoFundMeScraperInput {
    #[serde(rename = "searchTerms")]
    pub search_terms: Vec<String>,
    #[serde(rename = "maxItems")]
    pub max_items: u32,
}

/// A single GoFundMe campaign from the Apify dataset.
#[derive(Debug, Clone, Deserialize)]
pub struct GoFundMeCampaign {
    pub url: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "currentAmount")]
    pub current_amount: Option<f64>,
    #[serde(rename = "goalAmount")]
    pub goal_amount: Option<f64>,
    #[serde(rename = "donationsCount")]
    pub donations_count: Option<i64>,
    #[serde(rename = "createdAt")]
    pub created_at: Option<String>,
    pub category: Option<String>,
    pub location: Option<String>,
    #[serde(rename = "organizerName")]
    pub organizer_name: Option<String>,
}

/// Input for the trudax/reddit-scraper actor.
#[derive(Debug, Clone, Serialize)]
pub struct RedditScraperInput {
    #[serde(rename = "startUrls")]
    pub start_urls: Vec<StartUrl>,
    #[serde(rename = "maxItems")]
    pub max_items: u32,
    pub sort: String,
}

/// A single Reddit post from the Apify dataset.
#[derive(Debug, Clone, Deserialize)]
pub struct RedditPost {
    pub url: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub subreddit: Option<String>,
    #[serde(rename = "upVotes")]
    pub up_votes: Option<i64>,
    #[serde(rename = "numberOfComments")]
    pub number_of_comments: Option<i64>,
    #[serde(rename = "createdAt")]
    pub created_at: Option<String>,
    /// Apify returns "community", "post", or "comment". Used to filter out non-posts.
    #[serde(rename = "dataType")]
    pub data_type: Option<String>,
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
