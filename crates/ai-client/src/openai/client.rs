use anyhow::{anyhow, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use tracing::debug;

use super::types::*;

const OPENAI_API_URL: &str = "https://api.openai.com/v1";

pub(crate) struct OpenAiClient {
    api_key: String,
    http: reqwest::Client,
    base_url: String,
}

impl OpenAiClient {
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            http: reqwest::Client::new(),
            base_url: OPENAI_API_URL.to_string(),
        }
    }

    pub fn with_base_url(mut self, url: &str) -> Self {
        self.base_url = url.to_string();
        self
    }

    fn headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.api_key))?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Ok(headers)
    }

    pub async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse> {
        let url = format!("{}/chat/completions", self.base_url);

        debug!(model = %request.model, "OpenAI chat request");

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
            return Err(anyhow!("OpenAI API error ({}): {}", status, error_text));
        }

        Ok(response.json().await?)
    }

    pub async fn structured_output(&self, request: &StructuredRequest) -> Result<String> {
        let url = format!("{}/chat/completions", self.base_url);

        debug!(model = %request.model, "OpenAI structured output request");

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
                "OpenAI structured output error ({}): {}",
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
            .ok_or_else(|| anyhow!("No response from OpenAI"))
    }

    pub async fn embed(&self, model: &str, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/embeddings", self.base_url);

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
                "OpenAI embedding error ({}): {}",
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
        let url = format!("{}/embeddings", self.base_url);

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
                "OpenAI batch embedding error ({}): {}",
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
