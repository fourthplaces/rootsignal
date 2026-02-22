use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;
use sqlx::PgPool;
use tracing::{error, info, warn};
use uuid::Uuid;

use rootsignal_common::{
    is_web_query, scraping_strategy, ScoutScope, DiscoveryMethod, ScrapingStrategy, SourceNode,
};
use rootsignal_graph::{GraphClient, GraphWriter, SimilarityBuilder};

use rootsignal_archive::{Archive, ArchiveConfig, FetchBackend, PageBackend};

use crate::budget::{BudgetTracker, OperationCost};
use crate::embedder::TextEmbedder;
use crate::extractor::{Extractor, SignalExtractor};
use crate::expansion::Expansion;
use crate::metrics::Metrics;
use crate::run_log::{EventKind, RunLog};
use crate::scrape_phase::{RunContext, ScrapePhase};
use crate::source_finder::SourceFinderStats;
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
    pub geo_filtered: u32,
    pub discovery_posts_found: u32,
    pub discovery_accounts_found: u32,
    pub expansion_queries_collected: u32,
    pub expansion_sources_created: u32,
    pub expansion_deferred_expanded: u32,
    pub expansion_social_topics_queued: u32,
}

impl std::fmt::Display for ScoutStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "\n=== Scout Run Complete ===")?;
        writeln!(f, "URLs scraped:       {}", self.urls_scraped)?;
        writeln!(f, "URLs unchanged:     {}", self.urls_unchanged)?;
        writeln!(f, "URLs failed:        {}", self.urls_failed)?;
        writeln!(f, "Social media posts: {}", self.social_media_posts)?;
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
            if self.expansion_social_topics_queued > 0 {
                writeln!(
                    f,
                    "  Social topics:     {}",
                    self.expansion_social_topics_queued
                )?;
            }
        }
        Ok(())
    }
}

fn check_cancelled_flag(cancelled: &AtomicBool) -> Result<()> {
    if cancelled.load(Ordering::Relaxed) {
        info!("Scout run cancelled by user");
        anyhow::bail!("Scout run cancelled");
    }
    Ok(())
}

// ============================================================================
// ScrapePipeline — decomposed scrape-pipeline phases
// ============================================================================

/// Bundles the shared dependencies for the scrape pipeline.
/// Each phase method borrows `&self` to access them.
pub struct ScrapePipeline<'a> {
    writer: GraphWriter,
    extractor: Arc<dyn SignalExtractor>,
    embedder: Arc<dyn TextEmbedder>,
    archive: Arc<dyn FetchBackend>,
    anthropic_api_key: String,
    region: ScoutScope,
    budget: &'a BudgetTracker,
    cancelled: Arc<AtomicBool>,
    run_id: String,
}

/// Phase 2 outputs that flow into subsequent phases.
pub(crate) struct ScheduledRun {
    all_sources: Vec<SourceNode>,
    scheduled_sources: Vec<SourceNode>,
    tension_phase_keys: HashSet<String>,
    response_phase_keys: HashSet<String>,
    scheduled_keys: HashSet<String>,
    phase: ScrapePhase,
}

impl<'a> ScrapePipeline<'a> {
    pub fn new(
        writer: GraphWriter,
        extractor: Arc<dyn SignalExtractor>,
        embedder: Arc<dyn TextEmbedder>,
        archive: Arc<dyn FetchBackend>,
        anthropic_api_key: String,
        region: ScoutScope,
        budget: &'a BudgetTracker,
        cancelled: Arc<AtomicBool>,
        run_id: String,
    ) -> Self {
        Self {
            writer,
            extractor,
            embedder,
            archive,
            anthropic_api_key,
            region,
            budget,
            cancelled,
            run_id,
        }
    }

