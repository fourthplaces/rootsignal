use apify_client::ApifyClient;
use async_trait::async_trait;
use chrono::Utc;
use rootsignal_core::error::{CrawlError, CrawlResult};
use rootsignal_core::ingestor::DiscoverConfig;
use rootsignal_core::types::RawPage;
use rootsignal_core::Ingestor;

pub struct InstagramIngestor {
    client: ApifyClient,
}

impl InstagramIngestor {
    pub fn new(api_key: String) -> Self {
        Self {
            client: ApifyClient::new(api_key),
        }
    }
}

#[async_trait]
impl Ingestor for InstagramIngestor {
    async fn discover(&self, config: &DiscoverConfig) -> CrawlResult<Vec<RawPage>> {
        let handle = config
            .options
            .get("handle")
            .map(|s| s.as_str())
            .unwrap_or(&config.url);

        let limit = config.limit as u32;

        let posts = self
            .client
            .scrape_instagram_posts(handle, limit)
            .await
            .map_err(|e| CrawlError::Http(Box::new(e)))?;

        let cutoff = Utc::now() - chrono::Duration::days(30);

        let pages = posts
            .into_iter()
            .filter(|p| p.timestamp.map(|t| t > cutoff).unwrap_or(true))
            .map(|post| {
                let content = post.caption.clone().unwrap_or_default();
                let url = post.url.clone();

                let mut page = RawPage::new(url, &content)
                    .with_content_type("social/instagram")
                    .with_metadata("platform", serde_json::Value::String("instagram".into()))
                    .with_metadata(
                        "post_type",
                        serde_json::Value::String(post.post_type.unwrap_or_default()),
                    );

                if let Some(ts) = post.timestamp {
                    page = page.with_fetched_at(ts);
                }
                if let Some(user) = &post.owner_username {
                    page = page.with_metadata("handle", serde_json::Value::String(user.clone()));
                }
                if let Some(likes) = post.likes_count {
                    page = page.with_metadata("likes", serde_json::json!(likes));
                }
                if let Some(comments) = post.comments_count {
                    page = page.with_metadata("comments", serde_json::json!(comments));
                }
                if let Some(loc) = &post.location_name {
                    page = page.with_metadata("location", serde_json::Value::String(loc.clone()));
                }
                page
            })
            .collect();

        Ok(pages)
    }

    async fn fetch_specific(&self, urls: &[String]) -> CrawlResult<Vec<RawPage>> {
        // Apify doesn't support fetching specific post URLs directly.
        // Return empty — callers should use discover() with a handle.
        tracing::warn!(
            count = urls.len(),
            "fetch_specific not supported for Instagram; use discover()"
        );
        Ok(Vec::new())
    }

    fn name(&self) -> &str {
        "apify_instagram"
    }
}
