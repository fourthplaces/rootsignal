use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

use crate::file_config::FileConfig;
use crate::template::{resolve_config_vars, resolve_runtime_vars, validate_template};

/// Holds pre-resolved prompt templates (config vars resolved, runtime vars intact).
#[derive(Debug, Clone)]
pub struct PromptRegistry {
    extraction: String,
    investigation: String,
    nlq: String,
    qualification: String,
    detect_entity: String,
    signal_extraction: String,
}

/// Allowed runtime variables per prompt type.
const EXTRACTION_RUNTIME_VARS: &[&str] = &["taxonomy"];
const INVESTIGATION_RUNTIME_VARS: &[&str] = &[];
const NLQ_RUNTIME_VARS: &[&str] = &["taxonomy", "today"];
const QUALIFICATION_RUNTIME_VARS: &[&str] = &[];
const DETECT_ENTITY_RUNTIME_VARS: &[&str] = &[];
const SIGNAL_EXTRACTION_RUNTIME_VARS: &[&str] = &[];

impl PromptRegistry {
    /// Load all prompt files, resolve config vars, validate runtime vars.
    pub fn load(config: &FileConfig, config_dir: &Path, toml_value: &toml::Value) -> Result<Self> {
        let extraction = load_and_resolve(
            &config.prompts.extraction,
            config_dir,
            toml_value,
            EXTRACTION_RUNTIME_VARS,
            "extraction",
        )?;

        let investigation = load_and_resolve(
            &config.prompts.investigation,
            config_dir,
            toml_value,
            INVESTIGATION_RUNTIME_VARS,
            "investigation",
        )?;

        let nlq = load_and_resolve(
            &config.prompts.nlq,
            config_dir,
            toml_value,
            NLQ_RUNTIME_VARS,
            "nlq",
        )?;

        let qualification = load_and_resolve(
            &config.prompts.qualification,
            config_dir,
            toml_value,
            QUALIFICATION_RUNTIME_VARS,
            "qualification",
        )?;

        let detect_entity = load_and_resolve(
            &config.prompts.detect_entity,
            config_dir,
            toml_value,
            DETECT_ENTITY_RUNTIME_VARS,
            "detect_entity",
        )?;

        let signal_extraction = load_and_resolve(
            &config.prompts.signal_extraction,
            config_dir,
            toml_value,
            SIGNAL_EXTRACTION_RUNTIME_VARS,
            "signal_extraction",
        )?;

        Ok(Self {
            extraction,
            investigation,
            nlq,
            qualification,
            detect_entity,
            signal_extraction,
        })
    }

    /// Get extraction prompt with runtime vars filled in.
    pub fn extraction_prompt(&self, taxonomy: &str) -> String {
        resolve_runtime_vars(&self.extraction, &HashMap::from([("taxonomy", taxonomy)]))
    }

    /// Get NLQ prompt with runtime vars filled in.
    pub fn nlq_prompt(&self, taxonomy: &str, today: &str) -> String {
        resolve_runtime_vars(
            &self.nlq,
            &HashMap::from([("taxonomy", taxonomy), ("today", today)]),
        )
    }

    /// Get investigation prompt (no runtime vars currently).
    pub fn investigation_prompt(&self) -> &str {
        &self.investigation
    }

    /// Get qualification prompt (no runtime vars).
    pub fn qualification_prompt(&self) -> &str {
        &self.qualification
    }

    /// Get detect-entity prompt (no runtime vars).
    pub fn detect_entity_prompt(&self) -> &str {
        &self.detect_entity
    }

    /// Get signal extraction prompt (no runtime vars).
    pub fn signal_extraction_prompt(&self) -> &str {
        &self.signal_extraction
    }
}

/// Load a prompt file, resolve config-time variables, and validate.
fn load_and_resolve(
    relative_path: &Path,
    config_dir: &Path,
    toml_value: &toml::Value,
    allowed_runtime: &[&str],
    prompt_name: &str,
) -> Result<String> {
    let full_path = config_dir.join(relative_path);
    let content = std::fs::read_to_string(&full_path).with_context(|| {
        format!(
            "Failed to read {} prompt file: {}",
            prompt_name,
            full_path.display()
        )
    })?;

    if content.trim().is_empty() {
        anyhow::bail!(
            "Prompt file is empty: {} ({})",
            full_path.display(),
            prompt_name
        );
    }

    let resolved = resolve_config_vars(&content, toml_value).with_context(|| {
        format!(
            "Failed to resolve config variables in {} prompt: {}",
            prompt_name,
            full_path.display()
        )
    })?;

    validate_template(&resolved, toml_value, allowed_runtime).with_context(|| {
        format!(
            "Template validation failed for {} prompt: {}",
            prompt_name,
            full_path.display()
        )
    })?;

    Ok(resolved)
}
