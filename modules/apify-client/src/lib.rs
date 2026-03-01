pub mod error;
pub mod types;

pub use error::{ApifyError, Result};
pub use types::{
    DiscoveredPost, FacebookPost, FacebookScraperInput, InstagramHashtagInput, InstagramPost,
    InstagramScraperInput, RedditPost, RedditScraperInput, RunData, StartUrl, TikTokPost,
    TikTokScraperInput, TikTokSearchInput, Tweet, TweetAuthor, TweetScraperInput, TweetSearchInput,
};
use serde::de::DeserializeOwned;
use types::ApiResponse;


const BASE_URL: &str = "https://api.apify.com/v2";

/// Actor ID for apify/instagram-post-scraper.
const INSTAGRAM_POST_SCRAPER: &str = "nH2AHrwxeTRJoN5hX";

/// Actor slug for apify/instagram-hashtag-scraper.
const INSTAGRAM_HASHTAG_SCRAPER: &str = "apify~instagram-hashtag-scraper";

/// Actor ID for apify/facebook-posts-scraper.
const FACEBOOK_POSTS_SCRAPER: &str = "KoJrdxJCTtpon81KY";

/// Actor ID for apidojo/tweet-scraper.
const TWEET_SCRAPER: &str = "61RPP7dywgiy0JPD0";

/// Actor ID for clockworks/tiktok-scraper.
const TIKTOK_SCRAPER: &str = "GdWCkxBtKWOsKjdch";

/// Actor ID for trudax/reddit-scraper.
const REDDIT_SCRAPER: &str = "FgJtjDwJCLhRH9saM";

pub struct ApifyClient {
    client: reqwest::Client,
    token: String,
}

