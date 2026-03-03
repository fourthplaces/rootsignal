//! Scrape domain activity functions: pure logic extracted from handlers.
//!
//! Each function takes specific inputs and returns accumulated output.
//! No `&mut PipelineState` — state flows through `ScrapeOutput`.

pub(crate) mod scraper;
pub(crate) mod signal_events;
mod social_scrape;
mod topic_discovery;
pub(crate) mod types;
mod url_resolution;
mod web_scrape;

// Re-exports for external consumers.
pub(crate) use scraper::Scraper;
pub(crate) use signal_events::register_sources_events;
pub(crate) use types::{
    FetchExtractResult, FetchExtractStats, ScrapeOutput, StatsDelta, UrlResolution,
};
