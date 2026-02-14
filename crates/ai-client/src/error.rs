use thiserror::Error;

#[derive(Debug, Error)]
pub enum AiError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("API error: {0}")]
    Api(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Tool error: {0}")]
    Tool(String),

    #[error("Max turns exceeded: {0}")]
    MaxTurns(usize),
}

impl From<reqwest::Error> for AiError {
    fn from(e: reqwest::Error) -> Self {
        AiError::Network(e.to_string())
    }
}

impl From<serde_json::Error> for AiError {
    fn from(e: serde_json::Error) -> Self {
        AiError::Parse(e.to_string())
    }
}
