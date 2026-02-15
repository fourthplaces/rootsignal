use apify_client::ApifyClient;
use async_trait::async_trait;
use rootsignal_core::error::{CrawlError, CrawlResult};
use rootsignal_core::ingestor::DiscoverConfig;
use rootsignal_core::types::RawPage;
use rootsignal_core::Ingestor;

pub struct GoFundMeIngestor {
    client: ApifyClient,
}

impl GoFundMeIngestor {
    pub fn new(api_key: String) -> Self {
        Self {
            client: ApifyClient::new(api_key),
        }
    }
}

#[async_trait]
impl Ingestor for GoFundMeIngestor {
    async fn discover(&self, config: &DiscoverConfig) -> CrawlResult<Vec<RawPage>> {
        let search_term = config
            .options
            .get("handle")
            .map(|s| s.as_str())
            .unwrap_or(&config.url);

        let limit = config.limit as u32;

        let campaigns = self
            .client
            .scrape_gofundme(search_term, limit)
            .await
            .map_err(|e| CrawlError::Http(Box::new(e)))?;

        let pages = campaigns
            .into_iter()
            .map(|campaign| {
                let title = campaign.title.clone().unwrap_or_default();
                let description = campaign.description.clone().unwrap_or_default();
                let content = format!("{}\n\n{}", title, description);
                let url = campaign
                    .url
                    .clone()
                    .unwrap_or_else(|| "https://www.gofundme.com".to_string());

                let mut page = RawPage::new(url, &content)
                    .with_content_type("fundraising/gofundme")
                    .with_metadata("platform", serde_json::Value::String("gofundme".into()));

                if let Some(title) = &campaign.title {
                    page = page.with_metadata("title", serde_json::Value::String(title.clone()));
                }
                if let Some(current) = campaign.current_amount {
                    page = page.with_metadata("current_amount", serde_json::json!(current));
                }
                if let Some(goal) = campaign.goal_amount {
                    page = page.with_metadata("goal_amount", serde_json::json!(goal));
                }
                if let Some(donations) = campaign.donations_count {
                    page = page.with_metadata("donations_count", serde_json::json!(donations));
                }
                if let Some(category) = &campaign.category {
                    page = page.with_metadata("category", serde_json::Value::String(category.clone()));
                }
                if let Some(location) = &campaign.location {
                    page = page.with_metadata("location", serde_json::Value::String(location.clone()));
                }
                if let Some(organizer) = &campaign.organizer_name {
                    page = page.with_metadata("organizer", serde_json::Value::String(organizer.clone()));
                }
                if let Some(created) = &campaign.created_at {
                    page = page.with_metadata("created_at", serde_json::Value::String(created.clone()));
                }
                page
            })
            .collect();

        Ok(pages)
    }

    async fn fetch_specific(&self, urls: &[String]) -> CrawlResult<Vec<RawPage>> {
        tracing::warn!(count = urls.len(), "fetch_specific not supported for GoFundMe; use discover()");
        Ok(Vec::new())
    }

    fn name(&self) -> &str {
        "apify_gofundme"
    }
}