    /// Remove stale signals from the graph.
    pub async fn reap_expired_signals(&self, run_log: &mut RunLog) {
        info!("Reaping expired signals...");
        match self.writer.reap_expired().await {
            Ok(reap) => {
                run_log.log(EventKind::ReapExpired {
                    gatherings: reap.gatherings,
                    needs: reap.needs,
                    stale: reap.stale,
                });
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
    }

    /// Load sources, run scheduler, build RunContext and ScrapePhase.
    /// Returns the ScheduledRun and RunContext needed by subsequent phases.
    pub(crate) async fn load_and_schedule_sources(
        &self,
        run_log: &mut RunLog,
    ) -> Result<(ScheduledRun, RunContext)> {
        let mut all_sources = match self.writer.get_active_sources().await {
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

        // Self-heal: if region has zero sources, re-run the cold-start bootstrapper.
        if all_sources.is_empty() {
            info!("No sources found — running cold-start bootstrap");
            let bootstrapper = crate::bootstrap::Bootstrapper::new(
                &self.writer,
                self.archive.clone(),
                &self.anthropic_api_key,
                self.region.clone(),
            );
            match bootstrapper.run().await {
                Ok(n) => {
                    run_log.log(EventKind::Bootstrap { sources_created: n as u64 });
                    info!(sources = n, "Bootstrap created seed sources");
                }
                Err(e) => warn!(error = %e, "Bootstrap failed"),
            }
            all_sources = self.writer
                .get_active_sources()
                .await
                .unwrap_or_default();
        }

        // Actor discovery — if no actors in region, discover from web pages
        let (min_lat, max_lat, min_lng, max_lng) = self.region.bounding_box();
        let actors_in_region = self.writer
            .find_actors_in_region(min_lat, max_lat, min_lng, max_lng)
            .await
            .unwrap_or_default();

        if actors_in_region.is_empty() {
            info!("No actors in region — running actor discovery");
            let bootstrapper = crate::bootstrap::Bootstrapper::new(
                &self.writer,
                self.archive.clone(),
                &self.anthropic_api_key,
                self.region.clone(),
            );
            let discovered = bootstrapper.discover_actor_pages().await;
            info!(count = discovered.len(), "Actor discovery complete");
        }

        // Actor sources — inject known actor accounts with elevated priority
        let actor_pairs = match self.writer
            .find_actors_in_region(min_lat, max_lat, min_lng, max_lng)
            .await
        {
            Ok(pairs) => {
                let actor_count = pairs.len();
                let source_count: usize = pairs.iter().map(|(_, s)| s.len()).sum();
                if actor_count > 0 {
                    info!(
                        actors = actor_count,
                        sources = source_count,
                        "Loaded actor accounts for region"
                    );
                }
                pairs
            }
            Err(e) => {
                warn!(error = %e, "Failed to load actor accounts, continuing without");
                Vec::new()
            }
        };

        // Boost existing entity sources or add new ones
        let _existing_keys: HashSet<String> =
            all_sources.iter().map(|s| s.canonical_key.clone()).collect();
        for (_actor, sources) in &actor_pairs {
            for source in sources {
                if let Some(existing) = all_sources
                    .iter_mut()
                    .find(|s| s.canonical_key == source.canonical_key)
                {
                    existing.weight = existing.weight.max(0.7);
                    existing.cadence_hours = Some(
                        existing.cadence_hours.map(|h| h.min(12)).unwrap_or(12),
                    );
                } else {
                    all_sources.push(source.clone());
                }
            }
        }

        let now_schedule = Utc::now();
        let scheduler = crate::scheduler::SourceScheduler::new();
        let schedule = scheduler.schedule(&all_sources, now_schedule);
        let scheduled_keys: HashSet<String> = schedule
            .scheduled
            .iter()
            .chain(schedule.exploration.iter())
            .map(|s| s.canonical_key.clone())
            .collect();

        let tension_phase_keys: HashSet<String> =
            schedule.tension_phase.iter().cloned().collect();
        let response_phase_keys: HashSet<String> =
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
        let wq_scheduled_keys: HashSet<String> =
            wq_schedule.scheduled.into_iter().collect();

        let scheduled_sources: Vec<SourceNode> = all_sources
            .iter()
            .filter(|s| {
                if !scheduled_keys.contains(&s.canonical_key) {
                    return false;
                }
                if !is_web_query(&s.canonical_value) {
                    return true;
                }
                wq_scheduled_keys.contains(&s.canonical_key)
            })
            .cloned()
            .collect();

        // Create shared run context and scrape phase
        let mut ctx = RunContext::new(&all_sources);

        // Populate actor contexts for location fallback during extraction
        for (actor, sources) in &actor_pairs {
            let actor_ctx = rootsignal_common::ActorContext {
                actor_name: actor.name.clone(),
                bio: actor.bio.clone(),
                location_name: actor.location_name.clone(),
                location_lat: actor.location_lat,
                location_lng: actor.location_lng,
            };
            for source in sources {
                ctx.actor_contexts
                    .insert(source.canonical_key.clone(), actor_ctx.clone());
            }
        }

        let phase = ScrapePhase::new(
            self.writer.clone(),
            self.extractor.clone(),
            self.embedder.clone(),
            self.archive.clone(),
            self.region.clone(),
            self.run_id.clone(),
        );

        let run = ScheduledRun {
            all_sources,
            scheduled_sources,
            tension_phase_keys,
            response_phase_keys,
            scheduled_keys,
            phase,
        };

        Ok((run, ctx))
    }

    /// Scrape tension + mixed sources (web pages, search queries, social accounts).
    /// This is the "find problems" pass.
    pub(crate) async fn scrape_tension_sources(
        &self,
        run: &ScheduledRun,
        ctx: &mut RunContext,
        run_log: &mut RunLog,
    ) {
        info!("=== Phase A: Find Problems ===");
        let phase_a_sources: Vec<&SourceNode> = run.scheduled_sources
            .iter()
            .filter(|s| run.tension_phase_keys.contains(&s.canonical_key))
            .collect();

        run.phase.run_web(&phase_a_sources, ctx, run_log).await;

        // Phase A social: tension + mixed social sources
        let phase_a_social: Vec<&SourceNode> = run.scheduled_sources
            .iter()
            .filter(|s| {
                matches!(scraping_strategy(s.value()), ScrapingStrategy::Social(_))
                    && run.tension_phase_keys.contains(&s.canonical_key)
            })
            .collect();
        if !phase_a_social.is_empty() {
            run.phase.run_social(&phase_a_social, ctx, run_log).await;
        }
    }

    /// Find new sources from graph analysis (actor-linked accounts, coverage gaps).
    /// Returns discovery stats and social topics discovered for later topic-based searching.
    pub(crate) async fn discover_mid_run_sources(&self) -> (SourceFinderStats, Vec<String>) {
        info!("=== Mid-Run Discovery ===");
        let discoverer = crate::source_finder::SourceFinder::new(
            &self.writer,
            &self.region.name,
            &self.region.name,
            Some(&self.anthropic_api_key),
            self.budget,
        )
        .with_embedder(&*self.embedder);
        let (stats, social_topics) = discoverer.run().await;
        if stats.actor_sources + stats.gap_sources > 0 {
            info!("{stats}");
        }
        (stats, social_topics)
    }

    /// Scrape response + fresh discovery sources (web + social), then search
    /// social platforms by topic to discover new accounts.
    pub(crate) async fn scrape_response_sources(
        &self,
        run: &ScheduledRun,
        social_topics: Vec<String>,
        ctx: &mut RunContext,
        run_log: &mut RunLog,
    ) -> Result<()> {
        info!("=== Phase B: Find Responses ===");

        // Reload sources to pick up fresh discovery sources from mid-run
        let fresh_sources = match self.writer.get_active_sources().await {
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
                run.response_phase_keys.contains(&s.canonical_key)
                    || (s.last_scraped.is_none() && !run.scheduled_keys.contains(&s.canonical_key))
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
            run.phase.run_web(&phase_b_sources, ctx, run_log).await;
        }

        // Phase B social: response social sources
        let phase_b_social: Vec<&SourceNode> = run.scheduled_sources
            .iter()
            .filter(|s| {
                matches!(scraping_strategy(s.value()), ScrapingStrategy::Social(_))
                    && run.response_phase_keys.contains(&s.canonical_key)
            })
            .collect();
        if !phase_b_social.is_empty() {
            run.phase.run_social(&phase_b_social, ctx, run_log).await;
        }

        check_cancelled_flag(&self.cancelled)?;

        // Topic discovery — search social media to find new accounts
        // Merge expansion-derived social topics with LLM-generated topics
        let mut all_social_topics = social_topics;
        all_social_topics.extend(ctx.social_expansion_topics.drain(..));
        run.phase
            .discover_from_topics(&all_social_topics, ctx, run_log)
            .await;

        Ok(())
    }

    /// Record source metrics, update weights/cadence, deactivate dead sources.
    pub(crate) async fn update_source_metrics(&self, run: &ScheduledRun, ctx: &RunContext) {
        let metrics = Metrics::new(&self.writer, &self.region.name);
        metrics.update(&run.all_sources, ctx, Utc::now()).await;

        // Log budget status before compute-heavy phases
        self.budget.log_status();
    }

    /// Create new sources from implied queries + end-of-run source discovery.
    pub(crate) async fn expand_and_discover(
        &self,
        run: &ScheduledRun,
        ctx: &mut RunContext,
        run_log: &mut RunLog,
    ) -> Result<()> {
        // Signal Expansion — create sources from implied queries
        let expansion = Expansion::new(&self.writer, &*self.embedder, &self.region.name);
        expansion.run(ctx, run_log).await;

        check_cancelled_flag(&self.cancelled)?;

        // End-of-run discovery — find new sources for next run
        let end_discoverer = crate::source_finder::SourceFinder::new(
            &self.writer,
            &self.region.name,
            &self.region.name,
            Some(&self.anthropic_api_key),
            self.budget,
        )
        .with_embedder(&*self.embedder);
        let (end_discovery_stats, end_social_topics) = end_discoverer.run().await;
        if end_discovery_stats.actor_sources + end_discovery_stats.gap_sources > 0 {
            info!("{end_discovery_stats}");
        }
        if !end_social_topics.is_empty() {
            info!(count = end_social_topics.len(), "Consuming end-of-run social topics");
            run.phase.discover_from_topics(&end_social_topics, ctx, run_log).await;
        }

        Ok(())
    }

    /// Save run log and return final stats.
    pub(crate) fn finalize(&self, ctx: RunContext, mut run_log: RunLog) -> ScoutStats {
        run_log.log(EventKind::BudgetCheckpoint {
            spent_cents: self.budget.total_spent(),
            remaining_cents: self.budget.remaining(),
        });
        if let Err(e) = run_log.save(&ctx.stats) {
            warn!(error = %e, "Failed to save scout run log");
        }

        info!("{}", ctx.stats);
        ctx.stats
    }

    /// Run all phases in sequence. Convenience for Scout::run_inner().
    pub async fn run_all(self) -> Result<ScoutStats> {
        let mut run_log = RunLog::new(self.run_id.clone(), self.region.name.clone());

        self.reap_expired_signals(&mut run_log).await;

        let (run, mut ctx) = self.load_and_schedule_sources(&mut run_log).await?;

        self.scrape_tension_sources(&run, &mut ctx, &mut run_log).await;
        check_cancelled_flag(&self.cancelled)?;

        let (_, social_topics) = self.discover_mid_run_sources().await;
        check_cancelled_flag(&self.cancelled)?;

        self.scrape_response_sources(&run, social_topics, &mut ctx, &mut run_log).await?;

        self.update_source_metrics(&run, &ctx).await;
        check_cancelled_flag(&self.cancelled)?;

        self.expand_and_discover(&run, &mut ctx, &mut run_log).await?;

        Ok(self.finalize(ctx, run_log))
    }
}

/// Step 7: Similarity edges + 6 parallel finders
pub async fn run_synthesis(
    graph_client: &GraphClient,
    writer: &GraphWriter,
    embedder: &dyn TextEmbedder,
    archive: Arc<dyn FetchBackend>,
    anthropic_api_key: &str,
    region: &ScoutScope,
    budget: &BudgetTracker,
    cancelled: Arc<AtomicBool>,
    run_id: &str,
) -> Result<()> {
    // Parallel synthesis — similarity edges + finders run concurrently.
    // Finders don't read SIMILAR_TO edges; only StoryWeaver does (runs after).
    info!("Starting parallel synthesis (similarity edges, response mapping, tension linker, response finder, gathering finder, investigation)...");

    let run_response_mapping = budget
        .has_budget(OperationCost::CLAUDE_HAIKU_SYNTHESIS * 10);
    let run_tension_linker = budget.has_budget(
        OperationCost::CLAUDE_HAIKU_TENSION_LINKER + OperationCost::SEARCH_TENSION_LINKER,
    );
    let run_response_finder = budget.has_budget(
        OperationCost::CLAUDE_HAIKU_RESPONSE_FINDER + OperationCost::SEARCH_RESPONSE_FINDER,
    );
    let run_gathering_finder = budget.has_budget(
        OperationCost::CLAUDE_HAIKU_GATHERING_FINDER + OperationCost::SEARCH_GATHERING_FINDER,
    );
    let run_investigation = budget.has_budget(
        OperationCost::CLAUDE_HAIKU_INVESTIGATION + OperationCost::SEARCH_INVESTIGATION,
    );

    let run_id_owned = run_id.to_string();

    let (sim_result, rm_result, tl_result, rf_result, gf_result, inv_result) = tokio::join!(
        async {
            info!("Building similarity edges...");
            let similarity = SimilarityBuilder::new(graph_client.clone());
            similarity.clear_edges().await.unwrap_or_else(|e| {
                warn!(error = %e, "Failed to clear similarity edges");
                0
            });
            match similarity.build_edges().await {
                Ok(edges) => info!(edges, "Similarity edges built"),
                Err(e) => warn!(error = %e, "Similarity edge building failed (non-fatal)"),
            }
        },
        async {
            if run_response_mapping {
                info!("Starting response mapping...");
                let response_mapper = rootsignal_graph::response::ResponseMapper::new(
                    graph_client.clone(),
                    anthropic_api_key,
                    region.center_lat,
                    region.center_lng,
                    region.radius_km,
                );
                match response_mapper.map_responses().await {
                    Ok(rm_stats) => info!("{rm_stats}"),
                    Err(e) => warn!(error = %e, "Response mapping failed (non-fatal)"),
                }
            } else if budget.is_active() {
                info!("Skipping response mapping (budget exhausted)");
            }
        },
        async {
            if run_tension_linker {
                info!("Starting tension linker...");
                let tension_linker = crate::tension_linker::TensionLinker::new(
                    writer,
                    archive.clone(),
                    embedder,
                    anthropic_api_key,
                    region.clone(),
                    cancelled.clone(),
                    run_id_owned.clone(),
                );
                let tl_stats = tension_linker.run().await;
                info!("{tl_stats}");
            } else if budget.is_active() {
                info!("Skipping tension linker (budget exhausted)");
            }
        },
        async {
            if run_response_finder {
                info!("Starting response finder...");
                let response_finder = crate::response_finder::ResponseFinder::new(
                    writer,
                    archive.clone(),
                    embedder,
                    anthropic_api_key,
                    region.clone(),
                    cancelled.clone(),
                    run_id_owned.clone(),
                );
                let rf_stats = response_finder.run().await;
                info!("{rf_stats}");
            } else if budget.is_active() {
                info!("Skipping response finder (budget exhausted)");
            }
        },
        async {
            if run_gathering_finder {
                info!("Starting gathering finder...");
                let gathering_finder = crate::gathering_finder::GatheringFinder::new(
                    writer,
                    archive.clone(),
                    embedder,
                    anthropic_api_key,
                    region.clone(),
                    cancelled.clone(),
                    run_id_owned.clone(),
                );
                let gf_stats = gathering_finder.run().await;
                info!("{gf_stats}");
            } else if budget.is_active() {
                info!("Skipping gathering finder (budget exhausted)");
            }
        },
        async {
            if run_investigation {
                info!("Starting investigation phase...");
                let investigator = crate::investigator::Investigator::new(
                    writer,
                    archive.clone(),
                    anthropic_api_key,
                    region,
                    cancelled.clone(),
                );
                let investigation_stats = investigator.run().await;
                info!("{investigation_stats}");
            } else if budget.is_active() {
                info!("Skipping investigation (budget exhausted)");
            }
        },
    );

    let _ = (sim_result, rm_result, tl_result, rf_result, gf_result, inv_result);

    info!("Parallel synthesis complete");
    Ok(())
}

/// Steps 8, 8b, 8c: Situation weaving + source boost + curiosity triggers
///
/// Returns the `SituationWeaverStats` from the weaving phase. Steps 8b (source
/// boost) and 8c (curiosity triggers) are always attempted regardless of 8's
/// outcome.
pub async fn run_situation_weaving(
    graph_client: &GraphClient,
    writer: &GraphWriter,
    embedder: Arc<dyn TextEmbedder>,
    anthropic_api_key: &str,
    region: &ScoutScope,
    budget: &BudgetTracker,
    run_id: &str,
) -> Result<rootsignal_graph::situation_weaver::SituationWeaverStats> {
    // ================================================================
    // 8. Situation Weaving (assigns signals to living situations)
    // ================================================================
    info!("Starting situation weaving...");
    let situation_weaver = rootsignal_graph::SituationWeaver::new(
        graph_client.clone(),
        anthropic_api_key,
        Arc::clone(&embedder),
        region.clone(),
    );
    let has_situation_budget = budget
        .has_budget(OperationCost::CLAUDE_HAIKU_STORY_WEAVE);
    let weaver_stats = match situation_weaver.run(run_id, has_situation_budget).await {
        Ok(sit_stats) => {
            info!("{sit_stats}");
            sit_stats
        }
        Err(e) => {
            warn!(error = %e, "Situation weaving failed (non-fatal)");
            Default::default()
        }
    };

    // ================================================================
    // 8b. Situation-driven source boost
    // ================================================================
    match writer.get_situation_landscape(20).await {
        Ok(situations) => {
            let hot: Vec<_> = situations
                .iter()
                .filter(|s| s.temperature >= 0.6 && s.sensitivity != "SENSITIVE" && s.sensitivity != "RESTRICTED")
                .collect();
            if !hot.is_empty() {
                info!(count = hot.len(), "Hot situations boosting source cadence");
                for sit in &hot {
                    if let Err(e) = writer
                        .boost_sources_for_situation_headline(&sit.headline, 1.2)
                        .await
                    {
                        warn!(error = %e, headline = sit.headline.as_str(), "Failed to boost sources for hot situation");
                    }
                }
            }

            let fuzzy: Vec<_> = situations
                .iter()
                .filter(|s| s.clarity == "Fuzzy" && s.temperature >= 0.3)
                .collect();
            if !fuzzy.is_empty() {
                info!(
                    count = fuzzy.len(),
                    "Fuzzy situations identified for investigation: {}",
                    fuzzy.iter().map(|s| s.headline.as_str()).collect::<Vec<_>>().join(", ")
                );
            }
        }
        Err(e) => warn!(error = %e, "Failed to fetch situation landscape for feedback"),
    }

    // ================================================================
    // 8c. Situation-triggered curiosity re-investigation
    // ================================================================
    match writer.trigger_situation_curiosity().await {
        Ok(0) => {}
        Ok(n) => info!(count = n, "Situations triggered curiosity re-investigation"),
        Err(e) => warn!(error = %e, "Failed to trigger situation curiosity"),
    }

    Ok(weaver_stats)
}

// ============================================================================
// Scout — thin orchestrator that calls the pipeline functions
// ============================================================================

pub struct Scout {
    graph_client: GraphClient,
    writer: GraphWriter,
    extractor: Arc<dyn SignalExtractor>,
    embedder: Arc<dyn TextEmbedder>,
    archive: Arc<dyn FetchBackend>,
    anthropic_api_key: String,
    region: ScoutScope,
    budget: BudgetTracker,
    cancelled: Arc<AtomicBool>,
}

impl Scout {
    pub fn new(
        graph_client: GraphClient,
        pool: PgPool,
        anthropic_api_key: &str,
        voyage_api_key: &str,
        serper_api_key: &str,
        apify_api_key: &str,
        region: ScoutScope,
        daily_budget_cents: u64,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self> {
        info!(region = region.name.as_str(), "Initializing scout");

        let region_slug = rootsignal_common::slugify(&region.name);
        let archive_config = ArchiveConfig {
            page_backend: match std::env::var("BROWSERLESS_URL") {
                Ok(url) => {
                    let token = std::env::var("BROWSERLESS_TOKEN").ok();
                    PageBackend::Browserless { base_url: url, token }
                }
                Err(_) => PageBackend::Chrome,
            },
            serper_api_key: serper_api_key.to_string(),
            apify_api_key: if apify_api_key.is_empty() {
                warn!("APIFY_API_KEY not set, social media scraping will return errors");
                None
            } else {
                Some(apify_api_key.to_string())
            },
            anthropic_api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
        };

        let archive = Arc::new(Archive::new(
            pool,
            archive_config,
            Uuid::new_v4(),
            region_slug,
        ));

        Ok(Self {
            graph_client: graph_client.clone(),
            writer: GraphWriter::new(graph_client),
            extractor: Arc::new(Extractor::new(
                anthropic_api_key,
                region.name.as_str(),
                region.center_lat,
                region.center_lng,
            )),
            embedder: Arc::new(crate::embedder::Embedder::new(voyage_api_key)),
            archive,
            anthropic_api_key: anthropic_api_key.to_string(),
            region,
            budget: BudgetTracker::new(daily_budget_cents),
            cancelled,
        })
    }

    /// Build a Scout with pre-built dependencies (for testing).
    pub fn new_for_test(
        graph_client: GraphClient,
        extractor: Arc<dyn SignalExtractor>,
        embedder: Arc<dyn TextEmbedder>,
        archive: Arc<dyn FetchBackend>,
        anthropic_api_key: &str,
        region: ScoutScope,
    ) -> Self {
        Self {
            graph_client: graph_client.clone(),
            writer: GraphWriter::new(graph_client),
            extractor,
            embedder,
            archive,
            anthropic_api_key: anthropic_api_key.to_string(),
            region,
            budget: BudgetTracker::new(0), // Unlimited for tests
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Check if the scout has been cancelled. Returns Err if so.
    fn check_cancelled(&self) -> Result<()> {
        check_cancelled_flag(&self.cancelled)
    }

    /// Run a full scout cycle.
    pub async fn run(&self) -> Result<ScoutStats> {
        // Acquire per-region lock (slugified to match reset/check operations)
        let region_slug = rootsignal_common::slugify(&self.region.name);
        if !self
            .writer
            .acquire_scout_lock(&region_slug)
            .await
            .context("Failed to check scout lock")?
        {
            anyhow::bail!("Another scout run is in progress for {}", self.region.name);
        }

        let result = self.run_inner().await;

        // Always release lock
        if let Err(e) = self.writer.release_scout_lock(&region_slug).await {
            error!("Failed to release scout lock: {e}");
        }

        result
    }

    async fn run_inner(&self) -> Result<ScoutStats> {
        // Ensure archive tables exist (idempotent)
        self.archive.migrate().await?;

        let run_id = Uuid::new_v4().to_string();
        info!(run_id = run_id.as_str(), "Scout run started");

        let pipeline = ScrapePipeline::new(
            self.writer.clone(),
            Arc::clone(&self.extractor),
            Arc::clone(&self.embedder),
            self.archive.clone(),
            self.anthropic_api_key.clone(),
            self.region.clone(),
            &self.budget,
            self.cancelled.clone(),
            run_id.clone(),
        );
        let stats = pipeline.run_all().await?;

        self.check_cancelled()?;

        run_synthesis(
            &self.graph_client,
            &self.writer,
            &*self.embedder,
            self.archive.clone(),
            &self.anthropic_api_key,
            &self.region,
            &self.budget,
            self.cancelled.clone(),
            &run_id,
        )
        .await?;

        self.check_cancelled()?;

        run_situation_weaving(
            &self.graph_client,
            &self.writer,
            Arc::clone(&self.embedder),
            &self.anthropic_api_key,
            &self.region,
            &self.budget,
            &run_id,
        )
        .await?;

        self.budget.log_status();
        Ok(stats)
    }
}
