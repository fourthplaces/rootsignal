use crate::tool::{DynTool, Tool};
use anyhow::Result;
use async_trait::async_trait;
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
// Agent Trait
// =============================================================================

pub trait Agent: Clone + Send + Sync {
    type PromptBuilder: PromptBuilder;

    fn tool<T: Tool + 'static>(self, tool: T) -> Self;
    fn dyn_tool(self, tool: Arc<dyn DynTool>) -> Self;
    fn prompt(&self, input: impl Into<String>) -> Self::PromptBuilder;
}

// =============================================================================
// PromptBuilder Trait
// =============================================================================

#[async_trait]
pub trait PromptBuilder: Send + Sized {
    fn preamble(self, preamble: impl Into<String>) -> Self;
    fn temperature(self, temperature: f32) -> Self;
    fn multi_turn(self, max_turns: usize) -> Self;
    fn messages(self, messages: Vec<Message>) -> Self;
    async fn send(self) -> Result<String>;
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
