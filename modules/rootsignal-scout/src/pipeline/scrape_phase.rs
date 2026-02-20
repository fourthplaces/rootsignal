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
use chrono::Utc;
use futures::stream::{self, StreamExt};
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{
    is_web_query, scraping_strategy, ActorNode, ActorType, RegionNode, DiscoveryMethod, EvidenceNode,
    GeoPoint, GeoPrecision, Node, NodeType, ScrapingStrategy, SocialPlatform as CommonSocialPlatform,
    SourceNode, SourceRole,
};
use rootsignal_graph::GraphWriter;

use crate::embedder::TextEmbedder;
use crate::extractor::{ResourceTag, SignalExtractor};
use crate::quality;
use crate::scout::ScoutStats;
use crate::scraper::{
    self, PageScraper, RssFetcher, SocialAccount, SocialPlatform, SocialPost, SocialScraper,
    WebSearcher,
};
use crate::sources;
use crate::util::{content_hash, sanitize_url};

// ---------------------------------------------------------------------------
// RunContext — shared mutable state for the entire scout run
// ---------------------------------------------------------------------------

/// Shared mutable state that flows across all pipeline stages within a single
/// scout run. Analogous to a Redux store — single source of truth, every stage
/// reads from and writes to it.
pub(crate) struct RunContext {
    pub embed_cache: EmbeddingCache,
    pub url_to_canonical_key: HashMap<String, String>,
    pub source_signal_counts: HashMap<String, u32>,
    pub expansion_queries: Vec<String>,
    pub stats: ScoutStats,
    pub query_api_errors: HashSet<String>,
}

impl RunContext {
    pub fn new(sources: &[SourceNode]) -> Self {
        let url_to_canonical_key = sources
            .iter()
            .filter_map(|s| {
                s.url
                    .as_ref()
                    .map(|u| (sanitize_url(u), s.canonical_key.clone()))
            })
            .collect();
        Self {
            embed_cache: EmbeddingCache::new(),
            url_to_canonical_key,
            source_signal_counts: HashMap::new(),
            expansion_queries: Vec::new(),
            stats: ScoutStats::default(),
            query_api_errors: HashSet::new(),
        }
    }

    /// Rebuild known_city_urls from current URL map state.
    /// Must be called before each social scrape to capture
    /// URLs resolved during the preceding web scrape.
    pub fn known_city_urls(&self) -> HashSet<String> {
        self.url_to_canonical_key.keys().cloned().collect()
    }
}

// ---------------------------------------------------------------------------
// EmbeddingCache — in-memory cross-batch dedup
// ---------------------------------------------------------------------------

/// In-memory embedding cache for the current scout run.
/// Catches duplicates that haven't been indexed in the graph yet (e.g. Instagram
/// and Facebook posts from the same org processed in the same batch).
pub(crate) struct EmbeddingCache {
    entries: Vec<CacheEntry>,
}

struct CacheEntry {
    embedding: Vec<f32>,
    node_id: Uuid,
    node_type: NodeType,
    source_url: String,
}

impl EmbeddingCache {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Find the best match above threshold. Returns (node_id, node_type, source_url, similarity).
    fn find_match(&self, embedding: &[f32], threshold: f64) -> Option<(Uuid, NodeType, &str, f64)> {
        let mut best: Option<(Uuid, NodeType, &str, f64)> = None;
        for entry in &self.entries {
            let sim = cosine_similarity_f32(embedding, &entry.embedding);
            if sim >= threshold && best.as_ref().is_none_or(|b| sim > b.3) {
                best = Some((entry.node_id, entry.node_type, &entry.source_url, sim));
            }
        }
        best
    }

    fn add(
        &mut self,
        embedding: Vec<f32>,
        node_id: Uuid,
        node_type: NodeType,
        source_url: String,
    ) {
        self.entries.push(CacheEntry {
            embedding,
            node_id,
            node_type,
            source_url,
        });
    }
}

/// Cosine similarity for f32 embedding vectors (Voyage AI).
fn cosine_similarity_f32(a: &[f32], b: &[f32]) -> f64 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    (dot / (norm_a * norm_b)) as f64
}

// ---------------------------------------------------------------------------
// ScrapeOutcome + helpers
// ---------------------------------------------------------------------------

enum ScrapeOutcome {
    New {
        content: String,
        nodes: Vec<Node>,
        resource_tags: Vec<(Uuid, Vec<ResourceTag>)>,
        signal_tags: Vec<(Uuid, Vec<String>)>,
    },
    Unchanged,
    Failed,
}

/// Normalize a title for dedup comparison: lowercase and trim.
fn normalize_title(title: &str) -> String {
    title.trim().to_lowercase()
}

fn node_meta_mut(node: &mut Node) -> Option<&mut rootsignal_common::NodeMeta> {
    match node {
        Node::Gathering(n) => Some(&mut n.meta),
        Node::Aid(n) => Some(&mut n.meta),
        Node::Need(n) => Some(&mut n.meta),
        Node::Notice(n) => Some(&mut n.meta),
        Node::Tension(n) => Some(&mut n.meta),
        Node::Evidence(_) => None,
    }
}

// ---------------------------------------------------------------------------
// ScrapePhase — the core scrape-extract-store-dedup pipeline
// ---------------------------------------------------------------------------

pub(crate) struct ScrapePhase<'a> {
    writer: &'a GraphWriter,
    extractor: &'a dyn SignalExtractor,
    embedder: &'a dyn TextEmbedder,
    scraper: Arc<dyn PageScraper>,
    searcher: Arc<dyn WebSearcher>,
    social: &'a dyn SocialScraper,
    region: &'a RegionNode,
    run_id: String,
}

impl<'a> ScrapePhase<'a> {
    pub fn new(
        writer: &'a GraphWriter,
        extractor: &'a dyn SignalExtractor,
        embedder: &'a dyn TextEmbedder,
        scraper: Arc<dyn PageScraper>,
        searcher: Arc<dyn WebSearcher>,
        social: &'a dyn SocialScraper,
        region: &'a RegionNode,
        run_id: String,
    ) -> Self {
        Self {
            writer,
            extractor,
            embedder,
            scraper,
            searcher,
            social,
            region,
            run_id,
        }
    }

