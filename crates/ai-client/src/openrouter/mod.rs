mod client;
pub mod prompt_builder;
pub(crate) mod types;

pub use prompt_builder::{OpenRouterOutputBuilder, OpenRouterPromptBuilder};

use crate::tool::{DynTool, Tool, ToolWrapper};
use crate::traits::{Agent, EmbedAgent};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::sync::Arc;

use client::OpenRouterClient;

// =============================================================================
// OpenRouter Agent
// =============================================================================

#[derive(Clone)]
pub struct OpenRouter {
    api_key: String,
    pub(crate) model: String,
    app_name: Option<String>,
    site_url: Option<String>,
    pub(crate) tools: Vec<Arc<dyn DynTool>>,
}

impl OpenRouter {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            app_name: None,
            site_url: None,
            tools: Vec::new(),
        }
    }

    pub fn from_env(model: impl Into<String>) -> Result<Self> {
        let api_key = std::env::var("OPENROUTER_API_KEY")
            .map_err(|_| anyhow!("OPENROUTER_API_KEY environment variable not set"))?;
        Ok(Self::new(api_key, model))
    }

    pub fn with_app_name(mut self, name: impl Into<String>) -> Self {
        self.app_name = Some(name.into());
        self
    }

    pub fn with_site_url(mut self, url: impl Into<String>) -> Self {
        self.site_url = Some(url.into());
        self
    }

    /// Get the model name.
    pub fn model(&self) -> &str {
        &self.model
    }

    pub(crate) fn client(&self) -> OpenRouterClient {
        let mut client = OpenRouterClient::new(&self.api_key);
        if let Some(ref name) = self.app_name {
            client = client.with_app_name(name);
        }
        if let Some(ref url) = self.site_url {
            client = client.with_site_url(url);
        }
        client
    }

    // =========================================================================
    // Convenience methods (matching OpenAi's API for drop-in switching)
    // =========================================================================

    /// Type-safe structured output extraction.
    pub async fn extract<T: crate::openai::StructuredOutput>(
        &self,
        model: &str,
        system_prompt: impl Into<String>,
        user_prompt: impl Into<String>,
    ) -> Result<T> {
        let schema = T::openai_schema();

        let mut request = types::ChatRequest::new(model).messages(vec![
            types::WireMessage::system(system_prompt),
            types::WireMessage::user(user_prompt),
        ]);
        request.temperature = Some(0.0);
        request.response_format = Some(serde_json::json!({
            "type": "json_schema",
            "json_schema": {
                "name": "structured_response",
                "strict": true,
                "schema": schema,
            }
        }));

        let json_str = self.client().structured_output(&request).await?;

        serde_json::from_str(&json_str)
            .map_err(|e| anyhow!("Failed to deserialize response: {}", e))
    }

    /// Simple chat completion.
    pub async fn chat_completion(
        &self,
        system: impl Into<String>,
        user: impl Into<String>,
    ) -> Result<String> {
        let request = types::ChatRequest::new(&self.model)
            .message(types::WireMessage::system(system))
            .message(types::WireMessage::user(user))
            .max_tokens(4096)
            .temperature(0.0);

        let response = self.client().chat(&request).await?;

        response
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .ok_or_else(|| anyhow!("No response from OpenRouter"))
    }

    /// Simple text completion.
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
}

// =============================================================================
// Agent Implementation
// =============================================================================

impl Agent for OpenRouter {
    type PromptBuilder = OpenRouterPromptBuilder;

    fn tool<T: Tool + 'static>(mut self, tool: T) -> Self {
        self.tools.push(Arc::new(ToolWrapper(tool)));
        self
    }

    fn dyn_tool(mut self, tool: Arc<dyn DynTool>) -> Self {
        self.tools.push(tool);
        self
    }

    fn prompt(&self, input: impl Into<String>) -> OpenRouterPromptBuilder {
        OpenRouterPromptBuilder::new(self.clone(), input.into())
    }
}

// =============================================================================
// EmbedAgent Implementation
// =============================================================================

#[async_trait]
impl EmbedAgent for OpenRouter {
    async fn embed(&self, text: impl Into<String> + Send) -> Result<Vec<f32>> {
        self.client().embed(&self.model, &text.into()).await
    }

    async fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        self.client().embed_batch(&self.model, &texts).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openrouter_new() {
        let or = OpenRouter::new("or-test", "deepseek/deepseek-v3.2");
        assert_eq!(or.model, "deepseek/deepseek-v3.2");
        assert_eq!(or.api_key, "or-test");
    }

    #[test]
    fn test_openrouter_with_app_name() {
        let or = OpenRouter::new("or-test", "deepseek/deepseek-v3.2")
            .with_app_name("MN Together")
            .with_site_url("https://mntogether.org");
        assert_eq!(or.app_name, Some("MN Together".to_string()));
        assert_eq!(or.site_url, Some("https://mntogether.org".to_string()));
    }
}
