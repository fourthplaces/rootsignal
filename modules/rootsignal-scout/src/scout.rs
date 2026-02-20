use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;
use futures::stream::{self, StreamExt};
use tracing::{error, info, warn};
use uuid::Uuid;

use rootsignal_common::{
    CityNode, DiscoveryMethod, EvidenceNode, Node, NodeType, SourceNode, SourceType,
};
use rootsignal_graph::{Clusterer, GraphClient, GraphWriter};

use crate::budget::{BudgetTracker, OperationCost};
use crate::embedder::{Embedder, TextEmbedder};
use crate::extractor::{Extractor, ResourceTag, SignalExtractor};
use crate::quality;
use crate::scraper::{
    self, NoopSocialScraper, PageScraper, SerperSearcher, SocialAccount, SocialPlatform,
    SocialPost, SocialScraper, WebSearcher,
};
use crate::sources;

/// Stats from a scout run.
#[derive(Debug, Default)]
pub struct ScoutStats {
    pub urls_scraped: u32,
    pub urls_unchanged: u32,
    pub urls_failed: u32,
    pub signals_extracted: u32,
    pub signals_deduplicated: u32,
    pub signals_stored: u32,
    pub by_type: [u32; 5], // Gathering, Aid, Need, Notice, Tension
    pub fresh_7d: u32,
    pub fresh_30d: u32,
    pub fresh_90d: u32,
    pub social_media_posts: u32,
    pub geo_stripped: u32,
    pub geo_filtered: u32,
    pub discovery_posts_found: u32,
    pub discovery_accounts_found: u32,
    pub expansion_queries_collected: u32,
    pub expansion_sources_created: u32,
    pub expansion_deferred_expanded: u32,
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
        writeln!(f, "Discovery posts:    {}", self.discovery_posts_found)?;
        writeln!(f, "Accounts discovered:{}", self.discovery_accounts_found)?;
        writeln!(f, "Signals extracted:  {}", self.signals_extracted)?;
        writeln!(f, "Signals deduped:    {}", self.signals_deduplicated)?;
        writeln!(f, "Signals stored:     {}", self.signals_stored)?;
        writeln!(f, "\nBy type:")?;
        writeln!(f, "  Gathering: {}", self.by_type[0])?;
        writeln!(f, "  Aid:       {}", self.by_type[1])?;
        writeln!(f, "  Need:    {}", self.by_type[2])?;
        writeln!(f, "  Notice:  {}", self.by_type[3])?;
        writeln!(f, "  Tension: {}", self.by_type[4])?;
        let total = self.signals_stored.max(1);
        writeln!(f, "\nFreshness:")?;
        writeln!(
            f,
            "  < 7 days:   {} ({:.0}%)",
            self.fresh_7d,
            self.fresh_7d as f64 / total as f64 * 100.0
        )?;
        writeln!(
            f,
            "  7-30 days:  {} ({:.0}%)",
            self.fresh_30d,
            self.fresh_30d as f64 / total as f64 * 100.0
        )?;
        writeln!(
            f,
            "  30-90 days: {} ({:.0}%)",
            self.fresh_90d,
            self.fresh_90d as f64 / total as f64 * 100.0
        )?;
        if self.expansion_queries_collected > 0 {
            writeln!(f, "\nSignal expansion:")?;
            writeln!(
                f,
                "  Queries collected: {}",
                self.expansion_queries_collected
            )?;
            writeln!(f, "  Sources created:   {}", self.expansion_sources_created)?;
            writeln!(
                f,
                "  Deferred expanded: {}",
                self.expansion_deferred_expanded
            )?;
        }
        Ok(())
    }
}

enum ScrapeOutcome {
    New {
        content: String,
        nodes: Vec<Node>,
        resource_tags: Vec<(Uuid, Vec<ResourceTag>)>,
    },
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
        Self {
            entries: Vec::new(),
        }
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
        self.entries.push(CacheEntry {
            embedding,
            node_id,
            node_type,
            source_url,
        });
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
    extractor: Box<dyn SignalExtractor>,
    embedder: Box<dyn TextEmbedder>,
    scraper: Arc<dyn PageScraper>,
    searcher: Arc<dyn WebSearcher>,
    social: Box<dyn SocialScraper>,
    anthropic_api_key: String,
    city_node: CityNode,
    budget: BudgetTracker,
    cancelled: Arc<AtomicBool>,
}

