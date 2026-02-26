//! ScrapePipeline — decomposed scrape-pipeline phases.
//!
//! Bundles the shared dependencies for the scrape pipeline and exposes
//! each phase as an async method. Used by both the Restate ScrapeWorkflow
//! and the legacy CLI binary.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Result};
use chrono::Utc;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::pipeline::traits::SignalStore;

use rootsignal_common::{
    is_web_query, scraping_strategy, ScoutScope, DiscoveryMethod, ScrapingStrategy, SourceNode,
};
use rootsignal_events::EventStore;
use rootsignal_graph::{enrich, enrich_embeddings, GraphClient, GraphProjector, GraphWriter};

use rootsignal_archive::Archive;

use crate::pipeline::event_sourced_store::EventSourcedStore;

use crate::scheduling::budget::BudgetTracker;
use crate::infra::embedder::TextEmbedder;
use crate::pipeline::extractor::SignalExtractor;
use crate::pipeline::expansion::Expansion;
use crate::enrichment::link_promoter::{self, PromotionConfig};
use crate::scheduling::metrics::Metrics;
use crate::infra::run_log::{EventKind, EventLogger, RunLogger};
use crate::pipeline::scrape_phase::{RunContext, ScrapePhase};
use crate::pipeline::stats::ScoutStats;
use crate::discovery::source_finder::SourceFinderStats;
use crate::infra::util::sanitize_url;

pub(crate) fn check_cancelled_flag(cancelled: &AtomicBool) -> Result<()> {
    if cancelled.load(Ordering::Relaxed) {
        info!("Scout run cancelled by user");
        anyhow::bail!("Scout run cancelled");
    }
    Ok(())
}

/// Bundles the shared dependencies for the scrape pipeline.
/// Each phase method borrows `&self` to access them.
pub struct ScrapePipeline<'a> {
    writer: GraphWriter,
    graph_client: GraphClient,
    store: Arc<EventSourcedStore>,
    extractor: Arc<dyn SignalExtractor>,
    embedder: Arc<dyn TextEmbedder>,
    archive: Arc<Archive>,
    anthropic_api_key: String,
    region: ScoutScope,
    budget: &'a BudgetTracker,
    cancelled: Arc<AtomicBool>,
    run_id: String,
    pg_pool: PgPool,
}

/// Phase 2 outputs that flow into subsequent phases.
pub(crate) struct ScheduledRun {
    all_sources: Vec<SourceNode>,
    scheduled_sources: Vec<SourceNode>,
    tension_phase_keys: HashSet<String>,
    response_phase_keys: HashSet<String>,
    scheduled_keys: HashSet<String>,
    phase: ScrapePhase,
    consumed_pin_ids: Vec<uuid::Uuid>,
}

impl<'a> ScrapePipeline<'a> {
    pub fn new(
        writer: GraphWriter,
        graph_client: GraphClient,
        event_store: EventStore,
        extractor: Arc<dyn SignalExtractor>,
        embedder: Arc<dyn TextEmbedder>,
        archive: Arc<Archive>,
        anthropic_api_key: String,
        region: ScoutScope,
        budget: &'a BudgetTracker,
        cancelled: Arc<AtomicBool>,
        run_id: String,
        pg_pool: PgPool,
    ) -> Self {
        let projector = GraphProjector::new(graph_client.clone());
        let store = Arc::new(EventSourcedStore::new(
            writer.clone(),
            projector,
            event_store,
            run_id.clone(),
        ));
        Self {
            writer,
            graph_client,
            store,
            extractor,
            embedder,
            archive,
            anthropic_api_key,
            region,
            budget,
            cancelled,
            run_id,
            pg_pool,
        }
    }

