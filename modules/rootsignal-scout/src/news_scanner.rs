use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use tracing::{info, warn};

use rootsignal_graph::GraphWriter;

use crate::pipeline::extractor::{Extractor, SignalExtractor};
use crate::pipeline::scraper::{PageScraper, RssFetcher};
use crate::scheduling::budget::BudgetTracker;

/// Hardcoded seed list of global/national RSS feeds for Driver B.
const NEWS_FEEDS: &[&str] = &[
    // Wire services
    "https://feeds.apnews.com/rss/apf-topnews",
    "https://www.reuters.com/rssFeed/topNews",
    // US national
    "https://feeds.npr.org/1001/rss.xml",
    "https://rss.nytimes.com/services/xml/rss/nyt/HomePage.xml",
    // International
    "https://feeds.bbci.co.uk/news/rss.xml",
    "https://www.aljazeera.com/xml/rss/all.xml",
    "https://www.theguardian.com/us-news/rss",
    // Topic: housing
    "https://www.curbed.com/rss/index.xml",
    "https://shelterforce.org/feed/",
    // Topic: environment
    "https://grist.org/feed/",
    "https://insideclimatenews.org/feed/",
    // Topic: public health
    "https://kffhealthnews.org/feed/",
    // Topic: community / local
    "https://civicnewscompany.com/feed/",
    "https://nextcity.org/feed",
    "https://www.governing.com/rss",
    // Topic: immigration
    "https://www.migrationpolicy.org/rss.xml",
    // Topic: labor
    "https://www.labornotes.org/rss.xml",
];

/// News scanner that fetches global RSS feeds, extracts signals, and stores them.
pub struct NewsScanner {
    rss: RssFetcher,
    extractor: Box<dyn SignalExtractor>,
    scraper: Arc<dyn PageScraper>,
    writer: GraphWriter,
    budget: BudgetTracker,
}

impl NewsScanner {
    pub fn new(
        anthropic_api_key: &str,
        voyage_api_key: &str,
        _serper_api_key: &str,
        writer: GraphWriter,
        daily_budget_cents: u64,
    ) -> Result<Self> {
        let scraper: Arc<dyn PageScraper> = match std::env::var("BROWSERLESS_URL") {
            Ok(url) => {
                let token = std::env::var("BROWSERLESS_TOKEN").ok();
                Arc::new(crate::pipeline::scraper::BrowserlessScraper::new(
                    &url,
                    token.as_deref(),
                ))
            }
            Err(_) => Arc::new(crate::pipeline::scraper::ChromeScraper::new()),
        };

        // Use a generic "Global" scope for extraction — no city bias
        let extractor = Box::new(Extractor::new(
            anthropic_api_key,
            "Global",
            0.0,
            0.0,
        ));

        let _ = voyage_api_key; // reserved for future embedding use

        Ok(Self {
            rss: RssFetcher::new(),
            extractor,
            scraper,
            writer,
            budget: BudgetTracker::new(daily_budget_cents),
        })
    }

    /// Scan all news feeds, extract signals, store them.
    /// Returns list of (lat, lng) for newly created signals.
    pub async fn scan(&self) -> Result<Vec<(f64, f64)>> {
        info!(feeds = NEWS_FEEDS.len(), "Starting news scan");

        // 1. Fetch all feeds
        let mut all_urls: Vec<(String, Option<String>)> = Vec::new();
        for feed_url in NEWS_FEEDS {
            match self.rss.fetch_items(feed_url).await {
                Ok(items) => {
                    for item in items {
                        all_urls.push((item.url, item.title));
                    }
                }
                Err(e) => {
                    warn!(feed = feed_url, error = %e, "Failed to fetch feed");
                }
            }
        }

        info!(articles = all_urls.len(), "Collected articles from feeds");

        // 2. Dedup against existing sources in the graph
        let mut seen = HashSet::new();
        let mut new_urls: Vec<(String, Option<String>)> = Vec::new();
        for (url, title) in all_urls {
            if seen.insert(url.clone()) {
                // Check if source already exists in graph
                let exists = self
                    .writer
                    .source_exists(&url)
                    .await
                    .unwrap_or(false);
                if !exists {
                    new_urls.push((url, title));
                }
            }
        }

        info!(new_articles = new_urls.len(), "New articles after dedup");

        // 3. Process each new article
        let mut locations = Vec::new();

        for (url, _title) in &new_urls {
            if self.budget.is_active() && !self.budget.has_budget(5) {
                info!("Budget exhausted, stopping news scan");
                break;
            }

            // Scrape
            let content = match self.scraper.scrape(url).await {
                Ok(c) if !c.is_empty() => c,
                Ok(_) => {
                    warn!(url, "Empty content from scraper");
                    continue;
                }
                Err(e) => {
                    warn!(url, error = %e, "Failed to scrape article");
                    continue;
                }
            };

            // Extract signals
            let extraction = match self.extractor.extract(&content, url).await {
                Ok(e) => e,
                Err(e) => {
                    warn!(url, error = %e, "Failed to extract signals");
                    continue;
                }
            };

            // Store signals
            for node in &extraction.nodes {
                if let Some(meta) = node.meta() {
                    if let Some(loc) = &meta.location {
                        locations.push((loc.lat, loc.lng));
                    }
                }

                match self
                    .writer
                    .upsert_node(node, "news_scanner")
                    .await
                {
                    Ok(_) => {}
                    Err(e) => warn!(error = %e, "Failed to store signal from news"),
                }
            }
        }

        info!(
            signals_with_location = locations.len(),
            "News scan complete"
        );
        Ok(locations)
    }
}