    /// Scrape a set of web sources: resolve queries → URLs, scrape pages, extract signals, store results.
    /// Used by both Phase A (tension/mixed sources) and Phase B (response/discovery sources).
    ///
    /// Mutates `ctx.url_to_canonical_key` (resolved WebQuery URLs are inserted so
    /// scrape results can be attributed back to the originating query source).
    ///
    /// Query API errors are inserted into `ctx.query_api_errors`. These queries
    /// should NOT be counted as empty scrapes — the query was never executed.
    pub async fn run_web(&self, sources: &[&SourceNode], ctx: &mut RunContext) {
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
            let search_results: Vec<_> = stream::iter(api_queries.iter().map(|source| {
                let query_str = source.canonical_value.clone();
                let canonical_key = source.canonical_key.clone();
                let searcher = &self.searcher;
                async move {
                    (
                        canonical_key,
                        query_str.clone(),
                        searcher.search(&query_str, 5).await,
                    )
                }
            }))
            .buffer_unordered(5)
            .collect()
            .await;

            for (canonical_key, query_str, result) in search_results {
                match result {
                    Ok(results) => {
                        // Map each resolved URL back to the query's canonical_key
                        // so scrape results get attributed to the originating query.
                        for r in &results {
                            let clean = sanitize_url(&r.url);
                            ctx.url_to_canonical_key
                                .entry(clean)
                                .or_insert_with(|| canonical_key.clone());
                        }
                        // Ensure the query source gets a source_signal_counts entry
                        // even if all its URLs end up deduped/empty (records a scrape).
                        ctx.source_signal_counts
                            .entry(canonical_key.clone())
                            .or_default();
                        for r in results {
                            phase_urls.push(r.url);
                        }
                    }
                    Err(e) => {
                        warn!(query_str, error = %e, "Web search failed");
                        ctx.query_api_errors.insert(canonical_key);
                    }
                }
            }
        }