impl Scout {
    pub fn new(
        graph_client: GraphClient,
        anthropic_api_key: &str,
        voyage_api_key: &str,
        serper_api_key: &str,
        apify_api_key: &str,
        city_node: CityNode,
        daily_budget_cents: u64,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self> {
        info!(city = city_node.name.as_str(), "Initializing scout");
        let social: Box<dyn SocialScraper> = if apify_api_key.is_empty() {
            warn!("APIFY_API_KEY not set, skipping social media scraping");
            Box::new(NoopSocialScraper)
        } else {
            Box::new(apify_client::ApifyClient::new(apify_api_key.to_string()))
        };
        let scraper: Arc<dyn PageScraper> = match std::env::var("BROWSERLESS_URL") {
            Ok(url) => {
                let token = std::env::var("BROWSERLESS_TOKEN").ok();
                Arc::new(scraper::BrowserlessScraper::new(&url, token.as_deref()))
            }
            Err(_) => Arc::new(scraper::ChromeScraper::new()),
        };
        Ok(Self {
            graph_client: graph_client.clone(),
            writer: GraphWriter::new(graph_client),
            extractor: Box::new(Extractor::new(
                anthropic_api_key,
                city_node.name.as_str(),
                city_node.center_lat,
                city_node.center_lng,
            )),
            embedder: Box::new(Embedder::new(voyage_api_key)),
            scraper,
            searcher: Arc::new(SerperSearcher::new(serper_api_key)),
            social,
            anthropic_api_key: anthropic_api_key.to_string(),
            city_node,
            budget: BudgetTracker::new(daily_budget_cents),
            cancelled,
        })
    }

    /// Build a Scout with pre-built trait objects (for testing).
    pub fn with_deps(
        graph_client: GraphClient,
        extractor: Box<dyn SignalExtractor>,
        embedder: Box<dyn TextEmbedder>,
        scraper: Arc<dyn PageScraper>,
        searcher: Arc<dyn WebSearcher>,
        social: Box<dyn SocialScraper>,
        anthropic_api_key: &str,
        city_node: CityNode,
    ) -> Self {
        Self {
            graph_client: graph_client.clone(),
            writer: GraphWriter::new(graph_client),
            extractor,
            embedder,
            scraper,
            searcher,
            social,
            anthropic_api_key: anthropic_api_key.to_string(),
            city_node,
            budget: BudgetTracker::new(0), // Unlimited for tests
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Check if the scout has been cancelled. Returns Err if so.
    fn check_cancelled(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Relaxed) {
            info!("Scout run cancelled by user");
            anyhow::bail!("Scout run cancelled");
        }
        Ok(())
    }

    /// Run a full scout cycle.
    pub async fn run(&self) -> Result<ScoutStats> {
        // Acquire per-city lock
        let city_slug = &self.city_node.slug;
        if !self
            .writer
            .acquire_scout_lock(city_slug)
            .await
            .context("Failed to check scout lock")?
        {
            anyhow::bail!("Another scout run is in progress for {}", city_slug);
        }

        let result = self.run_inner().await;

        // Always release lock
        if let Err(e) = self.writer.release_scout_lock(city_slug).await {
            error!("Failed to release scout lock: {e}");
        }

        result
    }

    async fn run_inner(&self) -> Result<ScoutStats> {
        let mut stats = ScoutStats::default();

        // Reap expired signals before scraping new ones
        info!("Reaping expired signals...");
        match self.writer.reap_expired().await {
            Ok(reap) => {
                if reap.gatherings + reap.needs + reap.stale > 0 {
                    info!(
                        gatherings = reap.gatherings,
                        needs = reap.needs,
                        stale = reap.stale,
                        "Expired signals removed"
                    );
                }
            }
            Err(e) => warn!(error = %e, "Failed to reap expired signals, continuing"),
        }

        // Load all active sources from graph (curated + discovered)
        let all_sources = match self.writer.get_active_sources(&self.city_node.slug).await {
            Ok(sources) => {
                let curated = sources
                    .iter()
                    .filter(|s| s.discovery_method == DiscoveryMethod::Curated)
                    .count();
                let discovered = sources.len() - curated;
                info!(
                    total = sources.len(),
                    curated, discovered, "Loaded sources from graph"
                );
                sources
            }
            Err(e) => {
                warn!(error = %e, "Failed to load sources from graph");
                Vec::new()
            }
        };

        // Schedule sources based on weight + cadence + exploration policy
        let now_schedule = Utc::now();
        let scheduler = crate::scheduler::SourceScheduler::new();
        let schedule = scheduler.schedule(&all_sources, now_schedule);
        let scheduled_keys: std::collections::HashSet<String> = schedule
            .scheduled
            .iter()
            .chain(schedule.exploration.iter())
            .map(|s| s.canonical_key.clone())
            .collect();

        let tension_phase_keys: std::collections::HashSet<String> =
            schedule.tension_phase.iter().cloned().collect();
        let response_phase_keys: std::collections::HashSet<String> =
            schedule.response_phase.iter().cloned().collect();

        info!(
            scheduled = schedule.scheduled.len(),
            exploration = schedule.exploration.len(),
            skipped = schedule.skipped,
            tension_phase = tension_phase_keys.len(),
            response_phase = response_phase_keys.len(),
            "Source scheduling complete"
        );

        // Web query tiered scheduling — limits which WebQuery sources run this cycle.
        // Non-query sources pass through unfiltered.
        let wq_schedule = crate::scheduler::schedule_web_queries(&all_sources, 0, now_schedule);
        let wq_scheduled_keys: std::collections::HashSet<String> =
            wq_schedule.scheduled.into_iter().collect();

        // Filter all_sources to only those scheduled for this run.
        // WebQuery sources must pass BOTH the regular scheduler AND the web query scheduler.
        let scheduled_sources: Vec<&SourceNode> = all_sources
            .iter()
            .filter(|s| {
                if !scheduled_keys.contains(&s.canonical_key) {
                    return false;
                }
                // Non-query sources always pass
                if s.source_type != SourceType::WebQuery {
                    return true;
                }
                // WebQuery sources must also be in the tiered schedule
                wq_scheduled_keys.contains(&s.canonical_key)
            })
            .collect();

        // Build URL→canonical_key lookup for mapping scrape results back to sources.
        // Mutable: scrape_phase inserts resolved WebQuery→URL mappings.
        let mut url_to_canonical_key: std::collections::HashMap<String, String> = all_sources
            .iter()
            .filter_map(|s| {
                s.url
                    .as_ref()
                    .map(|u| (sanitize_url(u), s.canonical_key.clone()))
            })
            .collect();

        let mut embed_cache = EmbeddingCache::new();
        let mut source_signal_counts: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();
        let mut expansion_queries: Vec<String> = Vec::new();

        // ================================================================
        // Phase A: Find Problems — scrape tension + mixed sources (web + social)
        // ================================================================
        info!("=== Phase A: Find Problems ===");
        let phase_a_sources: Vec<&SourceNode> = scheduled_sources
            .iter()
            .filter(|s| tension_phase_keys.contains(&s.canonical_key))
            .copied()
            .collect();

        let mut query_api_errors = self
            .scrape_phase(
                &phase_a_sources,
                &mut url_to_canonical_key,
                &mut stats,
                &mut embed_cache,
                &mut source_signal_counts,
                &mut expansion_queries,
            )
            .await;

        // Phase A social: tension + mixed social sources
        let phase_a_social: Vec<&SourceNode> = scheduled_sources
            .iter()
            .filter(|s| {
                matches!(
                    s.source_type,
                    SourceType::Instagram
                        | SourceType::Facebook
                        | SourceType::Reddit
                        | SourceType::Twitter
                        | SourceType::TikTok
                ) && tension_phase_keys.contains(&s.canonical_key)
            })
            .copied()
            .collect();
        let known_city_urls: std::collections::HashSet<String> =
            url_to_canonical_key.keys().cloned().collect();
        if !phase_a_social.is_empty() {
            self.scrape_social_media(
                &mut stats,
                &mut embed_cache,
                &mut source_signal_counts,
                &phase_a_social,
                &known_city_urls,
                &mut expansion_queries,
            )
            .await;
        }

        self.check_cancelled()?;

        // ================================================================
        // Mid-Run Discovery — use fresh tensions to create response-seeking queries
        // ================================================================
        info!("=== Mid-Run Discovery ===");
        let discoverer = crate::source_finder::SourceFinder::new(
            &self.writer,
            &self.city_node.slug,
            &self.city_node.name,
            Some(self.anthropic_api_key.as_str()),
            &self.budget,
        )
        .with_embedder(&*self.embedder);
        let (mid_discovery_stats, social_topics) = discoverer.run().await;
        if mid_discovery_stats.actor_sources + mid_discovery_stats.gap_sources > 0 {
            info!("{mid_discovery_stats}");
        }

        self.check_cancelled()?;

        // ================================================================
        // Phase B: Find Responses — scrape response sources + fresh discovery sources
        // ================================================================
        info!("=== Phase B: Find Responses ===");

        // Reload sources to pick up fresh discovery sources from mid-run
        let fresh_sources = match self.writer.get_active_sources(&self.city_node.slug).await {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "Failed to reload sources for Phase B");
                Vec::new()
            }
        };

        // Phase B includes: originally-scheduled response sources + never-scraped fresh discovery sources
        let phase_b_sources: Vec<&SourceNode> = fresh_sources
            .iter()
            .filter(|s| {
                response_phase_keys.contains(&s.canonical_key)
                    || (s.last_scraped.is_none() && !scheduled_keys.contains(&s.canonical_key))
            })
            .collect();

        // Extend URL→canonical_key with fresh sources
        for s in &fresh_sources {
            if let Some(ref url) = s.url {
                url_to_canonical_key
                    .entry(sanitize_url(url))
                    .or_insert_with(|| s.canonical_key.clone());
            }
        }

        if !phase_b_sources.is_empty() {
            info!(
                count = phase_b_sources.len(),
                "Phase B sources (response + fresh discovery)"
            );
            let phase_b_errors = self
                .scrape_phase(
                    &phase_b_sources,
                    &mut url_to_canonical_key,
                    &mut stats,
                    &mut embed_cache,
                    &mut source_signal_counts,
                    &mut expansion_queries,
                )
                .await;
            query_api_errors.extend(phase_b_errors);
        }

        // Phase B social: response social sources
        let phase_b_social: Vec<&SourceNode> = scheduled_sources
            .iter()
            .filter(|s| {
                matches!(
                    s.source_type,
                    SourceType::Instagram
                        | SourceType::Facebook
                        | SourceType::Reddit
                        | SourceType::Twitter
                        | SourceType::TikTok
                ) && response_phase_keys.contains(&s.canonical_key)
            })
            .copied()
            .collect();
        let known_city_urls: std::collections::HashSet<String> =
            url_to_canonical_key.keys().cloned().collect();
        if !phase_b_social.is_empty() {
            self.scrape_social_media(
                &mut stats,
                &mut embed_cache,
                &mut source_signal_counts,
                &phase_b_social,
                &known_city_urls,
                &mut expansion_queries,
            )
            .await;
        }

        self.check_cancelled()?;

        {
            // Topic discovery — search social media to find new accounts
            self.discover_from_topics(
                &social_topics,
                &mut stats,
                &mut embed_cache,
                &mut source_signal_counts,
                &known_city_urls,
            )
            .await;
        }

        // ================================================================
        // Source metrics + weight updates
        // ================================================================
        let now = Utc::now();

        // Update per-source metrics in the graph (keyed by canonical_key).
        // Skip queries where the search API itself errored — don't penalize them
        // with an empty-scrape increment since the query was never actually executed.
        for (canonical_key, signals_produced) in &source_signal_counts {
            if query_api_errors.contains(canonical_key) {
                continue;
            }
            if let Err(e) = self
                .writer
                .record_source_scrape(canonical_key, *signals_produced, now)
                .await
            {
                warn!(canonical_key, error = %e, "Failed to record source scrape metrics");
            }
        }

        // Update source weights based on scrape results.
        // Use fresh signal counts from this run to avoid stale snapshot.
        for source in &all_sources {
            let tension_count = self
                .writer
                .count_source_tensions(&source.canonical_key)
                .await
                .unwrap_or(0);
            let fresh_signals = source_signal_counts
                .get(&source.canonical_key)
                .copied()
                .unwrap_or(0);
            let total_signals = source.signals_produced + fresh_signals;
            let scrape_count =
                if fresh_signals > 0 || source_signal_counts.contains_key(&source.canonical_key) {
                    (source.scrape_count + 1).max(1)
                } else {
                    source.scrape_count.max(1)
                };
            let base_weight = crate::scheduler::compute_weight(
                total_signals,
                source.signals_corroborated,
                scrape_count,
                tension_count,
                if fresh_signals > 0 {
                    Some(now)
                } else {
                    source.last_produced_signal
                },
                now,
            );
            let new_weight = (base_weight * source.quality_penalty).clamp(0.1, 1.0);
            // Web query sources use exponential backoff based on consecutive empty runs
            let empty_runs =
                if source_signal_counts.contains_key(&source.canonical_key) && fresh_signals == 0 {
                    source.consecutive_empty_runs + 1
                } else {
                    source.consecutive_empty_runs
                };
            let cadence = if source.source_type == SourceType::WebQuery {
                crate::scheduler::cadence_hours_with_backoff(new_weight, empty_runs)
            } else {
                crate::scheduler::cadence_hours_for_weight(new_weight)
            };
            if let Err(e) = self
                .writer
                .update_source_weight(&source.canonical_key, new_weight, cadence)
                .await
            {
                warn!(canonical_key = source.canonical_key.as_str(), error = %e, "Failed to update source weight");
            }
        }

        // Deactivate dead sources (10+ consecutive empty runs, non-curated/human only)
        match self
            .writer
            .deactivate_dead_sources(&self.city_node.slug, 10)
            .await
        {
            Ok(n) if n > 0 => info!(deactivated = n, "Deactivated dead sources"),
            Ok(_) => {}
            Err(e) => warn!(error = %e, "Failed to deactivate dead sources"),
        }

        // Deactivate dead web queries (stricter: 5+ empty, 3+ scrapes, 0 signals)
        match self
            .writer
            .deactivate_dead_web_queries(&self.city_node.slug)
            .await
        {
            Ok(n) if n > 0 => info!(deactivated = n, "Deactivated dead web queries"),
            Ok(_) => {}
            Err(e) => warn!(error = %e, "Failed to deactivate dead web queries"),
        }

        // Source stats
        match self.writer.get_source_stats(&self.city_node.slug).await {
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

        // Log budget status before compute-heavy phases
        self.budget.log_status();

        self.check_cancelled()?;

        // ================================================================
        // Synthesis — clustering, response mapping, investigation, end-of-run discovery
        // ================================================================

        // Clustering — build similarity edges, run Leiden, create/update stories
        info!("Starting clustering...");
        let entity_mappings: Vec<rootsignal_common::EntityMappingOwned> = Vec::new();

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

        self.check_cancelled()?;

        // ----------------------------------------------------------------
        // Parallel synthesis — run independent finders concurrently.
        // Each finder targets a different slice of the graph (see pressure
        // test in commit message) so there are no write conflicts.
        // Story Weaving must run AFTER because it reads edges created here.
        // ----------------------------------------------------------------
        info!("Starting parallel synthesis (response mapping, tension linker, response finder, gathering finder, investigation)...");

        // Snapshot budget decisions before launching — all checks are on &self
        let run_response_mapping = self
            .budget
            .has_budget(OperationCost::CLAUDE_HAIKU_SYNTHESIS * 10);
        let run_tension_linker = self.budget.has_budget(
            OperationCost::CLAUDE_HAIKU_TENSION_LINKER + OperationCost::SEARCH_TENSION_LINKER,
        );
        let run_response_finder = self.budget.has_budget(
            OperationCost::CLAUDE_HAIKU_RESPONSE_FINDER + OperationCost::SEARCH_RESPONSE_FINDER,
        );
        let run_gathering_finder = self.budget.has_budget(
            OperationCost::CLAUDE_HAIKU_GATHERING_FINDER + OperationCost::SEARCH_GATHERING_FINDER,
        );
        let run_investigation = self.budget.has_budget(
            OperationCost::CLAUDE_HAIKU_INVESTIGATION + OperationCost::SEARCH_INVESTIGATION,
        );

        let (rm_result, tl_result, rf_result, gf_result, inv_result) = tokio::join!(
            // Response mapping — match Give/Event to Tensions/Needs
            async {
                if run_response_mapping {
                    info!("Starting response mapping...");
                    let response_mapper = rootsignal_graph::response::ResponseMapper::new(
                        self.graph_client.clone(),
                        &self.anthropic_api_key,
                        self.city_node.center_lat,
                        self.city_node.center_lng,
                        self.city_node.radius_km,
                    );
                    match response_mapper.map_responses().await {
                        Ok(rm_stats) => info!("{rm_stats}"),
                        Err(e) => warn!(error = %e, "Response mapping failed (non-fatal)"),
                    }
                } else if self.budget.is_active() {
                    info!("Skipping response mapping (budget exhausted)");
                }
            },
            // Tension linker — ask "why?" about signals without tension context
            async {
                if run_tension_linker {
                    info!("Starting tension linker...");
                    let tension_linker = crate::tension_linker::TensionLinker::new(
                        &self.writer,
                        self.searcher.clone(),
                        self.scraper.clone(),
                        &*self.embedder,
                        &self.anthropic_api_key,
                        self.city_node.clone(),
                        self.cancelled.clone(),
                    );
                    let tl_stats = tension_linker.run().await;
                    info!("{tl_stats}");
                } else if self.budget.is_active() {
                    info!("Skipping tension linker (budget exhausted)");
                }
            },
            // Response finder — find what diffuses known tensions
            async {
                if run_response_finder {
                    info!("Starting response finder...");
                    let response_finder = crate::response_finder::ResponseFinder::new(
                        &self.writer,
                        self.searcher.clone(),
                        self.scraper.clone(),
                        &*self.embedder,
                        &self.anthropic_api_key,
                        self.city_node.clone(),
                        self.cancelled.clone(),
                    );
                    let rf_stats = response_finder.run().await;
                    info!("{rf_stats}");
                } else if self.budget.is_active() {
                    info!("Skipping response finder (budget exhausted)");
                }
            },
            // Gathering finder — find where people gather around tensions
            async {
                if run_gathering_finder {
                    info!("Starting gathering finder...");
                    let gathering_finder = crate::gathering_finder::GatheringFinder::new(
                        &self.writer,
                        self.searcher.clone(),
                        self.scraper.clone(),
                        &*self.embedder,
                        &self.anthropic_api_key,
                        self.city_node.clone(),
                        self.cancelled.clone(),
                    );
                    let gf_stats = gathering_finder.run().await;
                    info!("{gf_stats}");
                } else if self.budget.is_active() {
                    info!("Skipping gathering finder (budget exhausted)");
                }
            },
            // Investigation — verify signals via web search
            async {
                if run_investigation {
                    info!("Starting investigation phase...");
                    let investigator = crate::investigator::Investigator::new(
                        &self.writer,
                        &*self.searcher,
                        &self.anthropic_api_key,
                        &self.city_node.name,
                        self.cancelled.clone(),
                    );
                    let investigation_stats = investigator.run().await;
                    info!("{investigation_stats}");
                } else if self.budget.is_active() {
                    info!("Skipping investigation (budget exhausted)");
                }
            },
        );

        // Suppress unused-variable warnings for the unit results
        let _ = (rm_result, tl_result, rf_result, gf_result, inv_result);

        info!("Parallel synthesis complete");

        self.check_cancelled()?;

        // Story weaving — materialize tension hubs as stories.
        // Runs after the parallel group because it reads RESPONDS_TO and
        // DRAWN_TO edges that the finders create.
        info!("Starting story weaving...");
        let weaver = rootsignal_graph::StoryWeaver::new(
            self.graph_client.clone(),
            &self.anthropic_api_key,
            self.city_node.center_lat,
            self.city_node.center_lng,
            self.city_node.radius_km,
        );
        let has_weave_budget = self
            .budget
            .has_budget(OperationCost::CLAUDE_HAIKU_STORY_WEAVE);
        match weaver.run(has_weave_budget).await {
            Ok(weave_stats) => info!("{weave_stats}"),
            Err(e) => warn!(error = %e, "Story weaving failed (non-fatal)"),
        }

        self.check_cancelled()?;

        // ================================================================
        // Signal Expansion — create sources from implied queries
        // ================================================================
        // Deferred expansion: collect implied queries from Give/Event signals
        // that are now linked to tensions via response mapping.
        match self
            .writer
            .get_recently_linked_signals_with_queries(&self.city_node.slug)
            .await
        {
            Ok(deferred) => {
                let deferred_count = deferred.len();
                expansion_queries.extend(deferred);
                if deferred_count > 0 {
                    info!(
                        deferred = deferred_count,
                        "Deferred signal expansion queries collected"
                    );
                }
                stats.expansion_deferred_expanded = deferred_count as u32;
            }
            Err(e) => warn!(error = %e, "Failed to get deferred expansion queries"),
        }

        stats.expansion_queries_collected = expansion_queries.len() as u32;

        if !expansion_queries.is_empty() {
            let existing = self
                .writer
                .get_active_web_queries(&self.city_node.slug)
                .await
                .unwrap_or_default();
            let deduped: Vec<String> = expansion_queries
                .iter()
                .filter(|q| {
                    !existing
                        .iter()
                        .any(|e| jaccard_similarity(q, e) > DEDUP_JACCARD_THRESHOLD)
                })
                .cloned()
                .take(MAX_EXPANSION_QUERIES_PER_RUN)
                .collect();

            let now_expansion = Utc::now();
            let mut created = 0u32;
            let mut expansion_dupes_skipped = 0u32;
            for query_text in &deduped {
                // Embedding-based dedup for expansion queries
                if let Ok(embedding) = self.embedder.embed(query_text).await {
                    match self
                        .writer
                        .find_similar_query(&embedding, &self.city_node.slug, 0.90)
                        .await
                    {
                        Ok(Some((existing_ck, sim))) => {
                            info!(
                                query = query_text.as_str(),
                                existing_key = existing_ck.as_str(),
                                similarity = format!("{sim:.3}").as_str(),
                                "Skipping semantically duplicate expansion query"
                            );
                            expansion_dupes_skipped += 1;
                            continue;
                        }
                        Ok(None) => {}
                        Err(e) => {
                            warn!(error = %e, "Expansion query dedup check failed, proceeding")
                        }
                    }
                }

                let cv = query_text.clone();
                let ck = crate::sources::make_canonical_key(
                    &self.city_node.slug,
                    SourceType::WebQuery,
                    &cv,
                );
                let source = SourceNode {
                    id: Uuid::new_v4(),
                    canonical_key: ck.clone(),
                    canonical_value: cv,
                    url: None,
                    source_type: SourceType::WebQuery,
                    discovery_method: DiscoveryMethod::SignalExpansion,
                    city: self.city_node.slug.clone(),
                    created_at: now_expansion,
                    last_scraped: None,
                    last_produced_signal: None,
                    signals_produced: 0,
                    signals_corroborated: 0,
                    consecutive_empty_runs: 0,
                    active: true,
                    gap_context: Some(
                        "Signal expansion: implied query from extracted signal".to_string(),
                    ),
                    weight: crate::source_finder::initial_weight_for_method(
                        DiscoveryMethod::SignalExpansion,
                        None,
                    ),
                    cadence_hours: None,
                    avg_signals_per_scrape: 0.0,
                    quality_penalty: 1.0,
                    source_role: rootsignal_common::SourceRole::Response,
                    scrape_count: 0,
                };
                match self.writer.upsert_source(&source).await {
                    Ok(_) => {
                        created += 1;
                        // Store embedding for future dedup
                        if let Ok(embedding) = self.embedder.embed(query_text).await {
                            if let Err(e) = self.writer.set_query_embedding(&ck, &embedding).await {
                                warn!(error = %e, "Failed to store expansion query embedding (non-fatal)");
                            }
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, query = query_text.as_str(), "Failed to create expansion source")
                    }
                }
            }
            stats.expansion_sources_created = created;
            info!(
                collected = expansion_queries.len(),
                created,
                deferred = stats.expansion_deferred_expanded,
                embedding_dupes = expansion_dupes_skipped,
                "Signal expansion complete"
            );
        }

        self.check_cancelled()?;

        // End-of-run discovery — find new sources for next run
        let end_discoverer = crate::source_finder::SourceFinder::new(
            &self.writer,
            &self.city_node.slug,
            &self.city_node.name,
            Some(self.anthropic_api_key.as_str()),
            &self.budget,
        )
        .with_embedder(&*self.embedder);
        let (end_discovery_stats, _end_social_topics) = end_discoverer.run().await;
        if end_discovery_stats.actor_sources + end_discovery_stats.gap_sources > 0 {
            info!("{end_discovery_stats}");
        }

        // Log final budget status
        self.budget.log_status();

        info!("{stats}");
        Ok(stats)
    }

    /// Scrape a set of sources: resolve queries → URLs, scrape pages, extract signals, store results.
    /// Used by both Phase A (tension/mixed sources) and Phase B (response/discovery sources).
    ///
    /// `url_to_canonical_key` is mutable: resolved WebQuery URLs are inserted so
    /// scrape results can be attributed back to the originating query source.
    ///
    /// Returns the set of canonical_keys for queries where the Serper API itself
    /// errored (HTTP failure, not "0 results"). These should NOT be counted as
    /// empty scrapes — the query was never actually executed.
    async fn scrape_phase(
        &self,
        sources: &[&SourceNode],
        url_to_canonical_key: &mut std::collections::HashMap<String, String>,
        stats: &mut ScoutStats,
        embed_cache: &mut EmbeddingCache,
        source_signal_counts: &mut std::collections::HashMap<String, u32>,
        expansion_queries: &mut Vec<String>,
    ) -> std::collections::HashSet<String> {
        // Partition by behavior type
        let query_sources: Vec<&&SourceNode> = sources
            .iter()
            .filter(|s| s.source_type.is_query())
            .collect();
        let page_sources: Vec<&&SourceNode> = sources
            .iter()
            .filter(|s| s.source_type == SourceType::Web)
            .collect();

        let mut phase_urls: Vec<String> = Vec::new();

        // Resolve query sources → URLs
        // Track API errors separately: queries where Serper itself failed should
        // NOT be counted as empty scrapes (the query was never executed).
        let mut query_api_errors: std::collections::HashSet<String> =
            std::collections::HashSet::new();

        let api_queries: Vec<&&&SourceNode> = query_sources
            .iter()
            .filter(|s| s.source_type == SourceType::WebQuery)
            .collect();
        if !api_queries.is_empty() {
            info!(
                queries = api_queries.len(),
                "Resolving web search queries..."
            );
            let search_results: Vec<_> = stream::iter(api_queries.iter().map(|source| {
                let query_str = source.canonical_value.clone();
                let canonical_key = source.canonical_key.clone();
                async move {
                    (
                        canonical_key,
                        query_str.clone(),
                        self.searcher.search(&query_str, 5).await,
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
                            url_to_canonical_key
                                .entry(clean)
                                .or_insert_with(|| canonical_key.clone());
                        }
                        // Ensure the query source gets a source_signal_counts entry
                        // even if all its URLs end up deduped/empty (records a scrape).
                        source_signal_counts
                            .entry(canonical_key.clone())
                            .or_default();
                        for r in results {
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
            .filter(|s| s.source_type.link_pattern().is_some())
            .collect();
        for source in &html_queries {
            if let (Some(url), Some(pattern)) = (&source.url, source.source_type.link_pattern()) {
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

        // Deduplicate
        phase_urls.sort();
        phase_urls.dedup();

        // Filter blocked URLs
        let mut allowed_urls = Vec::with_capacity(phase_urls.len());
        for url in &phase_urls {
            match self.writer.is_blocked(url).await {
                Ok(true) => info!(url, "Skipping blocked URL"),
                _ => allowed_urls.push(url.clone()),
            }
        }
        let phase_urls = allowed_urls;
        info!(urls = phase_urls.len(), "Phase URLs to scrape");

        if phase_urls.is_empty() {
            return query_api_errors;
        }

        // Scrape + extract in parallel
        let pipeline_results: Vec<_> = stream::iter(phase_urls.iter().map(|url| {
            let url = url.clone();
            async move {
                let clean_url = sanitize_url(&url);

                let content = match self.scraper.scrape(&url).await {
                    Ok(c) if !c.is_empty() => c,
                    Ok(_) => return (clean_url, ScrapeOutcome::Failed),
                    Err(e) => {
                        warn!(url, error = %e, "Scrape failed");
                        return (clean_url, ScrapeOutcome::Failed);
                    }
                };

                let hash = format!("{:x}", content_hash(&content));
                match self.writer.content_already_processed(&hash, &clean_url).await {
                    Ok(true) => {
                        info!(url = clean_url.as_str(), "Content unchanged, skipping extraction");
                        return (clean_url, ScrapeOutcome::Unchanged);
                    }
                    Ok(false) => {}
                    Err(e) => {
                        warn!(url = clean_url.as_str(), error = %e, "Hash check failed, proceeding with extraction");
                    }
                }

                match self.extractor.extract(&content, &clean_url).await {
                    Ok(result) => (clean_url, ScrapeOutcome::New {
                        content,
                        nodes: result.nodes,
                        resource_tags: result.resource_tags,
                    }),
                    Err(e) => {
                        warn!(url = clean_url.as_str(), error = %e, "Extraction failed");
                        (clean_url, ScrapeOutcome::Failed)
                    }
                }
            }
        }))
        .buffer_unordered(3)
        .collect()
        .await;

        // Process results
        let now = Utc::now();
        for (url, outcome) in pipeline_results {
            let ck = url_to_canonical_key
                .get(&url)
                .cloned()
                .unwrap_or_else(|| url.clone());
            match outcome {
                ScrapeOutcome::New {
                    content,
                    nodes,
                    resource_tags,
                } => {
                    // Collect implied queries from Tension + Need nodes for immediate expansion
                    for node in &nodes {
                        if matches!(node.node_type(), NodeType::Tension | NodeType::Need) {
                            if let Some(meta) = node.meta() {
                                expansion_queries.extend(meta.implied_queries.iter().cloned());
                            }
                        }
                    }

                    let signal_count_before = stats.signals_stored;
                    let known_urls: std::collections::HashSet<String> =
                        url_to_canonical_key.keys().cloned().collect();
                    match self
                        .store_signals(
                            &url,
                            &content,
                            nodes,
                            resource_tags,
                            stats,
                            embed_cache,
                            &known_urls,
                        )
                        .await
                    {
                        Ok(_) => {
                            stats.urls_scraped += 1;
                            let produced = stats.signals_stored - signal_count_before;
                            *source_signal_counts.entry(ck).or_default() += produced;
                        }
                        Err(e) => {
                            warn!(url, error = %e, "Failed to store signals");
                            stats.urls_failed += 1;
                            source_signal_counts.entry(ck).or_default();
                        }
                    }
                }
                ScrapeOutcome::Unchanged => {
                    match self.writer.refresh_url_signals(&url, now).await {
                        Ok(n) if n > 0 => info!(url, refreshed = n, "Refreshed unchanged signals"),
                        Ok(_) => {}
                        Err(e) => warn!(url, error = %e, "Failed to refresh signals"),
                    }
                    stats.urls_unchanged += 1;
                    source_signal_counts.entry(ck).or_default();
                }
                ScrapeOutcome::Failed => {
                    stats.urls_failed += 1;
                }
            }
        }

        query_api_errors
    }

    /// Scrape social media accounts, feed posts through LLM extraction.
    /// All social sources are loaded from the graph as SourceNodes.
    async fn scrape_social_media(
        &self,
        stats: &mut ScoutStats,
        embed_cache: &mut EmbeddingCache,
        source_signal_counts: &mut std::collections::HashMap<String, u32>,
        social_sources: &[&SourceNode],
        known_city_urls: &std::collections::HashSet<String>,
        expansion_queries: &mut Vec<String>,
    ) {
        use std::future::Future;
        use std::pin::Pin;

        type SocialResult = Option<(
            String,
            String,
            String,
            Vec<Node>,
            Vec<(Uuid, Vec<ResourceTag>)>,
            usize,
        )>; // (canonical_key, source_url, combined_text, nodes, resource_tags, post_count)

        // Build uniform list of SocialAccounts from SourceNodes
        let mut accounts: Vec<(String, String, SocialAccount)> = Vec::new(); // (canonical_key, source_url, account)

        for source in social_sources {
            let (platform, identifier) = match source.source_type {
                SourceType::Instagram => {
                    (SocialPlatform::Instagram, source.canonical_value.clone())
                }
                SourceType::Facebook => {
                    let url = source
                        .url
                        .as_deref()
                        .filter(|u| !u.is_empty())
                        .unwrap_or(&source.canonical_value);
                    (SocialPlatform::Facebook, url.to_string())
                }
                SourceType::Reddit => {
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
                SourceType::Twitter => (SocialPlatform::Twitter, source.canonical_value.clone()),
                SourceType::TikTok => (SocialPlatform::TikTok, source.canonical_value.clone()),
                _ => continue,
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
        let mut futures: Vec<Pin<Box<dyn Future<Output = SocialResult> + Send + '_>>> = Vec::new();

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
                        post_count,
                    ))
                }
            }));
        }

        let results: Vec<_> = stream::iter(futures).buffer_unordered(10).collect().await;

        for result in results.into_iter().flatten() {
            let (canonical_key, source_url, combined_text, nodes, resource_tags, post_count) =
                result;
            // Collect implied queries from Tension/Need social signals
            for node in &nodes {
                if matches!(node.node_type(), NodeType::Tension | NodeType::Need) {
                    if let Some(meta) = node.meta() {
                        expansion_queries.extend(meta.implied_queries.iter().cloned());
                    }
                }
            }
            stats.social_media_posts += post_count as u32;
            let signal_count_before = stats.signals_stored;
            if let Err(e) = self
                .store_signals(
                    &source_url,
                    &combined_text,
                    nodes,
                    resource_tags,
                    stats,
                    embed_cache,
                    known_city_urls,
                )
                .await
            {
                warn!(source_url = source_url.as_str(), error = %e, "Failed to store social media signals");
            }
            let produced = stats.signals_stored - signal_count_before;
            *source_signal_counts.entry(canonical_key).or_default() += produced;
        }
    }

    /// Discover new accounts by searching platform-agnostic topics (hashtags/keywords)
    /// across Instagram, X/Twitter, TikTok, and GoFundMe.
    async fn discover_from_topics(
        &self,
        topics: &[String],
        stats: &mut ScoutStats,
        embed_cache: &mut EmbeddingCache,
        source_signal_counts: &mut std::collections::HashMap<String, u32>,
        known_city_urls: &std::collections::HashSet<String>,
    ) {
        use crate::scraper::SocialPlatform;

        const MAX_SOCIAL_SEARCHES: usize = 3;
        const MAX_GOFUNDME_SEARCHES: usize = 2;
        const MAX_NEW_ACCOUNTS: usize = 5;
        const POSTS_PER_SEARCH: u32 = 20;
        const CAMPAIGNS_PER_SEARCH: u32 = 10;

        if topics.is_empty() {
            return;
        }

        info!(topics = ?topics, "Starting social topic discovery...");

        // Load existing sources for dedup across all platforms
        let existing_sources = match self.writer.get_active_sources(&self.city_node.slug).await {
            Ok(s) => s,
            Err(_) => Vec::new(),
        };
        let existing_canonical_values: std::collections::HashSet<String> = existing_sources
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
            (SocialPlatform::Instagram, SourceType::Instagram),
            (SocialPlatform::Twitter, SourceType::Twitter),
            (SocialPlatform::TikTok, SourceType::TikTok),
        ];

        for (platform, source_type) in &platforms {
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

            stats.discovery_posts_found += discovered_posts.len() as u32;

            // Group posts by author
            let mut by_author: std::collections::HashMap<String, Vec<&SocialPost>> =
                std::collections::HashMap::new();
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
                    SocialPlatform::Instagram => format!("https://www.instagram.com/{username}/"),
                    SocialPlatform::Twitter => format!("https://x.com/{username}"),
                    SocialPlatform::TikTok => format!("https://www.tiktok.com/@{username}"),
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
                let signal_count_before = stats.signals_stored;
                if let Err(e) = self
                    .store_signals(
                        &source_url,
                        &combined_text,
                        result.nodes,
                        result.resource_tags,
                        stats,
                        embed_cache,
                        known_city_urls,
                    )
                    .await
                {
                    warn!(username, error = %e, "Failed to store discovery signals");
                    continue;
                }
                let produced = stats.signals_stored - signal_count_before;

                // Create a Source node with correct platform type
                let cv = sources::canonical_value_from_url(*source_type, &source_url);
                let ck = sources::make_canonical_key(&self.city_node.slug, *source_type, &cv);
                let gap_context = format!(
                    "Topic: {}",
                    topics.first().map(|t| t.as_str()).unwrap_or("unknown")
                );
                let source = SourceNode {
                    id: Uuid::new_v4(),
                    canonical_key: ck.clone(),
                    canonical_value: cv,
                    url: Some(source_url.clone()),
                    source_type: *source_type,
                    discovery_method: DiscoveryMethod::HashtagDiscovery,
                    city: self.city_node.slug.clone(),
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
                    source_role: rootsignal_common::SourceRole::default(),
                    scrape_count: 0,
                };

                *source_signal_counts.entry(ck).or_default() += produced;

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

        // GoFundMe: search campaigns → extract signals (no auto-follow)
        let gofundme_topics: Vec<&str> = topics
            .iter()
            .take(MAX_GOFUNDME_SEARCHES)
            .map(|t| t.as_str())
            .collect();
        for topic in &gofundme_topics {
            let campaigns = match self
                .social
                .search_gofundme(topic, CAMPAIGNS_PER_SEARCH)
                .await
            {
                Ok(c) => c,
                Err(e) => {
                    warn!(topic, error = %e, "GoFundMe discovery failed");
                    continue;
                }
            };

            if campaigns.is_empty() {
                continue;
            }

            info!(topic, count = campaigns.len(), "GoFundMe campaigns found");

            for campaign in &campaigns {
                let title = campaign.title.as_deref().unwrap_or("Untitled campaign");
                let desc = campaign.description.as_deref().unwrap_or("");
                let location = campaign.location.as_deref().unwrap_or("");
                let organizer = campaign.organizer_name.as_deref().unwrap_or("");
                let amount = campaign.current_amount.unwrap_or(0.0);
                let goal = campaign.goal_amount.unwrap_or(0.0);

                let content = format!(
                    "GoFundMe Campaign: {title}\nOrganizer: {organizer}\nLocation: {location}\n\
                     Raised: ${amount:.0} of ${goal:.0} goal\n\n{desc}"
                );

                let source_url = campaign
                    .url
                    .as_deref()
                    .unwrap_or("https://www.gofundme.com");

                let result = match self.extractor.extract(&content, source_url).await {
                    Ok(r) => r,
                    Err(e) => {
                        warn!(title, error = %e, "GoFundMe extraction failed");
                        continue;
                    }
                };

                if result.nodes.is_empty() {
                    continue;
                }

                if let Err(e) = self
                    .store_signals(
                        source_url,
                        &content,
                        result.nodes,
                        result.resource_tags,
                        stats,
                        embed_cache,
                        known_city_urls,
                    )
                    .await
                {
                    warn!(title, error = %e, "Failed to store GoFundMe signals");
                }
            }
        }

        stats.discovery_accounts_found = new_accounts;
        info!(
            topics = topics.len(),
            new_accounts, "Social topic discovery complete"
        );
    }

    async fn store_signals(
        &self,
        url: &str,
        content: &str,
        mut nodes: Vec<Node>,
        resource_tags: Vec<(Uuid, Vec<ResourceTag>)>,
        stats: &mut ScoutStats,
        embed_cache: &mut EmbeddingCache,
        known_city_urls: &std::collections::HashSet<String>,
    ) -> Result<()> {
        let url = sanitize_url(url);
        stats.signals_extracted += nodes.len() as u32;

        // Build lookup map from node ID → resource tags
        let resource_map: std::collections::HashMap<Uuid, Vec<ResourceTag>> =
            resource_tags.into_iter().collect();

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
        let center_lat = self.city_node.center_lat;
        let center_lng = self.city_node.center_lng;
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
                    stats.geo_stripped += 1;
                }
            }
        }

        // Layered geo-check:
        // 1. Has coordinates within radius → accept
        // 2. Has coordinates outside radius → reject
        // 3. No coordinates, location_name matches a geo_term → accept
        // 4. No coordinates, no location_name match, source is city-local → accept with 0.8x confidence
        // 5. No coordinates, no match, source not city-local → reject
        let geo_terms = &self.city_node.geo_terms;
        let center_lat = self.city_node.center_lat;
        let center_lng = self.city_node.center_lng;
        let radius_km = self.city_node.radius_km;

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
                    stats.geo_filtered += 1;
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
                    stats.geo_filtered += 1;
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
                    meta.location = Some(rootsignal_common::GeoPoint {
                        lat: center_lat,
                        lng: center_lng,
                        precision: rootsignal_common::GeoPrecision::City,
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
            info!(
                url = url.as_str(),
                skipped = url_deduped,
                "URL-based title dedup"
            );
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
                    self.writer.create_evidence(&evidence, *existing_id).await?;
                    stats.signals_deduplicated += 1;
                    continue;
                } else {
                    // Same-source re-scrape: signal already exists from this URL.
                    // Refresh to prove it's still active, but don't inflate corroboration.
                    // create_evidence uses MERGE so it updates the existing evidence hash.
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
        // The LLM compresses semantic differences using similar vocabulary.
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
                embed_cache.find_match(&embedding, 0.85)
            {
                let is_same_source = cached_url == url;
                if is_same_source {
                    // Same-source re-extraction: refresh only, don't inflate corroboration
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

                    stats.signals_deduplicated += 1;
                    continue;
                } else if sim >= 0.92 {
                    // Cross-source corroboration: different URL, high confidence match
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

                    stats.signals_deduplicated += 1;
                    continue;
                }
            }

            // 3b: Check graph index (catches dupes from previous runs, city-scoped)
            let lat_delta = self.city_node.radius_km / 111.0;
            let lng_delta = self.city_node.radius_km
                / (111.0 * self.city_node.center_lat.to_radians().cos());
            match self
                .writer
                .find_duplicate(
                    &embedding,
                    node_type,
                    0.85,
                    self.city_node.center_lat - lat_delta,
                    self.city_node.center_lat + lat_delta,
                    self.city_node.center_lng - lng_delta,
                    self.city_node.center_lng + lng_delta,
                )
                .await
            {
                Ok(Some(dup)) => {
                    let dominated_url = sanitize_url(&dup.source_url);
                    let is_same_source = dominated_url == url;
                    if is_same_source {
                        // Same-source re-scrape: refresh only, don't inflate corroboration
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

                        embed_cache.add(embedding, dup.id, dup.node_type, dominated_url);

                        stats.signals_deduplicated += 1;
                        continue;
                    } else if dup.similarity >= 0.92 {
                        // Cross-source corroboration: different URL, high confidence match
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
                            let actor = rootsignal_common::ActorNode {
                                id: Uuid::new_v4(),
                                name: actor_name.clone(),
                                actor_type: rootsignal_common::ActorType::Organization,
                                entity_id: actor_name.to_lowercase().replace(' ', "-"),
                                domains: vec![],
                                social_urls: vec![],
                                city: self.city_node.name.clone(),
                                description: String::new(),
                                signal_count: 0,
                                first_seen: Utc::now(),
                                last_active: Utc::now(),
                                typical_roles: vec![],
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
                    for tag in tags.iter().filter(|t| t.confidence >= 0.3) {
                        let slug = rootsignal_common::slugify(&tag.slug);
                        let embed_text =
                            format!("{}: {}", tag.slug, tag.context.as_deref().unwrap_or(""));
                        let res_embedding = match self.embedder.embed(&embed_text).await {
                            Ok(e) => e,
                            Err(e) => {
                                warn!(error = %e, slug = slug.as_str(), "Resource embedding failed (non-fatal)");
                                continue;
                            }
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
        "_dt",
        "fbclid",
        "gclid",
        "utm_source",
        "utm_medium",
        "utm_campaign",
        "utm_term",
        "utm_content",
        "modal",
        "ref",
        "mc_cid",
        "mc_eid",
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
        Node::Gathering(n) => Some(&mut n.meta),
        Node::Aid(n) => Some(&mut n.meta),
        Node::Need(n) => Some(&mut n.meta),
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

// --- Signal Expansion helpers ---

const DEDUP_JACCARD_THRESHOLD: f64 = 0.6;
const MAX_EXPANSION_QUERIES_PER_RUN: usize = 10;

/// Token-based Jaccard similarity for query dedup.
/// Uses word overlap rather than substring matching to preserve specific long-tail queries.
fn jaccard_similarity(a: &str, b: &str) -> f64 {
    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();
    let a_tokens: std::collections::HashSet<&str> = a_lower.split_whitespace().collect();
    let b_tokens: std::collections::HashSet<&str> = b_lower.split_whitespace().collect();
    let intersection = a_tokens.intersection(&b_tokens).count();
    let union = a_tokens.union(&b_tokens).count();
    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jaccard_specific_vs_generic_passes() {
        // "emergency housing for detained immigrants" vs "housing"
        // Only share "housing" — 1 token overlap out of 5+1=5 unique → 1/5 = 0.2
        let sim = jaccard_similarity("emergency housing for detained immigrants", "housing");
        assert!(
            sim < DEDUP_JACCARD_THRESHOLD,
            "Specific long-tail query should not match generic: {sim}"
        );
    }

    #[test]
    fn jaccard_similar_queries_blocked() {
        // "housing assistance Minneapolis" vs "housing resources Minneapolis"
        // Tokens: {housing, assistance, minneapolis} vs {housing, resources, minneapolis}
        // Intersection: {housing, minneapolis} = 2
        // Union: {housing, assistance, minneapolis, resources} = 4
        // Jaccard = 2/4 = 0.5 — wait, let me recount
        // Actually with lowercased:
        // a: {housing, assistance, minneapolis} (3)
        // b: {housing, resources, minneapolis} (3)
        // intersection: {housing, minneapolis} (2)
        // union: {housing, assistance, resources, minneapolis} (4)
        // 2/4 = 0.5 — below 0.6 threshold
        // Let me use a more overlapping example
        let sim = jaccard_similarity(
            "housing assistance programs Minneapolis",
            "housing assistance resources Minneapolis",
        );
        // a: {housing, assistance, programs, minneapolis} (4)
        // b: {housing, assistance, resources, minneapolis} (4)
        // intersection: {housing, assistance, minneapolis} (3)
        // union: {housing, assistance, programs, resources, minneapolis} (5)
        // 3/5 = 0.6 — at threshold
        assert!(
            sim >= DEDUP_JACCARD_THRESHOLD,
            "Similar queries should be flagged as duplicate: {sim}"
        );
    }

    #[test]
    fn jaccard_identical_blocked() {
        let sim = jaccard_similarity(
            "immigration legal aid Minneapolis",
            "immigration legal aid Minneapolis",
        );
        assert!(
            (sim - 1.0).abs() < f64::EPSILON,
            "Identical queries should have Jaccard 1.0: {sim}"
        );
    }

    #[test]
    fn jaccard_empty_strings() {
        assert_eq!(jaccard_similarity("", ""), 0.0);
        assert_eq!(jaccard_similarity("hello", ""), 0.0);
    }

    #[test]
    fn jaccard_case_insensitive() {
        let sim = jaccard_similarity("Housing Minneapolis", "housing minneapolis");
        assert!(
            (sim - 1.0).abs() < f64::EPSILON,
            "Jaccard should be case-insensitive: {sim}"
        );
    }

    #[test]
    fn max_expansion_queries_constant() {
        assert_eq!(MAX_EXPANSION_QUERIES_PER_RUN, 10);
    }

    #[test]
    fn dedup_threshold_constant() {
        assert!((DEDUP_JACCARD_THRESHOLD - 0.6).abs() < f64::EPSILON);
    }
}
