use std::marker::PhantomData;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use tracing::debug;

use crate::traits::{Message, MessageRole, OutputBuilder, PromptBuilder};

use super::schema::StructuredOutput;
use super::types::*;
use super::OpenAi;

pub struct OpenAiPromptBuilder {
    agent: OpenAi,
    input: String,
    preamble: Option<String>,
    temperature: Option<f32>,
    max_turns: usize,
    messages: Vec<Message>,
}

impl OpenAiPromptBuilder {
    pub(crate) fn new(agent: OpenAi, input: String) -> Self {
        Self {
            agent,
            input,
            preamble: None,
            temperature: None,
            max_turns: 1,
            messages: Vec::new(),
        }
    }

    /// Create a structured output builder for extracting typed data.
    pub fn output<T: DeserializeOwned + JsonSchema + Send + 'static>(
        self,
    ) -> OpenAiOutputBuilder<T> {
        OpenAiOutputBuilder {
            builder: self,
            _phantom: PhantomData,
        }
    }
}

#[async_trait]
impl PromptBuilder for OpenAiPromptBuilder {
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

        let mut messages = Vec::new();

        if let Some(ref preamble) = self.preamble {
            messages.push(WireMessage::system(preamble));
        }

        for msg in &self.messages {
            match msg.role {
                MessageRole::System => messages.push(WireMessage::system(&msg.content)),
                MessageRole::User => messages.push(WireMessage::user(&msg.content)),
                MessageRole::Assistant => messages.push(WireMessage::assistant(&msg.content)),
            }
        }

        if !self.input.is_empty() {
            messages.push(WireMessage::user(&self.input));
        }

        let mut request = ChatRequest::new(&self.agent.model).messages(messages);

        if let Some(temp) = self.temperature {
            request = request.temperature(temp);
        }

        // Add tools
        for tool in &self.agent.tools {
            let def = tool.definition().await;
            request = request.tool(ToolDefinitionWire::function(
                &def.name,
                &def.description,
                def.parameters,
            ));
        }

        if request.tools.is_some() {
            request.tool_choice = Some(serde_json::json!("auto"));
        }

        // Multi-turn tool loop
        let mut turn = 0;
        loop {
            turn += 1;
            if turn > self.max_turns {
                return Err(anyhow!("Max turns ({}) exceeded", self.max_turns));
            }

            let response = client.chat(&request).await?;
            let choice = response
                .choices
                .first()
                .ok_or_else(|| anyhow!("No choices in response"))?;

            if let Some(ref tool_calls) = choice.message.tool_calls {
                if !tool_calls.is_empty() {
                    // Add assistant message with tool calls
                    request.messages.push(WireMessage {
                        role: Role::Assistant,
                        content: choice.message.content.clone(),
                        name: None,
                        tool_calls: Some(tool_calls.clone()),
                        tool_call_id: None,
                    });

                    // Execute tools and add results
                    for tc in tool_calls {
                        let tool = self
                            .agent
                            .tools
                            .iter()
                            .find(|t| t.name() == tc.function.name)
                            .ok_or_else(|| anyhow!("Tool not found: {}", tc.function.name))?;

                        let args: serde_json::Value =
                            serde_json::from_str(&tc.function.arguments).unwrap_or_default();

                        debug!(tool = %tc.function.name, "Executing tool call");

                        let result = match tool.call_json(args).await {
                            Ok(v) => serde_json::to_string(&v)?,
                            Err(e) => format!("Error: {}", e),
                        };

                        request.messages.push(WireMessage::tool(&tc.id, &result));
                    }

                    continue;
                }
            }

            return Ok(choice.message.content.clone().unwrap_or_default());
        }
    }
}

// =============================================================================
// Structured Output Builder
// =============================================================================

pub struct OpenAiOutputBuilder<T> {
    builder: OpenAiPromptBuilder,
    _phantom: PhantomData<T>,
}

#[async_trait]
impl<T: DeserializeOwned + JsonSchema + Send + 'static> OutputBuilder<T>
    for OpenAiOutputBuilder<T>
{
    async fn send(self) -> Result<T> {
        let schema = T::openai_schema();

        debug!(
            type_name = T::type_name(),
            "OpenAI structured output extraction"
        );

        let client = self.builder.agent.client();

        let mut messages = Vec::new();

        if let Some(ref preamble) = self.builder.preamble {
            messages.push(WireMessage::system(preamble));
        }

        for msg in &self.builder.messages {
            match msg.role {
                MessageRole::System => messages.push(WireMessage::system(&msg.content)),
                MessageRole::User => messages.push(WireMessage::user(&msg.content)),
                MessageRole::Assistant => messages.push(WireMessage::assistant(&msg.content)),
            }
        }

        if !self.builder.input.is_empty() {
            messages.push(WireMessage::user(&self.builder.input));
        }

        let model = &self.builder.agent.model;
        let temperature = if model.starts_with("gpt-5") {
            None
        } else {
            Some(0.0)
        };

        let request = StructuredRequest {
            model: model.clone(),
            messages,
            temperature,
            response_format: ResponseFormat {
                format_type: "json_schema".to_string(),
                json_schema: JsonSchemaFormat {
                    name: "structured_response".to_string(),
                    strict: true,
                    schema,
                },
            },
        };

        let json_str = client.structured_output(&request).await?;

        serde_json::from_str(&json_str)
            .map_err(|e| anyhow!("Failed to deserialize response: {}", e))
    }
}
