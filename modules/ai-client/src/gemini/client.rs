use anyhow::{anyhow, Result};
use tracing::debug;

use super::types::*;

const GEMINI_API_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

pub(crate) struct GeminiClient {
    api_key: String,
    http: reqwest::Client,
    base_url: String,
}

impl GeminiClient {
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            http: reqwest::Client::new(),
            base_url: GEMINI_API_URL.to_string(),
        }
    }

    pub fn with_base_url(mut self, url: &str) -> Self {
        self.base_url = url.to_string();
        self
    }

    pub async fn generate_content(
        &self,
        model: &str,
        request: &GenerateContentRequest,
    ) -> Result<GenerateContentResponse> {
        let url = format!(
            "{}/models/{}:generateContent?key={}",
            self.base_url, model, self.api_key
        );

        debug!(model, "Gemini generateContent request");

        let response = self
            .http
            .post(&url)
            .header("Content-Type", "application/json")
            .json(request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow!("Gemini API error ({}): {}", status, error_text));
        }

        Ok(response.json().await?)
    }

    pub fn extract_text(response: GenerateContentResponse) -> Result<String> {
        response
            .candidates
            .into_iter()
            .next()
            .and_then(|c| c.content)
            .and_then(|content| content.parts.into_iter().next())
            .and_then(|part| part.text)
            .ok_or_else(|| anyhow!("No text in Gemini response"))
    }
}
