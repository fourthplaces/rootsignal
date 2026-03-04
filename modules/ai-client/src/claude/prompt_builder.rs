use std::marker::PhantomData;
use std::pin::Pin;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures::Stream;
use futures::StreamExt;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use tracing::debug;

use crate::openai::StructuredOutput;
use crate::traits::{Message, MessageRole, OutputBuilder, PromptBuilder};

use super::streaming::ClaudeStreamEvent;
use super::types::*;
use super::Claude;

pub struct ClaudePromptBuilder {
    agent: Claude,
    input: String,
    preamble: Option<String>,
    temperature: Option<f32>,
    max_turns: usize,
    messages: Vec<Message>,
}

impl ClaudePromptBuilder {
    pub(crate) fn new(agent: Claude, input: String) -> Self {
        Self {
            agent,
            input,
            preamble: None,
            temperature: None,
            max_turns: 1,
            messages: Vec::new(),
        }
    }

    pub fn output<T: DeserializeOwned + JsonSchema + Send + 'static>(
        self,
    ) -> ClaudeOutputBuilder<T> {
        ClaudeOutputBuilder {
            builder: self,
            _phantom: PhantomData,
        }
    }

    /// Internal send implementation (shared by trait impl and direct callers).
    async fn send_impl(self) -> Result<String> {
        let client = self.agent.client();

        let mut request = ChatRequest::new(&self.agent.model);

        if let Some(temp) = self.temperature {
            request = request.temperature(temp);
        }

        if let Some(ref preamble) = self.preamble {
            request = request.system(preamble);
        }

        let mut messages = Vec::new();

        for msg in &self.messages {
            match msg.role {
                MessageRole::System => {
                    // Claude uses top-level system field, merge into it
                    let existing = request.system.take().unwrap_or_default();
                    let combined = if existing.is_empty() {
                        msg.content.clone()
                    } else {
                        format!("{}\n\n{}", existing, msg.content)
                    };
                    request = request.system(combined);
                }
                MessageRole::User => messages.push(WireMessage::user(&msg.content)),
                MessageRole::Assistant => messages.push(WireMessage::assistant(&msg.content)),
            }
        }

        if !self.input.is_empty() {
            messages.push(WireMessage::user(&self.input));
        }

        request = request.messages(messages);

        // Add tools
        for tool in &self.agent.tools {
            let def = tool.definition().await;
            request = request.tool(ToolDefinitionWire {
                name: def.name,
                description: def.description,
                input_schema: def.parameters,
            });
        }

        if request.tools.is_some() {
            request.tool_choice = Some(serde_json::json!({"type": "auto"}));
        }

        // Multi-turn tool loop
        let mut turn = 0;
        loop {
            turn += 1;
            if turn > self.max_turns {
                return Err(anyhow!("Max turns ({}) exceeded", self.max_turns));
            }

            let response = client.chat(&request).await?;

            let tool_uses = response.tool_uses();
            if !tool_uses.is_empty() && response.stop_reason.as_deref() == Some("tool_use") {
                // Add assistant message with all content blocks
                request
                    .messages
                    .push(WireMessage::assistant_blocks(response.content.clone()));

                // Execute tools and collect results
                let mut results = Vec::new();
                for block in &tool_uses {
                    if let ContentBlock::ToolUse { id, name, input } = block {
                        let tool = self
                            .agent
                            .tools
                            .iter()
                            .find(|t| t.name() == name.as_str())
                            .ok_or_else(|| anyhow!("Tool not found: {}", name))?;

                        debug!(tool = %name, "Executing tool call");

                        let result = match tool.call_json(input.clone()).await {
                            Ok(v) => serde_json::to_string(&v)?,
                            Err(e) => format!("Error: {}", e),
                        };

                        // Cap tool results to avoid blowing token limits in multi-turn loops
                        const MAX_TOOL_RESULT_CHARS: usize = 30_000;
                        let result = if result.len() > MAX_TOOL_RESULT_CHARS {
                            let boundary = result[..MAX_TOOL_RESULT_CHARS]
                                .rfind('\n')
                                .unwrap_or(MAX_TOOL_RESULT_CHARS);
                            format!("{}…\n(result truncated)", &result[..boundary])
                        } else {
                            result
                        };

                        results.push(ContentBlock::ToolResult {
                            tool_use_id: id.clone(),
                            content: result,
                        });
                    }
                }

                request.messages.push(WireMessage::tool_results(results));
                continue;
            }

            return Ok(response.text().unwrap_or_default());
        }
    }

    /// Internal stream implementation.
    async fn stream_impl(self) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
        let client = self.agent.client();

        let mut request = ChatRequest::new(&self.agent.model).streaming();

        if let Some(temp) = self.temperature {
            request = request.temperature(temp);
        }

        if let Some(ref preamble) = self.preamble {
            request = request.system(preamble);
        }

        let mut messages = Vec::new();

        for msg in &self.messages {
            match msg.role {
                MessageRole::System => {
                    let existing = request.system.take().unwrap_or_default();
                    let combined = if existing.is_empty() {
                        msg.content.clone()
                    } else {
                        format!("{}\n\n{}", existing, msg.content)
                    };
                    request = request.system(combined);
                }
                MessageRole::User => messages.push(WireMessage::user(&msg.content)),
                MessageRole::Assistant => messages.push(WireMessage::assistant(&msg.content)),
            }
        }

        if !self.input.is_empty() {
            messages.push(WireMessage::user(&self.input));
        }

        request = request.messages(messages);

        let stream = client.chat_stream(&request).await?;

        Ok(Box::pin(stream.filter_map(|event| async move {
            match event {
                Ok(ClaudeStreamEvent::Delta(text)) => Some(Ok(text)),
                Ok(ClaudeStreamEvent::Done) => None,
                Err(e) => Some(Err(e)),
            }
        })))
    }
}

