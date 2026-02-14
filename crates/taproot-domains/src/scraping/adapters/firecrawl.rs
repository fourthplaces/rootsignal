use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use taproot_core::{DiscoverConfig, Ingestor, RawPage};

/// Firecrawl adapter — uses Firecrawl API for JavaScript-rendered crawling.
pub struct FirecrawlIngestor {
    api_key: String,
    client: reqwest::Client,
    base_url: String,
}

#[derive(Debug, Serialize)]
struct CrawlRequest {
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_depth: Option<u32>,
    scrape_options: ScrapeOptions,
}

#[derive(Debug, Serialize)]
struct ScrapeOptions {
    formats: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ScrapeRequest {
    url: String,
    formats: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ScrapeResponse {
    success: bool,
    data: Option<ScrapeData>,
}

#[derive(Debug, Deserialize)]
struct ScrapeData {
    markdown: Option<String>,
    html: Option<String>,
    metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct CrawlResponse {
    success: bool,
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CrawlStatusResponse {
    status: String,
    data: Option<Vec<CrawlPageData>>,
}

#[derive(Debug, Deserialize)]
struct CrawlPageData {
    markdown: Option<String>,
    html: Option<String>,
    metadata: Option<CrawlPageMetadata>,
}

#[derive(Debug, Deserialize)]
struct CrawlPageMetadata {
    #[serde(rename = "sourceURL")]
    source_url: Option<String>,
    title: Option<String>,
}

impl FirecrawlIngestor {
    pub fn new(api_key: String, client: reqwest::Client) -> Self {
        Self {
            api_key,
            client,
            base_url: "https://api.firecrawl.dev/v1".to_string(),
        }
    }

    async fn poll_crawl(&self, crawl_id: &str) -> Result<Vec<RawPage>> {
        let mut pages = Vec::new();
        let mut attempts = 0;
        let max_attempts = 60; // 5 minutes at 5s intervals

        loop {
            attempts += 1;
            if attempts > max_attempts {
                tracing::warn!(crawl_id, "Crawl timed out after {} attempts", max_attempts);
                break;
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

            let resp: CrawlStatusResponse = self
                .client
                .get(format!("{}/crawl/{}", self.base_url, crawl_id))
                .bearer_auth(&self.api_key)
                .send()
                .await?
                .json()
                .await?;

            match resp.status.as_str() {
                "completed" => {
                    if let Some(data) = resp.data {
                        for page_data in data {
                            let url = page_data
                                .metadata
                                .as_ref()
                                .and_then(|m| m.source_url.clone())
                                .unwrap_or_default();
                            let content = page_data.markdown.unwrap_or_default();
                            if content.is_empty() {
                                continue;
                            }
                            let mut page = RawPage::new(&url, &content)
                                .with_content_type("text/markdown".to_string())
                                .with_metadata(
                                    "fetched_via",
                                    serde_json::Value::String("firecrawl".to_string()),
                                );
                            if let Some(html) = page_data.html {
                                page = page.with_html(html);
                            }
                            if let Some(meta) = &page_data.metadata {
                                if let Some(title) = &meta.title {
                                    page = page.with_title(title.clone());
                                }
                            }
                            pages.push(page);
                        }
                    }
                    break;
                }
                "failed" => {
                    tracing::error!(crawl_id, "Crawl failed");
                    break;
                }
                _ => {
                    tracing::debug!(crawl_id, status = %resp.status, "Crawl still in progress");
                }
            }
        }

        Ok(pages)
    }
}

#[async_trait]
impl Ingestor for FirecrawlIngestor {
    async fn discover(&self, config: &DiscoverConfig) -> Result<Vec<RawPage>> {
        let request = CrawlRequest {
            url: config.url.clone(),
            limit: Some(config.limit),
            max_depth: Some(config.max_depth),
            scrape_options: ScrapeOptions {
                formats: vec!["markdown".to_string(), "html".to_string()],
            },
        };

        let resp: CrawlResponse = self
            .client
            .post(format!("{}/crawl", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .await?
            .json()
            .await?;

        if !resp.success {
            return Err(anyhow::anyhow!("Firecrawl crawl request failed"));
        }

        let crawl_id = resp
            .id
            .ok_or_else(|| anyhow::anyhow!("No crawl ID returned"))?;

        tracing::info!(crawl_id = %crawl_id, url = %config.url, "Started Firecrawl crawl");
        self.poll_crawl(&crawl_id).await
    }

    async fn fetch_specific(&self, urls: &[String]) -> Result<Vec<RawPage>> {
        let mut pages = Vec::new();

        for url in urls {
            let request = ScrapeRequest {
                url: url.clone(),
                formats: vec!["markdown".to_string(), "html".to_string()],
            };

            match self
                .client
                .post(format!("{}/scrape", self.base_url))
                .bearer_auth(&self.api_key)
                .json(&request)
                .send()
                .await
            {
                Ok(response) => {
                    let resp: ScrapeResponse = response.json().await?;
                    if resp.success {
                        if let Some(data) = resp.data {
                            let content = data.markdown.unwrap_or_default();
                            if !content.is_empty() {
                                let mut page = RawPage::new(url, &content)
                                    .with_content_type("text/markdown".to_string())
                                    .with_metadata(
                                        "fetched_via",
                                        serde_json::Value::String("firecrawl".to_string()),
                                    );
                                if let Some(html) = data.html {
                                    page = page.with_html(html);
                                }
                                pages.push(page);
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(url = %url, error = %e, "Firecrawl scrape error");
                }
            }
        }

        Ok(pages)
    }
}
