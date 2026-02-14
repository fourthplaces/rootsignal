use crate::error::{CrawlError, CrawlResult};
use crate::security::UrlValidator;
use crate::types::RawPage;
use async_trait::async_trait;
use std::collections::HashMap;

/// Configuration for a discovery crawl.
#[derive(Debug, Clone)]
pub struct DiscoverConfig {
    pub url: String,
    pub max_depth: usize,
    pub limit: usize,
    /// URL patterns to include (substring match on path)
    pub include_patterns: Vec<String>,
    /// URL patterns to exclude (substring match on path)
    pub exclude_patterns: Vec<String>,
    pub options: HashMap<String, String>,
}

impl DiscoverConfig {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            max_depth: 2,
            limit: 50,
            include_patterns: Vec::new(),
            exclude_patterns: Vec::new(),
            options: HashMap::new(),
        }
    }

    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    /// Add an include pattern (substring match on URL path).
    pub fn include(mut self, pattern: impl Into<String>) -> Self {
        self.include_patterns.push(pattern.into());
        self
    }

    /// Add an exclude pattern (substring match on URL path).
    pub fn exclude(mut self, pattern: impl Into<String>) -> Self {
        self.exclude_patterns.push(pattern.into());
        self
    }

    pub fn with_option(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.options.insert(key.into(), value.into());
        self
    }
}

/// Universal ingestor trait — all adapters produce `Vec<RawPage>`.
#[async_trait]
pub trait Ingestor: Send + Sync {
    /// Discover and crawl pages from a source.
    async fn discover(&self, config: &DiscoverConfig) -> CrawlResult<Vec<RawPage>>;

    /// Fetch specific URLs.
    async fn fetch_specific(&self, urls: &[String]) -> CrawlResult<Vec<RawPage>>;

    /// Fetch a single URL (convenience method).
    async fn fetch_one(&self, url: &str) -> CrawlResult<RawPage> {
        let pages = self.fetch_specific(&[url.to_string()]).await?;
        pages
            .into_iter()
            .next()
            .ok_or_else(|| CrawlError::Http(format!("Failed to fetch {}", url).into()))
    }

    /// Get the ingestor name (for logging/debugging).
    fn name(&self) -> &str {
        "unknown"
    }
}

/// Web search trait for discovery queries.
#[async_trait]
pub trait WebSearcher: Send + Sync {
    /// Search the web and return raw pages of results.
    async fn search(&self, query: &str, max_results: u32) -> anyhow::Result<Vec<RawPage>>;
}

/// An ingestor that validates URLs before fetching (SSRF protection).
///
/// Wraps any URL-based ingestor to ensure all URLs are validated
/// before fetching. This prevents Server-Side Request Forgery attacks.
pub struct ValidatedIngestor<I: Ingestor> {
    inner: I,
    validator: UrlValidator,
}

impl<I: Ingestor> ValidatedIngestor<I> {
    /// Create a new validated ingestor with default security rules.
    pub fn new(ingestor: I) -> Self {
        Self {
            inner: ingestor,
            validator: UrlValidator::new(),
        }
    }

    /// Create with a custom validator.
    pub fn with_validator(ingestor: I, validator: UrlValidator) -> Self {
        Self {
            inner: ingestor,
            validator,
        }
    }

    async fn validate_url(&self, url: &str) -> CrawlResult<()> {
        self.validator
            .validate_with_dns(url)
            .await
            .map_err(CrawlError::Security)
    }
}

#[async_trait]
impl<I: Ingestor> Ingestor for ValidatedIngestor<I> {
    async fn discover(&self, config: &DiscoverConfig) -> CrawlResult<Vec<RawPage>> {
        self.validate_url(&config.url).await?;

        let pages = self.inner.discover(config).await?;

        // Filter out any pages with invalid URLs (in case of redirects)
        let validated: Vec<_> = pages
            .into_iter()
            .filter(|p| self.validator.validate(&p.url).is_ok())
            .collect();

        Ok(validated)
    }

    async fn fetch_specific(&self, urls: &[String]) -> CrawlResult<Vec<RawPage>> {
        let mut valid_urls = Vec::with_capacity(urls.len());
        for url in urls {
            if let Err(e) = self.validate_url(url).await {
                tracing::warn!(url = %url, error = %e, "Skipping blocked URL");
                continue;
            }
            valid_urls.push(url.clone());
        }

        if valid_urls.is_empty() {
            return Ok(Vec::new());
        }

        let pages = self.inner.fetch_specific(&valid_urls).await?;

        let validated: Vec<_> = pages
            .into_iter()
            .filter(|p| self.validator.validate(&p.url).is_ok())
            .collect();

        Ok(validated)
    }

    fn name(&self) -> &str {
        self.inner.name()
    }
}
