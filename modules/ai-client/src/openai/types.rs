use serde::{Deserialize, Serialize};

// =============================================================================
// Chat Completion
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct WireMessage {
    pub role: Role,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallWire>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl WireMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: Some(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: Some(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: Some(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: Some(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ToolCallWire {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCallWire,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FunctionCallWire {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ToolDefinitionWire {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinitionWire,
}

impl ToolDefinitionWire {
    pub fn function(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            tool_type: "function".to_string(),
            function: FunctionDefinitionWire {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FunctionDefinitionWire {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

// =============================================================================
// Chat Request
// =============================================================================

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ChatRequest {
    pub model: String,
    pub messages: Vec<WireMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinitionWire>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
}

impl ChatRequest {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            messages: Vec::new(),
            temperature: None,
            max_tokens: None,
            max_completion_tokens: None,
            tools: None,
            tool_choice: None,
        }
    }

    pub fn message(mut self, message: WireMessage) -> Self {
        self.messages.push(message);
        self
    }

    pub fn messages(mut self, messages: impl IntoIterator<Item = WireMessage>) -> Self {
        self.messages.extend(messages);
        self
    }

    pub fn temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn max_completion_tokens(mut self, max_completion_tokens: u32) -> Self {
        self.max_completion_tokens = Some(max_completion_tokens);
        self
    }

    pub fn tool(mut self, tool: ToolDefinitionWire) -> Self {
        self.tools.get_or_insert_with(Vec::new).push(tool);
        self
    }
}

// =============================================================================
// Chat Response
// =============================================================================

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ChatResponse {
    pub choices: Vec<Choice>,
    #[serde(default)]
    #[allow(dead_code)]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Choice {
    pub message: WireMessage,
    #[allow(dead_code)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

// =============================================================================
// Structured Output
// =============================================================================

#[derive(Debug, Serialize)]
pub(crate) struct StructuredRequest {
    pub model: String,
    pub messages: Vec<WireMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    pub response_format: ResponseFormat,
}

#[derive(Debug, Serialize)]
pub(crate) struct ResponseFormat {
    #[serde(rename = "type")]
    pub format_type: String,
    pub json_schema: JsonSchemaFormat,
}

#[derive(Debug, Serialize)]
pub(crate) struct JsonSchemaFormat {
    pub name: String,
    pub strict: bool,
    pub schema: serde_json::Value,
}

// =============================================================================
// Embeddings
// =============================================================================

#[derive(Debug, Serialize)]
pub(crate) struct EmbeddingRequest {
    pub model: String,
    pub input: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub(crate) struct EmbeddingResponse {
    pub data: Vec<EmbeddingData>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct EmbeddingData {
    pub embedding: Vec<f32>,
}

// =============================================================================
// Utilities
// =============================================================================

/// Check if a model requires max_completion_tokens instead of max_tokens.
pub(crate) fn uses_max_completion_tokens(model: &str) -> bool {
    model.starts_with("o1")
        || model.starts_with("o3")
        || model.starts_with("gpt-5")
        || model.contains("-o1")
        || model.contains("-o3")
}
