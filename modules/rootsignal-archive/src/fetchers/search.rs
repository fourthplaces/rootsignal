// Serper (Google Search) fetcher.
// Moved from scout::pipeline::scraper in Phase 3.

use std::time::Duration;

use anyhow::{Context, Result};
use rootsignal_common::SearchResult;
use tracing::info;

#[derive(Debug, serde::Deserialize)]
struct SerperResponse {
    #[serde(default)]
    organic: Vec<SerperResult>,
}

#[derive(Debug, serde::Deserialize)]
struct SerperResult {
    #[serde(default)]
    link: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    snippet: String,
}

pub(crate) struct SerperFetcher {
    api_key: String,
    client: reqwest::Client,
}

impl SerperFetcher {
    pub(crate) fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("Failed to build HTTP client"),
        }
    }

    pub(crate) async fn search(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>> {
        info!(query, max_results, "Serper search");

        let body = serde_json::json!({
            "q": query,
            "num": max_results,
        });

        let resp = self
            .client
            .post("https://google.serper.dev/search")
            .header("X-API-KEY", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Serper API request failed")?;

        let data: SerperResponse = resp
            .json()
            .await
            .context("Failed to parse Serper response")?;

        let results: Vec<SearchResult> = data
            .organic
            .into_iter()
            .map(|r| SearchResult {
                url: r.link,
                title: r.title,
                snippet: r.snippet,
            })
            .collect();

        info!(query, count = results.len(), "Serper search complete");
        Ok(results)
    }
}
