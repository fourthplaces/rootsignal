use serde::{Deserialize, Serialize};

// =============================================================================
// Generate Content Request
// =============================================================================

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GenerateContentRequest {
    pub contents: Vec<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GenerationConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Content {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub parts: Vec<Part>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl Content {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Some("user".to_string()),
            parts: vec![Part { text: Some(text.into()) }],
        }
    }

    pub fn model(text: impl Into<String>) -> Self {
        Self {
            role: Some("model".to_string()),
            parts: vec![Part { text: Some(text.into()) }],
        }
    }

    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: None,
            parts: vec![Part { text: Some(text.into()) }],
        }
    }
}

// =============================================================================
// Generation Config
// =============================================================================

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_json_schema: Option<serde_json::Value>,
}

// =============================================================================
// Generate Content Response
// =============================================================================

#[derive(Debug, Deserialize)]
pub(crate) struct GenerateContentResponse {
    #[serde(default)]
    pub candidates: Vec<Candidate>,
    #[serde(default)]
    pub usage_metadata: Option<UsageMetadata>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Candidate {
    pub content: Option<Content>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageMetadata {
    #[allow(dead_code)]
    pub prompt_token_count: Option<u32>,
    #[allow(dead_code)]
    pub candidates_token_count: Option<u32>,
    #[allow(dead_code)]
    pub total_token_count: Option<u32>,
}
