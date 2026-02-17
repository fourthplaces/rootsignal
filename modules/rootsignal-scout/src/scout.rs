use anyhow::{Context, Result};
use chrono::Utc;
use futures::stream::{self, StreamExt};
use tracing::{error, info, warn};
use uuid::Uuid;

use apify_client::ApifyClient;
use rootsignal_common::{DiscoveryMethod, EvidenceNode, Node, NodeType, SourceNode, SourceType};
use rootsignal_graph::{Clusterer, GraphWriter, GraphClient};

use crate::embedder::Embedder;
use crate::extractor::Extractor;
use crate::quality;
use crate::scraper::{self, PageScraper, TavilySearcher};
use crate::sources::{self, CityProfile};

/// Stats from a scout run.
#[derive(Debug, Default)]
pub struct ScoutStats {
    pub urls_scraped: u32,
    pub urls_unchanged: u32,
    pub urls_failed: u32,
    pub signals_extracted: u32,
    pub signals_deduplicated: u32,
    pub signals_stored: u32,
    pub by_type: [u32; 5], // Event, Give, Ask, Notice, Tension
    pub fresh_7d: u32,
    pub fresh_30d: u32,
    pub fresh_90d: u32,
    pub social_media_posts: u32,
    pub geo_stripped: u32,
    pub geo_filtered: u32,
    pub audience_roles: std::collections::HashMap<String, u32>,
}

impl std::fmt::Display for ScoutStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "\n=== Scout Run Complete ===")?;
        writeln!(f, "URLs scraped:       {}", self.urls_scraped)?;
        writeln!(f, "URLs unchanged:     {}", self.urls_unchanged)?;
        writeln!(f, "URLs failed:        {}", self.urls_failed)?;
        writeln!(f, "Social media posts: {}", self.social_media_posts)?;
        writeln!(f, "Geo stripped:       {}", self.geo_stripped)?;
        writeln!(f, "Geo filtered:       {}", self.geo_filtered)?;
        writeln!(f, "Signals extracted:  {}", self.signals_extracted)?;
        writeln!(f, "Signals deduped:    {}", self.signals_deduplicated)?;
        writeln!(f, "Signals stored:     {}", self.signals_stored)?;
        writeln!(f, "\nBy type:")?;
        writeln!(f, "  Event:   {}", self.by_type[0])?;
        writeln!(f, "  Give:    {}", self.by_type[1])?;
        writeln!(f, "  Ask:     {}", self.by_type[2])?;
        writeln!(f, "  Notice:  {}", self.by_type[3])?;
        writeln!(f, "  Tension: {}", self.by_type[4])?;
        let total = self.signals_stored.max(1);
        writeln!(f, "\nFreshness:")?;
        writeln!(f, "  < 7 days:   {} ({:.0}%)", self.fresh_7d, self.fresh_7d as f64 / total as f64 * 100.0)?;
        writeln!(f, "  7-30 days:  {} ({:.0}%)", self.fresh_30d, self.fresh_30d as f64 / total as f64 * 100.0)?;
        writeln!(f, "  30-90 days: {} ({:.0}%)", self.fresh_90d, self.fresh_90d as f64 / total as f64 * 100.0)?;
        writeln!(f, "\nAudience roles:")?;
        let mut roles: Vec<_> = self.audience_roles.iter().collect();
        roles.sort_by(|a, b| b.1.cmp(a.1));
        for (role, count) in roles {
            writeln!(f, "  {}: {}", role, count)?;
        }
        Ok(())
    }
}

enum ScrapeOutcome {
    New { content: String, nodes: Vec<Node> },
    Unchanged,
    Failed,
}

/// In-memory embedding cache for the current scout run.
/// Catches duplicates that haven't been indexed in the graph yet (e.g. Instagram
/// and Facebook posts from the same org processed in the same batch).
struct EmbeddingCache {
    entries: Vec<CacheEntry>,
}

struct CacheEntry {
    embedding: Vec<f32>,
    node_id: Uuid,
    node_type: NodeType,
    source_url: String,
}

