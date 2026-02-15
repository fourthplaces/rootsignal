use async_trait::async_trait;
use rootsignal_core::error::CrawlResult;
use rootsignal_core::{DiscoverConfig, Ingestor, RawPage};
use spider::website::Website;

/// Spider adapter — local Rust-native HTTP crawler. No API key, no per-page cost.
pub struct SpiderIngestor;

impl SpiderIngestor {
    pub fn new() -> Self {
        Self
    }

    /// Extract `<title>` from HTML content.
    fn extract_title(html: &str) -> Option<String> {
        let lower = html.to_lowercase();
        let start = lower.find("<title")?;
        let after_tag = html[start..].find('>')?;
        let content_start = start + after_tag + 1;
        let end = lower[content_start..].find("</title>")?;
        let title = html[content_start..content_start + end].trim();
        if title.is_empty() {
            None
        } else {
            Some(title.to_string())
        }
    }
}

#[async_trait]
impl Ingestor for SpiderIngestor {
    async fn discover(&self, config: &DiscoverConfig) -> CrawlResult<Vec<RawPage>> {
        let mut website = Website::new(&config.url);

        website.with_depth(config.max_depth);
        website.with_limit(config.limit as u32);

        // Apply URL filters
        if !config.exclude_patterns.is_empty() {
            let blacklist: Vec<spider::compact_str::CompactString> = config
                .exclude_patterns
                .iter()
                .map(|s| s.as_str().into())
                .collect();
            website.with_blacklist_url(Some(blacklist));
        }

        tracing::info!(
            url = %config.url,
            depth = config.max_depth,
            limit = config.limit,
            "Spider: starting crawl"
        );

        website.scrape().await;

        let mut pages = Vec::new();

        if let Some(site_pages) = website.get_pages() {
            for page in site_pages {
                let url = page.get_url().to_string();

                // Apply include patterns: if set, URL path must contain at least one
                if !config.include_patterns.is_empty() {
                    let matches = config
                        .include_patterns
                        .iter()
                        .any(|pattern| url.contains(pattern));
                    if !matches {
                        continue;
                    }
                }

                let html = page.get_html();
                if html.trim().is_empty() {
                    continue;
                }

                let title = Self::extract_title(&html);

                let mut raw_page = RawPage::new(&url, &html)
                    .with_content_type("text/html".to_string())
                    .with_html(html.clone())
                    .with_metadata(
                        "fetched_via",
                        serde_json::Value::String("spider".to_string()),
                    );

                if let Some(t) = title {
                    raw_page = raw_page.with_title(t);
                }

                pages.push(raw_page);
            }
        }

        tracing::info!(
            url = %config.url,
            pages = pages.len(),
            "Spider: crawl complete"
        );

        Ok(pages)
    }

    async fn fetch_specific(&self, urls: &[String]) -> CrawlResult<Vec<RawPage>> {
        let mut pages = Vec::new();

        for url in urls {
            let mut website = Website::new(url);
            website.with_limit(1);
            website.with_depth(0);

            website.scrape().await;

            if let Some(site_pages) = website.get_pages() {
                if let Some(page) = site_pages.first() {
                    let html = page.get_html();
                    if !html.trim().is_empty() {
                        let title = Self::extract_title(&html);
                        let mut raw_page = RawPage::new(url, &html)
                            .with_content_type("text/html".to_string())
                            .with_html(html.clone())
                            .with_metadata(
                                "fetched_via",
                                serde_json::Value::String("spider".to_string()),
                            );
                        if let Some(t) = title {
                            raw_page = raw_page.with_title(t);
                        }
                        pages.push(raw_page);
                    }
                }
            }
        }

        Ok(pages)
    }

    fn name(&self) -> &str {
        "spider"
    }
}
