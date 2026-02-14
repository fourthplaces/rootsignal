pub mod error;
pub mod types;

pub use error::{ApifyError, Result};
pub use types::{
    FacebookPost, FacebookScraperInput, InstagramPost, InstagramScraperInput, RunData, StartUrl,
    Tweet, TweetAuthor, TweetScraperInput,
};

use serde::de::DeserializeOwned;
use types::ApiResponse;

const BASE_URL: &str = "https://api.apify.com/v2";

/// Actor ID for apify/instagram-post-scraper.
const INSTAGRAM_POST_SCRAPER: &str = "nH2AHrwxeTRJoN5hX";

/// Actor ID for apify/facebook-posts-scraper.
const FACEBOOK_POSTS_SCRAPER: &str = "KoJrdxJCTtpon81KY";

/// Actor ID for apidojo/tweet-scraper.
const TWEET_SCRAPER: &str = "61RPP7dywgiy0JPD0";

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

    /// Scrape Facebook page posts end-to-end: start run, poll, fetch results.
    pub async fn scrape_facebook_posts(
        &self,
        page_url: &str,
        limit: u32,
    ) -> Result<Vec<FacebookPost>> {
        tracing::info!(page_url, limit, "Starting Facebook page scrape");

        let input = FacebookScraperInput {
            start_urls: vec![StartUrl {
                url: page_url.to_string(),
            }],
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
