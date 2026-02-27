use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ai_client::tool::{Tool, ToolDefinition};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use rootsignal_graph::FieldCorrection;

use crate::traits::ContentFetcher;

// ---------------------------------------------------------------------------
// Shared verdict state — collected by tool calls during lint
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum LintVerdict {
    Pass,
    Correct { corrections: Vec<FieldCorrection> },
    Reject { reason: String },
}

/// Shared state passed to all lint tools. Accumulates verdicts keyed by node_id.
#[derive(Clone)]
pub struct LintState {
    pub verdicts: Arc<Mutex<HashMap<String, LintVerdict>>>,
}

impl LintState {
    pub fn new() -> Self {
        Self {
            verdicts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn into_verdicts(self) -> HashMap<String, LintVerdict> {
        Arc::try_unwrap(self.verdicts)
            .map(|mutex| mutex.into_inner().unwrap())
            .unwrap_or_else(|arc| arc.lock().unwrap().clone())
    }
}

// ---------------------------------------------------------------------------
// Error type shared across lint tools
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct LintToolError(pub String);

impl std::fmt::Display for LintToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for LintToolError {}

// ---------------------------------------------------------------------------
// ReadSourceTool — fetch archived content for a URL
// ---------------------------------------------------------------------------

pub struct ReadSourceTool {
    pub fetcher: Arc<dyn ContentFetcher>,
}

#[derive(Debug, Deserialize)]
pub struct ReadSourceArgs {
    pub url: String,
}

#[async_trait]
impl Tool for ReadSourceTool {
    const NAME: &'static str = "read_source";
    type Error = LintToolError;
    type Args = ReadSourceArgs;
    type Output = String;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Fetch the archived content for a source URL. Returns the page as markdown text. If the content is unavailable, returns an error — reject all signals from this source with reason SOURCE_UNREADABLE.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The source URL to fetch archived content for"
                    }
                },
                "required": ["url"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let page = self.fetcher.page(&args.url).await
            .map_err(|e| LintToolError(format!("SOURCE_UNREADABLE: {e}")))?;

        let content = page.markdown;
        if content.is_empty() {
            return Err(LintToolError("SOURCE_UNREADABLE: archived content is empty".to_string()));
        }

        // Truncate to ~50k chars to stay within token budget
        let max_len = 50_000;
        if content.len() > max_len {
            let mut end = max_len;
            while !content.is_char_boundary(end) {
                end -= 1;
            }
            Ok(format!(
                "{}...\n\n[Content truncated at {} chars]",
                &content[..end],
                max_len
            ))
        } else {
            Ok(content)
        }
    }
}

// ---------------------------------------------------------------------------
// CorrectSignalTool — fix a field on a signal
// ---------------------------------------------------------------------------

pub struct CorrectSignalTool {
    pub state: LintState,
}

#[derive(Debug, Deserialize)]
pub struct CorrectSignalArgs {
    pub node_id: String,
    pub field: String,
    pub old_value: String,
    pub new_value: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct CorrectSignalOutput {
    pub status: String,
}

const CORRECTABLE_FIELDS: &[&str] = &[
    "title", "summary", "starts_at", "ends_at", "content_date",
    "location_name", "lat", "lng", "action_url",
    "organizer", "what_needed", "goal",
    "sensitivity", "severity", "category",
];

#[async_trait]
impl Tool for CorrectSignalTool {
    const NAME: &'static str = "correct_signal";
    type Error = LintToolError;
    type Args = CorrectSignalArgs;
    type Output = CorrectSignalOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Correct a specific field on a signal. Can be called multiple times for the same signal to fix different fields. The signal is auto-promoted after corrections. Allowed fields: title, summary, starts_at, ends_at, content_date, location_name, lat, lng, action_url, organizer, what_needed, goal, sensitivity, severity, category.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "node_id": {
                        "type": "string",
                        "description": "The signal's node ID"
                    },
                    "field": {
                        "type": "string",
                        "description": "The field name to correct"
                    },
                    "old_value": {
                        "type": "string",
                        "description": "The current (incorrect) value"
                    },
                    "new_value": {
                        "type": "string",
                        "description": "The corrected value"
                    },
                    "reason": {
                        "type": "string",
                        "description": "Why this correction is needed"
                    }
                },
                "required": ["node_id", "field", "old_value", "new_value", "reason"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        if !CORRECTABLE_FIELDS.contains(&args.field.as_str()) {
            return Err(LintToolError(format!(
                "Field '{}' is not correctable. Immutable fields require rejection instead.",
                args.field
            )));
        }

        let correction = FieldCorrection {
            field: args.field,
            old_value: args.old_value,
            new_value: args.new_value,
            reason: args.reason,
        };

        let mut verdicts = self.state.verdicts.lock()
            .map_err(|_| LintToolError("Lock poisoned".to_string()))?;

        match verdicts.get_mut(&args.node_id) {
            Some(LintVerdict::Correct { corrections }) => {
                corrections.push(correction);
            }
            Some(LintVerdict::Reject { .. }) => {
                return Err(LintToolError("Signal already rejected".to_string()));
            }
            _ => {
                verdicts.insert(args.node_id, LintVerdict::Correct {
                    corrections: vec![correction],
                });
            }
        }

        Ok(CorrectSignalOutput { status: "corrected".to_string() })
    }
}