impl EmbeddingCache {
    fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// Find the best match above threshold. Returns (node_id, node_type, source_url, similarity).
    fn find_match(&self, embedding: &[f32], threshold: f64) -> Option<(Uuid, NodeType, &str, f64)> {
        let mut best: Option<(Uuid, NodeType, &str, f64)> = None;
        for entry in &self.entries {
            let sim = cosine_similarity(embedding, &entry.embedding);
            if sim >= threshold {
                if best.as_ref().map_or(true, |b| sim > b.3) {
                    best = Some((entry.node_id, entry.node_type, &entry.source_url, sim));
                }
            }
        }
        best
    }

    fn add(&mut self, embedding: Vec<f32>, node_id: Uuid, node_type: NodeType, source_url: String) {
        self.entries.push(CacheEntry { embedding, node_id, node_type, source_url });
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    (dot / (norm_a * norm_b)) as f64
}

pub struct Scout {
    graph_client: GraphClient,
    writer: GraphWriter,
    extractor: Extractor,
    embedder: Embedder,
    scraper: Box<dyn PageScraper>,
    tavily: TavilySearcher,
    apify: Option<ApifyClient>,
    anthropic_api_key: String,
    profile: CityProfile,
}

impl Scout {
    pub fn new(
        graph_client: GraphClient,
        anthropic_api_key: &str,
        voyage_api_key: &str,
        tavily_api_key: &str,
        apify_api_key: &str,
        city: &str,
    ) -> Result<Self> {
        let profile = sources::city_profile(city);
        info!(city = profile.name, "Initializing scout");
        let apify = if apify_api_key.is_empty() {
            warn!("APIFY_API_KEY not set, skipping social media scraping");
            None
        } else {
            Some(ApifyClient::new(apify_api_key.to_string()))
        };
        Ok(Self {
            graph_client: graph_client.clone(),
            writer: GraphWriter::new(graph_client),
            extractor: Extractor::new(anthropic_api_key, profile.name, profile.default_lat, profile.default_lng),
            embedder: Embedder::new(voyage_api_key),
            scraper: Box::new(scraper::ChromeScraper::new()),
            tavily: TavilySearcher::new(tavily_api_key),
            apify,
            anthropic_api_key: anthropic_api_key.to_string(),
            profile,
        })
    }

    /// Run a full scout cycle.
    pub async fn run(&self) -> Result<ScoutStats> {
        // Acquire lock
        if !self.writer.acquire_scout_lock().await.context("Failed to check scout lock")? {
            anyhow::bail!("Another scout run is in progress");
        }

        let result = self.run_inner().await;

        // Always release lock
        if let Err(e) = self.writer.release_scout_lock().await {
            error!("Failed to release scout lock: {e}");
        }

        result
    }

