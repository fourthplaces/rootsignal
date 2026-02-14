mod client;
pub mod prompt_builder;
pub(crate) mod schema;
pub(crate) mod types;

pub use prompt_builder::{OpenAiOutputBuilder, OpenAiPromptBuilder};
pub use schema::StructuredOutput;

use crate::tool::{DynTool, Tool, ToolWrapper};
use crate::traits::{Agent, EmbedAgent};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::sync::Arc;

use client::OpenAiClient;

// =============================================================================
// OpenAi Agent
// =============================================================================

#[derive(Clone)]
pub struct OpenAi {
    api_key: String,
    pub(crate) model: String,
    embedding_model: String,
    pub(crate) tools: Vec<Arc<dyn DynTool>>,
    base_url: Option<String>,
}

impl OpenAi {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            embedding_model: "text-embedding-3-small".to_string(),
            tools: Vec::new(),
            base_url: None,
        }
    }

    pub fn from_env(model: impl Into<String>) -> Result<Self> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| anyhow!("OPENAI_API_KEY environment variable not set"))?;
        Ok(Self::new(api_key, model))
    }

    pub fn with_embedding_model(mut self, model: impl Into<String>) -> Self {
        self.embedding_model = model.into();
        self
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Get the model name.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Get the API key.
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    pub(crate) fn client(&self) -> OpenAiClient {
        let client = OpenAiClient::new(&self.api_key);
        if let Some(ref url) = self.base_url {
            client.with_base_url(url)
        } else {
            client
        }
    }

    // =========================================================================
    // Convenience methods (backwards-compatible with mntogether's OpenAIClient)
    // =========================================================================

    /// Type-safe structured output extraction (convenience method).
    pub async fn extract<T: StructuredOutput>(
        &self,
        model: &str,
        system_prompt: impl Into<String>,
        user_prompt: impl Into<String>,
    ) -> Result<T> {
        let schema = T::openai_schema();

        let request = types::StructuredRequest {
            model: model.to_string(),
            messages: vec![
                types::WireMessage::system(system_prompt),
                types::WireMessage::user(user_prompt),
            ],
            temperature: if model.starts_with("gpt-5") {
                None
            } else {
                Some(0.0)
            },
            response_format: types::ResponseFormat {
                format_type: "json_schema".to_string(),
                json_schema: types::JsonSchemaFormat {
                    name: "structured_response".to_string(),
                    strict: true,
                    schema,
                },
            },
        };

        let json_str = self.client().structured_output(&request).await?;

        serde_json::from_str(&json_str)
            .map_err(|e| anyhow!("Failed to deserialize response: {}", e))
    }

    /// Simple chat completion (convenience method).
    pub async fn chat_completion(
        &self,
        system: impl Into<String>,
        user: impl Into<String>,
    ) -> Result<String> {
        let mut request = types::ChatRequest::new(&self.model)
            .message(types::WireMessage::system(system))
            .message(types::WireMessage::user(user));

        if types::uses_max_completion_tokens(&self.model) {
            request = request.max_completion_tokens(4096);
        } else {
            request = request.max_tokens(4096).temperature(0.0);
        }

        let response = self.client().chat(&request).await?;

        response
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .ok_or_else(|| anyhow!("No response from OpenAI"))
    }

    /// Simple text completion (convenience method).
    pub async fn complete(&self, prompt: &str) -> Result<String> {
        self.chat_completion("You are a helpful assistant.", prompt)
            .await
    }

    /// Create embedding for text.
    pub async fn create_embedding(&self, text: &str, model: &str) -> Result<Vec<f32>> {
        self.client().embed(model, text).await
    }

    /// Create embeddings for multiple texts.
    pub async fn create_embeddings_batch(
        &self,
        texts: &[&str],
        model: &str,
    ) -> Result<Vec<Vec<f32>>> {
        let string_texts: Vec<String> = texts.iter().map(|t| t.to_string()).collect();
        self.client().embed_batch(model, &string_texts).await
    }

    /// Generate structured output with raw schema (convenience method).
    pub async fn structured_output(
        &self,
        system: &str,
        user: &str,
        schema: serde_json::Value,
    ) -> Result<String> {
        let request = types::StructuredRequest {
            model: self.model.clone(),
            messages: vec![
                types::WireMessage::system(system),
                types::WireMessage::user(user),
            ],
            temperature: if self.model.starts_with("gpt-5") {
                None
            } else {
                Some(0.0)
            },
            response_format: types::ResponseFormat {
                format_type: "json_schema".to_string(),
                json_schema: types::JsonSchemaFormat {
                    name: "structured_response".to_string(),
                    strict: true,
                    schema,
                },
            },
        };

        self.client().structured_output(&request).await
    }

    /// Function calling (tool use) with raw JSON messages/tools.
    pub async fn function_calling(
        &self,
        messages: &[serde_json::Value],
        tools: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = format!(
            "{}/chat/completions",
            self.base_url
                .as_deref()
                .unwrap_or("https://api.openai.com/v1")
        );

        let request_body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "tools": tools,
            "tool_choice": "auto",
        });

        let response = reqwest::Client::new()
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("OpenAI tools API error: {}", error_text));
        }

        let response_json: serde_json::Value = response.json().await?;

        Ok(response_json["choices"][0]["message"].clone())
    }
}

// =============================================================================
// Agent Implementation
// =============================================================================

impl Agent for OpenAi {
    type PromptBuilder = OpenAiPromptBuilder;

    fn tool<T: Tool + 'static>(mut self, tool: T) -> Self {
        self.tools.push(Arc::new(ToolWrapper(tool)));
        self
    }

    fn dyn_tool(mut self, tool: Arc<dyn DynTool>) -> Self {
        self.tools.push(tool);
        self
    }

    fn prompt(&self, input: impl Into<String>) -> OpenAiPromptBuilder {
        OpenAiPromptBuilder::new(self.clone(), input.into())
    }
}

// =============================================================================
// EmbedAgent Implementation
// =============================================================================

#[async_trait]
impl EmbedAgent for OpenAi {
    async fn embed(&self, text: impl Into<String> + Send) -> Result<Vec<f32>> {
        self.client()
            .embed(&self.embedding_model, &text.into())
            .await
    }

    async fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        self.client()
            .embed_batch(&self.embedding_model, &texts)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_new() {
        let ai = OpenAi::new("sk-test", "gpt-4o");
        assert_eq!(ai.model, "gpt-4o");
        assert_eq!(ai.api_key, "sk-test");
        assert_eq!(ai.embedding_model, "text-embedding-3-small");
    }

    #[test]
    fn test_openai_with_embedding_model() {
        let ai = OpenAi::new("sk-test", "gpt-4o").with_embedding_model("text-embedding-3-large");
        assert_eq!(ai.embedding_model, "text-embedding-3-large");
    }

    #[test]
    fn test_openai_with_base_url() {
        let ai = OpenAi::new("sk-test", "gpt-4o").with_base_url("https://custom.api.com");
        assert_eq!(ai.base_url, Some("https://custom.api.com".to_string()));
    }
}
