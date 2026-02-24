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

use rootsignal_common::{
    canonical_value, channel_type, is_web_query, scraping_strategy, ActorNode, ActorType, ActorContext, ScoutScope,
    DiscoveryMethod, EvidenceNode, Node, NodeType, Post, ScrapingStrategy,
    SocialPlatform, SourceNode, SourceRole,
};
use crate::enrichment::link_promoter;
use crate::infra::embedder::TextEmbedder;
use crate::pipeline::extractor::{ResourceTag, SignalExtractor};
use crate::enrichment::quality;
use crate::infra::run_log::{EventKind, RunLog};
use crate::pipeline::stats::ScoutStats;
use crate::infra::util::{content_hash, sanitize_url};

// ---------------------------------------------------------------------------
// CollectedLink — a discovered outbound link with its provenance
// ---------------------------------------------------------------------------

/// A link discovered during scraping, used by `promote_links` to create new sources.
pub struct CollectedLink {
    pub url: String,
    pub discovered_on: String,
}

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
    /// RSS/Atom pub_date keyed by article URL, used as fallback content_date.
    pub url_to_pub_date: HashMap<String, DateTime<Utc>>,
    /// Links collected during scraping, carrying the discovering source's coordinates.
    pub collected_links: Vec<CollectedLink>,
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
            url_to_pub_date: HashMap::new(),
            collected_links: Vec::new(),
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
pub(crate) fn normalize_title(title: &str) -> String {
    title.trim().to_lowercase()
}

/// Within-batch dedup by (normalized_title, node_type).
/// Keeps the first occurrence of each (title, type) pair, drops duplicates.
pub(crate) fn batch_title_dedup(nodes: Vec<Node>) -> Vec<Node> {
    let mut seen = HashSet::new();
    nodes
        .into_iter()
        .filter(|n| seen.insert((normalize_title(n.title()), n.node_type())))
        .collect()
}

/// Returns true if this scraping strategy represents an "owned" source — one
/// where the author of the content is the account holder, not an aggregator.
/// Social accounts and dedicated web pages are owned; RSS feeds and web
/// queries aggregate content from many authors.
pub(crate) fn is_owned_source(strategy: &ScrapingStrategy) -> bool {
    matches!(strategy, ScrapingStrategy::Social(_))
}

/// Scores quality, populates from/about locations, and removes Evidence nodes.
///
/// Pure pipeline step: given raw extracted nodes, returns signal nodes with
/// quality scores, source URLs, and location provenance.
pub(crate) fn score_and_filter(
    mut nodes: Vec<Node>,
    url: &str,
    actor_ctx: Option<&ActorContext>,
) -> Vec<Node> {
    // 1. Score quality and stamp source URL
    for node in &mut nodes {
        let q = quality::score(node);
        if let Some(meta) = node.meta_mut() {
            meta.confidence = q.confidence;
            meta.source_url = url.to_string();
        }
    }

    // 2. Filter to signal nodes only (skip Evidence)
    nodes
        .into_iter()
        .filter(|n| !matches!(n.node_type(), NodeType::Evidence))
        .collect()
}

// ---------------------------------------------------------------------------
// DedupVerdict — pure decision function for multi-layer deduplication
// ---------------------------------------------------------------------------

/// Threshold for cross-source corroboration via vector similarity.
/// Same-source matches always refresh regardless of similarity (as long as
/// they passed the 0.85 entry threshold from the caller).
const CROSS_SOURCE_SIM_THRESHOLD: f64 = 0.92;

/// The outcome of the multi-layer deduplication check for a single signal node.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DedupVerdict {
    /// No existing match — create a new signal node.
    Create,
    /// Cross-source match — corroborate the existing signal.
    Corroborate {
        existing_id: Uuid,
        existing_type: NodeType,
        similarity: f64,
    },
    /// Same-source match — refresh (re-confirm) the existing signal.
    Refresh {
        existing_id: Uuid,
        existing_type: NodeType,
        similarity: f64,
    },
}

impl DedupVerdict {
    /// Returns the existing signal ID if the verdict is not Create.
    #[cfg(test)]
    fn existing_id(&self) -> Option<Uuid> {
        match self {
            DedupVerdict::Create => None,
            DedupVerdict::Corroborate { existing_id, .. } => Some(*existing_id),
            DedupVerdict::Refresh { existing_id, .. } => Some(*existing_id),
        }
    }
}

/// Pure decision function for the multi-layer dedup pipeline.
///
/// Layers are checked in priority order:
/// 1. Global exact title+type match (similarity = 1.0)
/// 2. In-memory embed cache match (≥0.85 entry, ≥0.92 cross-source)
/// 3. Graph vector index match (≥0.85 entry, ≥0.92 cross-source)
/// 4. No match → Create
///
/// Within each layer, same-source → Refresh, cross-source above threshold → Corroborate.
/// All URLs should be pre-sanitized before calling.
pub(crate) fn dedup_verdict(
    current_url: &str,
    node_type: NodeType,
    global_match: Option<(Uuid, &str)>,
    cache_match: Option<(Uuid, NodeType, &str, f64)>,
    graph_match: Option<(Uuid, NodeType, &str, f64)>,
) -> DedupVerdict {
    // Layer 2.5: Global exact title+type match — always acts (no threshold)
    if let Some((existing_id, existing_url)) = global_match {
        return if existing_url != current_url {
            DedupVerdict::Corroborate {
                existing_id,
                existing_type: node_type,
                similarity: 1.0,
            }
        } else {
            DedupVerdict::Refresh {
                existing_id,
                existing_type: node_type,
                similarity: 1.0,
            }
        };
    }

    // Layer 3a: In-memory embed cache
    if let Some((cached_id, cached_type, cached_url, sim)) = cache_match {
        if cached_url == current_url {
            return DedupVerdict::Refresh {
                existing_id: cached_id,
                existing_type: cached_type,
                similarity: sim,
            };
        } else if sim >= CROSS_SOURCE_SIM_THRESHOLD {
            return DedupVerdict::Corroborate {
                existing_id: cached_id,
                existing_type: cached_type,
                similarity: sim,
            };
        }
    }

    // Layer 3b: Graph vector index
    if let Some((dup_id, dup_type, dup_url, sim)) = graph_match {
        if dup_url == current_url {
            return DedupVerdict::Refresh {
                existing_id: dup_id,
                existing_type: dup_type,
                similarity: sim,
            };
        } else if sim >= CROSS_SOURCE_SIM_THRESHOLD {
            return DedupVerdict::Corroborate {
                existing_id: dup_id,
                existing_type: dup_type,
                similarity: sim,
            };
        }
    }

    // Layer 4: No match
    DedupVerdict::Create
}