    /// Seed all curated sources from CityProfile as Source nodes in the graph.
    /// Idempotent — uses MERGE on url, so safe to call every run.
    async fn seed_curated_sources(&self) {
        let now = Utc::now();
        let city = self.profile.name.to_string();

        let mut sources: Vec<SourceNode> = Vec::new();

        // Web sources
        for (url, trust) in &self.profile.curated_sources {
            sources.push(SourceNode {
                id: Uuid::new_v4(),
                url: url.to_string(),
                source_type: SourceType::Web,
                discovery_method: DiscoveryMethod::Curated,
                city: city.clone(),
                trust: *trust,
                initial_trust: *trust,
                created_at: now,
                last_scraped: None,
                last_produced_signal: None,
                signals_produced: 0,
                signals_corroborated: 0,
                consecutive_empty_runs: 0,
                active: true,
                gap_context: None,
            });
        }

        // Instagram
        for (username, trust) in &self.profile.instagram_accounts {
            sources.push(SourceNode {
                id: Uuid::new_v4(),
                url: format!("https://www.instagram.com/{username}/"),
                source_type: SourceType::Instagram,
                discovery_method: DiscoveryMethod::Curated,
                city: city.clone(),
                trust: *trust,
                initial_trust: *trust,
                created_at: now,
                last_scraped: None,
                last_produced_signal: None,
                signals_produced: 0,
                signals_corroborated: 0,
                consecutive_empty_runs: 0,
                active: true,
                gap_context: None,
            });
        }

        // Facebook
        for (page_url, trust) in &self.profile.facebook_pages {
            sources.push(SourceNode {
                id: Uuid::new_v4(),
                url: page_url.to_string(),
                source_type: SourceType::Facebook,
                discovery_method: DiscoveryMethod::Curated,
                city: city.clone(),
                trust: *trust,
                initial_trust: *trust,
                created_at: now,
                last_scraped: None,
                last_produced_signal: None,
                signals_produced: 0,
                signals_corroborated: 0,
                consecutive_empty_runs: 0,
                active: true,
                gap_context: None,
            });
        }

        // Reddit
        for (sub_url, trust) in &self.profile.reddit_subreddits {
            sources.push(SourceNode {
                id: Uuid::new_v4(),
                url: sub_url.to_string(),
                source_type: SourceType::Reddit,
                discovery_method: DiscoveryMethod::Curated,
                city: city.clone(),
                trust: *trust,
                initial_trust: *trust,
                created_at: now,
                last_scraped: None,
                last_produced_signal: None,
                signals_produced: 0,
                signals_corroborated: 0,
                consecutive_empty_runs: 0,
                active: true,
                gap_context: None,
            });
        }

        let total = sources.len();
        let mut seeded = 0u32;
        for source in &sources {
            match self.writer.upsert_source(source).await {
                Ok(_) => seeded += 1,
                Err(e) => warn!(url = source.url.as_str(), error = %e, "Failed to seed source"),
            }
        }
        info!(seeded, total, "Curated sources seeded");
    }