        // HTML-based queries
        let html_queries: Vec<&&&SourceNode> = query_sources
            .iter()
            .filter(|s| matches!(scraping_strategy(s.value()), ScrapingStrategy::HtmlListing { .. }))
            .collect();
        for source in &html_queries {
            let link_pattern = match scraping_strategy(source.value()) {
                ScrapingStrategy::HtmlListing { link_pattern } => Some(link_pattern),
                _ => None,
            };
            if let (Some(url), Some(pattern)) = (&source.url, link_pattern) {
                match self.scraper.scrape_raw(url).await {
                    Ok(html) if !html.is_empty() => {
                        let links = scraper::extract_links_by_pattern(&html, url, pattern);
                        info!(url = url.as_str(), links = links.len(), "Query resolved");
                        phase_urls.extend(links);
                    }
                    Ok(_) => warn!(url = url.as_str(), "Empty HTML from query source"),
                    Err(e) => warn!(url = url.as_str(), error = %e, "Query scrape failed"),
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
            let fetcher = RssFetcher::new();
            for source in &rss_sources {
                if let Some(ref feed_url) = source.url {
                    match fetcher.fetch_items(feed_url).await {
                        Ok(items) => {
                            // Ensure the RSS source gets a source_signal_counts entry
                            ctx.source_signal_counts
                                .entry(source.canonical_key.clone())
                                .or_default();
                            for item in items {
                                ctx.url_to_canonical_key
                                    .entry(item.url.clone())
                                    .or_insert_with(|| source.canonical_key.clone());
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
            .writer
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
            return;
        }

        // Scrape + extract in parallel
        let scraper = &self.scraper;
        let writer = &self.writer;
        let extractor = &self.extractor;
        let pipeline_results: Vec<_> = stream::iter(phase_urls.iter().map(|url| {
            let url = url.clone();
            async move {
                let clean_url = sanitize_url(&url);

                let content = match scraper.scrape(&url).await {
                    Ok(c) if !c.is_empty() => c,
                    Ok(_) => return (clean_url, ScrapeOutcome::Failed),
                    Err(e) => {
                        warn!(url, error = %e, "Scrape failed");
                        return (clean_url, ScrapeOutcome::Failed);
                    }
                };

                let hash = format!("{:x}", content_hash(&content));
                match writer.content_already_processed(&hash, &clean_url).await {
                    Ok(true) => {
                        info!(url = clean_url.as_str(), "Content unchanged, skipping extraction");
                        return (clean_url, ScrapeOutcome::Unchanged);
                    }
                    Ok(false) => {}
                    Err(e) => {
                        warn!(url = clean_url.as_str(), error = %e, "Hash check failed, proceeding with extraction");
                    }
                }

                match extractor.extract(&content, &clean_url).await {
                    Ok(result) => (
                        clean_url,
                        ScrapeOutcome::New {
                            content,
                            nodes: result.nodes,
                            resource_tags: result.resource_tags,
                            signal_tags: result.signal_tags,
                        },
                    ),
                    Err(e) => {
                        warn!(url = clean_url.as_str(), error = %e, "Extraction failed");
                        (clean_url, ScrapeOutcome::Failed)
                    }
                }
            }
        }))
        .buffer_unordered(6)
        .collect()
        .await;

        // Process results
        let now = Utc::now();
        let known_urls = ctx.known_city_urls();
        for (url, outcome) in pipeline_results {
            let ck = ctx
                .url_to_canonical_key
                .get(&url)
                .cloned()
                .unwrap_or_else(|| url.clone());
            match outcome {
                ScrapeOutcome::New {
                    content,
                    nodes,
                    resource_tags,
                    signal_tags,
                } => {
                    // Collect implied queries from Tension + Need nodes for immediate expansion
                    for node in &nodes {
                        if matches!(node.node_type(), NodeType::Tension | NodeType::Need) {
                            if let Some(meta) = node.meta() {
                                ctx.expansion_queries
                                    .extend(meta.implied_queries.iter().cloned());
                            }
                        }
                    }

                    let signal_count_before = ctx.stats.signals_stored;
                    match self
                        .store_signals(
                            &url,
                            &content,
                            nodes,
                            resource_tags,
                            signal_tags,
                            ctx,
                            &known_urls,
                        )
                        .await
                    {
                        Ok(_) => {
                            ctx.stats.urls_scraped += 1;
                            let produced = ctx.stats.signals_stored - signal_count_before;
                            *ctx.source_signal_counts.entry(ck).or_default() += produced;
                        }
                        Err(e) => {
                            warn!(url, error = %e, "Failed to store signals");
                            ctx.stats.urls_failed += 1;
                            ctx.source_signal_counts.entry(ck).or_default();
                        }
                    }
                }
                ScrapeOutcome::Unchanged => {
                    match self.writer.refresh_url_signals(&url, now).await {
                        Ok(n) if n > 0 => {
                            info!(url, refreshed = n, "Refreshed unchanged signals")
                        }
                        Ok(_) => {}
                        Err(e) => warn!(url, error = %e, "Failed to refresh signals"),
                    }
                    ctx.stats.urls_unchanged += 1;
                    ctx.source_signal_counts.entry(ck).or_default();
                }
                ScrapeOutcome::Failed => {
                    ctx.stats.urls_failed += 1;
                }
            }
        }
    }

    /// Scrape social media accounts, feed posts through LLM extraction.
    pub async fn run_social(&self, social_sources: &[&SourceNode], ctx: &mut RunContext) {
        type SocialResult = Option<(
            String,
            String,
            String,
            Vec<Node>,
            Vec<(Uuid, Vec<ResourceTag>)>,
            Vec<(Uuid, Vec<String>)>,
            usize,
        )>; // (canonical_key, source_url, combined_text, nodes, resource_tags, signal_tags, post_count)

        // Build uniform list of SocialAccounts from SourceNodes
        let mut accounts: Vec<(String, String, SocialAccount)> = Vec::new(); // (canonical_key, source_url, account)

        for source in social_sources {
            let common_platform = match scraping_strategy(source.value()) {
                ScrapingStrategy::Social(p) => p,
                _ => continue,
            };
            let (platform, identifier) = match common_platform {
                CommonSocialPlatform::Instagram => {
                    (SocialPlatform::Instagram, source.url.as_deref().unwrap_or(&source.canonical_value).to_string())
                }
                CommonSocialPlatform::Facebook => {
                    let url = source
                        .url
                        .as_deref()
                        .filter(|u| !u.is_empty())
                        .unwrap_or(&source.canonical_value);
                    (SocialPlatform::Facebook, url.to_string())
                }
                CommonSocialPlatform::Reddit => {
                    let url = source
                        .url
                        .as_deref()
                        .filter(|u| !u.is_empty())
                        .unwrap_or(&source.canonical_value);
                    // If identifier is just a subreddit name (e.g. "r/Minneapolis"), build a full URL
                    let identifier = if !url.starts_with("http") {
                        let name = url.trim_start_matches("r/");
                        format!("https://www.reddit.com/r/{}/", name)
                    } else {
                        url.to_string()
                    };
                    (SocialPlatform::Reddit, identifier)
                }
                CommonSocialPlatform::Twitter => {
                    (SocialPlatform::Twitter, source.url.as_deref().unwrap_or(&source.canonical_value).to_string())
                }
                CommonSocialPlatform::TikTok => {
                    (SocialPlatform::TikTok, source.url.as_deref().unwrap_or(&source.canonical_value).to_string())
                }
                CommonSocialPlatform::Bluesky => continue, // Not yet supported by social scraper
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
                SocialAccount {
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

        // Collect all futures into a single Vec<Pin<Box<...>>> so types unify
        let mut futures: Vec<Pin<Box<dyn Future<Output = SocialResult> + Send + '_>>> =
            Vec::new();

        for (canonical_key, source_url, account) in &accounts {
            let canonical_key = canonical_key.clone();
            let source_url = source_url.clone();
            let is_reddit = matches!(account.platform, SocialPlatform::Reddit);
            let limit: u32 = if is_reddit { 20 } else { 10 };

            futures.push(Box::pin(async move {
                let posts = match self.social.search_posts(account, limit).await {
                    Ok(p) => p,
                    Err(e) => {
                        warn!(source_url, error = %e, "Social media scrape failed");
                        return None;
                    }
                };
                let post_count = posts.len();

                // Format a post header including the specific post URL when available.
                let post_header = |i: usize, p: &SocialPost| -> String {
                    match &p.url {
                        Some(url) => format!("--- Post {} ({}) ---\n{}", i + 1, url, p.content),
                        None => format!("--- Post {} ---\n{}", i + 1, p.content),
                    }
                };

                if is_reddit {
                    // Reddit: batch posts 10 at a time for extraction
                    let batches: Vec<_> = posts.chunks(10).collect();
                    let mut all_nodes = Vec::new();
                    let mut all_resource_tags = Vec::new();
                    let mut all_signal_tags = Vec::new();
                    let mut combined_all = String::new();
                    for batch in batches {
                        let combined_text: String = batch
                            .iter()
                            .enumerate()
                            .map(|(i, p)| post_header(i, p))
                            .collect::<Vec<_>>()
                            .join("\n\n");
                        if combined_text.is_empty() {
                            continue;
                        }
                        combined_all.push_str(&combined_text);
                        match self.extractor.extract(&combined_text, &source_url).await {
                            Ok(result) => {
                                all_nodes.extend(result.nodes);
                                all_resource_tags.extend(result.resource_tags);
                                all_signal_tags.extend(result.signal_tags);
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
                        combined_all,
                        all_nodes,
                        all_resource_tags,
                        all_signal_tags,
                        post_count,
                    ))
                } else {
                    // Instagram/Facebook: combine all posts then extract
                    let combined_text: String = posts
                        .iter()
                        .enumerate()
                        .map(|(i, p)| post_header(i, p))
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    if combined_text.is_empty() {
                        return None;
                    }
                    let result = match self.extractor.extract(&combined_text, &source_url).await {
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
                        combined_text,
                        result.nodes,
                        result.resource_tags,
                        result.signal_tags,
                        post_count,
                    ))
                }
            }));
        }

        let results: Vec<_> = stream::iter(futures).buffer_unordered(10).collect().await;

        let known_city_urls = ctx.known_city_urls();
        for result in results.into_iter().flatten() {
            let (
                canonical_key,
                source_url,
                combined_text,
                nodes,
                resource_tags,
                signal_tags,
                post_count,
            ) = result;
            // Collect implied queries from Tension/Need social signals
            for node in &nodes {
                if matches!(node.node_type(), NodeType::Tension | NodeType::Need) {
                    if let Some(meta) = node.meta() {
                        ctx.expansion_queries
                            .extend(meta.implied_queries.iter().cloned());
                    }
                }
            }
            ctx.stats.social_media_posts += post_count as u32;
            let signal_count_before = ctx.stats.signals_stored;
            if let Err(e) = self
                .store_signals(
                    &source_url,
                    &combined_text,
                    nodes,
                    resource_tags,
                    signal_tags,
                    ctx,
                    &known_city_urls,
                )
                .await
            {
                warn!(source_url = source_url.as_str(), error = %e, "Failed to store social media signals");
            }
            let produced = ctx.stats.signals_stored - signal_count_before;
            *ctx.source_signal_counts
                .entry(canonical_key)
                .or_default() += produced;
        }
    }

    /// Discover new accounts by searching platform-agnostic topics (hashtags/keywords)
    /// across Instagram, X/Twitter, TikTok, and GoFundMe.
    pub async fn discover_from_topics(&self, topics: &[String], ctx: &mut RunContext) {
        const MAX_SOCIAL_SEARCHES: usize = 3;
        const MAX_NEW_ACCOUNTS: usize = 5;
        const POSTS_PER_SEARCH: u32 = 20;
        const MAX_SITE_SEARCH_TOPICS: usize = 2;
        const SITE_SEARCH_RESULTS: usize = 5;

        if topics.is_empty() {
            return;
        }

        info!(topics = ?topics, "Starting social topic discovery...");

        let known_city_urls = ctx.known_city_urls();

        // Load existing sources for dedup across all platforms
        let existing_sources = self
            .writer
            .get_active_sources(&self.region.slug)
            .await
            .unwrap_or_default();
        let existing_canonical_values: HashSet<String> = existing_sources
            .iter()
            .map(|s| s.canonical_value.clone())
            .collect();

        let mut new_accounts = 0u32;
        let topic_strs: Vec<&str> = topics
            .iter()
            .take(MAX_SOCIAL_SEARCHES)
            .map(|t| t.as_str())
            .collect();

        // Search each social platform with the same topics
        let platforms = [
            SocialPlatform::Instagram,
            SocialPlatform::Twitter,
            SocialPlatform::TikTok,
        ];

        for platform in &platforms {
            if new_accounts >= MAX_NEW_ACCOUNTS as u32 {
                break;
            }

            let platform_name = match platform {
                SocialPlatform::Instagram => "instagram",
                SocialPlatform::Twitter => "x",
                SocialPlatform::TikTok => "tiktok",
                _ => continue,
            };

            let discovered_posts = match self
                .social
                .search_topics(platform, &topic_strs, POSTS_PER_SEARCH)
                .await
            {
                Ok(posts) => posts,
                Err(e) => {
                    warn!(platform = platform_name, error = %e, "Topic discovery failed for platform");
                    continue; // Platform failure doesn't block others
                }
            };

            if discovered_posts.is_empty() {
                info!(
                    platform = platform_name,
                    "No posts found from topic discovery"
                );
                continue;
            }

            ctx.stats.discovery_posts_found += discovered_posts.len() as u32;

            // Group posts by author
            let mut by_author: HashMap<String, Vec<&SocialPost>> = HashMap::new();
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

            for (username, posts) in &by_author {
                if new_accounts >= MAX_NEW_ACCOUNTS as u32 {
                    info!("Discovery account budget exhausted");
                    break;
                }

                // Platform-aware source URL
                let source_url = match platform {
                    SocialPlatform::Instagram => {
                        format!("https://www.instagram.com/{username}/")
                    }
                    SocialPlatform::Twitter => format!("https://x.com/{username}"),
                    SocialPlatform::TikTok => {
                        format!("https://www.tiktok.com/@{username}")
                    }
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
                    .map(|(i, p)| match &p.url {
                        Some(url) => format!("--- Post {} ({}) ---\n{}", i + 1, url, p.content),
                        None => format!("--- Post {} ---\n{}", i + 1, p.content),
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
                let signal_count_before = ctx.stats.signals_stored;
                if let Err(e) = self
                    .store_signals(
                        &source_url,
                        &combined_text,
                        result.nodes,
                        result.resource_tags,
                        result.signal_tags,
                        ctx,
                        &known_city_urls,
                    )
                    .await
                {
                    warn!(username, error = %e, "Failed to store discovery signals");
                    continue;
                }
                let produced = ctx.stats.signals_stored - signal_count_before;

                // Create a Source node with correct platform type
                let cv = rootsignal_common::canonical_value(&source_url);
                let ck = sources::make_canonical_key(&source_url);
                let gap_context = format!(
                    "Topic: {}",
                    topics.first().map(|t| t.as_str()).unwrap_or("unknown")
                );
                let source = SourceNode {
                    id: Uuid::new_v4(),
                    canonical_key: ck.clone(),
                    canonical_value: cv,
                    url: Some(source_url.clone()),
                    discovery_method: DiscoveryMethod::HashtagDiscovery,
                    created_at: Utc::now(),
                    last_scraped: Some(Utc::now()),
                    last_produced_signal: if produced > 0 { Some(Utc::now()) } else { None },
                    signals_produced: produced,
                    signals_corroborated: 0,
                    consecutive_empty_runs: 0,
                    active: true,
                    gap_context: Some(gap_context),
                    weight: 0.3,
                    cadence_hours: None,
                    avg_signals_per_scrape: 0.0,
                    quality_penalty: 1.0,
                    source_role: SourceRole::default(),
                    scrape_count: 0,
                };

                *ctx.source_signal_counts.entry(ck).or_default() += produced;

                match self.writer.upsert_source(&source, &self.region.slug).await {
                    Ok(()) => {
                        new_accounts += 1;
                        info!(
                            username,
                            platform = platform_name,
                            signals = produced,
                            "Discovered new account via topic search"
                        );
                    }
                    Err(e) => {
                        warn!(username, error = %e, "Failed to create Source node for discovered account");
                    }
                }
            }
        }

        // Site-scoped search: find WebQuery sources with `site:` prefix,
        // search Serper for each topic, scrape + extract results.
        let site_sources: Vec<&SourceNode> = existing_sources
            .iter()
            .filter(|s| {
                is_web_query(&s.canonical_value)
                    && s.canonical_value.starts_with("site:")
            })
            .collect();

        for source in &site_sources {
            let site_prefix = &source.canonical_value; // e.g. "site:gofundme.com/f/ Minneapolis"
            for topic in topics.iter().take(MAX_SITE_SEARCH_TOPICS) {
                let query = format!("{} {}", site_prefix, topic);
                let results = match self.searcher.search(&query, SITE_SEARCH_RESULTS).await {
                    Ok(r) => r,
                    Err(e) => {
                        warn!(query, error = %e, "Site-scoped search failed");
                        continue;
                    }
                };

                if results.is_empty() {
                    continue;
                }

                info!(query, count = results.len(), "Site-scoped search results");

                for result in &results {
                    if known_city_urls.contains(&result.url) {
                        continue;
                    }

                    let content = match self.scraper.scrape(&result.url).await {
                        Ok(c) if !c.is_empty() => c,
                        Ok(_) => continue,
                        Err(e) => {
                            warn!(url = result.url, error = %e, "Site-scoped scrape failed");
                            continue;
                        }
                    };

                    let extracted =
                        match self.extractor.extract(&content, &result.url).await {
                            Ok(r) => r,
                            Err(e) => {
                                warn!(url = result.url, error = %e, "Site-scoped extraction failed");
                                continue;
                            }
                        };

                    if extracted.nodes.is_empty() {
                        continue;
                    }

                    if let Err(e) = self
                        .store_signals(
                            &result.url,
                            &content,
                            extracted.nodes,
                            extracted.resource_tags,
                            extracted.signal_tags,
                            ctx,
                            &known_city_urls,
                        )
                        .await
                    {
                        warn!(url = result.url, error = %e, "Failed to store site-scoped signals");
                    }
                }
            }
        }

        ctx.stats.discovery_accounts_found = new_accounts;
        info!(
            topics = topics.len(),
            new_accounts, "Social topic discovery complete"
        );
    }

    // -----------------------------------------------------------------------
    // store_signals — multi-layer dedup + graph storage (private)
    // -----------------------------------------------------------------------

    async fn store_signals(
        &self,
        url: &str,
        content: &str,
        mut nodes: Vec<Node>,
        resource_tags: Vec<(Uuid, Vec<ResourceTag>)>,
        signal_tags: Vec<(Uuid, Vec<String>)>,
        ctx: &mut RunContext,
        known_city_urls: &HashSet<String>,
    ) -> Result<()> {
        let url = sanitize_url(url);
        ctx.stats.signals_extracted += nodes.len() as u32;

        // Build lookup map from node ID → resource tags
        let resource_map: HashMap<Uuid, Vec<ResourceTag>> = resource_tags.into_iter().collect();

        // Build lookup map from extraction-time node ID → tag slugs
        let tag_map: HashMap<Uuid, Vec<String>> = signal_tags.into_iter().collect();

        // Entity mappings for source diversity (domain-based fallback in resolve_entity handles it)
        let entity_mappings: Vec<rootsignal_common::EntityMappingOwned> = Vec::new();

        // Score quality, set confidence, and apply sanitized URL
        for node in &mut nodes {
            let q = quality::score(node);
            if let Some(meta) = node_meta_mut(node) {
                meta.confidence = q.confidence;
                meta.source_url = url.clone();
            }
        }

        // Strip fake city-center coordinates.
        // Safety net: if the LLM echoes the default city coords, remove them.
        let center_lat = self.region.center_lat;
        let center_lng = self.region.center_lng;
        for node in &mut nodes {
            let is_fake = node
                .meta()
                .and_then(|m| m.location.as_ref())
                .map(|loc| {
                    (loc.lat - center_lat).abs() < 0.01 && (loc.lng - center_lng).abs() < 0.01
                })
                .unwrap_or(false);

            if is_fake {
                if let Some(meta) = node_meta_mut(node) {
                    meta.location = None;
                    ctx.stats.geo_stripped += 1;
                }
            }
        }

        // Layered geo-check:
        // 1. Has coordinates within radius → accept
        // 2. Has coordinates outside radius → reject
        // 3. No coordinates, location_name matches a geo_term → accept
        // 4. No coordinates, no location_name match, source is city-local → accept with 0.8x confidence
        // 5. No coordinates, no match, source not city-local → reject
        let geo_terms = &self.region.geo_terms;
        let center_lat = self.region.center_lat;
        let center_lng = self.region.center_lng;
        let radius_km = self.region.radius_km;

        // Determine if this source URL belongs to a city-local source
        let is_city_local = known_city_urls.contains(&url);

        let before_geo = nodes.len();
        let mut nodes_filtered = Vec::new();
        for mut node in nodes {
            let has_coords = node.meta().and_then(|m| m.location.as_ref()).is_some();
            let loc_name = node
                .meta()
                .and_then(|m| m.location_name.as_deref())
                .unwrap_or("")
                .to_string();

            if has_coords {
                let loc = node.meta().unwrap().location.as_ref().unwrap();
                let dist =
                    rootsignal_common::haversine_km(center_lat, center_lng, loc.lat, loc.lng);
                if dist <= radius_km {
                    // Case 1: coordinates within radius → accept
                    nodes_filtered.push(node);
                } else {
                    // Case 2: coordinates outside radius → reject
                    ctx.stats.geo_filtered += 1;
                }
            } else if !loc_name.is_empty() && loc_name != "<UNKNOWN>" {
                let loc_lower = loc_name.to_lowercase();
                if geo_terms
                    .iter()
                    .any(|term| loc_lower.contains(&term.to_lowercase()))
                {
                    // Case 3: location_name matches geo_term → accept
                    nodes_filtered.push(node);
                } else if is_city_local {
                    // Case 4: city-local source, no name match → accept with confidence penalty
                    if let Some(meta) = node_meta_mut(&mut node) {
                        meta.confidence *= 0.8;
                    }
                    nodes_filtered.push(node);
                } else {
                    // Case 5: non-local source, no match → reject
                    ctx.stats.geo_filtered += 1;
                }
            } else {
                // No coordinates, no location_name — keep with benefit of the doubt
                nodes_filtered.push(node);
            }
        }
        // Backfill city-center coordinates on signals that passed the geo-filter
        // but have no coordinates. This ensures they're visible in geo bounding-box
        // queries (admin UI, API). Precision is marked City so consumers know it's
        // approximate, not a specific location.
        for node in &mut nodes_filtered {
            let needs_coords = node.meta().map(|m| m.location.is_none()).unwrap_or(false);
            if needs_coords {
                if let Some(meta) = node_meta_mut(node) {
                    meta.location = Some(GeoPoint {
                        lat: center_lat,
                        lng: center_lng,
                        precision: GeoPrecision::City,
                    });
                }
            }
        }

        let nodes = nodes_filtered;
        let geo_filtered = before_geo - nodes.len();
        if geo_filtered > 0 {
            info!(
                url = url.as_str(),
                filtered = geo_filtered,
                "Off-geography signals dropped"
            );
        }

        // Filter to signal nodes only (skip Evidence)
        let nodes: Vec<_> = nodes
            .into_iter()
            .filter(|n| !matches!(n.node_type(), NodeType::Evidence))
            .collect();

        if nodes.is_empty() {
            return Ok(());
        }

        // --- Layer 1: Within-batch dedup by (normalized_title, node_type) ---
        let mut seen = HashSet::new();
        let nodes: Vec<_> = nodes
            .into_iter()
            .filter(|n| seen.insert((normalize_title(n.title()), n.node_type())))
            .collect();

        // --- Layer 2: URL-based title dedup against existing database ---
        let existing_titles: HashSet<String> = self
            .writer
            .existing_titles_for_url(&url)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|t| normalize_title(&t))
            .collect();

        let before_url_dedup = nodes.len();
        let nodes: Vec<_> = nodes
            .into_iter()
            .filter(|n| !existing_titles.contains(&normalize_title(n.title())))
            .collect();
        let url_deduped = before_url_dedup - nodes.len();
        if url_deduped > 0 {
            info!(
                url = url.as_str(),
                skipped = url_deduped,
                "URL-based title dedup"
            );
            ctx.stats.signals_deduplicated += url_deduped as u32;
        }

        if nodes.is_empty() {
            return Ok(());
        }

        // --- Layer 2.5: Global exact-title+type dedup (single batch query) ---
        let now = Utc::now();
        let content_hash_str = format!("{:x}", content_hash(content));

        let title_type_pairs: Vec<(String, NodeType)> = nodes
            .iter()
            .map(|n| (normalize_title(n.title()), n.node_type()))
            .collect();

        let global_matches = self
            .writer
            .find_by_titles_and_types(&title_type_pairs)
            .await
            .unwrap_or_default();

        let mut remaining_nodes = Vec::new();
        for node in nodes {
            let key = (normalize_title(node.title()), node.node_type());
            if let Some((existing_id, existing_url)) = global_matches.get(&key) {
                if *existing_url != url {
                    // Cross-source: different URL confirms the same signal — real corroboration
                    info!(
                        existing_id = %existing_id,
                        title = node.title(),
                        existing_source = existing_url.as_str(),
                        new_source = url.as_str(),
                        "Global title+type match from different source, corroborating"
                    );
                    self.writer
                        .corroborate(*existing_id, node.node_type(), now, &entity_mappings)
                        .await?;
                    let evidence = EvidenceNode {
                        id: Uuid::new_v4(),
                        source_url: url.clone(),
                        retrieved_at: now,
                        content_hash: content_hash_str.clone(),
                        snippet: node.meta().map(|m| m.summary.clone()),
                        relevance: None,
                        evidence_confidence: None,
                    };
                    self.writer
                        .create_evidence(&evidence, *existing_id)
                        .await?;
                    ctx.stats.signals_deduplicated += 1;
                    continue;
                } else {
                    // Same-source re-scrape: signal already exists from this URL.
                    // Refresh to prove it's still active, but don't inflate corroboration.
                    info!(
                        existing_id = %existing_id,
                        title = node.title(),
                        source = url.as_str(),
                        "Same-source title match, refreshing (no corroboration)"
                    );
                    self.writer
                        .refresh_signal(*existing_id, node.node_type(), now)
                        .await?;
                    let evidence = EvidenceNode {
                        id: Uuid::new_v4(),
                        source_url: url.clone(),
                        retrieved_at: now,
                        content_hash: content_hash_str.clone(),
                        snippet: node.meta().map(|m| m.summary.clone()),
                        relevance: None,
                        evidence_confidence: None,
                    };
                    self.writer
                        .create_evidence(&evidence, *existing_id)
                        .await?;
                    ctx.stats.signals_deduplicated += 1;
                    continue;
                }
            }
            remaining_nodes.push(node);
        }
        let nodes = remaining_nodes;

        if nodes.is_empty() {
            return Ok(());
        }

        // Batch embed remaining signals (1 API call instead of N)
        let content_snippet = if content.len() > 500 {
            let mut end = 500;
            while !content.is_char_boundary(end) {
                end -= 1;
            }
            &content[..end]
        } else {
            content
        };
        let embed_texts: Vec<String> = nodes
            .iter()
            .map(|n| format!("{} {}", n.title(), content_snippet))
            .collect();

        let embeddings = match self.embedder.embed_batch(embed_texts).await {
            Ok(e) => e,
            Err(e) => {
                warn!(url = url.as_str(), error = %e, "Batch embedding failed, skipping all signals");
                return Ok(());
            }
        };

        // Pre-compute resource tag embeddings in a single batch API call
        let mut res_embed_texts: Vec<String> = Vec::new();
        let mut res_embed_keys: Vec<(Uuid, usize)> = Vec::new(); // (meta_id, tag_index)
        for node in &nodes {
            if let Some(meta) = node.meta() {
                if let Some(tags) = resource_map.get(&meta.id) {
                    for (i, tag) in tags.iter().enumerate() {
                        if tag.confidence >= 0.3 {
                            res_embed_texts.push(format!(
                                "{}: {}",
                                tag.slug,
                                tag.context.as_deref().unwrap_or("")
                            ));
                            res_embed_keys.push((meta.id, i));
                        }
                    }
                }
            }
        }
        let res_embeddings: HashMap<(Uuid, usize), Vec<f32>> = if !res_embed_texts.is_empty() {
            match self.embedder.embed_batch(res_embed_texts).await {
                Ok(embeds) => res_embed_keys
                    .into_iter()
                    .zip(embeds)
                    .collect(),
                Err(e) => {
                    warn!(error = %e, "Resource tag batch embedding failed (non-fatal)");
                    HashMap::new()
                }
            }
        } else {
            HashMap::new()
        };

        // --- Layer 3: Vector dedup (in-memory cache + graph) with URL-aware threshold ---

        for (node, embedding) in nodes.into_iter().zip(embeddings.into_iter()) {
            let node_type = node.node_type();
            let type_idx = match node_type {
                NodeType::Gathering => 0,
                NodeType::Aid => 1,
                NodeType::Need => 2,
                NodeType::Notice => 3,
                NodeType::Tension => 4,
                NodeType::Evidence => continue,
            };

            // 3a: Check in-memory cache first (catches cross-batch dupes not yet indexed)
            if let Some((cached_id, cached_type, cached_url, sim)) =
                ctx.embed_cache.find_match(&embedding, 0.85)
            {
                let is_same_source = cached_url == url;
                if is_same_source {
                    info!(
                        existing_id = %cached_id,
                        similarity = sim,
                        title = node.title(),
                        source = "cache",
                        "Same-source duplicate in cache, refreshing (no corroboration)"
                    );
                    self.writer
                        .refresh_signal(cached_id, cached_type, now)
                        .await?;

                    let evidence = EvidenceNode {
                        id: Uuid::new_v4(),
                        source_url: url.clone(),
                        retrieved_at: now,
                        content_hash: content_hash_str.clone(),
                        snippet: node.meta().map(|m| m.summary.clone()),
                        relevance: None,
                        evidence_confidence: None,
                    };
                    self.writer.create_evidence(&evidence, cached_id).await?;

                    ctx.stats.signals_deduplicated += 1;
                    continue;
                } else if sim >= 0.92 {
                    info!(
                        existing_id = %cached_id,
                        similarity = sim,
                        title = node.title(),
                        source = "cache",
                        "Cross-source duplicate in cache, corroborating"
                    );
                    self.writer
                        .corroborate(cached_id, cached_type, now, &entity_mappings)
                        .await?;

                    let evidence = EvidenceNode {
                        id: Uuid::new_v4(),
                        source_url: url.clone(),
                        retrieved_at: now,
                        content_hash: content_hash_str.clone(),
                        snippet: node.meta().map(|m| m.summary.clone()),
                        relevance: None,
                        evidence_confidence: None,
                    };
                    self.writer.create_evidence(&evidence, cached_id).await?;

                    ctx.stats.signals_deduplicated += 1;
                    continue;
                }
            }

            // 3b: Check graph index (catches dupes from previous runs, city-scoped)
            let lat_delta = self.region.radius_km / 111.0;
            let lng_delta = self.region.radius_km
                / (111.0 * self.region.center_lat.to_radians().cos());
            match self
                .writer
                .find_duplicate(
                    &embedding,
                    node_type,
                    0.85,
                    self.region.center_lat - lat_delta,
                    self.region.center_lat + lat_delta,
                    self.region.center_lng - lng_delta,
                    self.region.center_lng + lng_delta,
                )
                .await
            {
                Ok(Some(dup)) => {
                    let dominated_url = sanitize_url(&dup.source_url);
                    let is_same_source = dominated_url == url;
                    if is_same_source {
                        info!(
                            existing_id = %dup.id,
                            similarity = dup.similarity,
                            title = node.title(),
                            source = "graph",
                            "Same-source duplicate in graph, refreshing (no corroboration)"
                        );
                        self.writer
                            .refresh_signal(dup.id, dup.node_type, now)
                            .await?;

                        let evidence = EvidenceNode {
                            id: Uuid::new_v4(),
                            source_url: url.clone(),
                            retrieved_at: now,
                            content_hash: content_hash_str.clone(),
                            snippet: node.meta().map(|m| m.summary.clone()),
                            relevance: None,
                            evidence_confidence: None,
                        };
                        self.writer.create_evidence(&evidence, dup.id).await?;

                        ctx.embed_cache
                            .add(embedding, dup.id, dup.node_type, dominated_url);

                        ctx.stats.signals_deduplicated += 1;
                        continue;
                    } else if dup.similarity >= 0.92 {
                        let cross_type = dup.node_type != node_type;
                        info!(
                            existing_id = %dup.id,
                            similarity = dup.similarity,
                            title = node.title(),
                            cross_type,
                            source = "graph",
                            "Cross-source duplicate in graph, corroborating"
                        );
                        self.writer
                            .corroborate(dup.id, dup.node_type, now, &entity_mappings)
                            .await?;

                        let evidence = EvidenceNode {
                            id: Uuid::new_v4(),
                            source_url: url.clone(),
                            retrieved_at: now,
                            content_hash: content_hash_str.clone(),
                            snippet: node.meta().map(|m| m.summary.clone()),
                            relevance: None,
                            evidence_confidence: None,
                        };
                        self.writer.create_evidence(&evidence, dup.id).await?;

                        ctx.embed_cache
                            .add(embedding, dup.id, dup.node_type, dominated_url);

                        ctx.stats.signals_deduplicated += 1;
                        continue;
                    }
                    // Below 0.92 from a different source — not confident enough, create new
                }
                Ok(None) => {}
                Err(e) => {
                    warn!(error = %e, "Dedup check failed, proceeding with creation");
                }
            }

            // Create new node
            let node_id = self.writer.create_node(&node, &embedding, "scraper", &self.run_id).await?;

            // Add to in-memory cache so subsequent batches can find it immediately
            ctx.embed_cache
                .add(embedding, node_id, node_type, url.clone());

            let evidence = EvidenceNode {
                id: Uuid::new_v4(),
                source_url: url.clone(),
                retrieved_at: now,
                content_hash: content_hash_str.clone(),
                snippet: node.meta().map(|m| m.summary.clone()),
                relevance: None,
                evidence_confidence: None,
            };
            self.writer.create_evidence(&evidence, node_id).await?;

            // Resolve mentioned actors → Actor nodes + ACTED_IN edges
            if let Some(meta) = node.meta() {
                for actor_name in &meta.mentioned_actors {
                    let actor_id = match self.writer.find_actor_by_name(actor_name).await {
                        Ok(Some(id)) => id,
                        Ok(None) => {
                            let actor = ActorNode {
                                id: Uuid::new_v4(),
                                name: actor_name.clone(),
                                actor_type: ActorType::Organization,
                                entity_id: actor_name.to_lowercase().replace(' ', "-"),
                                domains: vec![],
                                social_urls: vec![],
                                description: String::new(),
                                signal_count: 0,
                                first_seen: Utc::now(),
                                last_active: Utc::now(),
                                typical_roles: vec![],
                            };
                            if let Err(e) = self.writer.upsert_actor(&actor, &self.region.slug).await {
                                warn!(error = %e, actor = actor_name, "Failed to create actor (non-fatal)");
                                continue;
                            }
                            actor.id
                        }
                        Err(e) => {
                            warn!(error = %e, actor = actor_name, "Actor lookup failed (non-fatal)");
                            continue;
                        }
                    };
                    if let Err(e) = self
                        .writer
                        .link_actor_to_signal(actor_id, node_id, "mentioned")
                        .await
                    {
                        warn!(error = %e, actor = actor_name, "Failed to link actor to signal (non-fatal)");
                    }
                }
            }

            // Wire resource edges (Resource nodes + REQUIRES/PREFERS/OFFERS edges)
            if let Some(meta) = node.meta() {
                if let Some(tags) = resource_map.get(&meta.id) {
                    for (i, tag) in tags.iter().enumerate().filter(|(_, t)| t.confidence >= 0.3) {
                        let slug = rootsignal_common::slugify(&tag.slug);
                        let res_embedding = match res_embeddings.get(&(meta.id, i)) {
                            Some(e) => e.clone(),
                            None => continue, // Embedding failed in batch, skip
                        };
                        let resource_id = match self
                            .writer
                            .find_or_create_resource(
                                &tag.slug,
                                &slug,
                                tag.context.as_deref().unwrap_or(""),
                                &res_embedding,
                            )
                            .await
                        {
                            Ok(id) => id,
                            Err(e) => {
                                warn!(error = %e, slug = slug.as_str(), "Resource creation failed (non-fatal)");
                                continue;
                            }
                        };
                        let confidence = tag.confidence.clamp(0.0, 1.0) as f32;
                        let edge_result = match tag.role.as_str() {
                            "requires" => {
                                self.writer
                                    .create_requires_edge(
                                        node_id,
                                        resource_id,
                                        confidence,
                                        tag.context.as_deref(),
                                        None,
                                    )
                                    .await
                            }
                            "prefers" => {
                                self.writer
                                    .create_prefers_edge(node_id, resource_id, confidence)
                                    .await
                            }
                            "offers" => {
                                self.writer
                                    .create_offers_edge(
                                        node_id,
                                        resource_id,
                                        confidence,
                                        tag.context.as_deref(),
                                    )
                                    .await
                            }
                            other => {
                                warn!(role = other, slug = slug.as_str(), "Unknown resource role");
                                continue;
                            }
                        };
                        if let Err(e) = edge_result {
                            warn!(error = %e, slug = slug.as_str(), "Resource edge creation failed (non-fatal)");
                        }
                    }
                }
            }

            // Wire signal tags (Tag nodes + TAGGED edges)
            if let Some(meta) = node.meta() {
                if let Some(slugs) = tag_map.get(&meta.id) {
                    if !slugs.is_empty() {
                        if let Err(e) =
                            self.writer.batch_tag_signals(node_id, slugs).await
                        {
                            warn!(error = %e, "Signal tag creation failed (non-fatal)");
                        }
                    }
                }
            }

            // Update stats
            ctx.stats.signals_stored += 1;
            ctx.stats.by_type[type_idx] += 1;

            if let Some(meta) = node.meta() {
                let age = now - meta.extracted_at;
                if age.num_days() < 7 {
                    ctx.stats.fresh_7d += 1;
                } else if age.num_days() < 30 {
                    ctx.stats.fresh_30d += 1;
                } else if age.num_days() < 90 {
                    ctx.stats.fresh_90d += 1;
                }
            }
        }

        Ok(())
    }
}