// ---------------------------------------------------------------------------
// ScrapePhase — the core scrape-extract-store-dedup pipeline
// ---------------------------------------------------------------------------

pub(crate) struct ScrapePhase {
    store: Arc<dyn super::traits::SignalStore>,
    extractor: Arc<dyn SignalExtractor>,
    embedder: Arc<dyn TextEmbedder>,
    fetcher: Arc<dyn super::traits::ContentFetcher>,
    region: ScoutScope,
    run_id: String,
}

impl ScrapePhase {
    pub fn new(
        store: Arc<dyn super::traits::SignalStore>,
        extractor: Arc<dyn SignalExtractor>,
        embedder: Arc<dyn TextEmbedder>,
        fetcher: Arc<dyn super::traits::ContentFetcher>,
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
            let fetcher = self.fetcher.clone();
            let query_inputs: Vec<_> = api_queries
                .iter()
                .map(|source| (source.canonical_key.clone(), source.canonical_value.clone()))
                .collect();
            let search_results: Vec<_> = stream::iter(query_inputs.into_iter().map(|(canonical_key, query_str)| {
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
                            ctx.url_to_canonical_key
                                .entry(clean)
                                .or_insert_with(|| canonical_key.clone());
                        }
                        ctx.source_signal_counts
                            .entry(canonical_key.clone())
                            .or_default();
                        for r in archived.results {
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
                let html = match self.fetcher.page(url).await {
                    Ok(page) => Some(page.raw_html),
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
                    let feed_result = self.fetcher.feed(feed_url).await;
                    match feed_result {
                        Ok(archived) => {
                            run_log.log(EventKind::ScrapeFeed {
                                url: feed_url.clone(),
                                items: archived.items.len() as u32,
                            });
                            ctx.source_signal_counts
                                .entry(source.canonical_key.clone())
                                .or_default();
                            for item in archived.items {
                                ctx.url_to_canonical_key
                                    .entry(item.url.clone())
                                    .or_insert_with(|| source.canonical_key.clone());
                                if let Some(pub_date) = item.pub_date {
                                    ctx.url_to_pub_date.insert(item.url.clone(), pub_date);
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
            return;
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
        let known_urls = ctx.known_urls();
        let ck_to_source_id: HashMap<String, Uuid> = sources
            .iter()
            .map(|s| (s.canonical_key.clone(), s.id))
            .collect();
        for (url, outcome, page_links) in pipeline_results {
            // Extract outbound links for promotion as new sources
            let discovered = link_promoter::extract_links(&page_links);
            for link_url in discovered {
                ctx.collected_links.push(CollectedLink {
                    url: link_url,
                    discovered_on: url.clone(),
                });
            }

            let ck = ctx
                .url_to_canonical_key
                .get(&url)
                .cloned()
                .unwrap_or_else(|| url.clone());
            match outcome {
                ScrapeOutcome::New {
                    content,
                    mut nodes,
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

                    // Apply RSS/Atom pub_date as fallback content_date
                    if let Some(pub_date) = ctx.url_to_pub_date.get(&url) {
                        for node in &mut nodes {
                            if let Some(meta) = node.meta_mut() {
                                if meta.content_date.is_none() {
                                    meta.content_date = Some(*pub_date);
                                }
                            }
                        }
                    }

                    let source_id = ck_to_source_id.get(&ck).copied();
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
                            source_id,
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
                    match self.store.refresh_url_signals(&url, now).await {
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
            SocialPlatform,
            String,
            Vec<Node>,
            Vec<(Uuid, Vec<ResourceTag>)>,
            Vec<(Uuid, Vec<String>)>,
            usize,
            Vec<String>,
            Option<DateTime<Utc>>, // most recent published_at for content_date fallback
        )>; // (canonical_key, source_url, platform, combined_text, nodes, resource_tags, signal_tags, post_count, mentions, newest_published_at)

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

                // Find the most recent published_at for content_date fallback
                let newest_published_at = posts.iter()
                    .filter_map(|p| p.published_at)
                    .max();

                // Collect @mentions from posts
                let source_mentions: Vec<String> = posts.iter()
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
                        platform,
                        combined_all,
                        all_nodes,
                        all_resource_tags,
                        all_signal_tags,
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
                        post_count,
                        source_mentions,
                        newest_published_at,
                    ))
                }
            }));
        }

        let results: Vec<_> = stream::iter(futures).buffer_unordered(10).collect().await;

        let known_urls = ctx.known_urls();
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
                post_count,
                mentions,
                newest_published_at,
            ) = result;

            // Apply social published_at as fallback content_date when LLM didn't extract one
            if let Some(pub_at) = newest_published_at {
                for node in &mut nodes {
                    if let Some(meta) = node.meta_mut() {
                        if meta.content_date.is_none() {
                            meta.content_date = Some(pub_at);
                        }
                    }
                }
            }

            // Accumulate mentions as URLs for promotion (capped per source)
            for handle in mentions.into_iter().take(promotion_config.max_per_source) {
                let mention_url = link_promoter::platform_url(&result_platform, &handle);
                ctx.collected_links.push(CollectedLink {
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
                        ctx.expansion_queries
                            .extend(meta.implied_queries.iter().cloned());
                    }
                }
            }
            ctx.stats.social_media_posts += post_count as u32;
            let source_id = ck_to_source_id.get(&canonical_key).copied();
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
                    source_id,
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
        const MAX_SOCIAL_SEARCHES: usize = 10;
        const MAX_NEW_ACCOUNTS: usize = 10;
        const POSTS_PER_SEARCH: u32 = 30;
        const MAX_SITE_SEARCH_TOPICS: usize = 4;
        const SITE_SEARCH_RESULTS: usize = 5;

        if topics.is_empty() {
            return;
        }

        info!(topics = ?topics, "Starting social topic discovery...");

        let known_urls = ctx.known_urls();

        // Load existing sources for dedup across all platforms
        let existing_sources = self
            .store
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

            let discovered_posts = match self.fetcher.search_topics(platform_url, &topic_strs, POSTS_PER_SEARCH).await {
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

            ctx.stats.discovery_posts_found += discovered_posts.len() as u32;

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
                        None,
                    )
                    .await
                {
                    warn!(username, error = %e, "Failed to store discovery signals");
                    continue;
                }
                let produced = ctx.stats.signals_stored - signal_count_before;

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

                *ctx.source_signal_counts.entry(ck).or_default() += produced;

                match self.store.upsert_source(&source).await {
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

                let search_results = match self.fetcher.site_search(&query, SITE_SEARCH_RESULTS).await {
                    Ok(r) => r,
                    Err(e) => {
                        warn!(query, error = %e, "Site-scoped search failed");
                        continue;
                    }
                };

                if search_results.results.is_empty() {
                    continue;
                }

                info!(query, count = search_results.results.len(), "Site-scoped search results");

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
                            None,
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
        source_id: Option<Uuid>,
    ) -> Result<()> {
        let url = sanitize_url(url);
        ctx.stats.signals_extracted += nodes.len() as u32;

        // Build lookup map from node ID → resource tags
        let resource_map: HashMap<Uuid, Vec<ResourceTag>> = resource_tags.into_iter().collect();

        // Build lookup map from extraction-time node ID → tag slugs
        let tag_map: HashMap<Uuid, Vec<String>> = signal_tags.into_iter().collect();

        // Entity mappings for source diversity (domain-based fallback in resolve_entity handles it)
        let entity_mappings: Vec<rootsignal_common::EntityMappingOwned> = Vec::new();

        // Score quality, populate from/about locations, remove Evidence nodes
        let ck_for_fallback = ctx
            .url_to_canonical_key
            .get(&url)
            .cloned()
            .unwrap_or_else(|| url.clone());
        let actor_ctx = ctx.actor_contexts.get(&ck_for_fallback);
        let nodes = score_and_filter(nodes, &url, actor_ctx);

        if nodes.is_empty() {
            return Ok(());
        }

        // --- Layer 1: Within-batch dedup by (normalized_title, node_type) ---
        let nodes = batch_title_dedup(nodes);

        // --- Layer 2: URL-based title dedup against existing database ---
        let existing_titles: HashSet<String> = self
            .store
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
            .store
            .find_by_titles_and_types(&title_type_pairs)
            .await
            .unwrap_or_default();

        let mut remaining_nodes = Vec::new();
        for node in nodes {
            let key = (normalize_title(node.title()), node.node_type());
            let global_hit = global_matches
                .get(&key)
                .map(|(id, u)| (*id, u.as_str()));

            match dedup_verdict(&url, node.node_type(), global_hit, None, None) {
                DedupVerdict::Corroborate { existing_id, existing_type, similarity } => {
                    run_log.log(EventKind::SignalCorroborated {
                        existing_id: existing_id.to_string(),
                        signal_type: format!("{}", node.node_type()),
                        new_source_url: url.clone(),
                        similarity,
                    });
                    info!(
                        existing_id = %existing_id,
                        title = node.title(),
                        new_source = url.as_str(),
                        "Global title+type match from different source, corroborating"
                    );
                    self.store
                        .corroborate(existing_id, existing_type, now, &entity_mappings)
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
                    self.store
                        .create_evidence(&evidence, existing_id)
                        .await?;
                    ctx.stats.signals_deduplicated += 1;
                }
                DedupVerdict::Refresh { existing_id, existing_type, similarity } => {
                    run_log.log(EventKind::SignalDeduplicated {
                        signal_type: format!("{}", node.node_type()),
                        title: node.title().to_string(),
                        matched_id: existing_id.to_string(),
                        similarity,
                        action: "refresh".to_string(),
                    });
                    info!(
                        existing_id = %existing_id,
                        title = node.title(),
                        source = url.as_str(),
                        "Same-source title match, refreshing (no corroboration)"
                    );
                    self.store
                        .refresh_signal(existing_id, existing_type, now)
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
                    self.store
                        .create_evidence(&evidence, existing_id)
                        .await?;
                    ctx.stats.signals_deduplicated += 1;
                }
                DedupVerdict::Create => {
                    remaining_nodes.push(node);
                }
            }
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
            let cache_hit = ctx.embed_cache.find_match(&embedding, 0.85);

            // 3b: Check graph index (catches dupes from previous runs, region-scoped)
            let lat_delta = self.region.radius_km / 111.0;
            let lng_delta = self.region.radius_km
                / (111.0 * self.region.center_lat.to_radians().cos());
            let graph_hit = match self
                .store
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
                    let sanitized = sanitize_url(&dup.source_url);
                    Some((dup.id, dup.node_type, sanitized, dup.similarity))
                }
                Ok(None) => None,
                Err(e) => {
                    warn!(error = %e, "Dedup check failed, proceeding with creation");
                    None
                }
            };

            let cache_match = cache_hit.as_ref().map(|(id, ty, u, s)| (*id, *ty, &**u, *s));
            let graph_match = graph_hit.as_ref().map(|(id, ty, u, s)| (*id, *ty, &**u, *s));

            match dedup_verdict(&url, node_type, None, cache_match, graph_match) {
                DedupVerdict::Refresh { existing_id, existing_type, similarity } => {
                    let source_layer = if cache_hit.is_some() { "cache" } else { "graph" };
                    run_log.log(EventKind::SignalDeduplicated {
                        signal_type: format!("{}", existing_type),
                        title: node.title().to_string(),
                        matched_id: existing_id.to_string(),
                        similarity,
                        action: "refresh".to_string(),
                    });
                    info!(
                        existing_id = %existing_id,
                        similarity,
                        title = node.title(),
                        source = source_layer,
                        "Same-source duplicate, refreshing (no corroboration)"
                    );
                    self.store
                        .refresh_signal(existing_id, existing_type, now)
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
                    self.store.create_evidence(&evidence, existing_id).await?;
                    // Update embed cache if verdict came from graph
                    if cache_hit.is_none() {
                        if let Some((_, _, ref sanitized_url, _)) = graph_hit {
                            ctx.embed_cache.add(embedding, existing_id, existing_type, sanitized_url.clone());
                        }
                    }
                    ctx.stats.signals_deduplicated += 1;
                    continue;
                }
                DedupVerdict::Corroborate { existing_id, existing_type, similarity } => {
                    let source_layer = if cache_match.map(|c| c.0) == Some(existing_id) { "cache" } else { "graph" };
                    run_log.log(EventKind::SignalCorroborated {
                        existing_id: existing_id.to_string(),
                        signal_type: format!("{}", existing_type),
                        new_source_url: url.clone(),
                        similarity,
                    });
                    info!(
                        existing_id = %existing_id,
                        similarity,
                        title = node.title(),
                        source = source_layer,
                        "Cross-source duplicate, corroborating"
                    );
                    self.store
                        .corroborate(existing_id, existing_type, now, &entity_mappings)
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
                    self.store.create_evidence(&evidence, existing_id).await?;
                    // Update embed cache if verdict came from graph
                    if cache_hit.is_none() {
                        if let Some((_, _, ref sanitized_url, _)) = graph_hit {
                            ctx.embed_cache.add(embedding, existing_id, existing_type, sanitized_url.clone());
                        }
                    }
                    ctx.stats.signals_deduplicated += 1;
                    continue;
                }
                DedupVerdict::Create => {}
            }

            // Create new node
            let node_id = self.store.create_node(&node, &embedding, "scraper", &self.run_id).await?;

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
            self.store.create_evidence(&evidence, node_id).await?;

            // Wire PRODUCED_BY edge (signal → source)
            if let Some(sid) = source_id {
                if let Err(e) = self.store.link_signal_to_source(node_id, sid).await {
                    warn!(error = %e, "Failed to link signal to source (non-fatal)");
                }
            }

            // Resolve author_actor → Actor node on owned sources only.
            // Mentioned actors are kept as metadata strings but do NOT create nodes.
            let strategy = scraping_strategy(&url);
            if is_owned_source(&strategy) {
                if let Some(meta) = node.meta() {
                    if let Some(author_name) = &meta.author_actor {
                        let author_name = author_name.trim();
                        if !author_name.is_empty() {
                            let entity_id = canonical_value(&url);
                            let actor_id = match self.store.find_actor_by_entity_id(&entity_id).await {
                                Ok(Some(id)) => Some(id),
                                Ok(None) => {
                                    let actor = ActorNode {
                                        id: Uuid::new_v4(),
                                        name: author_name.to_string(),
                                        actor_type: ActorType::Organization,
                                        entity_id: entity_id.clone(),
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
                                        discovery_depth: actor_ctx.map(|ac| ac.discovery_depth + 1).unwrap_or(0),
                                    };
                                    match self.store.upsert_actor(&actor).await {
                                        Ok(_) => {
                                            // Wire HAS_SOURCE edge (actor → source)
                                            if let Some(sid) = source_id {
                                                if let Err(e) = self.store.link_actor_to_source(actor.id, sid).await {
                                                    warn!(error = %e, actor = author_name, "Failed to link actor to source (non-fatal)");
                                                }
                                            }
                                            Some(actor.id)
                                        }
                                        Err(e) => {
                                            warn!(error = %e, actor = author_name, "Failed to create author actor (non-fatal)");
                                            None
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!(error = %e, actor = author_name, "Actor entity_id lookup failed (non-fatal)");
                                    None
                                }
                            };
                            if let Some(actor_id) = actor_id {
                                if let Err(e) = self
                                    .store
                                    .link_actor_to_signal(actor_id, node_id, "authored")
                                    .await
                                {
                                    warn!(error = %e, actor = author_name, "Failed to link author actor to signal (non-fatal)");
                                }
                            }
                        }
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
                            .store
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
                                self.store
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
                                self.store
                                    .create_prefers_edge(node_id, resource_id, confidence)
                                    .await
                            }
                            "offers" => {
                                self.store
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
                            self.store.batch_tag_signals(node_id, slugs).await
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

    /// Enrich actor locations by triangulating from their authored signals.
    ///
    /// Finds all actors active in this phase's region, then calls
    /// `enrich_actor_locations` to update each actor's location from signal mode.
    pub async fn enrich_actors(&self) {
        let actors = self.store.list_all_actors().await.unwrap_or_default();
        let updated = crate::enrichment::actor_location::enrich_actor_locations(
            &*self.store,
            &actors,
        )
        .await;
        if updated > 0 {
            info!(updated, "Enriched actor locations");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_title_trims_whitespace() {
        assert_eq!(normalize_title("  Free Legal Clinic  "), "free legal clinic");
    }

    #[test]
    fn normalize_title_lowercases() {
        assert_eq!(normalize_title("FREE LEGAL CLINIC"), "free legal clinic");
    }

    #[test]
    fn normalize_title_mixed_case_and_whitespace() {
        assert_eq!(normalize_title("  Community Garden CLEANUP  "), "community garden cleanup");
    }

    #[test]
    fn normalize_title_empty() {
        assert_eq!(normalize_title(""), "");
    }

    #[test]
    fn normalize_title_already_normalized() {
        assert_eq!(normalize_title("food distribution"), "food distribution");
    }

    // --- batch_title_dedup tests ---

    use rootsignal_common::safety::SensitivityLevel;
    use rootsignal_common::types::{Severity, Urgency, TensionNode, NeedNode, NodeMeta};

    fn tension(title: &str) -> Node {
        Node::Tension(TensionNode {
            meta: NodeMeta {
                id: Uuid::new_v4(),
                title: title.to_string(),
                summary: String::new(),
                sensitivity: SensitivityLevel::General,
                confidence: 0.8,
                freshness_score: 0.9,
                corroboration_count: 0,
                about_location: None,
                about_location_name: None,
                from_location: None,
                source_url: "https://example.com".to_string(),
                extracted_at: Utc::now(),
                content_date: None,
                last_confirmed_active: Utc::now(),
                source_diversity: 1,
                external_ratio: 0.0,
                cause_heat: 0.0,
                implied_queries: Vec::new(),
                channel_diversity: 1,
                mentioned_actors: Vec::new(),
                author_actor: None,
            },
            severity: Severity::Medium,
            category: None,
            what_would_help: None,
        })
    }

    fn need(title: &str) -> Node {
        Node::Need(NeedNode {
            meta: NodeMeta {
                id: Uuid::new_v4(),
                title: title.to_string(),
                summary: String::new(),
                sensitivity: SensitivityLevel::General,
                confidence: 0.8,
                freshness_score: 0.9,
                corroboration_count: 0,
                about_location: None,
                about_location_name: None,
                from_location: None,
                source_url: "https://example.com".to_string(),
                extracted_at: Utc::now(),
                content_date: None,
                last_confirmed_active: Utc::now(),
                source_diversity: 1,
                external_ratio: 0.0,
                cause_heat: 0.0,
                implied_queries: Vec::new(),
                channel_diversity: 1,
                mentioned_actors: Vec::new(),
                author_actor: None,
            },
            urgency: Urgency::Medium,
            what_needed: None,
            action_url: None,
            goal: None,
        })
    }

    #[test]
    fn batch_dedup_removes_same_title_same_type() {
        let nodes = vec![
            tension("Housing Crisis"),
            tension("Housing Crisis"),
            tension("Bus Route Cut"),
        ];
        let deduped = batch_title_dedup(nodes);
        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].title(), "Housing Crisis");
        assert_eq!(deduped[1].title(), "Bus Route Cut");
    }

    #[test]
    fn batch_dedup_keeps_same_title_different_type() {
        let nodes = vec![
            tension("Housing Crisis"),
            need("Housing Crisis"),
        ];
        let deduped = batch_title_dedup(nodes);
        assert_eq!(deduped.len(), 2);
    }

    #[test]
    fn batch_dedup_case_insensitive() {
        let nodes = vec![
            tension("housing crisis"),
            tension("HOUSING CRISIS"),
            tension("Housing Crisis"),
        ];
        let deduped = batch_title_dedup(nodes);
        assert_eq!(deduped.len(), 1);
    }

    #[test]
    fn batch_dedup_whitespace_normalized() {
        let nodes = vec![
            tension("  Housing Crisis  "),
            tension("Housing Crisis"),
        ];
        let deduped = batch_title_dedup(nodes);
        assert_eq!(deduped.len(), 1);
    }

    #[test]
    fn batch_dedup_empty_input() {
        let deduped = batch_title_dedup(Vec::new());
        assert!(deduped.is_empty());
    }

    #[test]
    fn batch_dedup_all_unique() {
        let nodes = vec![
            tension("Housing Crisis"),
            tension("Bus Route Cut"),
            need("Food Distribution"),
        ];
        let deduped = batch_title_dedup(nodes);
        assert_eq!(deduped.len(), 3);
    }

    // -----------------------------------------------------------------------
    // dedup_verdict tests
    // -----------------------------------------------------------------------

    const URL_A: &str = "https://example.com/page-a";
    const URL_B: &str = "https://other.com/page-b";

    fn id1() -> Uuid {
        Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()
    }
    fn id2() -> Uuid {
        Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap()
    }
    fn id3() -> Uuid {
        Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap()
    }

    // --- Layer 2.5: Global title match ---

    #[test]
    fn global_match_cross_source_corroborates() {
        let v = dedup_verdict(URL_A, NodeType::Tension, Some((id1(), URL_B)), None, None);
        assert_eq!(v, DedupVerdict::Corroborate {
            existing_id: id1(),
            existing_type: NodeType::Tension,
            similarity: 1.0,
        });
    }

    #[test]
    fn global_match_same_source_refreshes() {
        let v = dedup_verdict(URL_A, NodeType::Tension, Some((id1(), URL_A)), None, None);
        assert_eq!(v, DedupVerdict::Refresh {
            existing_id: id1(),
            existing_type: NodeType::Tension,
            similarity: 1.0,
        });
    }

    #[test]
    fn global_match_uses_new_node_type() {
        // Global match always uses the new node's type, not a stored type
        let v = dedup_verdict(URL_A, NodeType::Aid, Some((id1(), URL_B)), None, None);
        assert_eq!(v, DedupVerdict::Corroborate {
            existing_id: id1(),
            existing_type: NodeType::Aid,
            similarity: 1.0,
        });
    }

    #[test]
    fn global_match_takes_priority_over_cache() {
        let v = dedup_verdict(
            URL_A,
            NodeType::Tension,
            Some((id1(), URL_B)),                         // global: corroborate
            Some((id2(), NodeType::Tension, URL_A, 0.99)), // cache: would refresh
            None,
        );
        assert_eq!(v.existing_id(), Some(id1()), "global match should win over cache");
    }

    #[test]
    fn global_match_takes_priority_over_graph() {
        let v = dedup_verdict(
            URL_A,
            NodeType::Tension,
            Some((id1(), URL_A)),                         // global: refresh
            None,
            Some((id3(), NodeType::Tension, URL_B, 0.95)), // graph: would corroborate
        );
        assert_eq!(v.existing_id(), Some(id1()), "global match should win over graph");
    }

    // --- Layer 3a: Cache match ---

    #[test]
    fn cache_same_source_refreshes() {
        let v = dedup_verdict(
            URL_A, NodeType::Need, None,
            Some((id2(), NodeType::Need, URL_A, 0.88)),
            None,
        );
        assert_eq!(v, DedupVerdict::Refresh {
            existing_id: id2(),
            existing_type: NodeType::Need,
            similarity: 0.88,
        });
    }

    #[test]
    fn cache_cross_source_above_threshold_corroborates() {
        let v = dedup_verdict(
            URL_A, NodeType::Tension, None,
            Some((id2(), NodeType::Tension, URL_B, 0.95)),
            None,
        );
        assert_eq!(v, DedupVerdict::Corroborate {
            existing_id: id2(),
            existing_type: NodeType::Tension,
            similarity: 0.95,
        });
    }

    #[test]
    fn cache_cross_source_at_threshold_corroborates() {
        let v = dedup_verdict(
            URL_A, NodeType::Aid, None,
            Some((id2(), NodeType::Aid, URL_B, 0.92)),
            None,
        );
        assert_eq!(v, DedupVerdict::Corroborate {
            existing_id: id2(),
            existing_type: NodeType::Aid,
            similarity: 0.92,
        });
    }

    #[test]
    fn cache_cross_source_below_threshold_falls_through() {
        let v = dedup_verdict(
            URL_A, NodeType::Tension, None,
            Some((id2(), NodeType::Tension, URL_B, 0.91)),
            None,
        );
        assert_eq!(v, DedupVerdict::Create, "0.91 cross-source should fall through to Create");
    }

    #[test]
    fn cache_cross_source_at_entry_threshold_falls_through() {
        let v = dedup_verdict(
            URL_A, NodeType::Tension, None,
            Some((id2(), NodeType::Tension, URL_B, 0.85)),
            None,
        );
        assert_eq!(v, DedupVerdict::Create, "0.85 cross-source should fall through");
    }

    #[test]
    fn cache_takes_priority_over_graph() {
        let v = dedup_verdict(
            URL_A, NodeType::Tension, None,
            Some((id2(), NodeType::Tension, URL_A, 0.90)), // cache: same-source refresh
            Some((id3(), NodeType::Tension, URL_B, 0.95)), // graph: would corroborate
        );
        assert_eq!(v.existing_id(), Some(id2()), "cache should win over graph");
    }

    // --- Layer 3b: Graph match ---

    #[test]
    fn graph_same_source_refreshes() {
        let v = dedup_verdict(
            URL_A, NodeType::Notice, None, None,
            Some((id3(), NodeType::Notice, URL_A, 0.87)),
        );
        assert_eq!(v, DedupVerdict::Refresh {
            existing_id: id3(),
            existing_type: NodeType::Notice,
            similarity: 0.87,
        });
    }

    #[test]
    fn graph_cross_source_above_threshold_corroborates() {
        let v = dedup_verdict(
            URL_A, NodeType::Tension, None, None,
            Some((id3(), NodeType::Tension, URL_B, 0.95)),
        );
        assert_eq!(v, DedupVerdict::Corroborate {
            existing_id: id3(),
            existing_type: NodeType::Tension,
            similarity: 0.95,
        });
    }

    #[test]
    fn graph_cross_source_at_threshold_corroborates() {
        let v = dedup_verdict(
            URL_A, NodeType::Gathering, None, None,
            Some((id3(), NodeType::Gathering, URL_B, 0.92)),
        );
        assert_eq!(v, DedupVerdict::Corroborate {
            existing_id: id3(),
            existing_type: NodeType::Gathering,
            similarity: 0.92,
        });
    }

    #[test]
    fn graph_cross_source_below_threshold_creates() {
        let v = dedup_verdict(
            URL_A, NodeType::Tension, None, None,
            Some((id3(), NodeType::Tension, URL_B, 0.91)),
        );
        assert_eq!(v, DedupVerdict::Create);
    }

    // --- Layer 4: No match ---

    #[test]
    fn no_matches_creates() {
        let v = dedup_verdict(URL_A, NodeType::Tension, None, None, None);
        assert_eq!(v, DedupVerdict::Create);
    }

    #[test]
    fn both_below_threshold_creates() {
        let v = dedup_verdict(
            URL_A, NodeType::Tension, None,
            Some((id2(), NodeType::Tension, URL_B, 0.87)), // cache: cross-source, below 0.92
            Some((id3(), NodeType::Tension, URL_B, 0.89)), // graph: cross-source, below 0.92
        );
        assert_eq!(v, DedupVerdict::Create);
    }

    // --- Priority / interaction ---

    #[test]
    fn cache_below_threshold_falls_to_graph_refresh() {
        let v = dedup_verdict(
            URL_A, NodeType::Tension, None,
            Some((id2(), NodeType::Tension, URL_B, 0.87)), // cache: cross-source, below threshold → skip
            Some((id3(), NodeType::Tension, URL_A, 0.90)), // graph: same-source → refresh
        );
        assert_eq!(v, DedupVerdict::Refresh {
            existing_id: id3(),
            existing_type: NodeType::Tension,
            similarity: 0.90,
        });
    }

    #[test]
    fn cache_below_threshold_falls_to_graph_corroborate() {
        let v = dedup_verdict(
            URL_A, NodeType::Tension, None,
            Some((id2(), NodeType::Tension, URL_B, 0.88)), // cache: cross-source, below threshold
            Some((id3(), NodeType::Tension, URL_B, 0.93)), // graph: cross-source, above threshold
        );
        assert_eq!(v, DedupVerdict::Corroborate {
            existing_id: id3(),
            existing_type: NodeType::Tension,
            similarity: 0.93,
        });
    }

    // -----------------------------------------------------------------------
    // score_and_filter tests
    // -----------------------------------------------------------------------

    use rootsignal_common::types::GeoPoint;
    use rootsignal_common::GeoPrecision;

    fn tension_at(title: &str, lat: f64, lng: f64) -> Node {
        Node::Tension(TensionNode {
            meta: NodeMeta {
                id: Uuid::new_v4(),
                title: title.to_string(),
                summary: String::new(),
                sensitivity: SensitivityLevel::General,
                confidence: 0.0, // will be overwritten by score_and_filter
                freshness_score: 0.9,
                corroboration_count: 0,
                about_location: Some(GeoPoint { lat, lng, precision: GeoPrecision::Approximate }),
                about_location_name: None,
                from_location: None,
                source_url: String::new(),
                extracted_at: Utc::now(),
                content_date: None,
                last_confirmed_active: Utc::now(),
                source_diversity: 1,
                external_ratio: 0.0,
                cause_heat: 0.0,
                implied_queries: Vec::new(),
                channel_diversity: 1,
                mentioned_actors: Vec::new(),
                author_actor: None,
            },
            severity: Severity::Medium,
            category: None,
            what_would_help: None,
        })
    }

    #[test]
    fn score_filter_signal_stored_regardless_of_location() {
        let nodes = vec![tension_at("Pothole on Lake St", 44.9485, -93.2983)]; // Minneapolis
        let result = score_and_filter(nodes, URL_A, None);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn score_filter_out_of_region_signal_still_stored() {
        // With geo-filter removed, all signals are stored regardless of location
        let nodes = vec![tension_at("NYC subway delay", 40.7128, -74.0060)]; // New York
        let result = score_and_filter(nodes, URL_A, None);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn score_filter_stamps_source_url() {
        let nodes = vec![tension_at("Test signal", 44.95, -93.27)];
        let result = score_and_filter(nodes, "https://mpls-news.com/article", None);
        assert_eq!(result[0].meta().unwrap().source_url, "https://mpls-news.com/article");
    }

    #[test]
    fn score_filter_sets_confidence() {
        let nodes = vec![tension_at("Test signal", 44.95, -93.27)];
        let result = score_and_filter(nodes, URL_A, None);
        // quality::score should produce a non-zero confidence for a valid node
        assert!(result[0].meta().unwrap().confidence > 0.0, "confidence should be set by quality::score");
    }

    #[test]
    fn score_filter_removes_evidence_nodes() {
        use rootsignal_common::EvidenceNode;
        let evidence = Node::Evidence(EvidenceNode {
            id: Uuid::new_v4(),
            source_url: "https://example.com".to_string(),
            retrieved_at: Utc::now(),
            content_hash: "abc".to_string(),
            snippet: None,
            relevance: None,
            evidence_confidence: None,
            channel_type: None,
        });
        let nodes = vec![
            tension_at("Real signal", 44.95, -93.27),
            evidence,
        ];
        let result = score_and_filter(nodes, URL_A, None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title(), "Real signal");
    }

    fn tension_with_name(title: &str, location_name: &str) -> Node {
        Node::Tension(TensionNode {
            meta: NodeMeta {
                id: Uuid::new_v4(),
                title: title.to_string(),
                summary: String::new(),
                sensitivity: SensitivityLevel::General,
                confidence: 0.0,
                freshness_score: 0.9,
                corroboration_count: 0,
                about_location: None,
                about_location_name: Some(location_name.to_string()),
                from_location: None,
                source_url: String::new(),
                extracted_at: Utc::now(),
                content_date: None,
                last_confirmed_active: Utc::now(),
                source_diversity: 1,
                external_ratio: 0.0,
                cause_heat: 0.0,
                implied_queries: Vec::new(),
                channel_diversity: 1,
                mentioned_actors: Vec::new(),
                author_actor: None,
            },
            severity: Severity::Medium,
            category: None,
            what_would_help: None,
        })
    }

    #[test]
    fn score_filter_does_not_fabricate_about_location_from_actor() {
        // Signal has no about_location. Actor has coords.
        // score_and_filter should NOT backfill about_location — that's derived at query time.
        let nodes = vec![tension_with_name("Local Event", "Minneapolis")];
        let actor = ActorContext {
            actor_name: "Local Org".to_string(),
            bio: None,
            location_name: Some("Minneapolis".to_string()),
            location_lat: Some(44.9778),
            location_lng: Some(-93.2650),
            discovery_depth: 0,
        };
        let result = score_and_filter(nodes, URL_A, Some(&actor));
        assert_eq!(result.len(), 1);
        let meta = result[0].meta().unwrap();
        assert!(meta.about_location.is_none(), "about_location should NOT be backfilled from actor");
        assert!(meta.from_location.is_none(), "from_location should NOT be set at write time");
    }

    #[test]
    fn score_filter_no_location_no_actor_still_stored() {
        // Signal with no location and no actor context is still stored (no geo-filter rejection)
        let nodes = vec![tension("Floating Signal")];
        let result = score_and_filter(nodes, URL_A, None);
        assert_eq!(result.len(), 1);
        let meta = result[0].meta().unwrap();
        assert!(meta.about_location.is_none());
        assert!(meta.from_location.is_none());
    }

    #[test]
    fn score_filter_actor_preserves_existing_about_location() {
        // Signal has explicit about_location — it should be preserved as-is.
        // No from_location should be set (derived at query time via actor graph).
        let nodes = vec![tension_at("Located Signal", 44.95, -93.28)];
        let actor = ActorContext {
            actor_name: "Far Away Org".to_string(),
            bio: None,
            location_name: None,
            location_lat: Some(40.7128),  // NYC
            location_lng: Some(-74.0060),
            discovery_depth: 0,
        };
        let result = score_and_filter(nodes, URL_A, Some(&actor));
        let meta = result[0].meta().unwrap();
        let about = meta.about_location.as_ref().unwrap();
        assert!((about.lat - 44.95).abs() < 0.001, "existing about_location should be preserved");
        assert!(meta.from_location.is_none(), "from_location should NOT be set at write time");
    }

    #[test]
    fn score_filter_all_signals_stored_regardless_of_region() {
        // With geo-filter removed, ALL signals are stored regardless of location
        let nodes = vec![
            tension_at("Minneapolis", 44.95, -93.27),
            tension_at("Los Angeles", 34.05, -118.24),
            tension_at("Also Minneapolis", 44.98, -93.25),
        ];
        let result = score_and_filter(nodes, URL_A, None);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn score_filter_does_not_set_from_location() {
        // Actor coords should NOT populate from_location at write time.
        // from_location is derived at query time via actor graph traversal.
        let nodes = vec![
            tension_at("Uptown Event", 44.95, -93.30),
            tension("Floating Signal"),
        ];
        let actor = ActorContext {
            actor_name: "Local Org".to_string(),
            bio: None,
            location_name: None,
            location_lat: Some(44.9778),
            location_lng: Some(-93.2650),
            discovery_depth: 0,
        };
        let result = score_and_filter(nodes, URL_A, Some(&actor));
        for node in &result {
            let meta = node.meta().unwrap();
            assert!(meta.from_location.is_none(), "{} should NOT have from_location at write time", meta.title);
        }
    }

    #[test]
    fn actor_without_coords_does_not_set_locations() {
        // Actor has no lat/lng — neither from_location nor about_location should be touched.
        let nodes = vec![tension("No Location Signal")];
        let actor = ActorContext {
            actor_name: "Anonymous Org".to_string(),
            bio: None,
            location_name: Some("Minneapolis".to_string()),
            location_lat: None,
            location_lng: None,
            discovery_depth: 0,
        };
        let result = score_and_filter(nodes, URL_A, Some(&actor));
        let meta = result[0].meta().unwrap();
        assert!(meta.about_location.is_none());
        assert!(meta.from_location.is_none());
    }

    #[test]
    fn evidence_nodes_are_filtered_out() {
        // Evidence nodes should be removed by score_and_filter
        let evidence = Node::Evidence(rootsignal_common::EvidenceNode {
            id: Uuid::new_v4(),
            content_hash: "abc".to_string(),
            source_url: "https://example.com".to_string(),
            retrieved_at: Utc::now(),
            snippet: None,
            relevance: None,
            evidence_confidence: None,
            channel_type: None,
        });
        let nodes = vec![tension("Real Signal"), evidence];
        let result = score_and_filter(nodes, URL_A, None);
        assert_eq!(result.len(), 1, "evidence nodes should be filtered out");
        assert_eq!(result[0].title(), "Real Signal");
    }

    #[test]
    fn source_url_stamped_on_all_signals() {
        let nodes = vec![tension("Signal A"), tension("Signal B")];
        let result = score_and_filter(nodes, "https://test-source.org", None);
        for node in &result {
            let meta = node.meta().unwrap();
            assert_eq!(meta.source_url, "https://test-source.org");
        }
    }

    // --- is_owned_source tests ---

    #[test]
    fn is_owned_source_social_returns_true() {
        assert!(is_owned_source(&ScrapingStrategy::Social(SocialPlatform::Instagram)));
        assert!(is_owned_source(&ScrapingStrategy::Social(SocialPlatform::Facebook)));
        assert!(is_owned_source(&ScrapingStrategy::Social(SocialPlatform::Twitter)));
    }

    #[test]
    fn is_owned_source_web_page_returns_false() {
        assert!(!is_owned_source(&ScrapingStrategy::WebPage));
    }

    #[test]
    fn is_owned_source_rss_returns_false() {
        assert!(!is_owned_source(&ScrapingStrategy::Rss));
    }

    #[test]
    fn is_owned_source_web_query_returns_false() {
        assert!(!is_owned_source(&ScrapingStrategy::WebQuery));
    }
}
