use anyhow::Result;
use async_trait::async_trait;
use taproot_core::{DiscoverConfig, Ingestor, RawPage};

/// Simple HTTP ingestor — fetches pages directly.
pub struct HttpIngestor {
    client: reqwest::Client,
}

impl HttpIngestor {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Ingestor for HttpIngestor {
    async fn discover(&self, config: &DiscoverConfig) -> Result<Vec<RawPage>> {
        // For HTTP, discover just fetches the root URL
        self.fetch_specific(&[config.url.clone()]).await
    }

    async fn fetch_specific(&self, urls: &[String]) -> Result<Vec<RawPage>> {
        let mut pages = Vec::new();

        for url in urls {
            match self.client.get(url).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        let html = response.text().await.unwrap_or_default();
                        let page = RawPage::new(url, &html)
                            .with_html(html.clone())
                            .with_content_type("text/html".to_string())
                            .with_metadata("fetched_via", serde_json::Value::String("http".to_string()));
                        pages.push(page);
                    } else {
                        tracing::warn!(url = %url, status = %response.status(), "HTTP fetch failed");
                    }
                }
                Err(e) => {
                    tracing::warn!(url = %url, error = %e, "HTTP fetch error");
                }
            }
        }

        Ok(pages)
    }
}
