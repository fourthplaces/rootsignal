use anyhow::Result;
use rootsignal_core::ServerDeps;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Result of adversarial validation.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ValidationResult {
    pub quote_checks: Vec<QuoteCheck>,
    pub counter_hypothesis: String,
    pub simpler_explanation_likely: bool,
    pub sufficient_sources: bool,
    pub sufficient_evidence_types: bool,
    pub scope_proportional: bool,
    pub rejected: bool,
    pub reasoning: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct QuoteCheck {
    pub connection_role: String,
    pub quote_found_in_evidence: bool,
    pub note: String,
}

/// Validate a finding using adversarial LLM extraction.
///
/// This is a single structured extraction call (not an agent loop) that
/// pressure-tests the investigation result.
pub async fn validate_finding<T: serde::Serialize>(
    investigation_output: &T,
    raw_response: &str,
    deps: &Arc<ServerDeps>,
) -> Result<ValidationResult> {
    let system_prompt = deps.prompts.finding_validation_prompt();

    let user_prompt = format!(
        "Validate this investigation result:\n\n## Investigation Output\n```json\n{}\n```\n\n## Raw Agent Response\n{}",
        serde_json::to_string_pretty(investigation_output).unwrap_or_default(),
        &raw_response[..raw_response.len().min(8000)]
    );

    let model = &deps.file_config.models.investigation;

    let result: ValidationResult = deps
        .ai
        .extract(model, system_prompt, &user_prompt)
        .await?;

    Ok(result)
}
