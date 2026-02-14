use apify_client::ApifyClient;
use async_trait::async_trait;
use rootsignal_core::error::{CrawlError, CrawlResult};
use rootsignal_core::ingestor::DiscoverConfig;
use rootsignal_core::types::RawPage;
use rootsignal_core::Ingestor;

pub struct FacebookIngestor {
    client: ApifyClient,
}

impl FacebookIngestor {
    pub fn new(api_key: String) -> Self {
        Self {
            client: ApifyClient::new(api_key),
        }
    }
}

#[async_trait]
impl Ingestor for FacebookIngestor {
    async fn discover(&self, config: &DiscoverConfig) -> CrawlResult<Vec<RawPage>> {
        let handle = config
            .options
            .get("handle")
            .map(|s| s.as_str())
            .unwrap_or(&config.url);

        let page_url = if handle.starts_with("http") {
            handle.to_string()
        } else {
            format!("https://www.facebook.com/{}", handle)
        };

        let limit = config.limit as u32;

        let posts = self
            .client
            .scrape_facebook_posts(&page_url, limit)
            .await
            .map_err(|e| CrawlError::Http(Box::new(e)))?;

        let pages = posts
            .into_iter()
            .map(|post| {
                let content = post.text.clone().unwrap_or_default();
                let url = post
                    .url
                    .clone()
                    .unwrap_or_else(|| page_url.clone());

                let mut page = RawPage::new(url, &content)
                    .with_content_type("social/facebook")
                    .with_metadata("platform", serde_json::Value::String("facebook".into()));

                if let Some(name) = &post.page_name {
                    page = page.with_metadata("page_name", serde_json::Value::String(name.clone()));
                }
                if let Some(time) = &post.time {
                    page = page.with_metadata("posted_at", serde_json::Value::String(time.clone()));
                }
                if let Some(likes) = post.likes {
                    page = page.with_metadata("likes", serde_json::json!(likes));
                }
                if let Some(comments) = post.comments {
                    page = page.with_metadata("comments", serde_json::json!(comments));
                }
                if let Some(shares) = post.shares {
                    page = page.with_metadata("shares", serde_json::json!(shares));
                }
                page
            })
            .collect();

        Ok(pages)
    }

    async fn fetch_specific(&self, urls: &[String]) -> CrawlResult<Vec<RawPage>> {
        tracing::warn!(count = urls.len(), "fetch_specific not supported for Facebook; use discover()");
        Ok(Vec::new())
    }

    fn name(&self) -> &str {
        "apify_facebook"
    }
}