// =============================================================================
// Object-safe PromptBuilder implementation
// =============================================================================

#[async_trait]
impl PromptBuilder for ClaudePromptBuilder {
    fn preamble(mut self: Box<Self>, preamble: &str) -> Box<dyn PromptBuilder> {
        self.preamble = Some(preamble.to_string());
        self
    }

    fn temperature(mut self: Box<Self>, temperature: f32) -> Box<dyn PromptBuilder> {
        self.temperature = Some(temperature);
        self
    }

    fn multi_turn(mut self: Box<Self>, max_turns: usize) -> Box<dyn PromptBuilder> {
        self.max_turns = max_turns;
        self
    }

    fn messages(mut self: Box<Self>, messages: Vec<Message>) -> Box<dyn PromptBuilder> {
        self.messages = messages;
        self
    }

    async fn send(self: Box<Self>) -> Result<String> {
        (*self).send_impl().await
    }

    async fn stream(self: Box<Self>) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
        (*self).stream_impl().await
    }
}

// =============================================================================
// Structured Output Builder
// =============================================================================

pub struct ClaudeOutputBuilder<T> {
    builder: ClaudePromptBuilder,
    _phantom: PhantomData<T>,
}

#[async_trait]
impl<T: DeserializeOwned + JsonSchema + Send + 'static> OutputBuilder<T>
    for ClaudeOutputBuilder<T>
{
    async fn send(self) -> Result<T> {
        let schema = T::openai_schema();

        debug!(
            type_name = T::type_name(),
            "Claude structured output extraction"
        );

        let client = self.builder.agent.client();

        let mut request = ChatRequest::new(&self.builder.agent.model).temperature(0.0); // Structured extraction must be deterministic

        if let Some(ref preamble) = self.builder.preamble {
            request = request.system(preamble);
        }

        let mut messages = Vec::new();

        for msg in &self.builder.messages {
            match msg.role {
                MessageRole::System => {
                    let existing = request.system.take().unwrap_or_default();
                    let combined = if existing.is_empty() {
                        msg.content.clone()
                    } else {
                        format!("{}\n\n{}", existing, msg.content)
                    };
                    request = request.system(combined);
                }
                MessageRole::User => messages.push(WireMessage::user(&msg.content)),
                MessageRole::Assistant => messages.push(WireMessage::assistant(&msg.content)),
            }
        }

        if !self.builder.input.is_empty() {
            messages.push(WireMessage::user(&self.builder.input));
        }

        request = request.messages(messages);

        // Use forced tool use for structured output
        let tool_name = "structured_response";
        request = request.tool(ToolDefinitionWire {
            name: tool_name.to_string(),
            description: "Extract structured data from the input.".to_string(),
            input_schema: schema,
        });
        request.tool_choice = Some(serde_json::json!({
            "type": "tool",
            "name": tool_name,
        }));

        let response = client.chat(&request).await?;

        // Extract the tool use input as our structured output
        for block in &response.content {
            if let ContentBlock::ToolUse { input, .. } = block {
                return serde_json::from_value(input.clone())
                    .map_err(|e| anyhow!("Failed to deserialize response: {}", e));
            }
        }

        Err(anyhow!("No structured output in Claude response"))
    }
}