    /// Remove stale signals from the graph.
    pub async fn reap_expired_signals(&self, run_log: &RunLogger) {
        info!("Reaping expired signals...");
        match self.store.reap_expired().await {
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
        run_log: &RunLogger,
    ) -> Result<(ScheduledRun, RunContext)> {
        let mut all_sources = match self.writer.get_sources_for_region(
            self.region.center_lat,
            self.region.center_lng,
            self.region.radius_km,
        ).await {
            Ok(sources) => {
                let curated = sources
                    .iter()
                    .filter(|s| s.discovery_method == DiscoveryMethod::Curated)
                    .count();
                let discovered = sources.len() - curated;
                info!(
                    total = sources.len(),
                    curated, discovered, "Loaded region-scoped sources from graph"
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
            let bootstrapper = crate::discovery::bootstrap::Bootstrapper::new(
                &self.writer,
                self.store.as_ref() as &dyn crate::pipeline::traits::SignalStore,
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
                .get_sources_for_region(
                    self.region.center_lat,
                    self.region.center_lng,
                    self.region.radius_km,
                )
                .await
                .unwrap_or_default();
        }

        // Actor discovery — if no actors in region, discover from web pages
        let (min_lat, max_lat, min_lng, max_lng) = self.region.bounding_box();
        let actors_in_region = self.writer
            .find_actors_in_region(min_lat, max_lat, min_lng, max_lng)
            .await
            .unwrap_or_default();

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

        // Pin consumption — add pin sources to the pool
        let existing_keys: HashSet<String> =
            all_sources.iter().map(|s| s.canonical_key.clone()).collect();
        let consumed_pin_ids = match self.writer
            .find_pins_in_region(min_lat, max_lat, min_lng, max_lng)
            .await
        {
            Ok(pins) => {
                let mut ids = Vec::new();
                for (pin, source) in pins {
                    if !existing_keys.contains(&source.canonical_key) {
                        all_sources.push(source);
                    }
                    ids.push(pin.id);
                }
                if !ids.is_empty() {
                    info!(pins = ids.len(), "Consumed pins from region");
                }
                ids
            }
            Err(e) => {
                warn!(error = %e, "Failed to load pins, continuing without");
                Vec::new()
            }
        };

        let now_schedule = Utc::now();
        let scheduler = crate::scheduling::scheduler::SourceScheduler::new();
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
        let wq_schedule = crate::scheduling::scheduler::schedule_web_queries(&all_sources, 0, now_schedule);
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
                discovery_depth: actor.discovery_depth,
            };
            for source in sources {
                ctx.actor_contexts
                    .insert(source.canonical_key.clone(), actor_ctx.clone());
            }
        }

        let phase = ScrapePhase::new(
            self.store.clone() as Arc<dyn crate::pipeline::traits::SignalStore>,
            self.extractor.clone(),
            self.embedder.clone(),
            self.archive.clone() as Arc<dyn crate::pipeline::traits::ContentFetcher>,
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
            consumed_pin_ids,
        };

        Ok((run, ctx))
    }

    /// Promote any links collected during scraping into new SourceNodes.
    /// Clears the collected_links buffer after processing.
    async fn promote_collected_links(&self, ctx: &mut RunContext) {
        if ctx.collected_links.is_empty() {
            return;
        }
        let config = PromotionConfig::default();
        match link_promoter::promote_links(
            &ctx.collected_links,
            self.store.as_ref() as &dyn crate::pipeline::traits::SignalStore,
            &config,
        )
        .await
        {
            Ok(n) if n > 0 => info!(promoted = n, "Promoted linked URLs"),
            Ok(_) => {}
            Err(e) => warn!(error = %e, "Link promotion failed"),
        }
        ctx.collected_links.clear();
    }

    /// Scrape tension + mixed sources (web pages, search queries, social accounts).
    /// This is the "find problems" pass.
    pub(crate) async fn scrape_tension_sources(
        &self,
        run: &ScheduledRun,
        ctx: &mut RunContext,
        run_log: &RunLogger,
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

        self.promote_collected_links(ctx).await;
    }

    /// Find new sources from graph analysis (actor-linked accounts, coverage gaps).
    /// Returns discovery stats and social topics discovered for later topic-based searching.
    pub(crate) async fn discover_mid_run_sources(&self) -> (SourceFinderStats, Vec<String>) {
        info!("=== Mid-Run Discovery ===");
        let discoverer = crate::discovery::source_finder::SourceFinder::new(
            &self.writer,
            self.store.as_ref() as &dyn crate::pipeline::traits::SignalStore,
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
        run_log: &RunLogger,
    ) -> Result<()> {
        info!("=== Phase B: Find Responses ===");

        // Reload sources to pick up fresh discovery sources from mid-run
        let fresh_sources = match self.writer.get_sources_for_region(
            self.region.center_lat,
            self.region.center_lng,
            self.region.radius_km,
        ).await {
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

        self.promote_collected_links(ctx).await;

        check_cancelled_flag(&self.cancelled)?;

        // Topic discovery — search social media to find new accounts
        // Merge expansion-derived social topics with LLM-generated topics
        let mut all_social_topics = social_topics;
        all_social_topics.extend(ctx.social_expansion_topics.drain(..));
        run.phase
            .discover_from_topics(&all_social_topics, ctx, run_log)
            .await;

        self.promote_collected_links(ctx).await;

        Ok(())
    }

    /// Record source metrics, update weights/cadence, deactivate dead sources.
    pub(crate) async fn update_source_metrics(&self, run: &ScheduledRun, ctx: &RunContext) {
        let metrics = Metrics::new(
            &self.writer,
            self.store.as_ref() as &dyn crate::pipeline::traits::SignalStore,
            &self.region.name,
        );
        metrics.update(&run.all_sources, &ctx.source_signal_counts, &ctx.query_api_errors, Utc::now()).await;

        // Log budget status before compute-heavy phases
        self.budget.log_status();
    }

    /// Create new sources from implied queries + end-of-run source discovery.
    pub(crate) async fn expand_and_discover(
        &self,
        run: &ScheduledRun,
        ctx: &mut RunContext,
        run_log: &RunLogger,
    ) -> Result<()> {
        // Signal Expansion — create sources from implied queries
        let expansion = Expansion::new(
            &self.writer,
            self.store.as_ref() as &dyn crate::pipeline::traits::SignalStore,
            &*self.embedder,
            &self.region.name,
        );
        expansion.run(ctx, run_log).await;

        check_cancelled_flag(&self.cancelled)?;

        // End-of-run discovery — find new sources for next run
        let end_discoverer = crate::discovery::source_finder::SourceFinder::new(
            &self.writer,
            self.store.as_ref() as &dyn crate::pipeline::traits::SignalStore,
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
            self.promote_collected_links(ctx).await;
        }

        Ok(())
    }

    /// Save run log and return final stats.
    pub(crate) async fn finalize(&self, ctx: RunContext, run_log: RunLogger) -> ScoutStats {
        run_log.log(EventKind::BudgetCheckpoint {
            spent_cents: self.budget.total_spent(),
            remaining_cents: self.budget.remaining(),
        });
        if let Err(e) = run_log.save_stats(&self.pg_pool, &ctx.stats).await {
            warn!(error = %e, "Failed to save scout run log");
        }

        info!("{}", ctx.stats);
        ctx.stats
    }

    /// Run all phases in sequence.
    pub async fn run_all(self) -> Result<ScoutStats> {
        let run_log = RunLogger::new(self.run_id.clone(), self.region.name.clone(), self.pg_pool.clone()).await;

        self.reap_expired_signals(&run_log).await;

        let (run, mut ctx) = self.load_and_schedule_sources(&run_log).await?;

        self.scrape_tension_sources(&run, &mut ctx, &run_log).await;
        check_cancelled_flag(&self.cancelled)?;

        let (_, social_topics) = self.discover_mid_run_sources().await;
        check_cancelled_flag(&self.cancelled)?;

        self.scrape_response_sources(&run, social_topics, &mut ctx, &run_log).await?;

        // Delete consumed pins now that their sources have been scraped
        if !run.consumed_pin_ids.is_empty() {
            match self.store.delete_pins(&run.consumed_pin_ids).await {
                Ok(_) => info!(count = run.consumed_pin_ids.len(), "Deleted consumed pins"),
                Err(e) => warn!(error = %e, "Failed to delete consumed pins"),
            }
        }

        // Enrich actor locations from signal mode before metrics/expansion
        run.phase.enrich_actors().await;

        // Bounding box used by actor extraction and metric enrichment
        let (min_lat, max_lat, min_lng, max_lng) = self.region.bounding_box();

        // Actor extraction — extract actors from signals that have none
        info!("=== Actor Extraction ===");
        let actor_stats = crate::enrichment::actor_extractor::run_actor_extraction(
            self.store.as_ref() as &dyn crate::pipeline::traits::SignalStore,
            &self.graph_client,
            &self.anthropic_api_key,
            &self.region.name,
            min_lat,
            max_lat,
            min_lng,
            max_lng,
        )
        .await;
        info!("{actor_stats}");

        // === Post-projection enrichment ===
        // 1. Embedding enrichment: backfill nodes missing embeddings
        info!("=== Embedding Enrichment ===");
        match enrich_embeddings(&self.graph_client, &*self.embedder, 50).await {
            Ok(stats) => info!("{stats}"),
            Err(e) => warn!(error = %e, "Embedding enrichment failed, continuing"),
        }

        // 2. Metric enrichment: diversity, actor stats, cause heat
        //    Entity mappings are empty — domain fallback in resolve_entity handles grouping.
        //    Threshold 0.3 matches Pipeline::new in integration tests.
        info!("=== Metric Enrichment ===");
        match enrich(
            &self.graph_client,
            &[],
            0.3,
            min_lat, max_lat, min_lng, max_lng,
        ).await {
            Ok(stats) => info!(?stats, "Metric enrichment complete"),
            Err(e) => warn!(error = %e, "Metric enrichment failed, continuing"),
        }

        self.update_source_metrics(&run, &ctx).await;
        check_cancelled_flag(&self.cancelled)?;

        self.expand_and_discover(&run, &mut ctx, &run_log).await?;

        Ok(self.finalize(ctx, run_log).await)
    }
}