// ---------------------------------------------------------------------------
// RejectSignalTool — mark a signal as unfixable
// ---------------------------------------------------------------------------

pub struct RejectSignalTool {
    pub state: LintState,
}

#[derive(Debug, Deserialize)]
pub struct RejectSignalArgs {
    pub node_id: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct RejectSignalOutput {
    pub status: String,
}

#[async_trait]
impl Tool for RejectSignalTool {
    const NAME: &'static str = "reject_signal";
    type Error = LintToolError;
    type Args = RejectSignalArgs;
    type Output = RejectSignalOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Reject a signal that cannot be corrected. Use when the signal is fundamentally wrong: wrong type, hallucinated content, no basis in source, or source is unreadable. Rejected signals are hidden from public view and flagged for human review.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "node_id": {
                        "type": "string",
                        "description": "The signal's node ID"
                    },
                    "reason": {
                        "type": "string",
                        "description": "Why this signal is being rejected"
                    }
                },
                "required": ["node_id", "reason"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut verdicts = self.state.verdicts.lock()
            .map_err(|_| LintToolError("Lock poisoned".to_string()))?;

        verdicts.insert(args.node_id, LintVerdict::Reject { reason: args.reason });

        Ok(RejectSignalOutput { status: "rejected".to_string() })
    }
}

// ---------------------------------------------------------------------------
// PassSignalTool — mark a signal as verified
// ---------------------------------------------------------------------------

pub struct PassSignalTool {
    pub state: LintState,
}

#[derive(Debug, Deserialize)]
pub struct PassSignalArgs {
    pub node_id: String,
}

#[derive(Debug, Serialize)]
pub struct PassSignalOutput {
    pub status: String,
}

#[async_trait]
impl Tool for PassSignalTool {
    const NAME: &'static str = "pass_signal";
    type Error = LintToolError;
    type Args = PassSignalArgs;
    type Output = PassSignalOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Mark a signal as verified and correct with no changes needed. The signal will be promoted to live status. Only call this for signals that need zero corrections — corrected signals are auto-promoted.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "node_id": {
                        "type": "string",
                        "description": "The signal's node ID"
                    }
                },
                "required": ["node_id"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut verdicts = self.state.verdicts.lock()
            .map_err(|_| LintToolError("Lock poisoned".to_string()))?;

        if let Some(LintVerdict::Reject { .. }) = verdicts.get(&args.node_id) {
            return Err(LintToolError("Signal already rejected, cannot pass".to_string()));
        }

        if let Some(LintVerdict::Correct { .. }) = verdicts.get(&args.node_id) {
            return Err(LintToolError("Signal already has corrections and is auto-promoted. Do not call pass_signal for corrected signals.".to_string()));
        }

        verdicts.insert(args.node_id, LintVerdict::Pass);

        Ok(PassSignalOutput { status: "passed".to_string() })
    }
}
