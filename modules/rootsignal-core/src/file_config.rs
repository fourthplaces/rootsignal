use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// TOML-backed configuration loaded from disk.
/// Secrets (API keys, DB URL) stay as env vars.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    pub identity: IdentityConfig,
    pub models: ModelsConfig,
    pub prompts: PromptsConfig,
    pub clustering: ClusteringConfig,
    pub server: ServerConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IdentityConfig {
    pub region: String,
    pub description: String,
    pub system_name: String,
    pub locales: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelsConfig {
    pub extraction: String,
    pub nlq: String,
    pub investigation: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PromptsConfig {
    pub extraction: PathBuf,
    pub investigation: PathBuf,
    pub nlq: PathBuf,
    pub detect_entity: PathBuf,
    pub signal_extraction: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClusteringConfig {
    pub similarity_threshold: f64,
    pub match_score_threshold: f64,
    pub merge_coherence_threshold: f64,
    pub geo_radius_meters: f64,
    pub time_window_hours: i64,
    pub batch_size: i64,
    pub hnsw_ef_search: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
    pub allowed_origins: Vec<String>,
}

/// Load and parse a TOML config file.
pub fn load_config(path: &Path) -> Result<FileConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;
    let config: FileConfig = toml::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
    Ok(config)
}

/// Load and parse the raw TOML value tree (for template variable resolution).
pub fn load_toml_value(path: &Path) -> Result<toml::Value> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;
    let value: toml::Value = content
        .parse()
        .with_context(|| format!("Failed to parse config as TOML: {}", path.display()))?;
    Ok(value)
}
