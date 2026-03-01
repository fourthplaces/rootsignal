//! Scrape-Store-Dedup pipeline stage.
//!
//! Extracted from `scout.rs` — handles URL resolution, page/social scraping,
//! LLM extraction, signal storage, and multi-layer deduplication.
//! Used by both Phase A (tension sources) and Phase B (response sources).

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt};
use tracing::{info, warn};
use uuid::Uuid;

use crate::domains::enrichment::activities::link_promoter::{self, CollectedLink};
use crate::domains::enrichment::activities::quality;
use crate::infra::embedder::TextEmbedder;
use crate::infra::run_log::{EventKind, EventLogger, RunLogger};
use crate::infra::util::{content_hash, sanitize_url};
use crate::core::events::{PipelineEvent, ScoutEvent};
use crate::core::extractor::{ResourceTag, SignalExtractor};
use crate::core::aggregate::ExtractedBatch;
use rootsignal_common::{
    canonical_value, is_web_query, scraping_strategy, ActorContext, DiscoveryMethod, Node,
    NodeType, Post, ScoutScope, ScrapingStrategy, SocialPlatform, SourceNode, SourceRole,
};
pub(crate) use crate::core::aggregate::PipelineState as RunContext;
use rootsignal_common::events::SystemEvent;

// RunContext retired — use PipelineState from crate::core::aggregate instead.

// ---------------------------------------------------------------------------
// ScrapeOutput — accumulated output from a scrape phase
// ---------------------------------------------------------------------------

/// Accumulated output from a scrape phase (web, social, or topic discovery).
/// Replaces direct mutations to PipelineState during scraping.
pub struct ScrapeOutput {
    /// Events to emit (SignalsExtracted, FreshnessConfirmed, etc.)
    pub events: Vec<ScoutEvent>,
    /// New URL→canonical_key mappings discovered during this scrape.
    pub url_mappings: HashMap<String, String>,
    /// Per-source signal counts (canonical_key → count).
    pub source_signal_counts: HashMap<String, u32>,
    /// Canonical keys where the query API errored.
    pub query_api_errors: HashSet<String>,
    /// RSS/Atom pub_dates keyed by article URL.
    pub pub_dates: HashMap<String, DateTime<Utc>>,
    /// Links collected during scraping for promotion.
    pub collected_links: Vec<CollectedLink>,
    /// Expansion queries extracted from signals.
    pub expansion_queries: Vec<String>,
    /// Direct stat mutations not tracked through events.
    pub stats_delta: StatsDelta,
}

/// Direct stat mutations accumulated during scraping.
#[derive(Default)]
pub struct StatsDelta {
    pub social_media_posts: u32,
    pub discovery_posts_found: u32,
    pub discovery_accounts_found: u32,
}

impl ScrapeOutput {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            url_mappings: HashMap::new(),
            source_signal_counts: HashMap::new(),
            query_api_errors: HashSet::new(),
            pub_dates: HashMap::new(),
            collected_links: Vec::new(),
            expansion_queries: Vec::new(),
            stats_delta: StatsDelta::default(),
        }
    }

    /// Take events out, leaving the state-update portion.
    pub fn take_events(&mut self) -> Vec<ScoutEvent> {
        std::mem::take(&mut self.events)
    }

    /// Merge another ScrapeOutput into this one.
    pub fn merge(&mut self, other: ScrapeOutput) {
        self.events.extend(other.events);
        self.url_mappings.extend(other.url_mappings);
        for (k, v) in other.source_signal_counts {
            *self.source_signal_counts.entry(k).or_default() += v;
        }
        self.query_api_errors.extend(other.query_api_errors);
        self.pub_dates.extend(other.pub_dates);
        self.collected_links.extend(other.collected_links);
        self.expansion_queries.extend(other.expansion_queries);
        self.stats_delta.social_media_posts += other.stats_delta.social_media_posts;
        self.stats_delta.discovery_posts_found += other.stats_delta.discovery_posts_found;
        self.stats_delta.discovery_accounts_found += other.stats_delta.discovery_accounts_found;
    }
}

pub(crate) use crate::core::embedding_cache::EmbeddingCache;

// ---------------------------------------------------------------------------
// ScrapeOutcome + helpers
// ---------------------------------------------------------------------------

enum ScrapeOutcome {
    New {
        content: String,
        nodes: Vec<Node>,
        resource_tags: Vec<(Uuid, Vec<ResourceTag>)>,
        signal_tags: Vec<(Uuid, Vec<String>)>,
        author_actors: HashMap<Uuid, String>,
    },
    Unchanged,
    Failed,
}

pub(crate) use crate::domains::signals::activities::dedup_utils::{
    batch_title_dedup, dedup_verdict, is_owned_source, normalize_title, score_and_filter,
    DedupVerdict,
};

