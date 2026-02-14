use thiserror::Error;

pub type Result<T> = std::result::Result<T, ApifyError>;

#[derive(Debug, Error)]
pub enum ApifyError {
    #[error("Network error: {0}")]
    Network(String),

    #[error("API error (status {status}): {message}")]
    Api { status: u16, message: String },

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Run failed with status: {0}")]
    RunFailed(String),
}

impl From<reqwest::Error> for ApifyError {
    fn from(err: reqwest::Error) -> Self {
        ApifyError::Network(err.to_string())
    }
}

impl From<serde_json::Error> for ApifyError {
    fn from(err: serde_json::Error) -> Self {
        ApifyError::Parse(err.to_string())
    }
}
