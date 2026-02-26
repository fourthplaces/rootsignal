mod client;
pub mod prompt_builder;
pub(crate) mod types;

pub use prompt_builder::{ClaudeOutputBuilder, ClaudePromptBuilder};

use crate::openai::StructuredOutput;
use crate::tool::{DynTool, Tool, ToolWrapper};
use crate::traits::Agent;
use anyhow::{anyhow, Result};
use std::sync::Arc;

use client::ClaudeClient;
use types::*;

// =============================================================================
// Claude Agent
// =============================================================================

#[derive(Clone)]
pub struct Claude {
    api_key: String,
    pub(crate) model: String,
    pub(crate) tools: Vec<Arc<dyn DynTool>>,
    base_url: Option<String>,
}

impl Claude {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            tools: Vec::new(),
            base_url: None,
        }
    }

    pub fn from_env(model: impl Into<String>) -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| anyhow!("ANTHROPIC_API_KEY environment variable not set"))?;
        Ok(Self::new(api_key, model))
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    pub(crate) fn client(&self) -> ClaudeClient {
        let client = ClaudeClient::new(&self.api_key);
        if let Some(ref url) = self.base_url {
            client.with_base_url(url)
        } else {
            client
        }
    }

    // =========================================================================
    // Convenience methods
    // =========================================================================

    pub async fn extract<T: StructuredOutput>(
        &self,
        system_prompt: impl Into<String>,
        user_prompt: impl Into<String>,
    ) -> Result<T> {
        let schema = T::openai_schema();

        let tool_name = "structured_response";
        let mut request = ChatRequest::new(&self.model)
            .system(system_prompt)
            .message(WireMessage::user(user_prompt))
            .tool(ToolDefinitionWire {
                name: tool_name.to_string(),
                description: "Extract structured data from the input.".to_string(),
                input_schema: schema,
            });
        request.tool_choice = Some(serde_json::json!({
            "type": "tool",
            "name": tool_name,
        }));

        let response = self.client().chat(&request).await?;

        for block in &response.content {
            if let ContentBlock::ToolUse { input, .. } = block {
                return serde_json::from_value(input.clone())
                    .map_err(|e| anyhow!("Failed to deserialize response: {}", e));
            }
        }

        Err(anyhow!("No structured output in Claude response"))
    }

    pub async fn chat_completion(
        &self,
        system: impl Into<String>,
        user: impl Into<String>,
    ) -> Result<String> {
        let request = ChatRequest::new(&self.model)
            .system(system)
            .message(WireMessage::user(user))
            .max_tokens(4096)
            .temperature(0.0);

        let response = self.client().chat(&request).await?;

        response
            .text()
            .ok_or_else(|| anyhow!("No response from Claude"))
    }

    pub async fn complete(&self, prompt: &str) -> Result<String> {
        self.chat_completion("You are a helpful assistant.", prompt)
            .await
    }

    /// Send an image to Claude vision and return extracted text.
    pub async fn describe_image(
        &self,
        bytes: &[u8],
        mime_type: &str,
        prompt: &str,
    ) -> Result<String> {
        use base64::Engine;

        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        let source = ImageSource {
            source_type: "base64".to_string(),
            media_type: mime_type.to_string(),
            data: encoded,
        };

        let request = ChatRequest::new(&self.model)
            .message(WireMessage::user_with_image(source, prompt))
            .max_tokens(4096)
            .temperature(0.0);

        let response = self.client().chat(&request).await?;

        response
            .text()
            .ok_or_else(|| anyhow!("No text response from Claude vision"))
    }
}

// =============================================================================
// Agent Implementation
// =============================================================================

impl Agent for Claude {
    type PromptBuilder = ClaudePromptBuilder;

    fn tool<T: Tool + 'static>(mut self, tool: T) -> Self {
        self.tools.push(Arc::new(ToolWrapper(tool)));
        self
    }

    fn dyn_tool(mut self, tool: Arc<dyn DynTool>) -> Self {
        self.tools.push(tool);
        self
    }

    fn prompt(&self, input: impl Into<String>) -> ClaudePromptBuilder {
        ClaudePromptBuilder::new(self.clone(), input.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_new() {
        let ai = Claude::new("sk-ant-test", "claude-sonnet-4-20250514");
        assert_eq!(ai.model, "claude-sonnet-4-20250514");
        assert_eq!(ai.api_key, "sk-ant-test");
    }

    #[test]
    fn test_claude_with_base_url() {
        let ai = Claude::new("sk-ant-test", "claude-sonnet-4-20250514")
            .with_base_url("https://custom.api.com");
        assert_eq!(ai.base_url, Some("https://custom.api.com".to_string()));
    }
}
