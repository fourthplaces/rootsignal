use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use spider_transformations::transformation::content::{
    transform_content_input, ReturnFormat, TransformConfig, TransformInput,
};
use tracing::{info, warn};

// --- PageScraper trait ---

#[async_trait]
pub trait PageScraper: Send + Sync {
    async fn scrape(&self, url: &str) -> Result<String>;
    fn name(&self) -> &str;
}

// --- Chrome + Readability scraper ---

/// Scraper that uses headless Chromium --dump-dom for JS rendering, then
/// spider_transformations Readability extraction for clean main content.
pub struct ChromeScraper;

impl ChromeScraper {
    pub fn new() -> Self {
        info!("Using ChromeScraper (dump-dom + Readability extraction)");
        Self
    }
}

#[async_trait]
impl PageScraper for ChromeScraper {
    async fn scrape(&self, url: &str) -> Result<String> {
        info!(url, scraper = "chrome", "Scraping URL");

        let chrome_bin = std::env::var("CHROME_BIN").unwrap_or_else(|_| "chromium".to_string());
        let tmp_dir = tempfile::tempdir().context("Failed to create temp profile dir")?;

        let output = tokio::time::timeout(
            Duration::from_secs(30),
            tokio::process::Command::new(&chrome_bin)
                .args([
                    "--headless",
                    "--no-sandbox",
                    "--disable-gpu",
                    "--disable-dev-shm-usage",
                    &format!("--user-data-dir={}", tmp_dir.path().display()),
                    "--dump-dom",
                    url,
                ])
                .output(),
        )
        .await
        .context(format!("Chrome timed out after 30s for {url}"))?
        .context(format!("Failed to run Chrome for {url}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(url, scraper = "chrome", stderr = %stderr, "Chrome exited with error");
            return Ok(String::new());
        }

        let html = &output.stdout;

        if html.is_empty() {
            warn!(url, scraper = "chrome", "Empty DOM output");
            return Ok(String::new());
        }

        let parsed_url = url::Url::parse(url).ok();
        let config = TransformConfig {
            readability: true,
            main_content: true,
            return_format: ReturnFormat::Markdown,
            filter_images: true,
            filter_svg: true,
            clean_html: true,
        };
        let input = TransformInput {
            url: parsed_url.as_ref(),
            content: html,
            screenshot_bytes: None,
            encoding: None,
            selector_config: None,
            ignore_tags: None,
        };

        let text = transform_content_input(input, &config);

        if text.trim().is_empty() {
            warn!(url, scraper = "chrome", "Empty content after Readability extraction");
            return Ok(String::new());
        }

        info!(url, scraper = "chrome", bytes = text.len(), "Scraped successfully");
        Ok(text)
    }

    fn name(&self) -> &str {
        "chrome"
    }
}

// --- Tavily (unchanged) ---

pub struct TavilySearcher {
    api_key: String,
    client: reqwest::Client,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub url: String,
    pub title: String,
    pub snippet: String,
}

#[derive(Debug, serde::Deserialize)]
struct TavilyResponse {
    #[serde(default)]
    results: Vec<TavilyResult>,
}

#[derive(Debug, serde::Deserialize)]
struct TavilyResult {
    url: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    content: String,
}

impl TavilySearcher {
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("Failed to build HTTP client"),
        }
    }

    pub async fn search(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>> {
        info!(query, max_results, "Tavily search");

        let body = serde_json::json!({
            "query": query,
            "max_results": max_results,
            "search_depth": "advanced",
            "include_answer": false,
        });

        let resp = self
            .client
            .post("https://api.tavily.com/search")
            .header("Content-Type", "application/json")
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .context("Tavily API request failed")?;

        let data: TavilyResponse = resp.json().await.context("Failed to parse Tavily response")?;

        let results: Vec<SearchResult> = data
            .results
            .into_iter()
            .map(|r| SearchResult {
                url: r.url,
                title: r.title,
                snippet: r.content,
            })
            .collect();

        info!(query, count = results.len(), "Tavily search complete");
        Ok(results)
    }
}
