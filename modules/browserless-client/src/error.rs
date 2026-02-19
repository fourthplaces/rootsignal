use thiserror::Error;

pub type Result<T> = std::result::Result<T, BrowserlessError>;

#[derive(Debug, Error)]
pub enum BrowserlessError {
    #[error("Network error: {0}")]
    Network(String),

    #[error("API error (status {status}): {message}")]
    Api { status: u16, message: String },
}

impl From<reqwest::Error> for BrowserlessError {
    fn from(err: reqwest::Error) -> Self {
        BrowserlessError::Network(err.to_string())
    }
}
