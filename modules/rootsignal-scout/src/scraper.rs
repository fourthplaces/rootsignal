use anyhow::{Context, Result};
use async_trait::async_trait;
use tracing::{info, warn};

// --- PageScraper trait ---

#[async_trait]
pub trait PageScraper: Send + Sync {
    async fn scrape(&self, url: &str) -> Result<String>;
    fn name(&self) -> &str;
}

// --- Spider (default, no API key) ---

/// Scraper that uses Chromium's --dump-dom for full JS rendering.
/// Bypasses Spider's broken chromiumoxide CDP layer entirely.
pub struct ChromeScraper;

impl ChromeScraper {
    pub fn new() -> Self {
        Self
    }

    fn html_to_text(html: &str) -> String {
        html2text::from_read(html.as_bytes(), 120).unwrap_or_default()
    }
}

#[async_trait]
impl PageScraper for ChromeScraper {
    async fn scrape(&self, url: &str) -> Result<String> {
        info!(url, scraper = "chrome", "Scraping URL");

        let chrome_bin = std::env::var("CHROME_BIN").unwrap_or_else(|_| "chromium".to_string());

        let output = tokio::process::Command::new(&chrome_bin)
            .args([
                "--headless",
                "--no-sandbox",
                "--disable-gpu",
                "--disable-dev-shm-usage",
                "--dump-dom",
                url,
            ])
            .output()
            .await
            .context(format!("Failed to run Chrome for {url}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(url, scraper = "chrome", stderr = %stderr, "Chrome exited with error");
            return Ok(String::new());
        }

        let html = String::from_utf8_lossy(&output.stdout).to_string();

        if html.trim().is_empty() {
            warn!(url, scraper = "chrome", "Empty DOM output");
            return Ok(String::new());
        }

        let text = Self::html_to_text(&html);
        info!(url, scraper = "chrome", bytes = text.len(), "Scraped successfully");
        Ok(text)
    }

    fn name(&self) -> &str {
        "chrome"
    }
}

// --- Firecrawl (API-based fallback) ---

pub struct FirecrawlScraper {
    app: firecrawl::FirecrawlApp,
}

impl FirecrawlScraper {
    pub fn new(api_key: &str) -> Result<Self> {
        let app = firecrawl::FirecrawlApp::new(api_key)
            .context("Failed to create Firecrawl client")?;
        Ok(Self { app })
    }
}

#[async_trait]
impl PageScraper for FirecrawlScraper {
    async fn scrape(&self, url: &str) -> Result<String> {
        info!(url, scraper = "firecrawl", "Scraping URL");

        let result = self
            .app
            .scrape_url(url, None)
            .await
            .context(format!("Failed to scrape {url}"))?;

        let markdown = result.markdown.unwrap_or_default();

        if markdown.is_empty() {
            warn!(url, scraper = "firecrawl", "Scrape returned empty content");
        } else {
            info!(url, scraper = "firecrawl", bytes = markdown.len(), "Scraped successfully");
        }

        Ok(markdown)
    }

    fn name(&self) -> &str {
        "firecrawl"
    }
}

// --- Fallback: tries Spider first, then Firecrawl ---

pub struct FallbackScraper {
    primary: Box<dyn PageScraper>,
    fallback: Box<dyn PageScraper>,
}

impl FallbackScraper {
    pub fn new(primary: Box<dyn PageScraper>, fallback: Box<dyn PageScraper>) -> Self {
        Self { primary, fallback }
    }
}

#[async_trait]
impl PageScraper for FallbackScraper {
    async fn scrape(&self, url: &str) -> Result<String> {
        match self.primary.scrape(url).await {
            Ok(content) if !content.is_empty() => Ok(content),
            Ok(_) => {
                warn!(
                    url,
                    primary = self.primary.name(),
                    fallback = self.fallback.name(),
                    "Primary scraper returned empty, trying fallback"
                );
                self.fallback.scrape(url).await
            }
            Err(e) => {
                warn!(
                    url,
                    primary = self.primary.name(),
                    fallback = self.fallback.name(),
                    error = %e,
                    "Primary scraper failed, trying fallback"
                );
                self.fallback.scrape(url).await
            }
        }
    }

    fn name(&self) -> &str {
        "fallback"
    }
}

// --- Builder helper ---

pub fn build_scraper(firecrawl_api_key: &str) -> Result<Box<dyn PageScraper>> {
    info!("Using Chrome scraper (headless Chromium --dump-dom)");
    Ok(Box::new(ChromeScraper::new()))
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

impl TavilySearcher {
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            client: reqwest::Client::new(),
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

        let data: serde_json::Value = resp.json().await.context("Failed to parse Tavily response")?;

        let results = data["results"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|r| {
                        let url = r["url"].as_str()?.to_string();
                        let title = r["title"].as_str().unwrap_or("").to_string();
                        let snippet = r["content"].as_str().unwrap_or("").to_string();
                        Some(SearchResult { url, title, snippet })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        info!(query, count = results.len(), "Tavily search complete");
        Ok(results)
    }
}
