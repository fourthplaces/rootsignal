use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;
use tracing::{error, info, warn};

use rootsignal_common::{
    CityNode, DiscoveryMethod, SourceNode, SourceType,
};
use rootsignal_graph::{GraphClient, GraphWriter, SimilarityBuilder};

use crate::budget::{BudgetTracker, OperationCost};
use crate::embedder::TextEmbedder;
use crate::extractor::{Extractor, SignalExtractor};
use crate::expansion::Expansion;
use crate::metrics::Metrics;
use crate::scrape_phase::{RunContext, ScrapePhase};
use crate::scraper::{
    self, NoopSocialScraper, PageScraper, SerperSearcher, SocialScraper, WebSearcher,
};
use crate::util::sanitize_url;

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
            embedder: Box::new(crate::embedder::Embedder::new(voyage_api_key)),
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
        // ================================================================
        // 1. Reap expired signals
        // ================================================================
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

        // ================================================================
        // 2. Load sources + Schedule
        // ================================================================
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

        // Web query tiered scheduling
        let wq_schedule = crate::scheduler::schedule_web_queries(&all_sources, 0, now_schedule);
        let wq_scheduled_keys: std::collections::HashSet<String> =
            wq_schedule.scheduled.into_iter().collect();

        let scheduled_sources: Vec<&SourceNode> = all_sources
            .iter()
            .filter(|s| {
                if !scheduled_keys.contains(&s.canonical_key) {
                    return false;
                }
                if s.source_type != SourceType::WebQuery {
                    return true;
                }
                wq_scheduled_keys.contains(&s.canonical_key)
            })
            .collect();

        // Create shared run context and scrape phase
        let mut ctx = RunContext::new(&all_sources);

        let phase = ScrapePhase::new(
            &self.writer,
            &*self.extractor,
            &*self.embedder,
            self.scraper.clone(),
            self.searcher.clone(),
            &*self.social,
            &self.city_node,
        );

        // ================================================================
        // 3. Phase A: Find Problems — scrape tension + mixed sources
        // ================================================================
        info!("=== Phase A: Find Problems ===");
        let phase_a_sources: Vec<&SourceNode> = scheduled_sources
            .iter()
            .filter(|s| tension_phase_keys.contains(&s.canonical_key))
            .copied()
            .collect();

        phase.run_web(&phase_a_sources, &mut ctx).await;

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
        if !phase_a_social.is_empty() {
            phase.run_social(&phase_a_social, &mut ctx).await;
        }

        self.check_cancelled()?;

        // ================================================================
        // 4. Mid-Run Discovery
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
        // 5. Phase B: Find Responses — scrape response + fresh discovery sources
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
                ctx.url_to_canonical_key
                    .entry(sanitize_url(url))
                    .or_insert_with(|| s.canonical_key.clone());
            }
        }

        if !phase_b_sources.is_empty() {
            info!(
                count = phase_b_sources.len(),
                "Phase B sources (response + fresh discovery)"
            );
            phase.run_web(&phase_b_sources, &mut ctx).await;
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
        if !phase_b_social.is_empty() {
            phase.run_social(&phase_b_social, &mut ctx).await;
        }

        self.check_cancelled()?;

        // Topic discovery — search social media to find new accounts
        phase
            .discover_from_topics(&social_topics, &mut ctx)
            .await;

        // ================================================================
        // 6. Source metrics + weight updates
        // ================================================================
        let metrics = Metrics::new(&self.writer, &self.city_node.slug);
        metrics.update(&all_sources, &ctx, Utc::now()).await;

        // Log budget status before compute-heavy phases
        self.budget.log_status();

        self.check_cancelled()?;

        // ================================================================
        // 7. Synthesis — similarity edges + parallel finders
        // ================================================================

        // Build similarity edges (Leiden removed — StoryWeaver is the sole story creator)
        info!("Building similarity edges...");
        let similarity = SimilarityBuilder::new(self.graph_client.clone());
        similarity.clear_edges().await.unwrap_or_else(|e| {
            warn!(error = %e, "Failed to clear similarity edges");
            0
        });
        match similarity.build_edges().await {
            Ok(edges) => info!(edges, "Similarity edges built"),
            Err(e) => warn!(error = %e, "Similarity edge building failed (non-fatal)"),
        }

        self.check_cancelled()?;

        // Parallel synthesis — run independent finders concurrently.
        info!("Starting parallel synthesis (response mapping, tension linker, response finder, gathering finder, investigation)...");

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

        let _ = (rm_result, tl_result, rf_result, gf_result, inv_result);

        info!("Parallel synthesis complete");

        self.check_cancelled()?;

        // ================================================================
        // 8. Story Weaving (must run AFTER synthesis — reads edges created above)
        // ================================================================
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
        // 9. Signal Expansion — create sources from implied queries
        // ================================================================
        let expansion = Expansion::new(&self.writer, &*self.embedder, &self.city_node.slug);
        expansion.run(&mut ctx).await;

        self.check_cancelled()?;

        // ================================================================
        // 10. End-of-run discovery — find new sources for next run
        // ================================================================
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

        info!("{}", ctx.stats);
        Ok(ctx.stats)
    }
}
