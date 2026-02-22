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
    channel_type, is_web_query, scraping_strategy, ActorNode, ActorType, ActorContext, ScoutScope,
    DiscoveryMethod, EvidenceNode, Node, NodeType, ScrapingStrategy,
    SocialPlatform, SocialPost, SourceNode, SourceRole,
};
use rootsignal_graph::GraphWriter;

use super::geo_filter;
use crate::embedder::TextEmbedder;
use crate::extractor::{ResourceTag, SignalExtractor};
use crate::quality;
use crate::run_log::{EventKind, RunLog};
use crate::scout::ScoutStats;
use rootsignal_archive::{Content, FetchBackend, FetchBackendExt};

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
    pub social_expansion_topics: Vec<String>,
    pub stats: ScoutStats,
    pub query_api_errors: HashSet<String>,
    /// Entity context keyed by source canonical_key. When a source is linked to
    /// Actor context keyed by source canonical_key. When a source is linked to
    /// a known actor via HAS_ACCOUNT, its context is stored here for location
    /// fallback during signal extraction.
    pub actor_contexts: HashMap<String, ActorContext>,
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
            social_expansion_topics: Vec::new(),
            stats: ScoutStats::default(),
            query_api_errors: HashSet::new(),
            actor_contexts: HashMap::new(),
        }
    }

    /// Rebuild known URLs from current URL map state.
    /// Must be called before each social scrape to capture
    /// URLs resolved during the preceding web scrape.
    pub fn known_urls(&self) -> HashSet<String> {
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

// ---------------------------------------------------------------------------
// ScrapePhase — the core scrape-extract-store-dedup pipeline
// ---------------------------------------------------------------------------

pub(crate) struct ScrapePhase {
    writer: GraphWriter,
    extractor: Arc<dyn SignalExtractor>,
    embedder: Arc<dyn TextEmbedder>,
    archive: Arc<dyn FetchBackend>,
    region: ScoutScope,
    run_id: String,
}

impl ScrapePhase {
    pub fn new(
        writer: GraphWriter,
        extractor: Arc<dyn SignalExtractor>,
        embedder: Arc<dyn TextEmbedder>,
        archive: Arc<dyn FetchBackend>,
        region: ScoutScope,
        run_id: String,
    ) -> Self {
        Self {
            writer,
            extractor,
            embedder,
            archive,
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
    pub async fn run_web(&self, sources: &[&SourceNode], ctx: &mut RunContext, run_log: &mut RunLog) {
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
            let archive = self.archive.clone();
            let query_inputs: Vec<_> = api_queries
                .iter()
                .map(|source| (source.canonical_key.clone(), source.canonical_value.clone()))
                .collect();
            let search_results: Vec<_> = stream::iter(query_inputs.into_iter().map(|(canonical_key, query_str)| {
                let archive = archive.clone();
                async move {
                    (
                        canonical_key,
                        query_str.clone(),
                        archive.fetch(&query_str).content().await,
                    )
                }
            }))
            .buffer_unordered(5)
            .collect()
            .await;

            for (canonical_key, query_str, result) in search_results {
                match result {
                    Ok(Content::SearchResults(results)) => {
                        run_log.log(EventKind::SearchQuery {
                            query: query_str.clone(),
                            provider: "serper".to_string(),
                            result_count: results.len() as u32,
                            canonical_key: canonical_key.clone(),
                        });
                        for r in &results {
                            let clean = sanitize_url(&r.url);
                            ctx.url_to_canonical_key
                                .entry(clean)
                                .or_insert_with(|| canonical_key.clone());
                        }
                        ctx.source_signal_counts
                            .entry(canonical_key.clone())
                            .or_default();
                        for r in results {
                            phase_urls.push(r.url);
                        }
                    }
                    Ok(_) => {
                        // Non-search content returned — no URLs to resolve
                        ctx.source_signal_counts
                            .entry(canonical_key)
                            .or_default();
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
                let raw_result = self.archive.fetch(url).content().await;
                let html = match raw_result {
                    Ok(Content::Page(page)) => Some(page.raw_html),
                    Ok(Content::Raw(text)) => Some(text),
                    Ok(_) => None,
                    Err(e) => {
                        warn!(url = url.as_str(), error = %e, "Query scrape failed");
                        None
                    }
                };
                match html {
                    Some(html) if !html.is_empty() => {
                        let links = rootsignal_archive::extract_links_by_pattern(&html, url, pattern);
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
                    let feed_result = self.archive.fetch(feed_url).content().await
                        .map(|content| match content {
                            Content::Feed(items) => items,
                            _ => Vec::new(),
                        })
                        .map_err(|e| anyhow::anyhow!("{e}"));
                    match feed_result {
                        Ok(items) => {
                            run_log.log(EventKind::ScrapeFeed {
                                url: feed_url.clone(),
                                items: items.len() as u32,
                            });
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
        let archive = self.archive.clone();
        let writer = self.writer.clone();
        let extractor = self.extractor.clone();
        let pipeline_results: Vec<_> = stream::iter(phase_urls.into_iter().map(|url| {
            let archive = archive.clone();
            let writer = writer.clone();
            let extractor = extractor.clone();
            async move {
                let clean_url = sanitize_url(&url);

                let content = match archive.fetch(&url).text().await {
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
        let known_urls = ctx.known_urls();
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
                                ctx.expansion_queries
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
                            run_log,
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
                    run_log.log(EventKind::ScrapeUrl {
                        url: url.clone(),
                        strategy: "web".to_string(),
                        success: false,
                        content_bytes: 0,
                    });
                    ctx.stats.urls_failed += 1;
                }
            }
        }
    }

    /// Scrape social media accounts, feed posts through LLM extraction.
    pub async fn run_social(&self, social_sources: &[&SourceNode], ctx: &mut RunContext, run_log: &mut RunLog) {
        type SocialResult = Option<(
            String,
            String,
            String,
            Vec<Node>,
            Vec<(Uuid, Vec<ResourceTag>)>,
            Vec<(Uuid, Vec<String>)>,
            usize,
        )>; // (canonical_key, source_url, combined_text, nodes, resource_tags, signal_tags, post_count)

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
                SocialPlatform::Instagram => {
                    (SocialPlatform::Instagram, source.url.as_deref().unwrap_or(&source.canonical_value).to_string())
                }
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
                SocialPlatform::Twitter => {
                    (SocialPlatform::Twitter, source.url.as_deref().unwrap_or(&source.canonical_value).to_string())
                }
                SocialPlatform::TikTok => {
                    (SocialPlatform::TikTok, source.url.as_deref().unwrap_or(&source.canonical_value).to_string())
                }
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
                ctx.actor_contexts.get(ck).map(|ac| {
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
        let mut futures: Vec<Pin<Box<dyn Future<Output = SocialResult> + Send>>> =
            Vec::new();

        let archive = self.archive.clone();
        let extractor = self.extractor.clone();
        for (canonical_key, source_url, account) in &accounts {
            let canonical_key = canonical_key.clone();
            let source_url = source_url.clone();
            let is_reddit = matches!(account.platform, SocialPlatform::Reddit);
            let actor_prefix = actor_prefixes.get(&canonical_key).cloned();
            let firsthand_prefix = if actor_prefix.is_none() {
                Some(firsthand_filter.to_string())
            } else {
                None
            };
            let archive = archive.clone();
            let extractor = extractor.clone();
            let identifier = account.identifier.clone();

            futures.push(Box::pin(async move {
                let posts = match archive.fetch(&identifier).content().await {
                    Ok(Content::SocialPosts(posts)) => posts,
                    Ok(_) => Vec::new(),
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

        let known_urls = ctx.known_urls();
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
                    &known_urls,
                    run_log,
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
    pub async fn discover_from_topics(&self, topics: &[String], ctx: &mut RunContext, run_log: &mut RunLog) {
        const MAX_SOCIAL_SEARCHES: usize = 6;
        const MAX_NEW_ACCOUNTS: usize = 10;
        const POSTS_PER_SEARCH: u32 = 30;
        const MAX_SITE_SEARCH_TOPICS: usize = 2;
        const SITE_SEARCH_RESULTS: usize = 5;

        if topics.is_empty() {
            return;
        }

        info!(topics = ?topics, "Starting social topic discovery...");

        let known_urls = ctx.known_urls();

        // Load existing sources for dedup across all platforms
        let existing_sources = self
            .writer
            .get_active_sources()
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
            SocialPlatform::Reddit,
        ];

        for platform in &platforms {
            if new_accounts >= MAX_NEW_ACCOUNTS as u32 {
                break;
            }

            let platform_name = match platform {
                SocialPlatform::Instagram => "instagram",
                SocialPlatform::Twitter => "x",
                SocialPlatform::TikTok => "tiktok",
                SocialPlatform::Reddit => "reddit",
                _ => continue,
            };

            let social_search = rootsignal_archive::SocialSearch {
                platform: *platform,
                topics: topic_strs.iter().map(|s| s.to_string()).collect(),
                limit: POSTS_PER_SEARCH,
            };
            let discovered_posts = match self.archive.fetch(&social_search.to_string()).content().await {
                Ok(Content::SocialPosts(posts)) => posts,
                Ok(_) => Vec::new(),
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
                    SocialPlatform::Reddit => {
                        format!("https://www.reddit.com/user/{username}/")
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
                        &known_urls,
                        run_log,
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

                match self.writer.upsert_source(&source).await {
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
                let results = match self.archive.fetch(&query).content().await {
                    Ok(Content::SearchResults(r)) => r,
                    Ok(_) => continue,
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
                    if known_urls.contains(&result.url) {
                        continue;
                    }

                    let content = match self.archive.fetch(&result.url).text().await {
                        Ok(c) if !c.is_empty() => c,
                        Ok(_) => continue,
                        Err(e) => {
                            warn!(url = result.url.as_str(), error = %e, "Site-scoped scrape failed");
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
                            &known_urls,
                            run_log,
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
        known_urls: &HashSet<String>,
        run_log: &mut RunLog,
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
            if let Some(meta) = node.meta_mut() {
                meta.confidence = q.confidence;
                meta.source_url = url.clone();
            }
        }

        // Geographic filtering: reject off-geography signals and backfill
        // region-center coordinates on survivors that lack coords.
        let geo_config = geo_filter::GeoFilterConfig {
            center_lat: self.region.center_lat,
            center_lng: self.region.center_lng,
            radius_km: self.region.radius_km,
            geo_terms: &self.region.geo_terms,
        };
        let is_known_source = known_urls.contains(&url);
        let before_geo = nodes.len();
        let (nodes, geo_stats) = geo_filter::filter_nodes(nodes, &geo_config, is_known_source);
        ctx.stats.geo_filtered += geo_stats.filtered;
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
                    run_log.log(EventKind::SignalCorroborated {
                        existing_id: existing_id.to_string(),
                        signal_type: format!("{}", node.node_type()),
                        new_source_url: url.clone(),
                        similarity: 1.0,
                    });
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
                        channel_type: Some(channel_type(&url)),
                    };
                    self.writer
                        .create_evidence(&evidence, *existing_id)
                        .await?;
                    ctx.stats.signals_deduplicated += 1;
                    continue;
                } else {
                    // Same-source re-scrape: signal already exists from this URL.
                    // Refresh to prove it's still active, but don't inflate corroboration.
                    run_log.log(EventKind::SignalDeduplicated {
                        signal_type: format!("{}", node.node_type()),
                        title: node.title().to_string(),
                        matched_id: existing_id.to_string(),
                        similarity: 1.0,
                        action: "refresh".to_string(),
                    });
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
                        channel_type: Some(channel_type(&url)),
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
                    run_log.log(EventKind::SignalDeduplicated {
                        signal_type: format!("{}", node_type),
                        title: node.title().to_string(),
                        matched_id: cached_id.to_string(),
                        similarity: sim,
                        action: "refresh".to_string(),
                    });
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
                        channel_type: Some(channel_type(&url)),
                    };
                    self.writer.create_evidence(&evidence, cached_id).await?;

                    ctx.stats.signals_deduplicated += 1;
                    continue;
                } else if sim >= 0.92 {
                    run_log.log(EventKind::SignalCorroborated {
                        existing_id: cached_id.to_string(),
                        signal_type: format!("{}", cached_type),
                        new_source_url: url.clone(),
                        similarity: sim,
                    });
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
                        channel_type: Some(channel_type(&url)),
                    };
                    self.writer.create_evidence(&evidence, cached_id).await?;

                    ctx.stats.signals_deduplicated += 1;
                    continue;
                }
            }

            // 3b: Check graph index (catches dupes from previous runs, region-scoped)
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
                        run_log.log(EventKind::SignalDeduplicated {
                            signal_type: format!("{}", dup.node_type),
                            title: node.title().to_string(),
                            matched_id: dup.id.to_string(),
                            similarity: dup.similarity,
                            action: "refresh".to_string(),
                        });
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
                            channel_type: Some(channel_type(&url)),
                        };
                        self.writer.create_evidence(&evidence, dup.id).await?;

                        ctx.embed_cache
                            .add(embedding, dup.id, dup.node_type, dominated_url);

                        ctx.stats.signals_deduplicated += 1;
                        continue;
                    } else if dup.similarity >= 0.92 {
                        let cross_type = dup.node_type != node_type;
                        run_log.log(EventKind::SignalCorroborated {
                            existing_id: dup.id.to_string(),
                            signal_type: format!("{}", dup.node_type),
                            new_source_url: url.clone(),
                            similarity: dup.similarity,
                        });
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
                            channel_type: Some(channel_type(&url)),
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

            run_log.log(EventKind::SignalCreated {
                node_id: node_id.to_string(),
                signal_type: format!("{}", node_type),
                title: node.title().to_string(),
                confidence: node.meta().map(|m| m.confidence as f64).unwrap_or(0.0),
                source_url: url.clone(),
            });

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
                channel_type: Some(channel_type(&url)),
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
                                bio: None,
                                location_lat: None,
                                location_lng: None,
                                location_name: None,
                            };
                            if let Err(e) = self.writer.upsert_actor(&actor).await {
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
