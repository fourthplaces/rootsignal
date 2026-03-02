//! Scraper — dependency bundle for the scrape-extract-store-dedup pipeline.

use std::sync::Arc;

use crate::core::extractor::SignalExtractor;

/// Core scrape-extract-store-dedup pipeline. Holds infrastructure deps
/// needed by resolve, fetch, social scrape, and topic discovery methods.
pub(crate) struct Scraper {
    pub(crate) store: Arc<dyn crate::traits::SignalReader>,
    pub(crate) extractor: Arc<dyn SignalExtractor>,
    pub(crate) fetcher: Arc<dyn crate::traits::ContentFetcher>,
}

impl Scraper {
    pub fn new(
        store: Arc<dyn crate::traits::SignalReader>,
        extractor: Arc<dyn SignalExtractor>,
        fetcher: Arc<dyn crate::traits::ContentFetcher>,
    ) -> Self {
        Self {
            store,
            extractor,
            fetcher,
        }
    }
}
