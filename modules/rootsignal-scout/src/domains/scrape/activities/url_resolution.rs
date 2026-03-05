//! URL resolution: query resolution, HTML listing, RSS, page URLs.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt};
use tracing::{info, warn};

use ai_client::Agent;
use rootsignal_common::{is_web_query, scraping_strategy, ScrapingStrategy, SourceNode};

use crate::core::engine::ScoutEngineDeps;
use crate::domains::enrichment::activities::domain_filter;
use crate::infra::util::sanitize_url;

use super::types::UrlResolution;

/// Resolve web sources to URLs: query resolution, HTML listing, RSS, page URLs.
/// Deduplicates and filters blocked URLs.
pub(crate) async fn resolve_web_urls(
    deps: &ScoutEngineDeps,
    sources: &[&SourceNode],
    url_to_canonical_key: &HashMap<String, String>,
    ai: Option<&dyn Agent>,
    region_name: Option<&str>,
) -> UrlResolution {
        let mut url_mappings: HashMap<String, String> = HashMap::new();
        let mut pub_dates: HashMap<String, DateTime<Utc>> = HashMap::new();
        let mut query_api_errors: HashSet<String> = HashSet::new();
    let fetcher = deps.fetcher.as_ref().expect("fetcher required");
    let store = &deps.store;

        let mut phase_urls: Vec<String> = Vec::new();

        // Partition by behavior type
        let query_sources: Vec<&&SourceNode> = sources
            .iter()
            .filter(|s| {
                matches!(
                    scraping_strategy(s.value()),
                    ScrapingStrategy::WebQuery | ScrapingStrategy::HtmlListing { .. }
                )
            })
            .collect();
        let page_sources: Vec<&&SourceNode> = sources
            .iter()
            .filter(|s| matches!(scraping_strategy(s.value()), ScrapingStrategy::WebPage))
            .collect();

        // Resolve query sources → URLs
        let api_queries: Vec<&&&SourceNode> = query_sources
            .iter()
            .filter(|s| is_web_query(s.value()))
            .collect();
        if !api_queries.is_empty() {
            info!(
                queries = api_queries.len(),
                "Resolving web search queries..."
            );
            let fetcher = fetcher.clone();
            let query_inputs: Vec<_> = api_queries
                .iter()
                .map(|source| (source.canonical_key.clone(), source.canonical_value.clone()))
                .collect();
            let search_results: Vec<_> =
                stream::iter(query_inputs.into_iter().map(|(canonical_key, query_str)| {
                    let fetcher = fetcher.clone();
                    async move {
                        let result = fetcher.search(&query_str).await;
                        (canonical_key, query_str, result)
                    }
                }))
                .buffer_unordered(5)
                .collect()
                .await;

            for (canonical_key, query_str, result) in search_results {
                match result {
                    Ok(archived) => {
                        for r in &archived.results {
                            let clean = sanitize_url(&r.url);
                            url_mappings
                                .entry(clean)
                                .or_insert_with(|| canonical_key.clone());
                        }
                        for r in archived.results {
                            phase_urls.push(r.url);
                        }
                    }
                    Err(e) => {
                        warn!(query_str, error = %e, "Web search failed");
                        query_api_errors.insert(canonical_key);
                    }
                }
            }
        }

        // HTML-based queries
        let html_queries: Vec<&&&SourceNode> = query_sources
            .iter()
            .filter(|s| {
                matches!(
                    scraping_strategy(s.value()),
                    ScrapingStrategy::HtmlListing { .. }
                )
            })
            .collect();
        for source in &html_queries {
            let link_pattern = match scraping_strategy(source.value()) {
                ScrapingStrategy::HtmlListing { link_pattern } => Some(link_pattern),
                _ => None,
            };
            if let (Some(url), Some(pattern)) = (&source.url, link_pattern) {
                let html = match fetcher.page(url).await {
                    Ok(page) => Some(page.raw_html),
                    Err(e) => {
                        warn!(url = url.as_str(), error = %e, "Query scrape failed");
                        None
                    }
                };
                match html {
                    Some(html) if !html.is_empty() => {
                        let links =
                            rootsignal_archive::extract_links_by_pattern(&html, url, pattern);
                        info!(url = url.as_str(), links = links.len(), "Query resolved");
                        phase_urls.extend(links);
                    }
                    Some(_) => warn!(url = url.as_str(), "Empty HTML from query source"),
                    None => {} // error already logged above
                }
            }
        }

        // Add page source URLs directly
        for source in &page_sources {
            if let Some(ref url) = source.url {
                phase_urls.push(url.clone());
            }
        }

        // RSS feeds — fetch feed XML, extract article URLs
        let rss_sources: Vec<&&SourceNode> = sources
            .iter()
            .filter(|s| matches!(scraping_strategy(s.value()), ScrapingStrategy::Rss))
            .collect();
        if !rss_sources.is_empty() {
            info!(feeds = rss_sources.len(), "Fetching RSS/Atom feeds...");
            for source in &rss_sources {
                if let Some(ref feed_url) = source.url {
                    let feed_result = fetcher.feed(feed_url).await;
                    match feed_result {
                        Ok(archived) => {
                            for item in archived.items {
                                url_mappings
                                    .entry(item.url.clone())
                                    .or_insert_with(|| source.canonical_key.clone());
                                if let Some(pub_date) = item.pub_date {
                                    pub_dates.insert(item.url.clone(), pub_date);
                                }
                                phase_urls.push(item.url);
                            }
                        }
                        Err(e) => {
                            warn!(feed_url = feed_url.as_str(), error = %e, "RSS feed fetch failed");
                        }
                    }
                }
            }
        }

        // Deduplicate
        phase_urls.sort();
        phase_urls.dedup();

        // Filter blocked URLs (single batch query instead of N sequential checks)
        let blocked = store
            .blocked_urls(&phase_urls)
            .await
            .unwrap_or_default();
        if !blocked.is_empty() {
            for url in &blocked {
                info!(url, "Skipping blocked URL");
            }
        }
        let phase_urls: Vec<String> = phase_urls
            .into_iter()
            .filter(|u| !blocked.contains(u))
            .collect();

        // Domain filter: remove infrastructure URLs (SaaS, e-commerce, SEO spam, etc.)
        // Fail-open: skip filtering when AI or region is unavailable.
        let phase_urls = match (ai, region_name) {
            (Some(ai), Some(region)) => {
                let accepted = domain_filter::filter_domains_batch(
                    &phase_urls,
                    region,
                    ai,
                    &**store,
                )
                .await;
                let before = phase_urls.len();
                let after = accepted.len();
                if before != after {
                    info!(before, after, rejected = before - after, "Domain filter applied to resolved URLs");
                }
                accepted
            }
            _ => phase_urls,
        };

        info!(urls = phase_urls.len(), "Phase URLs to scrape");

        let source_count = sources.len() as u32;

        UrlResolution {
            urls: phase_urls,
            url_mappings,
            pub_dates,
            query_api_errors,
            source_count,
        }
    }
