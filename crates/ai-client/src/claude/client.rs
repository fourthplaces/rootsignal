use anyhow::{anyhow, Result};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use tracing::debug;

use super::types::*;

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1";
const ANTHROPIC_VERSION: &str = "2023-06-01";

pub(crate) struct ClaudeClient {
    api_key: String,
    http: reqwest::Client,
    base_url: String,
}

impl ClaudeClient {
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            http: reqwest::Client::new(),
            base_url: ANTHROPIC_API_URL.to_string(),
        }
    }

    pub fn with_base_url(mut self, url: &str) -> Self {
        self.base_url = url.to_string();
        self
    }

    fn headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(&self.api_key)?,
        );
        headers.insert(
            "anthropic-version",
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Ok(headers)
    }

    pub async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse> {
        let url = format!("{}/messages", self.base_url);

        debug!(model = %request.model, "Claude chat request");

        let response = self
            .http
            .post(&url)
            .headers(self.headers()?)
            .json(request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow!("Claude API error ({}): {}", status, error_text));
        }

        Ok(response.json().await?)
    }
}
