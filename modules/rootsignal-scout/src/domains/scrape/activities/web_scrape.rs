//! Web scraping: fetch pages, extract signals, process results.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt};
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::ActorContext;
#[cfg(test)]
use rootsignal_common::SourceNode;
use seesaw_core::Events;

use crate::core::aggregate::ExtractedBatch;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::enrichment::activities::link_promoter::{self, CollectedLink};
use crate::infra::util::{content_hash, sanitize_url};

use super::signal_events::refresh_url_signals_events;
use super::types::{batch_title_dedup, score_and_filter, FetchExtractResult, FetchExtractStats, ScrapeOutcome, UrlExtraction};
#[cfg(test)]
use super::types::ScrapeOutput;

/// Scrape a set of web sources: resolve queries → URLs, scrape pages, extract signals.
/// Returns accumulated `ScrapeOutput` with events and state updates.
///
/// Test convenience: combines `resolve_web_urls` + `fetch_and_extract` into one call.
/// Production handlers call those two steps separately to emit intermediate events.
#[cfg(test)]
pub(crate) async fn scrape_web_sources(
    deps: &ScoutEngineDeps,
    sources: &[&SourceNode],
    url_to_canonical_key: &HashMap<String, String>,
    actor_contexts: &HashMap<String, ActorContext>,
) -> ScrapeOutput {
    let resolution = super::url_resolution::resolve_web_urls(deps, sources, url_to_canonical_key).await;

    // Build merged url_to_ck for fetch_and_extract
    let mut url_to_ck = url_to_canonical_key.clone();
    url_to_ck.extend(resolution.url_mappings.iter().map(|(k, v)| (k.clone(), v.clone())));

    let source_keys: HashMap<String, Uuid> = sources
        .iter()
        .map(|s| (s.canonical_key.clone(), s.id))
        .collect();

    let fetch_result = fetch_and_extract(
        deps,
        &resolution.urls,
        &source_keys,
        &url_to_ck,
        actor_contexts,
        &resolution.pub_dates,
    ).await;

    let mut output = ScrapeOutput::new();
    output.url_mappings = resolution.url_mappings;
    output.pub_dates = resolution.pub_dates;
    output.query_api_errors = resolution.query_api_errors;
    output.events = fetch_result.events;
    output.source_signal_counts = fetch_result.source_signal_counts;
    output.collected_links = fetch_result.collected_links;
    output.expansion_queries = fetch_result.expansion_queries;
    output.extracted_batches = fetch_result.extracted_batches;
    output
}

