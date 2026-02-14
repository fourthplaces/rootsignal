use apify_client::ApifyClient;
use async_trait::async_trait;
use rootsignal_core::error::{CrawlError, CrawlResult};
use rootsignal_core::ingestor::DiscoverConfig;
use rootsignal_core::types::RawPage;
use rootsignal_core::Ingestor;

pub struct XIngestor {
    client: ApifyClient,
}

impl XIngestor {
    pub fn new(api_key: String) -> Self {
        Self {
            client: ApifyClient::new(api_key),
        }
    }
}

#[async_trait]
impl Ingestor for XIngestor {
    async fn discover(&self, config: &DiscoverConfig) -> CrawlResult<Vec<RawPage>> {
        let handle = config
            .options
            .get("handle")
            .map(|s| s.as_str())
            .unwrap_or(&config.url);

        let limit = config.limit as u32;

        let tweets = self
            .client
            .scrape_x_posts(handle, limit)
            .await
            .map_err(|e| CrawlError::Http(Box::new(e)))?;

        let pages = tweets
            .into_iter()
            .map(|tweet| {
                let content = tweet.content().unwrap_or_default().to_string();
                let url = tweet
                    .url
                    .clone()
                    .unwrap_or_else(|| format!("https://x.com/{}", handle));

                let mut page = RawPage::new(url, &content)
                    .with_content_type("social/x")
                    .with_metadata("platform", serde_json::Value::String("x".into()));

                if let Some(author) = &tweet.author {
                    if let Some(name) = &author.user_name {
                        page = page.with_metadata("handle", serde_json::Value::String(name.clone()));
                    }
                }
                if let Some(created) = &tweet.created_at {
                    page = page.with_metadata("posted_at", serde_json::Value::String(created.clone()));
                }
                if let Some(likes) = tweet.like_count {
                    page = page.with_metadata("likes", serde_json::json!(likes));
                }
                if let Some(retweets) = tweet.retweet_count {
                    page = page.with_metadata("retweets", serde_json::json!(retweets));
                }
                if let Some(replies) = tweet.reply_count {
                    page = page.with_metadata("replies", serde_json::json!(replies));
                }
                page
            })
            .collect();

        Ok(pages)
    }

    async fn fetch_specific(&self, urls: &[String]) -> CrawlResult<Vec<RawPage>> {
        tracing::warn!(count = urls.len(), "fetch_specific not supported for X; use discover()");
        Ok(Vec::new())
    }

    fn name(&self) -> &str {
        "apify_x"
    }
}
