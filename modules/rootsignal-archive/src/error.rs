/// Result type alias for archive operations.
pub type Result<T> = std::result::Result<T, ArchiveError>;

#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("No archived content for target: {0}")]
    NotFound(String),

    #[error("Unsupported platform: {0}")]
    UnsupportedPlatform(String),

    #[error("Fetch failed: {0}")]
    FetchFailed(String),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
