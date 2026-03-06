//! Scrape domain activity functions: pure logic extracted from handlers.
//!
//! Each function takes specific inputs and returns accumulated output.
//! No `&mut PipelineState` — state flows through `ScrapeOutput`.

pub(crate) mod shared;
pub(crate) mod signal_events;
pub(crate) mod social_scrape;
pub(crate) mod topic_discovery;
pub(crate) mod types;
pub(crate) mod url_resolution;
pub(crate) mod web_scrape;

pub(crate) use signal_events::register_sources_events;
pub(crate) use types::{
    FetchExtractResult, FetchExtractStats, ScrapeOutput, StatsDelta, UrlExtraction, UrlResolution,
};