// ---------------------------------------------------------------------------
// ScrapePhase — the core scrape-extract-store-dedup pipeline
// ---------------------------------------------------------------------------

pub(crate) struct ScrapePhase {
    store: Arc<dyn crate::traits::SignalReader>,
    extractor: Arc<dyn SignalExtractor>,
    embedder: Arc<dyn TextEmbedder>,
    fetcher: Arc<dyn crate::traits::ContentFetcher>,
    region: ScoutScope,
    run_id: String,
}

impl ScrapePhase {
    pub fn new(
        store: Arc<dyn crate::traits::SignalReader>,
        extractor: Arc<dyn SignalExtractor>,
        embedder: Arc<dyn TextEmbedder>,
        fetcher: Arc<dyn crate::traits::ContentFetcher>,
        region: ScoutScope,
        run_id: String,
    ) -> Self {
        Self {
            store,
            extractor,
            embedder,
            fetcher,
            region,
            run_id,
        }
    }

    /// Collect SourceDiscovered events for discovered sources (no dispatch).
    pub fn register_sources_events(
        sources: Vec<SourceNode>,
        discovered_by: &str,
    ) -> Vec<ScoutEvent> {
        sources
            .into_iter()
            .map(|source| {
                ScoutEvent::Pipeline(PipelineEvent::SourceDiscovered {
                    source,
                    discovered_by: discovered_by.into(),
                })
            })
            .collect()
    }

