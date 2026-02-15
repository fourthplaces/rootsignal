use anyhow::Result;
use async_trait::async_trait;
use rootsignal_core::{RawPage, WebSearcher};
use serde::{Deserialize, Serialize};

/// Tavily web search adapter.
pub struct TavilySearcher {
    api_key: String,
    client: reqwest::Client,
}

#[derive(Debug, Serialize)]
struct TavilySearchRequest {
    api_key: String,
    query: String,
    max_results: u32,
    include_raw_content: bool,
    search_depth: String,
}

#[derive(Debug, Deserialize)]
struct TavilySearchResponse {
    results: Vec<TavilyResult>,
}

#[derive(Debug, Deserialize)]
struct TavilyResult {
    url: String,
    title: String,
    content: String,
    raw_content: Option<String>,
}

impl TavilySearcher {
    pub fn new(api_key: String, client: reqwest::Client) -> Self {
        Self { api_key, client }
    }
}

#[async_trait]
impl WebSearcher for TavilySearcher {
    async fn search(&self, query: &str, max_results: u32) -> Result<Vec<RawPage>> {
        let request = TavilySearchRequest {
            api_key: self.api_key.clone(),
            query: query.to_string(),
            max_results,
            include_raw_content: true,
            search_depth: "advanced".to_string(),
        };

        let resp: TavilySearchResponse = self
            .client
            .post("https://api.tavily.com/search")
            .json(&request)
            .send()
            .await?
            .json()
            .await?;

        let pages = resp
            .results
            .into_iter()
            .map(|r| {
                let content = r.raw_content.unwrap_or(r.content);
                RawPage::new(&r.url, &content)
                    .with_title(r.title)
                    .with_content_type("text/markdown".to_string())
                    .with_metadata(
                        "fetched_via",
                        serde_json::Value::String("tavily".to_string()),
                    )
            })
            .collect();

        Ok(pages)
    }
}
