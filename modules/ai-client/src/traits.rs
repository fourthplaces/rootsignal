use crate::tool::DynTool;
use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::pin::Pin;
use std::sync::Arc;

// =============================================================================
// Message Types
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
        }
    }
}

// =============================================================================
// Agent Trait (object-safe)
// =============================================================================

#[async_trait]
pub trait Agent: Send + Sync {
    /// Extract structured JSON using a forced-tool-use schema.
    async fn extract_json(&self, system: &str, user: &str, schema: Value) -> Result<Value>;

    /// Simple chat completion.
    async fn chat(&self, system: &str, user: &str) -> Result<String>;

    /// Clone self with additional tools. Returns a boxed trait object.
    fn with_tools(&self, tools: Vec<Arc<dyn DynTool>>) -> Box<dyn Agent>;

    /// Start building a prompt. Returns a boxed PromptBuilder.
    fn prompt(&self, input: &str) -> Box<dyn PromptBuilder>;
}

// =============================================================================
// PromptBuilder Trait (object-safe, self: Box<Self>)
// =============================================================================

#[async_trait]
pub trait PromptBuilder: Send {
    fn preamble(self: Box<Self>, preamble: &str) -> Box<dyn PromptBuilder>;
    fn temperature(self: Box<Self>, temperature: f32) -> Box<dyn PromptBuilder>;
    fn multi_turn(self: Box<Self>, max_turns: usize) -> Box<dyn PromptBuilder>;
    fn messages(self: Box<Self>, messages: Vec<Message>) -> Box<dyn PromptBuilder>;
    async fn send(self: Box<Self>) -> Result<String>;
    async fn stream(self: Box<Self>) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>>;
}

// =============================================================================
// OutputBuilder Trait
// =============================================================================

#[async_trait]
pub trait OutputBuilder<T>: Send {
    async fn send(self) -> Result<T>;
}

// =============================================================================
// EmbedAgent Trait
// =============================================================================

#[async_trait]
pub trait EmbedAgent: Send + Sync {
    async fn embed(&self, text: impl Into<String> + Send) -> Result<Vec<f32>>;
    async fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>>;
}

// =============================================================================
// Helper: typed extraction via Agent trait
// =============================================================================

/// Extract a typed value from an LLM using the Agent trait.
/// Derives the JSON schema from `T` at runtime, calls `extract_json`, deserializes.
pub async fn ai_extract<T: DeserializeOwned + schemars::JsonSchema>(
    ai: &dyn Agent,
    system: &str,
    user: &str,
) -> Result<T> {
    let schema = serde_json::to_value(schemars::schema_for!(T))?;
    let json = ai.extract_json(system, user, schema).await?;
    serde_json::from_value(json).map_err(Into::into)
}
