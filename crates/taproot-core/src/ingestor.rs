use crate::types::RawPage;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;

/// Configuration for a discovery crawl.
#[derive(Debug, Clone)]
pub struct DiscoverConfig {
    pub url: String,
    pub max_depth: u32,
    pub limit: u32,
    pub options: HashMap<String, String>,
}

impl DiscoverConfig {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            max_depth: 2,
            limit: 50,
            options: HashMap::new(),
        }
    }

    pub fn with_max_depth(mut self, depth: u32) -> Self {
        self.max_depth = depth;
        self
    }

    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = limit;
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
    async fn discover(&self, config: &DiscoverConfig) -> Result<Vec<RawPage>>;

    /// Fetch specific URLs.
    async fn fetch_specific(&self, urls: &[String]) -> Result<Vec<RawPage>>;
}

/// Web search trait for discovery queries.
#[async_trait]
pub trait WebSearcher: Send + Sync {
    /// Search the web and return raw pages of results.
    async fn search(&self, query: &str, max_results: u32) -> Result<Vec<RawPage>>;
}
