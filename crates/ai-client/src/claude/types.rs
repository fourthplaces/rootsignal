use serde::{Deserialize, Serialize};

// =============================================================================
// Messages
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct WireMessage {
    pub role: Role,
    pub content: MessageContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

impl WireMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Text(content.into()),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: MessageContent::Text(content.into()),
        }
    }

    pub fn assistant_blocks(blocks: Vec<ContentBlock>) -> Self {
        Self {
            role: Role::Assistant,
            content: MessageContent::Blocks(blocks),
        }
    }

    pub fn tool_results(results: Vec<ContentBlock>) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Blocks(results),
        }
    }
}

// =============================================================================
// Tool Definition
// =============================================================================

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ToolDefinitionWire {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

// =============================================================================
// Chat Request
// =============================================================================

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ChatRequest {
    pub model: String,
    pub max_tokens: u32,
    pub messages: Vec<WireMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinitionWire>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
}

impl ChatRequest {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            max_tokens: 4096,
            messages: Vec::new(),
            system: None,
            temperature: None,
            tools: None,
            tool_choice: None,
        }
    }

    pub fn system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    pub fn message(mut self, message: WireMessage) -> Self {
        self.messages.push(message);
        self
    }

    pub fn messages(mut self, messages: impl IntoIterator<Item = WireMessage>) -> Self {
        self.messages.extend(messages);
        self
    }

    pub fn max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    pub fn temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
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
    pub content: Vec<ContentBlock>,
    pub stop_reason: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub usage: Option<Usage>,
}

impl ChatResponse {
    pub fn text(&self) -> Option<String> {
        self.content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            })
            .next()
    }

    pub fn tool_uses(&self) -> Vec<&ContentBlock> {
        self.content
            .iter()
            .filter(|block| matches!(block, ContentBlock::ToolUse { .. }))
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}
