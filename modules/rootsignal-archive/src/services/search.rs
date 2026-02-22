// Web search service (Serper / Google Search).
// Returns universal ArchivedSearchResults content type.

use std::time::Duration;

use anyhow::{Context, Result};
use rootsignal_common::SearchResult;
use tracing::info;
use uuid::Uuid;

use crate::store::InsertSearchResults;

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

pub(crate) struct FetchedSearchResults {
    pub results: InsertSearchResults,
}

pub(crate) struct SearchService {
    api_key: String,
    client: reqwest::Client,
}

impl SearchService {
    pub(crate) fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("Failed to build HTTP client"),
        }
    }

    /// Run a web search and return InsertSearchResults.
    pub(crate) async fn search(
        &self,
        query: &str,
        source_id: Uuid,
        max_results: usize,
    ) -> Result<FetchedSearchResults> {
        info!(query, max_results, "search: querying serper");

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

        let results_json =
            serde_json::to_value(&results).unwrap_or(serde_json::Value::Array(vec![]));
        let content_hash = rootsignal_common::content_hash(
            &serde_json::to_string(&results_json).unwrap_or_default(),
        )
        .to_string();

        info!(query, count = results.len(), "search: complete");

        Ok(FetchedSearchResults {
            results: InsertSearchResults {
                source_id,
                content_hash,
                query: query.to_string(),
                results: results_json,
            },
        })
    }
}