    async fn run_inner(&self) -> Result<ScoutStats> {
        let mut stats = ScoutStats::default();

        // Reap expired signals before scraping new ones
        info!("Reaping expired signals...");
        match self.writer.reap_expired().await {
            Ok(reap) => {
                if reap.events + reap.asks + reap.stale > 0 {
                    info!(
                        events = reap.events,
                        asks = reap.asks,
                        stale = reap.stale,
                        "Expired signals removed"
                    );
                }
            }
            Err(e) => warn!(error = %e, "Failed to reap expired signals, continuing"),
        }

        // Seed curated sources as Source nodes (idempotent — MERGE on url)
        self.seed_curated_sources().await;

        // Load discovered sources from graph
        let discovered_sources = match self.writer.get_active_sources(&self.profile.name).await {
            Ok(sources) => {
                let discovered_count = sources.iter().filter(|s| s.discovery_method != DiscoveryMethod::Curated).count();
                if discovered_count > 0 {
                    info!(discovered = discovered_count, "Loaded discovered sources from graph");
                }
                sources
            }
            Err(e) => {
                warn!(error = %e, "Failed to load discovered sources, using curated only");
                Vec::new()
            }
        };

        let mut all_urls: Vec<(String, f32)> = Vec::new();

        // 1. Tavily searches (parallel, 5 at a time)
        info!("Starting Tavily searches...");
        let queries = &self.profile.tavily_queries;
        let search_results: Vec<_> = stream::iter(queries.iter().map(|query| {
            async move {
                (*query, self.tavily.search(query, 5).await)
            }
        }))
        .buffer_unordered(5)
        .collect()
        .await;

        for (query, result) in search_results {
            match result {
                Ok(results) => {
                    for r in results {
                        let trust = sources::source_trust(&r.url);
                        all_urls.push((r.url, trust));
                    }
                }
                Err(e) => {
                    warn!(query, error = %e, "Tavily search failed");
                }
            }
        }

        // 2. Curated sources
        info!("Adding curated sources...");
        for (url, trust) in &self.profile.curated_sources {
            all_urls.push((url.to_string(), *trust));
        }

        // 2b. Discovered web sources from graph
        for source in &discovered_sources {
            if source.source_type == SourceType::Web && source.discovery_method != DiscoveryMethod::Curated {
                all_urls.push((source.url.clone(), source.trust));
            }
        }

        // Deduplicate URLs
        all_urls.sort_by(|a, b| a.0.cmp(&b.0));
        all_urls.dedup_by(|a, b| a.0 == b.0);
        info!(total_urls = all_urls.len(), "Unique URLs to scrape");

        // 3. Scrape + check content hash + extract in parallel, then write sequentially
        let pipeline_results: Vec<_> = stream::iter(all_urls.iter().map(|(url, source_trust)| {
            let url = url.clone();
            let source_trust = *source_trust;
            async move {
                let clean_url = sanitize_url(&url);

                // Scrape
                let content = match self.scraper.scrape(&url).await {
                    Ok(c) if !c.is_empty() => c,
                    Ok(_) => return (clean_url, source_trust, ScrapeOutcome::Failed),
                    Err(e) => {
                        warn!(url, error = %e, "Scrape failed");
                        return (clean_url, source_trust, ScrapeOutcome::Failed);
                    }
                };

                // Check content hash — skip extraction if content hasn't changed for this URL
                let hash = format!("{:x}", content_hash(&content));
                match self.writer.content_already_processed(&hash, &clean_url).await {
                    Ok(true) => {
                        info!(url = clean_url.as_str(), "Content unchanged, skipping extraction");
                        return (clean_url, source_trust, ScrapeOutcome::Unchanged);
                    }
                    Ok(false) => {} // New content, proceed
                    Err(e) => {
                        warn!(url = clean_url.as_str(), error = %e, "Hash check failed, proceeding with extraction");
                    }
                }

                // Extract (LLM call) — only reached for new/changed content
                match self.extractor.extract(&content, &clean_url).await {
                    Ok(nodes) => (clean_url, source_trust, ScrapeOutcome::New { content, nodes }),
                    Err(e) => {
                        warn!(url = clean_url.as_str(), error = %e, "Extraction failed");
                        (clean_url, source_trust, ScrapeOutcome::Failed)
                    }
                }
            }
        }))
        .buffer_unordered(10)
        .collect()
        .await;

        // Process results sequentially, with in-memory embedding cache for cross-batch dedup
        let now = Utc::now();
        let mut embed_cache = EmbeddingCache::new();
        let mut source_signal_counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        for (url, _source_trust, outcome) in pipeline_results {
            match outcome {
                ScrapeOutcome::New { content, nodes } => {
                    let signal_count_before = stats.signals_stored;
                    match self.store_signals(&url, &content, nodes, &mut stats, &mut embed_cache).await {
                        Ok(_) => {
                            stats.urls_scraped += 1;
                            let produced = stats.signals_stored - signal_count_before;
                            *source_signal_counts.entry(url).or_default() += produced;
                        }
                        Err(e) => {
                            warn!(url, error = %e, "Failed to store signals");
                            stats.urls_failed += 1;
                            source_signal_counts.entry(url).or_default();
                        }
                    }
                }
                ScrapeOutcome::Unchanged => {
                    // Refresh timestamps to keep existing signals fresh
                    match self.writer.refresh_url_signals(&url, now).await {
                        Ok(n) if n > 0 => info!(url, refreshed = n, "Refreshed unchanged signals"),
                        Ok(_) => {}
                        Err(e) => warn!(url, error = %e, "Failed to refresh signals"),
                    }
                    stats.urls_unchanged += 1;
                    // Unchanged still counts as a scrape (content was fetched, just didn't change)
                    source_signal_counts.entry(url).or_default();
                }
                ScrapeOutcome::Failed => {
                    stats.urls_failed += 1;
                    // Don't record failed scrapes — don't penalize for network errors
                }
            }
        }

        // 4. Social media via Apify (Instagram + Facebook)
        if let Some(ref apify) = self.apify {
            self.scrape_social_media(apify, &mut stats, &mut embed_cache, &mut source_signal_counts).await;
        }

        // 4a. Update per-source metrics in the graph
        for (url, signals_produced) in &source_signal_counts {
            if let Err(e) = self.writer.record_source_scrape(url, *signals_produced, now).await {
                warn!(url, error = %e, "Failed to record source scrape metrics");
            }
        }

        // 4b. Deactivate dead sources (10+ consecutive empty runs, non-curated only)
        match self.writer.deactivate_dead_sources(10).await {
            Ok(n) if n > 0 => info!(deactivated = n, "Deactivated dead sources"),
            Ok(_) => {}
            Err(e) => warn!(error = %e, "Failed to deactivate dead sources"),
        }

        // Source stats
        match self.writer.get_source_stats(&self.profile.name).await {
            Ok(ss) => {
                info!(
                    total = ss.total,
                    active = ss.active,
                    curated = ss.curated,
                    discovered = ss.discovered,
                    "Source registry stats"
                );
            }
            Err(e) => warn!(error = %e, "Failed to get source stats"),
        }

        // 5. Clustering — build similarity edges, run Leiden, create/update stories
        info!("Starting clustering...");
        let entity_mappings: Vec<rootsignal_graph::cluster::EntityMappingRef> = self
            .profile
            .entity_mappings
            .iter()
            .map(|m| rootsignal_graph::cluster::EntityMappingRef {
                entity_id: m.entity_id.to_string(),
                domains: m.domains.iter().map(|s| s.to_string()).collect(),
                instagram: m.instagram.iter().map(|s| s.to_string()).collect(),
                facebook: m.facebook.iter().map(|s| s.to_string()).collect(),
                reddit: m.reddit.iter().map(|s| s.to_string()).collect(),
            })
            .collect();

        let clusterer = Clusterer::new(
            self.graph_client.clone(),
            &self.anthropic_api_key,
            entity_mappings,
        );

        match clusterer.run().await {
            Ok(cluster_stats) => {
                info!("{cluster_stats}");
            }
            Err(e) => {
                warn!(error = %e, "Clustering failed (non-fatal)");
            }
        }

        // 6. Investigation
        info!("Starting investigation phase...");
        let investigator = crate::investigator::Investigator::new(
            &self.writer, &self.tavily, &self.anthropic_api_key, self.profile.name,
        );
        let investigation_stats = investigator.run().await;
        info!("{investigation_stats}");

        info!("{stats}");
        Ok(stats)
    }

