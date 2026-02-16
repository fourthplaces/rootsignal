use anyhow::{Context, Result};
use firecrawl::FirecrawlApp;
use tracing::{info, warn};

/// Scrape a URL using Firecrawl and return clean markdown content.
pub struct Scraper {
    app: FirecrawlApp,
}

impl Scraper {
    pub fn new(firecrawl_api_key: &str) -> Result<Self> {
        let app = FirecrawlApp::new(firecrawl_api_key)
            .context("Failed to create Firecrawl client")?;
        Ok(Self { app })
    }

    /// Scrape a single URL and return its markdown content.
    pub async fn scrape(&self, url: &str) -> Result<String> {
        info!(url, "Scraping URL");

        let result = self
            .app
            .scrape_url(url, None)
            .await
            .context(format!("Failed to scrape {url}"))?;

        let markdown = result.markdown.unwrap_or_default();

        if markdown.is_empty() {
            warn!(url, "Scrape returned empty content");
        } else {
            info!(url, bytes = markdown.len(), "Scraped successfully");
        }

        Ok(markdown)
    }
}

/// Search Tavily for civic signals and return a list of URLs with snippets.
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

    /// Search Tavily and return results.
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
