mod client;
pub(crate) mod types;

use crate::tool::DynTool;
use crate::traits::{Agent, PromptBuilder};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

use client::GeminiClient;
use types::*;

// =============================================================================
// Gemini Agent
// =============================================================================

#[derive(Clone)]
pub struct Gemini {
    api_key: String,
    pub(crate) model: String,
    base_url: Option<String>,
}

impl Gemini {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: None,
        }
    }

    pub fn from_env(model: impl Into<String>) -> Result<Self> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| anyhow!("GEMINI_API_KEY environment variable not set"))?;
        Ok(Self::new(api_key, model))
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    fn client(&self) -> GeminiClient {
        let client = GeminiClient::new(&self.api_key);
        if let Some(ref url) = self.base_url {
            client.with_base_url(url)
        } else {
            client
        }
    }

    /// Convert a schemars-generated JSON schema to Gemini's format.
    /// Gemini uses a subset of JSON Schema — strip unsupported keys like
    /// $schema, definitions, $ref, additionalProperties.
    fn to_gemini_schema(schema: &Value) -> Value {
        // If the schema has a top-level "properties" wrapper from schemars,
        // extract the inner schema definition.
        let inner = if let Some(props) = schema.get("properties") {
            // schemars wraps in { "type": "object", "properties": { ... } }
            schema.clone()
        } else if let Some(defs) = schema.get("definitions") {
            // schemars may use $ref — inline the referenced definition
            if let Some(r) = schema.get("$ref").and_then(|r| r.as_str()) {
                let def_name = r.trim_start_matches("#/definitions/");
                defs.get(def_name).cloned().unwrap_or_else(|| schema.clone())
            } else {
                schema.clone()
            }
        } else {
            schema.clone()
        };

        Self::clean_schema(&inner)
    }

    /// Recursively clean a JSON schema for Gemini compatibility.
    fn clean_schema(val: &Value) -> Value {
        match val {
            Value::Object(map) => {
                let mut cleaned = serde_json::Map::new();
                for (key, value) in map {
                    match key.as_str() {
                        // Strip unsupported keys
                        "$schema" | "$ref" | "definitions" | "additionalProperties"
                        | "title" | "default" => continue,
                        // Recurse into nested schemas
                        "properties" => {
                            if let Value::Object(props) = value {
                                let mut cleaned_props = serde_json::Map::new();
                                for (pk, pv) in props {
                                    cleaned_props.insert(pk.clone(), Self::clean_schema(pv));
                                }
                                cleaned.insert(key.clone(), Value::Object(cleaned_props));
                            }
                        }
                        "items" => {
                            cleaned.insert(key.clone(), Self::clean_schema(value));
                        }
                        // Handle nullable types from schemars (anyOf with null)
                        "anyOf" => {
                            if let Value::Array(variants) = value {
                                let non_null: Vec<&Value> = variants
                                    .iter()
                                    .filter(|v| v.get("type") != Some(&Value::String("null".into())))
                                    .collect();
                                if non_null.len() == 1 {
                                    // Optional field: use the non-null variant's type
                                    let inner = Self::clean_schema(non_null[0]);
                                    if let Value::Object(inner_map) = inner {
                                        for (ik, iv) in inner_map {
                                            cleaned.insert(ik, iv);
                                        }
                                    }
                                }
                                // else: keep as-is (complex union types)
                            }
                        }
                        _ => {
                            cleaned.insert(key.clone(), Self::clean_schema(value));
                        }
                    }
                }
                Value::Object(cleaned)
            }
            Value::Array(arr) => {
                Value::Array(arr.iter().map(|v| Self::clean_schema(v)).collect())
            }
            _ => val.clone(),
        }
    }
}

// =============================================================================
// Agent Implementation
// =============================================================================

#[async_trait]
impl Agent for Gemini {
    async fn extract_json(&self, system: &str, user: &str, schema: Value) -> Result<Value> {
        let gemini_schema = Self::to_gemini_schema(&schema);

        let request = GenerateContentRequest {
            contents: vec![Content::user(user)],
            system_instruction: Some(Content::system(system)),
            generation_config: Some(GenerationConfig {
                temperature: Some(0.0),
                max_output_tokens: Some(65536),
                response_mime_type: Some("application/json".to_string()),
                response_json_schema: Some(gemini_schema),
            }),
        };

        let response = self.client().generate_content(&self.model, &request).await?;
        let text = GeminiClient::extract_text(response)?;

        serde_json::from_str(&text)
            .map_err(|e| anyhow!("Failed to parse Gemini JSON response: {e}\nRaw: {text}"))
    }

    async fn chat(&self, system: &str, user: &str) -> Result<String> {
        let request = GenerateContentRequest {
            contents: vec![Content::user(user)],
            system_instruction: Some(Content::system(system)),
            generation_config: Some(GenerationConfig {
                temperature: Some(0.7),
                max_output_tokens: Some(4096),
                response_mime_type: None,
                response_json_schema: None,
            }),
        };

        let response = self.client().generate_content(&self.model, &request).await?;
        GeminiClient::extract_text(response)
    }

    fn with_tools(&self, _tools: Vec<Arc<dyn DynTool>>) -> Box<dyn Agent> {
        // Tool use not yet implemented for Gemini
        Box::new(self.clone())
    }

    fn prompt(&self, _input: &str) -> Box<dyn PromptBuilder> {
        unimplemented!("PromptBuilder not yet implemented for Gemini")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemini_new() {
        let g = Gemini::new("test-key", "gemini-2.5-flash");
        assert_eq!(g.model, "gemini-2.5-flash");
        assert_eq!(g.api_key, "test-key");
    }

    #[test]
    fn gemini_with_base_url() {
        let g = Gemini::new("test-key", "gemini-2.5-flash")
            .with_base_url("https://custom.api.com");
        assert_eq!(g.base_url, Some("https://custom.api.com".to_string()));
    }

    #[test]
    fn clean_schema_strips_unsupported_keys() {
        let schema = serde_json::json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "title": "TestSchema",
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "name": { "type": "string", "title": "Name" },
                "count": { "type": "integer", "default": 0 }
            },
            "required": ["name"]
        });

        let cleaned = Gemini::clean_schema(&schema);
        assert!(cleaned.get("$schema").is_none());
        assert!(cleaned.get("title").is_none());
        assert!(cleaned.get("additionalProperties").is_none());
        assert_eq!(cleaned.get("type"), Some(&Value::String("object".into())));
        assert!(cleaned.get("properties").unwrap().get("name").unwrap().get("title").is_none());
        assert!(cleaned.get("properties").unwrap().get("count").unwrap().get("default").is_none());
    }

    #[test]
    fn clean_schema_handles_nullable_anyof() {
        let schema = serde_json::json!({
            "anyOf": [
                { "type": "string" },
                { "type": "null" }
            ]
        });

        let cleaned = Gemini::clean_schema(&schema);
        assert_eq!(cleaned.get("type"), Some(&Value::String("string".into())));
        assert!(cleaned.get("anyOf").is_none());
    }

    #[test]
    fn to_gemini_schema_inlines_ref() {
        let schema = serde_json::json!({
            "$ref": "#/definitions/MyType",
            "definitions": {
                "MyType": {
                    "type": "object",
                    "properties": {
                        "value": { "type": "string" }
                    }
                }
            }
        });

        let result = Gemini::to_gemini_schema(&schema);
        assert_eq!(result.get("type"), Some(&Value::String("object".into())));
        assert!(result.get("definitions").is_none());
        assert!(result.get("$ref").is_none());
    }
}
