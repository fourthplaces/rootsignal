//! Generic return types for simulated web content.
//! No dependency on rootsignal types — these mirror what real scrapers return.

use serde::{Deserialize, Serialize};

/// A search engine result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimSearchResult {
    pub url: String,
    pub title: String,
    pub snippet: String,
}

/// A scraped web page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimPage {
    pub url: String,
    pub content: String,
    pub raw_html: Option<String>,
}

/// A social media post.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimPost {
    pub content: String,
    pub author: Option<String>,
    pub url: Option<String>,
    pub platform: String,
}
