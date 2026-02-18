use thiserror::Error;

#[derive(Error, Debug)]
pub enum RootSignalError {
    #[error("Database error: {0}")]
    Database(String),

    #[error("Extraction error: {0}")]
    Extraction(String),

    #[error("PII detected in extraction: {0}")]
    PiiDetected(String),

    #[error("Scraping error: {0}")]
    Scraping(String),

    #[error("Embedding error: {0}")]
    Embedding(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Scout lock conflict: another scout run is in progress")]
    ScoutLockConflict,

    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}