impl ApifyClient {
    pub fn new(token: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            token,
        }
    }

    /// Start an Instagram profile scrape run. Returns immediately with run metadata.
    pub async fn start_instagram_scrape(&self, username: &str, limit: u32) -> Result<RunData> {
        let input = InstagramScraperInput {
            username: vec![username.to_string()],
            results_limit: limit,
        };

        let url = format!("{}/acts/{}/runs", BASE_URL, INSTAGRAM_POST_SCRAPER);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&input)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ApifyError::Api {
                status: status.as_u16(),
                message: body,
            });
        }

        let api_resp: ApiResponse<RunData> = resp.json().await?;
        Ok(api_resp.data)
    }

    /// Poll until a run completes. Uses `waitForFinish=60` for efficient long-polling.
    pub async fn wait_for_run(&self, run_id: &str) -> Result<RunData> {
        loop {
            let url = format!("{}/actor-runs/{}?waitForFinish=60", BASE_URL, run_id);
            let resp = self
                .client
                .get(&url)
                .bearer_auth(&self.token)
                .send()
                .await?;

            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(ApifyError::Api {
                    status: status.as_u16(),
                    message: body,
                });
            }

            let api_resp: ApiResponse<RunData> = resp.json().await?;
            match api_resp.data.status.as_str() {
                "SUCCEEDED" => return Ok(api_resp.data),
                "FAILED" | "ABORTED" | "TIMED-OUT" => {
                    return Err(ApifyError::RunFailed(api_resp.data.status));
                }
                _ => {
                    tracing::debug!(run_id, status = %api_resp.data.status, "Run still in progress");
                    continue;
                }
            }
        }
    }

    /// Fetch dataset items from a completed run.
    pub async fn get_dataset_items<T: DeserializeOwned>(&self, dataset_id: &str) -> Result<Vec<T>> {
        let url = format!("{}/datasets/{}/items?format=json", BASE_URL, dataset_id);
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ApifyError::Api {
                status: status.as_u16(),
                message: body,
            });
        }

        let items: Vec<T> = resp.json().await?;
        Ok(items)
    }

    /// Scrape Instagram profile posts end-to-end: start run, poll, fetch results.
    pub async fn scrape_instagram_posts(
        &self,
        username: &str,
        limit: u32,
    ) -> Result<Vec<InstagramPost>> {
        tracing::info!(username, limit, "Starting Instagram profile scrape");

        let run = self.start_instagram_scrape(username, limit).await?;
        tracing::info!(run_id = %run.id, "Apify run started, polling for completion");

        let completed = self.wait_for_run(&run.id).await?;
        tracing::info!(
            run_id = %completed.id,
            dataset_id = %completed.default_dataset_id,
            "Run completed, fetching results"
        );

        let posts: Vec<InstagramPost> = self
            .get_dataset_items(&completed.default_dataset_id)
            .await?;
        tracing::info!(count = posts.len(), "Fetched Instagram posts");

        Ok(posts)
    }

    /// Search Instagram hashtags and return normalized DiscoveredPosts.
    /// Uses the apify/instagram-hashtag-scraper actor.
    pub async fn search_instagram_hashtags(
        &self,
        hashtags: &[&str],
        limit: u32,
    ) -> Result<Vec<DiscoveredPost>> {
        tracing::info!(?hashtags, limit, "Starting Instagram hashtag search");

        let input = InstagramHashtagInput {
            hashtags: hashtags.iter().map(|h| h.to_string()).collect(),
            results_limit: limit,
        };

        let url = format!("{}/acts/{}/runs", BASE_URL, INSTAGRAM_HASHTAG_SCRAPER);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&input)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ApifyError::Api {
                status: status.as_u16(),
                message: body,
            });
        }

        let api_resp: ApiResponse<RunData> = resp.json().await?;
        let run = api_resp.data;
        tracing::info!(run_id = %run.id, "Hashtag scrape started, polling for completion");

        let completed = self.wait_for_run(&run.id).await?;
        tracing::info!(
            run_id = %completed.id,
            dataset_id = %completed.default_dataset_id,
            "Run completed, fetching results"
        );

        let posts: Vec<InstagramPost> = self
            .get_dataset_items(&completed.default_dataset_id)
            .await?;

        let discovered: Vec<DiscoveredPost> = posts
            .into_iter()
            .filter_map(|p| p.into_discovered())
            .collect();

        tracing::info!(count = discovered.len(), "Fetched Instagram hashtag posts");
        Ok(discovered)
    }

    /// Scrape Facebook page posts end-to-end: start run, poll, fetch results.
    pub async fn scrape_facebook_posts(
        &self,
        page_url: &str,
        limit: u32,
    ) -> Result<Vec<FacebookPost>> {
        tracing::info!(page_url, limit, "Starting Facebook page scrape");

        // Accept both normalized URLs ("facebook.com/page") and full URLs
        let full_url = if page_url.starts_with("http") {
            page_url.to_string()
        } else {
            format!("https://{}", page_url)
        };

        let input = FacebookScraperInput {
            start_urls: vec![StartUrl { url: full_url }],
            results_limit: limit,
        };

        let url = format!("{}/acts/{}/runs", BASE_URL, FACEBOOK_POSTS_SCRAPER);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&input)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ApifyError::Api {
                status: status.as_u16(),
                message: body,
            });
        }

        let api_resp: ApiResponse<RunData> = resp.json().await?;
        let run = api_resp.data;
        tracing::info!(run_id = %run.id, "Apify run started, polling for completion");

        let completed = self.wait_for_run(&run.id).await?;
        tracing::info!(
            run_id = %completed.id,
            dataset_id = %completed.default_dataset_id,
            "Run completed, fetching results"
        );

        let posts: Vec<FacebookPost> = self
            .get_dataset_items(&completed.default_dataset_id)
            .await?;
        tracing::info!(count = posts.len(), "Fetched Facebook posts");

        Ok(posts)
    }

    /// Scrape TikTok profile posts end-to-end: start run, poll, fetch results.
    pub async fn scrape_tiktok_posts(&self, username: &str, limit: u32) -> Result<Vec<TikTokPost>> {
        tracing::info!(username, limit, "Starting TikTok scrape");

        let input = TikTokScraperInput {
            profiles: vec![username.to_string()],
            results_per_page: limit,
        };

        let url = format!("{}/acts/{}/runs", BASE_URL, TIKTOK_SCRAPER);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&input)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ApifyError::Api {
                status: status.as_u16(),
                message: body,
            });
        }

        let api_resp: ApiResponse<RunData> = resp.json().await?;
        let run = api_resp.data;
        tracing::info!(run_id = %run.id, "Apify run started, polling for completion");

        let completed = self.wait_for_run(&run.id).await?;
        tracing::info!(
            run_id = %completed.id,
            dataset_id = %completed.default_dataset_id,
            "Run completed, fetching results"
        );

        let posts: Vec<TikTokPost> = self
            .get_dataset_items(&completed.default_dataset_id)
            .await?;
        tracing::info!(count = posts.len(), "Fetched TikTok posts");

        Ok(posts)
    }

    /// Search Reddit by keywords. Uses the same trudax/reddit-scraper actor
    /// with Reddit search URLs as startUrls.
    pub async fn search_reddit_keywords(
        &self,
        keywords: &[&str],
        limit: u32,
    ) -> Result<Vec<RedditPost>> {
        tracing::info!(?keywords, limit, "Starting Reddit keyword search");

        let start_urls: Vec<StartUrl> = keywords
            .iter()
            .map(|k| StartUrl {
                url: format!(
                    "https://www.reddit.com/search/?q={}&sort=new",
                    k.replace(' ', "+")
                ),
            })
            .collect();

        let input = RedditScraperInput {
            start_urls,
            max_items: limit,
            sort: "new".to_string(),
        };

        let url = format!("{}/acts/{}/runs", BASE_URL, REDDIT_SCRAPER);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&input)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ApifyError::Api {
                status: status.as_u16(),
                message: body,
            });
        }

        let api_resp: ApiResponse<RunData> = resp.json().await?;
        let run = api_resp.data;
        tracing::info!(run_id = %run.id, "Reddit keyword search started, polling");

        let completed = self.wait_for_run(&run.id).await?;
        let posts: Vec<RedditPost> = self
            .get_dataset_items(&completed.default_dataset_id)
            .await?;
        tracing::info!(
            count = posts.len(),
            "Fetched Reddit posts from keyword search"
        );

        Ok(posts)
    }

    /// Scrape Reddit subreddit posts end-to-end: start run, poll, fetch results.
    pub async fn scrape_reddit_posts(
        &self,
        subreddit_url: &str,
        limit: u32,
    ) -> Result<Vec<RedditPost>> {
        tracing::info!(subreddit_url, limit, "Starting Reddit scrape");

        // Accept both bare identifiers ("TwinCities") and full URLs
        let full_url = if subreddit_url.starts_with("http") {
            subreddit_url.to_string()
        } else {
            format!("https://www.reddit.com/r/{}", subreddit_url)
        };

        let input = RedditScraperInput {
            start_urls: vec![StartUrl { url: full_url }],
            max_items: limit,
            sort: "new".to_string(),
        };

        let url = format!("{}/acts/{}/runs", BASE_URL, REDDIT_SCRAPER);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&input)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ApifyError::Api {
                status: status.as_u16(),
                message: body,
            });
        }

        let api_resp: ApiResponse<RunData> = resp.json().await?;
        let run = api_resp.data;
        tracing::info!(run_id = %run.id, "Apify run started, polling for completion");

        let completed = self.wait_for_run(&run.id).await?;
        tracing::info!(
            run_id = %completed.id,
            dataset_id = %completed.default_dataset_id,
            "Run completed, fetching results"
        );

        let posts: Vec<RedditPost> = self
            .get_dataset_items(&completed.default_dataset_id)
            .await?;
        tracing::info!(count = posts.len(), "Fetched Reddit posts");

        Ok(posts)
    }

    /// Search X/Twitter by keywords. Uses the same apidojo/tweet-scraper actor
    /// with searchTerms instead of twitterHandles.
    pub async fn search_x_keywords(&self, keywords: &[&str], limit: u32) -> Result<Vec<Tweet>> {
        tracing::info!(?keywords, limit, "Starting X/Twitter keyword search");

        let input = TweetSearchInput {
            search_terms: keywords.iter().map(|k| k.to_string()).collect(),
            max_items: limit,
        };

        let url = format!("{}/acts/{}/runs", BASE_URL, TWEET_SCRAPER);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&input)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ApifyError::Api {
                status: status.as_u16(),
                message: body,
            });
        }

        let api_resp: ApiResponse<RunData> = resp.json().await?;
        let run = api_resp.data;
        tracing::info!(run_id = %run.id, "X/Twitter keyword search started, polling");

        let completed = self.wait_for_run(&run.id).await?;
        let tweets: Vec<Tweet> = self
            .get_dataset_items(&completed.default_dataset_id)
            .await?;
        tracing::info!(count = tweets.len(), "Fetched tweets from keyword search");

        Ok(tweets)
    }

    /// Search TikTok by keywords. Uses the clockworks/tiktok-scraper actor
    /// with searchQueries instead of profiles.
    pub async fn search_tiktok_keywords(
        &self,
        keywords: &[&str],
        limit: u32,
    ) -> Result<Vec<TikTokPost>> {
        tracing::info!(?keywords, limit, "Starting TikTok keyword search");

        let input = TikTokSearchInput {
            search_queries: keywords.iter().map(|k| k.to_string()).collect(),
            results_per_page: limit,
        };

        let url = format!("{}/acts/{}/runs", BASE_URL, TIKTOK_SCRAPER);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&input)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ApifyError::Api {
                status: status.as_u16(),
                message: body,
            });
        }

        let api_resp: ApiResponse<RunData> = resp.json().await?;
        let run = api_resp.data;
        tracing::info!(run_id = %run.id, "TikTok keyword search started, polling");

        let completed = self.wait_for_run(&run.id).await?;
        let posts: Vec<TikTokPost> = self
            .get_dataset_items(&completed.default_dataset_id)
            .await?;
        tracing::info!(
            count = posts.len(),
            "Fetched TikTok posts from keyword search"
        );

        Ok(posts)
    }

    /// Scrape X/Twitter posts end-to-end: start run, poll, fetch results.
    pub async fn scrape_x_posts(&self, handle: &str, limit: u32) -> Result<Vec<Tweet>> {
        tracing::info!(handle, limit, "Starting X/Twitter scrape");

        let input = TweetScraperInput {
            twitter_handles: vec![handle.to_string()],
            max_items: limit,
        };

        let url = format!("{}/acts/{}/runs", BASE_URL, TWEET_SCRAPER);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&input)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ApifyError::Api {
                status: status.as_u16(),
                message: body,
            });
        }

        let api_resp: ApiResponse<RunData> = resp.json().await?;
        let run = api_resp.data;
        tracing::info!(run_id = %run.id, "Apify run started, polling for completion");

        let completed = self.wait_for_run(&run.id).await?;
        tracing::info!(
            run_id = %completed.id,
            dataset_id = %completed.default_dataset_id,
            "Run completed, fetching results"
        );

        let tweets: Vec<Tweet> = self
            .get_dataset_items(&completed.default_dataset_id)
            .await?;
        tracing::info!(count = tweets.len(), "Fetched tweets");

        Ok(tweets)
    }
}

