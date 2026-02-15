use async_trait::async_trait;
use rootsignal_core::error::{CrawlError, CrawlResult};
use rootsignal_core::{DiscoverConfig, Ingestor, RawPage};
use serde::{Deserialize, Serialize};

/// Firecrawl adapter — uses Firecrawl API for JavaScript-rendered crawling.
pub struct FirecrawlIngestor {
    api_key: String,
    client: reqwest::Client,
    base_url: String,
    poll_timeout_secs: u64,
    poll_interval_secs: u64,
}

#[derive(Debug, Serialize)]
struct CrawlRequest {
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_depth: Option<u32>,
    scrape_options: ScrapeOptions,
    #[serde(rename = "includePaths", skip_serializing_if = "Vec::is_empty")]
    include_paths: Vec<String>,
    #[serde(rename = "excludePaths", skip_serializing_if = "Vec::is_empty")]
    exclude_paths: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ScrapeOptions {
    formats: Vec<String>,
    #[serde(rename = "onlyMainContent")]
    only_main_content: bool,
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
    completed: Option<u32>,
    total: Option<u32>,
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
            poll_timeout_secs: 300,
            poll_interval_secs: 5,
        }
    }

    pub fn with_poll_timeout(mut self, secs: u64) -> Self {
        self.poll_timeout_secs = secs;
        self
    }

    pub fn with_poll_interval(mut self, secs: u64) -> Self {
        self.poll_interval_secs = secs;
        self
    }

    async fn poll_crawl(&self, crawl_id: &str, source_url: &str) -> CrawlResult<Vec<RawPage>> {
        let max_attempts = self.poll_timeout_secs / self.poll_interval_secs;
        let mut attempts = 0;

        loop {
            attempts += 1;
            if attempts > max_attempts {
                return Err(CrawlError::Timeout {
                    url: source_url.to_string(),
                });
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(self.poll_interval_secs)).await;

            let resp: CrawlStatusResponse = self
                .client
                .get(format!("{}/crawl/{}", self.base_url, crawl_id))
                .bearer_auth(&self.api_key)
                .send()
                .await
                .map_err(|e| CrawlError::Http(Box::new(e)))?
                .json()
                .await
                .map_err(|e| CrawlError::Http(Box::new(e)))?;

            match resp.status.as_str() {
                "completed" => {
                    let pages: Vec<RawPage> = resp
                        .data
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|page_data| {
                            let url = page_data
                                .metadata
                                .as_ref()
                                .and_then(|m| m.source_url.clone())
                                .unwrap_or_default();
                            let content = page_data.markdown.unwrap_or_default();
                            if content.is_empty() {
                                return None;
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
                            Some(page)
                        })
                        .collect();

                    return Ok(pages);
                }
                "failed" => {
                    return Err(CrawlError::Http(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Firecrawl crawl {} failed", crawl_id),
                    ))));
                }
                _ => {
                    if attempts % 6 == 0 {
                        tracing::info!(
                            crawl_id,
                            status = %resp.status,
                            completed = ?resp.completed,
                            total = ?resp.total,
                            "Crawl in progress"
                        );
                    } else {
                        tracing::debug!(crawl_id, status = %resp.status, "Crawl still in progress");
                    }
                }
            }
        }
    }
}

#[async_trait]
impl Ingestor for FirecrawlIngestor {
    async fn discover(&self, config: &DiscoverConfig) -> CrawlResult<Vec<RawPage>> {
        let request = CrawlRequest {
            url: config.url.clone(),
            limit: Some(config.limit as u32),
            max_depth: Some(config.max_depth as u32),
            scrape_options: ScrapeOptions {
                formats: vec!["markdown".to_string(), "html".to_string()],
                only_main_content: true,
            },
            include_paths: config.include_patterns.clone(),
            exclude_paths: config.exclude_patterns.clone(),
        };

        let resp: CrawlResponse = self
            .client
            .post(format!("{}/crawl", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .await
            .map_err(|e| CrawlError::Http(Box::new(e)))?
            .json()
            .await
            .map_err(|e| CrawlError::Http(Box::new(e)))?;

        if !resp.success {
            return Err(CrawlError::Http(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Firecrawl crawl request failed",
            ))));
        }

        let crawl_id = resp.id.ok_or_else(|| {
            CrawlError::Http(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                "No crawl ID returned",
            )))
        })?;

        tracing::info!(crawl_id = %crawl_id, url = %config.url, "Started Firecrawl crawl");
        self.poll_crawl(&crawl_id, &config.url).await
    }

    async fn fetch_specific(&self, urls: &[String]) -> CrawlResult<Vec<RawPage>> {
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
                    let resp: ScrapeResponse = response
                        .json()
                        .await
                        .map_err(|e| CrawlError::Http(Box::new(e)))?;
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

    fn name(&self) -> &str {
        "firecrawl"
    }
}