    /// Scrape Instagram, Facebook, and Reddit accounts via Apify, feed posts through LLM extraction.
    async fn scrape_social_media(&self, apify: &ApifyClient, stats: &mut ScoutStats, embed_cache: &mut EmbeddingCache, source_signal_counts: &mut std::collections::HashMap<String, u32>) {
        use std::pin::Pin;
        use std::future::Future;

        type SocialResult = Option<(String, String, Vec<Node>, usize)>;

        let ig_accounts = &self.profile.instagram_accounts;
        let fb_pages = &self.profile.facebook_pages;
        let reddit_subs = &self.profile.reddit_subreddits;
        info!(
            ig = ig_accounts.len(),
            fb = fb_pages.len(),
            reddit = reddit_subs.len(),
            "Scraping social media via Apify..."
        );

        // Collect all futures into a single Vec<Pin<Box<...>>> so types unify
        let mut futures: Vec<Pin<Box<dyn Future<Output = SocialResult> + Send + '_>>> = Vec::new();

        for (username, trust) in ig_accounts {
            let source_url = format!("https://www.instagram.com/{username}/");
            let username = username.to_string();
            let _trust = *trust;
            futures.push(Box::pin(async move {
                let posts = match apify.scrape_instagram_posts(&username, 10).await {
                    Ok(p) => p,
                    Err(e) => {
                        warn!(username, error = %e, "Instagram Apify scrape failed");
                        return None;
                    }
                };
                let post_count = posts.len();
                let combined_text: String = posts
                    .iter()
                    .filter_map(|p| p.caption.as_deref())
                    .enumerate()
                    .map(|(i, caption)| format!("--- Post {} ---\n{}", i + 1, caption))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                if combined_text.is_empty() {
                    return None;
                }
                let nodes = match self.extractor.extract(&combined_text, &source_url).await {
                    Ok(n) => n,
                    Err(e) => {
                        warn!(username, error = %e, "Instagram extraction failed");
                        return None;
                    }
                };
                info!(username, posts = post_count, "Instagram scrape complete");
                Some((source_url, combined_text, nodes, post_count))
            }));
        }

        for (page_url, trust) in fb_pages {
            let page_url = page_url.to_string();
            let _trust = *trust;
            futures.push(Box::pin(async move {
                let posts = match apify.scrape_facebook_posts(&page_url, 10).await {
                    Ok(p) => p,
                    Err(e) => {
                        warn!(page_url, error = %e, "Facebook Apify scrape failed");
                        return None;
                    }
                };
                let post_count = posts.len();
                let combined_text: String = posts
                    .iter()
                    .filter_map(|p| p.text.as_deref())
                    .enumerate()
                    .map(|(i, text)| format!("--- Post {} ---\n{}", i + 1, text))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                if combined_text.is_empty() {
                    return None;
                }
                let nodes = match self.extractor.extract(&combined_text, &page_url).await {
                    Ok(n) => n,
                    Err(e) => {
                        warn!(page_url, error = %e, "Facebook extraction failed");
                        return None;
                    }
                };
                info!(page_url, posts = post_count, "Facebook scrape complete");
                Some((page_url, combined_text, nodes, post_count))
            }));
        }

        for (subreddit_url, trust) in reddit_subs {
            let subreddit_url = subreddit_url.to_string();
            let _trust = *trust;
            futures.push(Box::pin(async move {
                let posts = match apify.scrape_reddit_posts(&subreddit_url, 20).await {
                    Ok(p) => p,
                    Err(e) => {
                        warn!(subreddit_url, error = %e, "Reddit Apify scrape failed");
                        return None;
                    }
                };
                let post_count = posts.len();
                // Batch posts: combine title + body per post, 10 at a time
                let batches: Vec<_> = posts.chunks(10).collect();
                let mut all_nodes = Vec::new();
                let mut combined_all = String::new();
                for batch in batches {
                    let combined_text: String = batch
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let title = p.title.as_deref().unwrap_or("");
                            let body = p.body.as_deref().unwrap_or("");
                            format!("--- Post {} ---\n{}\n\n{}", i + 1, title, body)
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    if combined_text.is_empty() {
                        continue;
                    }
                    combined_all.push_str(&combined_text);
                    match self.extractor.extract(&combined_text, &subreddit_url).await {
                        Ok(n) => all_nodes.extend(n),
                        Err(e) => {
                            warn!(subreddit_url, error = %e, "Reddit extraction failed");
                        }
                    }
                }
                if all_nodes.is_empty() {
                    return None;
                }
                info!(subreddit_url, posts = post_count, "Reddit scrape complete");
                Some((subreddit_url, combined_all, all_nodes, post_count))
            }));
        }

        let results: Vec<_> = stream::iter(futures)
            .buffer_unordered(10)
            .collect()
            .await;

        for result in results.into_iter().flatten() {
            let (source_url, combined_text, nodes, post_count) = result;
            stats.social_media_posts += post_count as u32;
            let signal_count_before = stats.signals_stored;
            if let Err(e) = self.store_signals(&source_url, &combined_text, nodes, stats, embed_cache).await {
                warn!(source_url = source_url.as_str(), error = %e, "Failed to store social media signals");
            }
            let produced = stats.signals_stored - signal_count_before;
            *source_signal_counts.entry(source_url).or_default() += produced;
        }
    }

