//! Typed errors for crawl and security operations.

use thiserror::Error;

/// Errors that can occur during crawl operations.
#[derive(Debug, Error)]
pub enum CrawlError {
    /// Security validation failed
    #[error("security error: {0}")]
    Security(#[from] SecurityError),

    /// HTTP request failed
    #[error("HTTP error: {0}")]
    Http(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// Rate limit exceeded
    #[error("rate limit exceeded")]
    RateLimitExceeded,

    /// Invalid URL format
    #[error("invalid URL: {url}")]
    InvalidUrl { url: String },

    /// Robots.txt disallows crawling
    #[error("robots.txt disallows: {url}")]
    RobotsDisallowed { url: String },

    /// Connection timeout
    #[error("timeout crawling: {url}")]
    Timeout { url: String },

    /// Max pages reached
    #[error("max pages reached: {count}")]
    MaxPagesReached { count: usize },
}

/// Security-related errors, primarily for SSRF protection.
#[derive(Debug, Error)]
pub enum SecurityError {
    /// URL scheme not allowed (e.g., file://, ftp://)
    #[error("disallowed URL scheme: {0}")]
    DisallowedScheme(String),

    /// Host is blocked (e.g., localhost, internal IPs)
    #[error("blocked host: {0}")]
    BlockedHost(String),

    /// IP in blocked CIDR range (e.g., 10.0.0.0/8)
    #[error("blocked IP range: {0}")]
    BlockedCidr(String),

    /// URL has no host
    #[error("URL has no host")]
    NoHost,

    /// DNS resolution failed
    #[error("DNS resolution failed: {0}")]
    DnsResolution(String),

    /// URL parsing failed
    #[error("URL parse error: {0}")]
    UrlParse(#[from] url::ParseError),
}

/// Result type alias for crawl operations.
pub type CrawlResult<T> = std::result::Result<T, CrawlError>;

/// Result type alias for security operations.
pub type SecurityResult<T> = std::result::Result<T, SecurityError>;