    /// Scrape a set of web sources: resolve queries → URLs, scrape pages, extract signals.
    /// Returns accumulated `ScrapeOutput` with events and state updates.
    ///
    /// Pure: takes specific inputs, returns output. No state mutation.
    pub async fn run_web(
        &self,
        sources: &[&SourceNode],
        url_to_canonical_key: &HashMap<String, String>,
        actor_contexts: &HashMap<String, ActorContext>,
        run_log: &RunLogger,
    ) -> ScrapeOutput {
        let mut output = ScrapeOutput::new();
        // Local url_to_ck seeded from caller — handles read-after-write cycle
        // (query resolution adds entries that store_signals_events reads later).
        let mut url_to_ck = url_to_canonical_key.clone();
        // Local pub_dates for RSS fallback within this invocation.
        let mut local_pub_dates: HashMap<String, DateTime<Utc>> = HashMap::new();
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

        let mut phase_urls: Vec<String> = Vec::new();

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
            let fetcher = self.fetcher.clone();
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
                        run_log.log(EventKind::SearchQuery {
                            query: query_str.clone(),
                            provider: "serper".to_string(),
                            result_count: archived.results.len() as u32,
                            canonical_key: canonical_key.clone(),
                        });
                        for r in &archived.results {
                            let clean = sanitize_url(&r.url);
                            url_to_ck
                                .entry(clean.clone())
                                .or_insert_with(|| canonical_key.clone());
                            output.url_mappings
                                .entry(clean)
                                .or_insert_with(|| canonical_key.clone());
                        }
                        output.source_signal_counts
                            .entry(canonical_key.clone())
                            .or_default();
                        for r in archived.results {
                            phase_urls.push(r.url);
                        }
                    }
                    Err(e) => {
                        warn!(query_str, error = %e, "Web search failed");
                        output.query_api_errors.insert(canonical_key);
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
                let html = match self.fetcher.page(url).await {
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
                    let feed_result = self.fetcher.feed(feed_url).await;
                    match feed_result {
                        Ok(archived) => {
                            run_log.log(EventKind::ScrapeFeed {
                                url: feed_url.clone(),
                                items: archived.items.len() as u32,
                            });
                            output.source_signal_counts
                                .entry(source.canonical_key.clone())
                                .or_default();
                            for item in archived.items {
                                url_to_ck
                                    .entry(item.url.clone())
                                    .or_insert_with(|| source.canonical_key.clone());
                                output.url_mappings
                                    .entry(item.url.clone())
                                    .or_insert_with(|| source.canonical_key.clone());
                                if let Some(pub_date) = item.pub_date {
                                    local_pub_dates.insert(item.url.clone(), pub_date);
                                    output.pub_dates.insert(item.url.clone(), pub_date);
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
        let blocked = self
            .store
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
        info!(urls = phase_urls.len(), "Phase URLs to scrape");

        if phase_urls.is_empty() {
            return output;
        }

        // Scrape + extract in parallel
        let fetcher = self.fetcher.clone();
        let store = self.store.clone();
        let extractor = self.extractor.clone();
        let pipeline_results: Vec<_> = stream::iter(phase_urls.into_iter().map(|url| {
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
        let ck_to_source_id: HashMap<String, Uuid> = sources
            .iter()
            .map(|s| (s.canonical_key.clone(), s.id))
            .collect();
        for (url, outcome, page_links) in pipeline_results {
            // Extract outbound links for promotion as new sources
            let discovered = link_promoter::extract_links(&page_links);
            for link_url in discovered {
                output.collected_links.push(CollectedLink {
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
                } => {
                    run_log.log(EventKind::ScrapeUrl {
                        url: url.clone(),
                        strategy: "web".to_string(),
                        success: true,
                        content_bytes: content.len(),
                    });

                    // Count implied queries for logging
                    let mut implied_q_count = 0u32;
                    // Collect implied queries from Tension + Need nodes for immediate expansion
                    for node in &nodes {
                        if matches!(node.node_type(), NodeType::Tension | NodeType::Need) {
                            if let Some(meta) = node.meta() {
                                implied_q_count += meta.implied_queries.len() as u32;
                                output.expansion_queries
                                    .extend(meta.implied_queries.iter().cloned());
                            }
                        }
                    }

                    run_log.log(EventKind::LlmExtraction {
                        source_url: url.clone(),
                        content_chars: content.len(),
                        signals_extracted: nodes.len() as u32,
                        implied_queries: implied_q_count,
                    });

                    // Apply RSS/Atom pub_date as fallback published_at
                    if let Some(pub_date) = local_pub_dates.get(&url) {
                        for node in &mut nodes {
                            if let Some(meta) = node.meta_mut() {
                                if meta.published_at.is_none() {
                                    meta.published_at = Some(*pub_date);
                                }
                            }
                        }
                    }

                    let source_id = ck_to_source_id.get(&ck).copied();
                    let events = self.store_signals_events(
                        &url,
                        &content,
                        nodes,
                        resource_tags,
                        signal_tags,
                        &author_actors,
                        &url_to_ck,
                        actor_contexts,
                        source_id,
                    );
                    if !events.is_empty() {
                        output.source_signal_counts.entry(ck).or_default();
                    }
                    output.events.extend(events);
                }
                ScrapeOutcome::Unchanged => {
                    match self.refresh_url_signals_events(&url, now).await {
                        Ok(events) if !events.is_empty() => {
                            info!(url, refreshed = events.len(), "Refreshed unchanged signals");
                            output.events.extend(events);
                        }
                        Ok(_) => {}
                        Err(e) => warn!(url, error = %e, "Failed to refresh signals"),
                    }
                    output.source_signal_counts.entry(ck).or_default();
                }
                ScrapeOutcome::Failed => {
                    run_log.log(EventKind::ScrapeUrl {
                        url: url.clone(),
                        strategy: "web".to_string(),
                        success: false,
                        content_bytes: 0,
                    });
                }
            }
        }
        output
    }

    /// Scrape social media accounts, feed posts through LLM extraction.
    /// Returns accumulated `ScrapeOutput` with events and state updates.
    pub async fn run_social(
        &self,
        social_sources: &[&SourceNode],
        url_to_canonical_key: &HashMap<String, String>,
        actor_contexts: &HashMap<String, ActorContext>,
        run_log: &RunLogger,
    ) -> ScrapeOutput {
        let mut output = ScrapeOutput::new();
        type SocialResult = Option<(
            String,
            String,
            SocialPlatform,
            String,
            Vec<Node>,
            Vec<(Uuid, Vec<ResourceTag>)>,
            Vec<(Uuid, Vec<String>)>,
            HashMap<Uuid, String>,
            usize,
            Vec<String>,
            Option<DateTime<Utc>>, // most recent published_at for published_at fallback
        )>; // (canonical_key, source_url, platform, combined_text, nodes, resource_tags, signal_tags, author_actors, post_count, mentions, newest_published_at)

        // Build uniform list of (canonical_key, source_url, platform, fetch_identifier) from SourceNodes
        struct SocialEntry {
            platform: SocialPlatform,
            identifier: String,
        }
        let mut accounts: Vec<(String, String, SocialEntry)> = Vec::new();

        for source in social_sources {
            let common_platform = match scraping_strategy(source.value()) {
                ScrapingStrategy::Social(p) => p,
                _ => continue,
            };
            let (platform, identifier) = match common_platform {
                SocialPlatform::Instagram => (
                    SocialPlatform::Instagram,
                    source
                        .url
                        .as_deref()
                        .unwrap_or(&source.canonical_value)
                        .to_string(),
                ),
                SocialPlatform::Facebook => {
                    let url = source
                        .url
                        .as_deref()
                        .filter(|u| !u.is_empty())
                        .unwrap_or(&source.canonical_value);
                    (SocialPlatform::Facebook, url.to_string())
                }
                SocialPlatform::Reddit => {
                    let url = source
                        .url
                        .as_deref()
                        .filter(|u| !u.is_empty())
                        .unwrap_or(&source.canonical_value);
                    let identifier = if !url.starts_with("http") {
                        let name = url.trim_start_matches("r/");
                        format!("https://www.reddit.com/r/{}/", name)
                    } else {
                        url.to_string()
                    };
                    (SocialPlatform::Reddit, identifier)
                }
                SocialPlatform::Twitter => (
                    SocialPlatform::Twitter,
                    source
                        .url
                        .as_deref()
                        .unwrap_or(&source.canonical_value)
                        .to_string(),
                ),
                SocialPlatform::TikTok => (
                    SocialPlatform::TikTok,
                    source
                        .url
                        .as_deref()
                        .unwrap_or(&source.canonical_value)
                        .to_string(),
                ),
                SocialPlatform::Bluesky => continue,
            };
            let source_url = source
                .url
                .as_deref()
                .filter(|u| !u.is_empty())
                .unwrap_or(&source.canonical_value)
                .to_string();
            accounts.push((
                source.canonical_key.clone(),
                source_url,
                SocialEntry {
                    platform,
                    identifier,
                },
            ));
        }

        let ig_count = accounts
            .iter()
            .filter(|(_, _, a)| matches!(a.platform, SocialPlatform::Instagram))
            .count();
        let fb_count = accounts
            .iter()
            .filter(|(_, _, a)| matches!(a.platform, SocialPlatform::Facebook))
            .count();
        let reddit_count = accounts
            .iter()
            .filter(|(_, _, a)| matches!(a.platform, SocialPlatform::Reddit))
            .count();
        let twitter_count = accounts
            .iter()
            .filter(|(_, _, a)| matches!(a.platform, SocialPlatform::Twitter))
            .count();
        let tiktok_count = accounts
            .iter()
            .filter(|(_, _, a)| matches!(a.platform, SocialPlatform::TikTok))
            .count();
        info!(
            ig = ig_count,
            fb = fb_count,
            reddit = reddit_count,
            twitter = twitter_count,
            tiktok = tiktok_count,
            "Scraping social media..."
        );

        // Build actor context prefixes for known actor sources
        let actor_prefixes: HashMap<String, String> = accounts
            .iter()
            .filter_map(|(ck, _, _)| {
                actor_contexts.get(ck).map(|ac| {
                    let mut prefix = format!(
                        "ACTOR CONTEXT: This content is from {}", ac.actor_name
                    );
                    if let Some(ref bio) = ac.bio {
                        prefix.push_str(&format!(", {}", bio));
                    }
                    if let Some(ref loc) = ac.location_name {
                        prefix.push_str(&format!(", located in {}", loc));
                    }
                    prefix.push_str(". Use this location as fallback if the post doesn't mention a specific place.\n\n");
                    (ck.clone(), prefix)
                })
            })
            .collect();

        // First-hand filter prefix for non-entity social sources
        let firsthand_filter = "FIRST-HAND FILTER (applies to this content):\n\
            This content comes from platform search results, which are flooded with \
            political commentary from people not directly involved. Apply strict filtering:\n\n\
            For each potential signal, assess: Is this person describing something happening \
            to them, their family, their community, or their neighborhood? Or are they \
            asking for help? If yes, mark is_firsthand: true. If this is political commentary \
            from someone not personally affected — regardless of viewpoint — mark \
            is_firsthand: false.\n\n\
            Signal: \"My family was taken.\" → is_firsthand: true\n\
            Signal: \"There were raids on 5th street today.\" → is_firsthand: true\n\
            Signal: \"We need legal observers.\" → is_firsthand: true\n\
            Noise: \"ICE is doing great work.\" → is_firsthand: false\n\
            Noise: \"The housing crisis is a failure of capitalism.\" → is_firsthand: false\n\n\
            Only extract signals where is_firsthand is true. Reject the rest.\n\n";

        // Collect all futures into a single Vec<Pin<Box<...>>> so types unify
        let mut futures: Vec<Pin<Box<dyn Future<Output = SocialResult> + Send>>> = Vec::new();

        let fetcher = self.fetcher.clone();
        let extractor = self.extractor.clone();
        for (canonical_key, source_url, account) in &accounts {
            let canonical_key = canonical_key.clone();
            let source_url = source_url.clone();
            let platform = account.platform;
            let is_reddit = matches!(platform, SocialPlatform::Reddit);
            let actor_prefix = actor_prefixes.get(&canonical_key).cloned();
            let firsthand_prefix = if actor_prefix.is_none() {
                Some(firsthand_filter.to_string())
            } else {
                None
            };
            let fetcher = fetcher.clone();
            let extractor = extractor.clone();
            let identifier = account.identifier.clone();

            futures.push(Box::pin(async move {
                let posts = match fetcher.posts(&identifier, 20).await {
                    Ok(posts) => posts,
                    Err(e) => {
                        warn!(source_url, error = %e, "Social media scrape failed");
                        return None;
                    }
                };
                let post_count = posts.len();

                // Find the most recent published_at for published_at fallback
                let newest_published_at = posts.iter().filter_map(|p| p.published_at).max();

                // Collect @mentions from posts
                let source_mentions: Vec<String> = posts
                    .iter()
                    .flat_map(|p| p.mentions.iter().cloned())
                    .collect();

                // Format a post header including the specific post URL when available.
                let post_header = |i: usize, p: &Post| -> String {
                    let text = p.text.as_deref().unwrap_or("");
                    match &p.permalink {
                        Some(url) => format!("--- Post {} ({}) ---\n{}", i + 1, url, text),
                        None => format!("--- Post {} ---\n{}", i + 1, text),
                    }
                };

                if is_reddit {
                    // Reddit: batch posts 10 at a time for extraction
                    let batches: Vec<_> = posts.chunks(10).collect();
                    let mut all_nodes = Vec::new();
                    let mut all_resource_tags = Vec::new();
                    let mut all_signal_tags = Vec::new();
                    let mut all_author_actors: HashMap<Uuid, String> = HashMap::new();
                    let mut combined_all = String::new();
                    for batch in batches {
                        let mut combined_text: String = batch
                            .iter()
                            .enumerate()
                            .map(|(i, p)| post_header(i, p))
                            .collect::<Vec<_>>()
                            .join("\n\n");
                        if combined_text.is_empty() {
                            continue;
                        }
                        // Prepend entity context for known actor sources,
                        // or first-hand filter for non-entity sources
                        if let Some(ref prefix) = actor_prefix {
                            combined_text = format!("{prefix}{combined_text}");
                        } else if let Some(ref prefix) = firsthand_prefix {
                            combined_text = format!("{prefix}{combined_text}");
                        }
                        combined_all.push_str(&combined_text);
                        match extractor.extract(&combined_text, &source_url).await {
                            Ok(result) => {
                                all_nodes.extend(result.nodes);
                                all_resource_tags.extend(result.resource_tags);
                                all_signal_tags.extend(result.signal_tags);
                                all_author_actors.extend(result.author_actors);
                            }
                            Err(e) => {
                                warn!(source_url, error = %e, "Reddit extraction failed");
                            }
                        }
                    }
                    if all_nodes.is_empty() {
                        return None;
                    }
                    info!(source_url, posts = post_count, "Reddit scrape complete");
                    Some((
                        canonical_key,
                        source_url,
                        platform,
                        combined_all,
                        all_nodes,
                        all_resource_tags,
                        all_signal_tags,
                        all_author_actors,
                        post_count,
                        source_mentions,
                        newest_published_at,
                    ))
                } else {
                    // Instagram/Facebook/Twitter/TikTok: combine all posts then extract
                    let mut combined_text: String = posts
                        .iter()
                        .enumerate()
                        .map(|(i, p)| post_header(i, p))
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    if combined_text.is_empty() {
                        return None;
                    }
                    // Prepend entity context for known actor sources,
                    // or first-hand filter for non-entity sources
                    if let Some(ref prefix) = actor_prefix {
                        combined_text = format!("{prefix}{combined_text}");
                    } else if let Some(ref prefix) = firsthand_prefix {
                        combined_text = format!("{prefix}{combined_text}");
                    }
                    let result = match extractor.extract(&combined_text, &source_url).await {
                        Ok(r) => r,
                        Err(e) => {
                            warn!(source_url, error = %e, "Social extraction failed");
                            return None;
                        }
                    };
                    info!(source_url, posts = post_count, "Social scrape complete");
                    Some((
                        canonical_key,
                        source_url,
                        platform,
                        combined_text,
                        result.nodes,
                        result.resource_tags,
                        result.signal_tags,
                        result.author_actors.into_iter().collect(),
                        post_count,
                        source_mentions,
                        newest_published_at,
                    ))
                }
            }));
        }

        let results: Vec<_> = stream::iter(futures).buffer_unordered(10).collect().await;

        let promotion_config = link_promoter::PromotionConfig::default();
        let ck_to_source_id: HashMap<String, Uuid> = social_sources
            .iter()
            .map(|s| (s.canonical_key.clone(), s.id))
            .collect();
        for result in results.into_iter().flatten() {
            let (
                canonical_key,
                source_url,
                result_platform,
                combined_text,
                mut nodes,
                resource_tags,
                signal_tags,
                author_actors,
                post_count,
                mentions,
                newest_published_at,
            ) = result;

            // Apply social published_at as fallback published_at when LLM didn't extract one
            if let Some(pub_at) = newest_published_at {
                for node in &mut nodes {
                    if let Some(meta) = node.meta_mut() {
                        if meta.published_at.is_none() {
                            meta.published_at = Some(pub_at);
                        }
                    }
                }
            }

            // Accumulate mentions as URLs for promotion (capped per source)
            for handle in mentions.into_iter().take(promotion_config.max_per_source) {
                let mention_url = link_promoter::platform_url(&result_platform, &handle);
                output.collected_links.push(CollectedLink {
                    url: mention_url,
                    discovered_on: source_url.clone(),
                });
            }

            run_log.log(EventKind::SocialScrape {
                platform: "social".to_string(),
                identifier: source_url.clone(),
                post_count: post_count as u32,
            });

            run_log.log(EventKind::LlmExtraction {
                source_url: source_url.clone(),
                content_chars: combined_text.len(),
                signals_extracted: nodes.len() as u32,
                implied_queries: 0,
            });

            // Collect implied queries from Tension/Need social signals
            for node in &nodes {
                if matches!(node.node_type(), NodeType::Tension | NodeType::Need) {
                    if let Some(meta) = node.meta() {
                        output.expansion_queries
                            .extend(meta.implied_queries.iter().cloned());
                    }
                }
            }
            output.stats_delta.social_media_posts += post_count as u32;
            let source_id = ck_to_source_id.get(&canonical_key).copied();
            let events = self.store_signals_events(
                &source_url,
                &combined_text,
                nodes,
                resource_tags,
                signal_tags,
                &author_actors,
                url_to_canonical_key,
                actor_contexts,
                source_id,
            );
            output.source_signal_counts.entry(canonical_key).or_default();
            output.events.extend(events);
        }
        output
    }

    /// Discover new accounts by searching platform-agnostic topics (hashtags/keywords)
    /// across Instagram, X/Twitter, TikTok, and GoFundMe.
    pub async fn discover_from_topics(
        &self,
        topics: &[String],
        url_to_canonical_key: &HashMap<String, String>,
        actor_contexts: &HashMap<String, ActorContext>,
        run_log: &RunLogger,
    ) -> ScrapeOutput {
        let mut output = ScrapeOutput::new();
        const MAX_SOCIAL_SEARCHES: usize = 10;
        const MAX_NEW_ACCOUNTS: usize = 10;
        const POSTS_PER_SEARCH: u32 = 30;
        const MAX_SITE_SEARCH_TOPICS: usize = 4;
        const SITE_SEARCH_RESULTS: usize = 5;

        if topics.is_empty() {
            return output;
        }

        info!(topics = ?topics, "Starting social topic discovery...");

        let known_urls: HashSet<String> = url_to_canonical_key.keys().cloned().collect();

        // Load existing sources for dedup across all platforms
        let existing_sources = self.store.get_active_sources().await.unwrap_or_default();
        let existing_canonical_values: HashSet<String> = existing_sources
            .iter()
            .map(|s| s.canonical_value.clone())
            .collect();

        let mut new_accounts = 0u32;
        let mut new_sources: Vec<SourceNode> = Vec::new();
        let topic_strs: Vec<&str> = topics
            .iter()
            .take(MAX_SOCIAL_SEARCHES)
            .map(|t| t.as_str())
            .collect();

        // Search each social platform with the same topics
        let platform_urls: &[(&str, &str)] = &[
            ("instagram", "https://www.instagram.com/topics"),
            ("x", "https://x.com/topics"),
            ("tiktok", "https://www.tiktok.com/topics"),
            ("reddit", "https://www.reddit.com/topics"),
        ];

        for &(platform_name, platform_url) in platform_urls {
            if new_accounts >= MAX_NEW_ACCOUNTS as u32 {
                break;
            }

            let discovered_posts = match self
                .fetcher
                .search_topics(platform_url, &topic_strs, POSTS_PER_SEARCH)
                .await
            {
                Ok(posts) => posts,
                Err(e) => {
                    warn!(platform = platform_name, error = %e, "Topic discovery failed for platform");
                    continue;
                }
            };

            if discovered_posts.is_empty() {
                info!(
                    platform = platform_name,
                    "No posts found from topic discovery"
                );
                continue;
            }

            run_log.log(EventKind::SocialTopicSearch {
                platform: platform_name.to_string(),
                topics: topic_strs.iter().map(|t| t.to_string()).collect(),
                posts_found: discovered_posts.len() as u32,
            });

            output.stats_delta.discovery_posts_found += discovered_posts.len() as u32;

            // Group posts by author
            let mut by_author: HashMap<String, Vec<&Post>> = HashMap::new();
            for post in &discovered_posts {
                if let Some(ref author) = post.author {
                    by_author.entry(author.clone()).or_default().push(post);
                }
            }

            info!(
                platform = platform_name,
                posts = discovered_posts.len(),
                unique_authors = by_author.len(),
                "Topic discovery posts grouped by author"
            );

            let platform_enum = match platform_name {
                "instagram" => Some(SocialPlatform::Instagram),
                "x" => Some(SocialPlatform::Twitter),
                "tiktok" => Some(SocialPlatform::TikTok),
                "reddit" => Some(SocialPlatform::Reddit),
                _ => None,
            };

            for (username, posts) in &by_author {
                if new_accounts >= MAX_NEW_ACCOUNTS as u32 {
                    info!("Discovery account budget exhausted");
                    break;
                }

                // Platform-aware source URL
                let source_url = match platform_name {
                    "instagram" => format!("https://www.instagram.com/{username}/"),
                    "x" => format!("https://x.com/{username}"),
                    "tiktok" => format!("https://www.tiktok.com/@{username}"),
                    "reddit" => format!("https://www.reddit.com/user/{username}/"),
                    _ => continue,
                };

                // Skip already-known sources
                if existing_canonical_values.contains(&username.to_string()) {
                    continue;
                }

                // Concatenate post content for extraction
                let combined_text: String = posts
                    .iter()
                    .enumerate()
                    .filter_map(|(i, p)| {
                        let text = p.text.as_deref()?;
                        Some(match &p.permalink {
                            Some(url) => format!("--- Post {} ({}) ---\n{}", i + 1, url, text),
                            None => format!("--- Post {} ---\n{}", i + 1, text),
                        })
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n");

                if combined_text.is_empty() {
                    continue;
                }

                // Extract signals via LLM
                let result = match self.extractor.extract(&combined_text, &source_url).await {
                    Ok(r) => r,
                    Err(e) => {
                        warn!(username, platform = platform_name, error = %e, "Discovery extraction failed");
                        continue;
                    }
                };

                if result.nodes.is_empty() {
                    continue; // No signal found — don't follow this person
                }

                // Store signals through normal pipeline
                let author_actors: HashMap<Uuid, String> =
                    result.author_actors.into_iter().collect();
                let events = self.store_signals_events(
                    &source_url,
                    &combined_text,
                    result.nodes,
                    result.resource_tags,
                    result.signal_tags,
                    &author_actors,
                    url_to_canonical_key,
                    actor_contexts,
                    None,
                );
                let produced = events.len() as u32;
                output.events.extend(events);

                // Only follow mentions from authors whose posts produced signals
                if produced > 0 {
                    if let Some(ref sp) = platform_enum {
                        for post in posts {
                            for handle in post.mentions.iter().take(5) {
                                let mention_url = link_promoter::platform_url(sp, handle);
                                output.collected_links.push(CollectedLink {
                                    url: mention_url,
                                    discovered_on: source_url.clone(),
                                });
                            }
                        }
                    }
                }

                // Create a Source node with correct platform type
                let cv = rootsignal_common::canonical_value(&source_url);
                let ck = canonical_value(&source_url);
                let gap_context = format!(
                    "Topic: {}",
                    topics.first().map(|t| t.as_str()).unwrap_or("unknown")
                );
                let source = SourceNode {
                    last_scraped: Some(Utc::now()),
                    last_produced_signal: if produced > 0 { Some(Utc::now()) } else { None },
                    signals_produced: produced,
                    ..SourceNode::new(
                        ck.clone(),
                        cv,
                        Some(source_url.clone()),
                        DiscoveryMethod::HashtagDiscovery,
                        0.3,
                        SourceRole::default(),
                        Some(gap_context),
                    )
                };

                *output.source_signal_counts.entry(ck).or_default() += produced;

                new_sources.push(source);
                new_accounts += 1;
                info!(
                    username,
                    platform = platform_name,
                    signals = produced,
                    "Discovered new account via topic search"
                );
            }
        }

        // Site-scoped search: find WebQuery sources with `site:` prefix,
        // search Serper for each topic, scrape + extract results.
        let site_sources: Vec<&SourceNode> = existing_sources
            .iter()
            .filter(|s| is_web_query(&s.canonical_value) && s.canonical_value.starts_with("site:"))
            .collect();

        for source in &site_sources {
            let site_prefix = &source.canonical_value; // e.g. "site:gofundme.com/f/ Minneapolis"
            for topic in topics.iter().take(MAX_SITE_SEARCH_TOPICS) {
                let query = format!("{} {}", site_prefix, topic);

                let search_results =
                    match self.fetcher.site_search(&query, SITE_SEARCH_RESULTS).await {
                        Ok(r) => r,
                        Err(e) => {
                            warn!(query, error = %e, "Site-scoped search failed");
                            continue;
                        }
                    };

                if search_results.results.is_empty() {
                    continue;
                }

                info!(
                    query,
                    count = search_results.results.len(),
                    "Site-scoped search results"
                );

                for result in &search_results.results {
                    if known_urls.contains(&result.url) {
                        continue;
                    }

                    let page = match self.fetcher.page(&result.url).await {
                        Ok(p) => p,
                        Err(e) => {
                            warn!(url = result.url.as_str(), error = %e, "Site-scoped scrape failed");
                            continue;
                        }
                    };
                    if page.markdown.is_empty() {
                        continue;
                    }
                    let content = page.markdown;

                    let extracted = match self.extractor.extract(&content, &result.url).await {
                        Ok(r) => r,
                        Err(e) => {
                            warn!(url = result.url, error = %e, "Site-scoped extraction failed");
                            continue;
                        }
                    };

                    if extracted.nodes.is_empty() {
                        continue;
                    }

                    let author_actors: HashMap<Uuid, String> =
                        extracted.author_actors.into_iter().collect();
                    let events = self.store_signals_events(
                        &result.url,
                        &content,
                        extracted.nodes,
                        extracted.resource_tags,
                        extracted.signal_tags,
                        &author_actors,
                        url_to_canonical_key,
                        actor_contexts,
                        None,
                    );
                    output.events.extend(events);
                }
            }
        }

        // Collect source discovery events
        if !new_sources.is_empty() {
            output.events.extend(Self::register_sources_events(new_sources, "topic_discovery"));
        }

        output.stats_delta.discovery_accounts_found = new_accounts;
        info!(
            topics = topics.len(),
            new_accounts, "Social topic discovery complete"
        );
        output
    }

    // -----------------------------------------------------------------------
    // store_signals — multi-layer dedup + graph storage (private)
    // -----------------------------------------------------------------------

    /// Collect events for extracted signals (no dispatch).
    /// Returns a SignalsExtracted event wrapping an ExtractedBatch.
    ///
    /// Pure: reads from the provided maps, does not mutate any shared state.
    fn store_signals_events(
        &self,
        url: &str,
        content: &str,
        nodes: Vec<Node>,
        resource_tags: Vec<(Uuid, Vec<ResourceTag>)>,
        signal_tags: Vec<(Uuid, Vec<String>)>,
        author_actors: &HashMap<Uuid, String>,
        url_to_canonical_key: &HashMap<String, String>,
        actor_contexts: &HashMap<String, ActorContext>,
        source_id: Option<Uuid>,
    ) -> Vec<ScoutEvent> {
        let url = sanitize_url(url);
        let raw_count = nodes.len() as u32;

        // Score quality, populate from/about locations, remove Evidence nodes
        let ck_for_fallback = url_to_canonical_key
            .get(&url)
            .cloned()
            .unwrap_or_else(|| url.clone());
        let actor_ctx = actor_contexts.get(&ck_for_fallback);
        let nodes = score_and_filter(nodes, &url, actor_ctx);

        if nodes.is_empty() {
            return Vec::new();
        }

        // Layer 1: Within-batch dedup by (normalized_title, node_type)
        let nodes = batch_title_dedup(nodes);

        let canonical_key = url_to_canonical_key
            .get(&url)
            .cloned()
            .unwrap_or_else(|| url.clone());

        let batch = ExtractedBatch {
            content: content.to_string(),
            nodes,
            resource_tags: resource_tags.into_iter().collect(),
            signal_tags: signal_tags.into_iter().collect(),
            author_actors: author_actors.clone(),
            source_id,
        };

        vec![ScoutEvent::Pipeline(PipelineEvent::SignalsExtracted {
            url,
            canonical_key,
            count: raw_count,
            batch: Box::new(batch),
        })]
    }

    /// Collect FreshnessConfirmed events for unchanged URLs (no dispatch).
    async fn refresh_url_signals_events(
        &self,
        url: &str,
        now: DateTime<Utc>,
    ) -> Result<Vec<ScoutEvent>> {

        let all_ids = self.store.signal_ids_for_url(url).await?;
        if all_ids.is_empty() {
            return Ok(Vec::new());
        }

        // Group by NodeType for batch FreshnessConfirmed events
        let mut by_type: HashMap<NodeType, Vec<Uuid>> = HashMap::new();
        for (id, nt) in &all_ids {
            by_type.entry(*nt).or_default().push(*id);
        }

        let events: Vec<ScoutEvent> = by_type
            .into_iter()
            .map(|(node_type, ids)| {
                ScoutEvent::System(SystemEvent::FreshnessConfirmed {
                    signal_ids: ids,
                    node_type,
                    confirmed_at: now,
                })
            })
            .collect();
        Ok(events)
    }
}

// Tests for extracted dedup/score utilities are in domains::signals::activities::dedup_utils.
// Tests for ScrapePhase integration are in boundary_tests.rs and chain_tests.rs.
#[cfg(test)]
mod _placeholder_end {}