    async fn store_signals(
        &self,
        url: &str,
        content: &str,
        mut nodes: Vec<Node>,
        stats: &mut ScoutStats,
        embed_cache: &mut EmbeddingCache,
    ) -> Result<()> {
        let url = sanitize_url(url);
        stats.signals_extracted += nodes.len() as u32;

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
        let center_lat = self.profile.default_lat;
        let center_lng = self.profile.default_lng;
        for node in &mut nodes {
            let is_fake = node.meta()
                .and_then(|m| m.location.as_ref())
                .map(|loc| {
                    (loc.lat - center_lat).abs() < 0.01
                        && (loc.lng - center_lng).abs() < 0.01
                })
                .unwrap_or(false);

            if is_fake {
                if let Some(meta) = node_meta_mut(node) {
                    meta.location = None;
                    stats.geo_stripped += 1;
                }
            }
        }

        // Filter off-geography signals.
        // If location_name is set and doesn't match any geo_term, drop it.
        // Signals with no location pass through (benefit of the doubt).
        let geo_terms = &self.profile.geo_terms;
        let before_geo = nodes.len();
        let nodes: Vec<_> = nodes
            .into_iter()
            .filter(|n| {
                let loc = n.meta().and_then(|m| m.location_name.as_deref()).unwrap_or("");
                if loc.is_empty() || loc == "<UNKNOWN>" {
                    return true; // no location info, keep
                }
                let loc_lower = loc.to_lowercase();
                geo_terms.iter().any(|term| loc_lower.contains(&term.to_lowercase()))
            })
            .collect();
        let geo_filtered = before_geo - nodes.len();
        if geo_filtered > 0 {
            info!(url = url.as_str(), filtered = geo_filtered, "Off-geography signals dropped");
            stats.geo_filtered += geo_filtered as u32;
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
        let mut seen = std::collections::HashSet::new();
        let nodes: Vec<_> = nodes
            .into_iter()
            .filter(|n| seen.insert((normalize_title(n.title()), n.node_type())))
            .collect();

        // --- Layer 2: URL-based title dedup against existing database ---
        let existing_titles: std::collections::HashSet<String> = self
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
            info!(url = url.as_str(), skipped = url_deduped, "URL-based title dedup");
            stats.signals_deduplicated += url_deduped as u32;
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
                    info!(
                        existing_id = %existing_id,
                        title = node.title(),
                        existing_source = existing_url.as_str(),
                        new_source = url.as_str(),
                        "Global title+type match, corroborating"
                    );
                    self.writer.corroborate(*existing_id, node.node_type(), now).await?;
                    let evidence = EvidenceNode {
                        id: Uuid::new_v4(),
                        source_url: url.clone(),
                        retrieved_at: now,
                        content_hash: content_hash_str.clone(),
                        snippet: node.meta().map(|m| m.summary.clone()),
                    };
                    self.writer.create_evidence(&evidence, *existing_id).await?;
                    stats.signals_deduplicated += 1;
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
        // Embed source content snippet (not LLM summary) to preserve semantic fingerprint.
        // The LLM compresses semantic differences using similar civic vocabulary.
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
            .map(|n| {
                format!("{} {}", n.title(), content_snippet)
            })
            .collect();

        let embeddings = match self.embedder.embed_batch(embed_texts).await {
            Ok(e) => e,
            Err(e) => {
                warn!(url = url.as_str(), error = %e, "Batch embedding failed, skipping all signals");
                return Ok(());
            }
        };

        // --- Layer 3: Vector dedup (in-memory cache + graph) with URL-aware threshold ---

        for (node, embedding) in nodes.into_iter().zip(embeddings.into_iter()) {
            let node_type = node.node_type();
            let type_idx = match node_type {
                NodeType::Event => 0,
                NodeType::Give => 1,
                NodeType::Ask => 2,
                NodeType::Notice => 3,
                NodeType::Tension => 4,
                NodeType::Evidence => continue,
            };

            // 3a: Check in-memory cache first (catches cross-batch dupes not yet indexed)
            if let Some((cached_id, cached_type, cached_url, sim)) =
                embed_cache.find_match(&embedding, 0.85)
            {
                let is_same_source = cached_url == url;
                if is_same_source || sim >= 0.92 {
                    info!(
                        existing_id = %cached_id,
                        similarity = sim,
                        title = node.title(),
                        source = "cache",
                        "Duplicate found in embedding cache, corroborating"
                    );
                    self.writer.corroborate(cached_id, cached_type, now).await?;

                    let evidence = EvidenceNode {
                        id: Uuid::new_v4(),
                        source_url: url.clone(),
                        retrieved_at: now,
                        content_hash: content_hash_str.clone(),
                        snippet: node.meta().map(|m| m.summary.clone()),
                    };
                    self.writer.create_evidence(&evidence, cached_id).await?;

                    stats.signals_deduplicated += 1;
                    continue;
                }
            }

            // 3b: Check graph index (catches dupes from previous runs)
            match self.writer.find_duplicate(&embedding, node_type, 0.85).await {
                Ok(Some(dup)) => {
                    let dominated_url = sanitize_url(&dup.source_url);
                    let is_same_source = dominated_url == url;
                    if is_same_source || dup.similarity >= 0.92 {
                        let cross_type = dup.node_type != node_type;
                        info!(
                            existing_id = %dup.id,
                            similarity = dup.similarity,
                            title = node.title(),
                            cross_type,
                            source = "graph",
                            "Duplicate found, corroborating"
                        );
                        self.writer.corroborate(dup.id, dup.node_type, now).await?;

                        let evidence = EvidenceNode {
                            id: Uuid::new_v4(),
                            source_url: url.clone(),
                            retrieved_at: now,
                            content_hash: content_hash_str.clone(),
                            snippet: node.meta().map(|m| m.summary.clone()),
                        };
                        self.writer.create_evidence(&evidence, dup.id).await?;

                        // Also add to cache so future batches can find this
                        embed_cache.add(embedding, dup.id, dup.node_type, dominated_url);

                        stats.signals_deduplicated += 1;
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
            let node_id = self.writer.create_node(&node, &embedding).await?;

            // Add to in-memory cache so subsequent batches can find it immediately
            embed_cache.add(embedding, node_id, node_type, url.clone());

            let evidence = EvidenceNode {
                id: Uuid::new_v4(),
                source_url: url.clone(),
                retrieved_at: now,
                content_hash: content_hash_str.clone(),
                snippet: node.meta().map(|m| m.summary.clone()),
            };
            self.writer.create_evidence(&evidence, node_id).await?;

            // Update stats
            stats.signals_stored += 1;
            stats.by_type[type_idx] += 1;

            if let Some(meta) = node.meta() {
                let age = now - meta.extracted_at;
                if age.num_days() < 7 {
                    stats.fresh_7d += 1;
                } else if age.num_days() < 30 {
                    stats.fresh_30d += 1;
                } else if age.num_days() < 90 {
                    stats.fresh_90d += 1;
                }

                for role in &meta.audience_roles {
                    *stats
                        .audience_roles
                        .entry(role.to_string())
                        .or_insert(0) += 1;
                }
            }
        }

        Ok(())
    }
}

/// Normalize a title for dedup comparison: lowercase and trim.
fn normalize_title(title: &str) -> String {
    title.trim().to_lowercase()
}

/// Strip tracking parameters from URLs that may contain PII or cause dedup mismatches.
fn sanitize_url(url: &str) -> String {
    const TRACKING_PARAMS: &[&str] = &[
        "_dt", "fbclid", "gclid", "utm_source", "utm_medium", "utm_campaign",
        "utm_term", "utm_content", "modal", "ref", "mc_cid", "mc_eid",
    ];

    let Ok(mut parsed) = url::Url::parse(url) else {
        return url.to_string();
    };

    let had_query = parsed.query().is_some();
    if !had_query {
        return url.to_string();
    }

    let clean_pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(key, _)| !TRACKING_PARAMS.contains(&key.as_ref()))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    if clean_pairs.is_empty() {
        parsed.set_query(None);
    } else {
        parsed.query_pairs_mut().clear().extend_pairs(clean_pairs);
    }

    parsed.to_string()
}

fn node_meta_mut(node: &mut Node) -> Option<&mut rootsignal_common::NodeMeta> {
    match node {
        Node::Event(n) => Some(&mut n.meta),
        Node::Give(n) => Some(&mut n.meta),
        Node::Ask(n) => Some(&mut n.meta),
        Node::Notice(n) => Some(&mut n.meta),
        Node::Tension(n) => Some(&mut n.meta),
        Node::Evidence(_) => None,
    }
}

/// Deterministic content hash for change detection (FNV-1a).
/// Must be stable across process restarts — DefaultHasher is NOT (HashDoS randomization).
fn content_hash(content: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325; // FNV offset basis
    for byte in content.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3); // FNV prime
    }
    hash
}