/// Fetch and extract signals from resolved URLs in parallel.
/// Collects extracted batches and freshness events per URL.
pub(crate) async fn fetch_and_extract(
    deps: &ScoutEngineDeps,
    urls: &[String],
    source_keys: &HashMap<String, Uuid>,
    url_to_ck: &HashMap<String, String>,
    actor_contexts: &HashMap<String, ActorContext>,
    pub_dates: &HashMap<String, DateTime<Utc>>,
) -> FetchExtractResult {
        let mut result = FetchExtractResult {
            events: Events::new(),
            source_signal_counts: HashMap::new(),
            collected_links: Vec::new(),
            expansion_queries: Vec::new(),
            stats: FetchExtractStats::default(),
            page_previews: HashMap::new(),
            extracted_batches: Vec::new(),
        };

        if urls.is_empty() {
            return result;
        }

        // Scrape + extract in parallel
        let fetcher = deps.fetcher.as_ref().expect("fetcher required").clone();
        let store = deps.store.clone();
        let extractor = deps.extractor.as_ref().expect("extractor required").clone();
        let pipeline_results: Vec<_> = stream::iter(urls.iter().cloned().map(|url| {
            let fetcher = fetcher.clone();
            let store = store.clone();
            let extractor = extractor.clone();
            async move {
                let clean_url = sanitize_url(&url);

                let (content, page_links) = match fetcher.page(&url).await {
                    Ok(p) if !p.markdown.is_empty() => (p.markdown, p.links),
                    Ok(p) => return (clean_url, ScrapeOutcome::Failed, p.links),
                    Err(e) => {
                        warn!(url, error = %e, "Scrape failed");
                        return (clean_url, ScrapeOutcome::Failed, Vec::new());
                    }
                };

                let hash = format!("{:x}", content_hash(&content));
                match store.content_already_processed(&hash, &clean_url).await {
                    Ok(true) => {
                        info!(url = clean_url.as_str(), "Content unchanged, skipping extraction");
                        return (clean_url, ScrapeOutcome::Unchanged, page_links);
                    }
                    Ok(false) => {}
                    Err(e) => {
                        warn!(url = clean_url.as_str(), error = %e, "Hash check failed, proceeding with extraction");
                    }
                }

                // Prepend first-hand filter for web search/feed sources
                let filtered_content = format!(
                    "FIRST-HAND FILTER (applies to this content):\n\
                    This content comes from web search results, which may contain \
                    political commentary from people not directly involved. Apply strict filtering:\n\n\
                    For each potential signal, assess: Is this person describing something happening \
                    to them, their family, their community, or their neighborhood? Or are they \
                    asking for help? If yes, mark is_firsthand: true. If this is political commentary \
                    from someone not personally affected — regardless of viewpoint — mark \
                    is_firsthand: false.\n\n\
                    Only extract signals where is_firsthand is true. Reject the rest.\n\n\
                    {content}"
                );

                match extractor.extract(&filtered_content, &clean_url).await {
                    Ok(result) => (
                        clean_url,
                        ScrapeOutcome::New {
                            content,
                            nodes: result.nodes,
                            resource_tags: result.resource_tags,
                            signal_tags: result.signal_tags,
                            author_actors: result.author_actors.into_iter().collect(),
                            logs: result.logs,
                        },
                        page_links,
                    ),
                    Err(e) => {
                        warn!(url = clean_url.as_str(), error = %e, "Extraction failed");
                        (clean_url, ScrapeOutcome::Failed, page_links)
                    }
                }
            }
        }))
        .buffer_unordered(6)
        .collect()
        .await;

        // Process results
        let now = Utc::now();
        for (url, outcome, page_links) in pipeline_results {
            // Extract outbound links for promotion as new sources
            let discovered = link_promoter::extract_links(&page_links, false);
            for link_url in discovered {
                result.collected_links.push(CollectedLink {
                    url: link_url,
                    discovered_on: url.clone(),
                });
            }

            let ck = url_to_ck
                .get(&url)
                .cloned()
                .unwrap_or_else(|| url.clone());
            match outcome {
                ScrapeOutcome::New {
                    content,
                    mut nodes,
                    resource_tags,
                    signal_tags,
                    author_actors,
                    logs,
                } => {
                    result.stats.urls_scraped += 1;

                    // Stash content preview for downstream page triage
                    let preview: String = content.chars().take(500).collect();
                    result.page_previews.insert(url.clone(), preview);

                    // Collect implied queries from Concern + HelpRequest nodes
                    let implied = super::shared::collect_implied_queries(&nodes);
                    result.expansion_queries.extend(implied);

                    result.stats.signals_extracted += nodes.len() as u32;

                    // Apply RSS/Atom pub_date as fallback published_at
                    if let Some(pub_date) = pub_dates.get(&url) {
                        super::shared::apply_published_at_fallback(&mut nodes, *pub_date);
                    }

                    let source_id = source_keys.get(&ck).copied();

                    // Score quality, populate from/about locations, remove Evidence nodes
                    let actor_ctx = actor_contexts.get(&ck);
                    let nodes = score_and_filter(nodes, &url, actor_ctx);

                    if !nodes.is_empty() {
                        // Within-batch dedup by (normalized_title, node_type)
                        let nodes = batch_title_dedup(nodes);

                        let canonical_key = url_to_ck
                            .get(&url)
                            .cloned()
                            .unwrap_or_else(|| url.clone());

                        let batch = ExtractedBatch {
                            content,
                            nodes,
                            resource_tags: resource_tags.into_iter().collect(),
                            signal_tags: signal_tags.into_iter().collect(),
                            author_actors,
                            source_id,
                        };

                        result.source_signal_counts.entry(ck).or_default();
                        result.extracted_batches.push(UrlExtraction {
                            url: url.clone(),
                            canonical_key,
                            batch,
                        });
                    }

                    for log in logs {
                        result.events.push(log);
                    }
                }
                ScrapeOutcome::Unchanged => {
                    result.stats.urls_unchanged += 1;
                    match refresh_url_signals_events(&*store, &url, now).await {
                        Ok(events) if !events.is_empty() => {
                            info!(url, refreshed = events.len(), "Refreshed unchanged signals");
                            result.events.extend(events);
                        }
                        Ok(_) => {}
                        Err(e) => warn!(url, error = %e, "Failed to refresh signals"),
                    }
                    result.source_signal_counts.entry(ck).or_default();
                }
                ScrapeOutcome::Failed => {
                    result.stats.urls_failed += 1;
                }
            }
        }
        result
    }

