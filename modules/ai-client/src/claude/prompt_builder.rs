use std::marker::PhantomData;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use tracing::debug;

use crate::openai::StructuredOutput;
use crate::traits::{Message, MessageRole, OutputBuilder, PromptBuilder};

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
}

#[async_trait]
impl PromptBuilder for ClaudePromptBuilder {
    fn preamble(mut self, preamble: impl Into<String>) -> Self {
        self.preamble = Some(preamble.into());
        self
    }

    fn temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    fn multi_turn(mut self, max_turns: usize) -> Self {
        self.max_turns = max_turns;
        self
    }

    fn messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }

    async fn send(self) -> Result<String> {
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

        let mut request = ChatRequest::new(&self.builder.agent.model)
            .temperature(0.0); // Structured extraction must be deterministic

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
