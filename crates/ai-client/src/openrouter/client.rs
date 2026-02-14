use anyhow::{anyhow, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use tracing::debug;

use super::types::*;

const OPENROUTER_API_URL: &str = "https://openrouter.ai/api/v1";

pub(crate) struct OpenRouterClient {
    api_key: String,
    http: reqwest::Client,
    app_name: Option<String>,
    site_url: Option<String>,
}

impl OpenRouterClient {
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            http: reqwest::Client::new(),
            app_name: None,
            site_url: None,
        }
    }

    pub fn with_app_name(mut self, name: &str) -> Self {
        self.app_name = Some(name.to_string());
        self
    }

    pub fn with_site_url(mut self, url: &str) -> Self {
        self.site_url = Some(url.to_string());
        self
    }

    fn headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.api_key))?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        if let Some(ref url) = self.site_url {
            if let Ok(val) = HeaderValue::from_str(url) {
                headers.insert("HTTP-Referer", val);
            }
        }

        if let Some(ref name) = self.app_name {
            if let Ok(val) = HeaderValue::from_str(name) {
                headers.insert("X-Title", val);
            }
        }

        Ok(headers)
    }

    pub async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse> {
        let url = format!("{}/chat/completions", OPENROUTER_API_URL);

        debug!(model = %request.model, "OpenRouter chat request");

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
            return Err(anyhow!("OpenRouter API error ({}): {}", status, error_text));
        }

        Ok(response.json().await?)
    }

    pub async fn structured_output(&self, request: &ChatRequest) -> Result<String> {
        let url = format!("{}/chat/completions", OPENROUTER_API_URL);

        debug!(model = %request.model, "OpenRouter structured output request");

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
            return Err(anyhow!(
                "OpenRouter structured output error ({}): {}",
                status,
                error_text
            ));
        }

        let chat_response: ChatResponse = response.json().await?;

        chat_response
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .ok_or_else(|| anyhow!("No response from OpenRouter"))
    }

    pub async fn embed(&self, model: &str, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/embeddings", OPENROUTER_API_URL);

        let request = EmbeddingRequest {
            model: model.to_string(),
            input: serde_json::Value::String(text.to_string()),
        };

        let response = self
            .http
            .post(&url)
            .headers(self.headers()?)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow!(
                "OpenRouter embedding error ({}): {}",
                status,
                error_text
            ));
        }

        let embed_response: EmbeddingResponse = response.json().await?;

        embed_response
            .data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .ok_or_else(|| anyhow!("No embedding in response"))
    }

    pub async fn embed_batch(&self, model: &str, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let url = format!("{}/embeddings", OPENROUTER_API_URL);

        let request = EmbeddingRequest {
            model: model.to_string(),
            input: serde_json::Value::Array(
                texts
                    .iter()
                    .map(|t| serde_json::Value::String(t.clone()))
                    .collect(),
            ),
        };

        let response = self
            .http
            .post(&url)
            .headers(self.headers()?)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow!(
                "OpenRouter batch embedding error ({}): {}",
                status,
                error_text
            ));
        }

        let embed_response: EmbeddingResponse = response.json().await?;

        Ok(embed_response
            .data
            .into_iter()
            .map(|d| d.embedding)
            .collect())
    }
}
