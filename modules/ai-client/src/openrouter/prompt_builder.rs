use std::marker::PhantomData;
use std::pin::Pin;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures::Stream;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use tracing::debug;

use crate::openai::schema::StructuredOutput;
use crate::traits::{Message, MessageRole, OutputBuilder, PromptBuilder};

use super::types::*;
use super::OpenRouter;

pub struct OpenRouterPromptBuilder {
    agent: OpenRouter,
    input: String,
    preamble: Option<String>,
    temperature: Option<f32>,
    max_turns: usize,
    messages: Vec<Message>,
}

impl OpenRouterPromptBuilder {
    pub(crate) fn new(agent: OpenRouter, input: String) -> Self {
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
    ) -> OpenRouterOutputBuilder<T> {
        OpenRouterOutputBuilder {
            builder: self,
            _phantom: PhantomData,
        }
    }

    /// Internal send implementation (shared by trait impl and direct callers).
    async fn send_impl(self) -> Result<String> {
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
                    request.messages.push(WireMessage {
                        role: Role::Assistant,
                        content: choice.message.content.clone(),
                        name: None,
                        tool_calls: Some(tool_calls.clone()),
                        tool_call_id: None,
                    });

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
// Object-safe PromptBuilder implementation
// =============================================================================

#[async_trait]
impl PromptBuilder for OpenRouterPromptBuilder {
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
        unimplemented!("streaming not yet supported for OpenRouter provider")
    }
}

// =============================================================================
// Structured Output Builder
// =============================================================================

pub struct OpenRouterOutputBuilder<T> {
    builder: OpenRouterPromptBuilder,
    _phantom: PhantomData<T>,
}

#[async_trait]
impl<T: DeserializeOwned + JsonSchema + Send + 'static> OutputBuilder<T>
    for OpenRouterOutputBuilder<T>
{
    async fn send(self) -> Result<T> {
        let schema = T::openai_schema();

        debug!(
            type_name = T::type_name(),
            "OpenRouter structured output extraction"
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

        // OpenRouter supports json_schema response_format for compatible models
        let mut request = ChatRequest::new(&self.builder.agent.model).messages(messages);
        request.temperature = Some(0.0);
        request.response_format = Some(serde_json::json!({
            "type": "json_schema",
            "json_schema": {
                "name": "structured_response",
                "strict": true,
                "schema": schema,
            }
        }));

        let json_str = client.structured_output(&request).await?;

        serde_json::from_str(&json_str)
            .map_err(|e| anyhow!("Failed to deserialize response: {}", e))
    }
}
