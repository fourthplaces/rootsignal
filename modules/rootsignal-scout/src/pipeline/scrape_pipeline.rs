//! ScrapePipeline — decomposed scrape-pipeline phases.
//!
//! Bundles the shared dependencies for the scrape pipeline and exposes
//! each phase as an async method. Used by both the Restate ScrapeWorkflow
//! and the legacy CLI binary.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::pipeline::events::ScoutEvent;
use crate::pipeline::state::{PipelineDeps, PipelineState};
use crate::pipeline::ScoutEngine;
use crate::traits::SignalReader;

use rootsignal_common::{
    is_web_query, scraping_strategy, DiscoveryMethod, ScoutScope, ScrapingStrategy, SourceNode,
};
use rootsignal_events::EventStore;
use rootsignal_graph::{enrich, enrich_embeddings, GraphClient, GraphProjector, GraphWriter};

use rootsignal_archive::Archive;

use crate::store::event_sourced::EventSourcedReader;

use crate::discovery::source_finder::SourceFinderStats;
use crate::enrichment::link_promoter::{self, PromotionConfig};
use crate::infra::embedder::TextEmbedder;
use crate::infra::run_log::{EventKind, EventLogger, RunLogger};
use crate::infra::util::sanitize_url;
use crate::pipeline::expansion::Expansion;
use crate::pipeline::extractor::SignalExtractor;
use crate::pipeline::scrape_phase::{RunContext, ScrapePhase};
use crate::pipeline::stats::ScoutStats;
use crate::scheduling::budget::BudgetTracker;
use crate::scheduling::metrics::Metrics;

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
    store: Arc<EventSourcedReader>,
    extractor: Arc<dyn SignalExtractor>,
    embedder: Arc<dyn TextEmbedder>,
    archive: Arc<Archive>,
    anthropic_api_key: String,
    region: ScoutScope,
    budget: &'a BudgetTracker,
    cancelled: Arc<AtomicBool>,
    run_id: String,
    pg_pool: PgPool,
    engine: Arc<ScoutEngine>,
}

/// Phase 2 outputs that flow into subsequent phases.
pub(crate) struct ScheduledRun {
    all_sources: Vec<SourceNode>,
    scheduled_sources: Vec<SourceNode>,
    tension_phase_keys: HashSet<String>,
    response_phase_keys: HashSet<String>,
    scheduled_keys: HashSet<String>,
    pub(crate) phase: ScrapePhase,
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
        let store = Arc::new(EventSourcedReader::new(writer.clone()));
        let engine_projector = GraphProjector::new(graph_client.clone());
        let engine = Arc::new(rootsignal_engine::Engine::new(
            crate::pipeline::reducer::ScoutReducer,
            crate::pipeline::router::ScoutRouter::new(Some(engine_projector)),
            Arc::new(event_store) as Arc<dyn rootsignal_engine::EventPersister>,
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
            engine,
        }
    }

    /// Find expired signals and dispatch EntityExpired events through the engine.
    pub async fn reap_expired_signals(&self, run_log: &RunLogger) {
        info!("Reaping expired signals...");
        let expired = match self.store.find_expired_signals().await {
            Ok(e) => e,
            Err(e) => {
                warn!(error = %e, "Failed to find expired signals, continuing");
                return;
            }
        };

        let mut gatherings = 0u64;
        let mut needs = 0u64;
        let mut stale = 0u64;

        let mut reap_state = PipelineState::new(Default::default());
        let reap_deps = self.reap_deps();

        for (signal_id, node_type, reason) in &expired {
            let event = ScoutEvent::System(rootsignal_common::events::SystemEvent::EntityExpired {
                signal_id: *signal_id,
                node_type: *node_type,
                reason: reason.clone(),
            });
            if let Err(e) = self
                .engine
                .dispatch(event, &mut reap_state, &reap_deps)
                .await
            {
                warn!(error = %e, signal_id = %signal_id, "Failed to expire signal");
                continue;
            }
            match node_type {
                rootsignal_common::types::NodeType::Gathering => gatherings += 1,
                rootsignal_common::types::NodeType::Need => needs += 1,
                _ => stale += 1,
            }
        }

        run_log.log(EventKind::ReapExpired {
            gatherings,
            needs,
            stale,
        });
        if gatherings + needs + stale > 0 {
            info!(gatherings, needs, stale, "Expired signals removed");
        }
    }

    /// Minimal PipelineDeps for engine dispatch during reaping (no fetcher/embedder needed).
    fn reap_deps(&self) -> PipelineDeps {
        PipelineDeps {
            store: self.store.clone() as Arc<dyn SignalReader>,
            embedder: Arc::new(crate::infra::embedder::NoOpEmbedder)
                as Arc<dyn crate::infra::embedder::TextEmbedder>,
            region: None,
            run_id: self.run_id.clone(),
            fetcher: None,
            anthropic_api_key: None,
        }
    }

    /// Load sources, run scheduler, build RunContext and ScrapePhase.
    /// Returns the ScheduledRun and RunContext needed by subsequent phases.
    pub(crate) async fn load_and_schedule_sources(
        &self,
        run_log: &RunLogger,
    ) -> Result<(ScheduledRun, RunContext)> {
        let mut all_sources = match self
            .writer
            .get_sources_for_region(
                self.region.center_lat,
                self.region.center_lng,
                self.region.radius_km,
            )
            .await
        {
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

        // Ensure sources exist — EngineStarted handler seeds if empty.
        {
            let pipe_deps = crate::pipeline::state::PipelineDeps {
                store: self.store.clone() as Arc<dyn SignalReader>,
                embedder: self.embedder.clone(),
                region: Some(self.region.clone()),
                run_id: self.run_id.clone(),
                fetcher: Some(self.archive.clone() as Arc<dyn crate::traits::ContentFetcher>),
                anthropic_api_key: Some(self.anthropic_api_key.clone()),
            };
            let mut boot_state =
                crate::pipeline::state::PipelineState::new(std::collections::HashMap::new());
            match self
                .engine
                .dispatch(
                    crate::pipeline::events::ScoutEvent::Pipeline(
                        crate::pipeline::events::PipelineEvent::EngineStarted {
                            run_id: self.run_id.clone(),
                        },
                    ),
                    &mut boot_state,
                    &pipe_deps,
                )
                .await
            {
                Ok(()) => {
                    let n = boot_state.stats.sources_discovered;
                    if n > 0 {
                        run_log.log(EventKind::Bootstrap {
                            sources_created: n as u64,
                        });
                        info!(sources = n, "EngineStarted seeded sources");
                    }
                }
                Err(e) => warn!(error = %e, "EngineStarted dispatch failed"),
            }
            // Reload sources if seeding occurred
            if all_sources.is_empty() {
                all_sources = self
                    .writer
                    .get_sources_for_region(
                        self.region.center_lat,
                        self.region.center_lng,
                        self.region.radius_km,
                    )
                    .await
                    .unwrap_or_default();
            }
        }

        // Actor discovery — if no actors in region, discover from web pages
        let (min_lat, max_lat, min_lng, max_lng) = self.region.bounding_box();
        let actors_in_region = self
            .writer
            .find_actors_in_region(min_lat, max_lat, min_lng, max_lng)
            .await
            .unwrap_or_default();

        // Actor sources — inject known actor accounts with elevated priority
        let actor_pairs = match self
            .writer
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
        let _existing_keys: HashSet<String> = all_sources
            .iter()
            .map(|s| s.canonical_key.clone())
            .collect();
        for (_actor, sources) in &actor_pairs {
            for source in sources {
                if let Some(existing) = all_sources
                    .iter_mut()
                    .find(|s| s.canonical_key == source.canonical_key)
                {
                    existing.weight = existing.weight.max(0.7);
                    existing.cadence_hours =
                        Some(existing.cadence_hours.map(|h| h.min(12)).unwrap_or(12));
                } else {
                    all_sources.push(source.clone());
                }
            }
        }

        // Pin consumption — add pin sources to the pool
        let existing_keys: HashSet<String> = all_sources
            .iter()
            .map(|s| s.canonical_key.clone())
            .collect();
        let consumed_pin_ids = match self
            .writer
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

        let tension_phase_keys: HashSet<String> = schedule.tension_phase.iter().cloned().collect();
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
        let wq_schedule =
            crate::scheduling::scheduler::schedule_web_queries(&all_sources, 0, now_schedule);
        let wq_scheduled_keys: HashSet<String> = wq_schedule.scheduled.into_iter().collect();

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
        let mut ctx = RunContext::from_sources(&all_sources);

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
            self.store.clone() as Arc<dyn crate::traits::SignalReader>,
            self.extractor.clone(),
            self.embedder.clone(),
            self.archive.clone() as Arc<dyn crate::traits::ContentFetcher>,
            self.region.clone(),
            self.run_id.clone(),
            self.engine.clone(),
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
    async fn promote_collected_links(&self, run: &ScheduledRun, ctx: &mut RunContext) {
        if ctx.collected_links.is_empty() {
            return;
        }
        let config = PromotionConfig::default();
        let sources = link_promoter::promote_links(&ctx.collected_links, &config);
        if !sources.is_empty() {
            let count = sources.len();
            if let Err(e) = run
                .phase
                .register_sources(sources, "link_promoter", ctx)
                .await
            {
                warn!(error = %e, "Failed to register promoted links");
            } else {
                info!(promoted = count, "Promoted linked URLs");
            }
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
        let phase_a_sources: Vec<&SourceNode> = run
            .scheduled_sources
            .iter()
            .filter(|s| run.tension_phase_keys.contains(&s.canonical_key))
            .collect();

        run.phase.run_web(&phase_a_sources, ctx, run_log).await;

        // Phase A social: tension + mixed social sources
        let phase_a_social: Vec<&SourceNode> = run
            .scheduled_sources
            .iter()
            .filter(|s| {
                matches!(scraping_strategy(s.value()), ScrapingStrategy::Social(_))
                    && run.tension_phase_keys.contains(&s.canonical_key)
            })
            .collect();
        if !phase_a_social.is_empty() {
            run.phase.run_social(&phase_a_social, ctx, run_log).await;
        }

        self.promote_collected_links(run, ctx).await;
    }

    /// Find new sources from graph analysis (actor-linked accounts, coverage gaps).
    /// Returns discovery stats, social topics, and discovered sources.
    pub(crate) async fn discover_mid_run_sources(
        &self,
    ) -> (SourceFinderStats, Vec<String>, Vec<SourceNode>) {
        info!("=== Mid-Run Discovery ===");
        let discoverer = crate::discovery::source_finder::SourceFinder::new(
            &self.writer,
            &self.region.name,
            &self.region.name,
            Some(&self.anthropic_api_key),
            self.budget,
        )
        .with_embedder(&*self.embedder);
        let (stats, social_topics, sources) = discoverer.run().await;
        if stats.actor_sources + stats.gap_sources > 0 {
            info!("{stats}");
        }
        (stats, social_topics, sources)
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
        let fresh_sources = match self
            .writer
            .get_sources_for_region(
                self.region.center_lat,
                self.region.center_lng,
                self.region.radius_km,
            )
            .await
        {
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
        let phase_b_social: Vec<&SourceNode> = run
            .scheduled_sources
            .iter()
            .filter(|s| {
                matches!(scraping_strategy(s.value()), ScrapingStrategy::Social(_))
                    && run.response_phase_keys.contains(&s.canonical_key)
            })
            .collect();
        if !phase_b_social.is_empty() {
            run.phase.run_social(&phase_b_social, ctx, run_log).await;
        }

        self.promote_collected_links(run, ctx).await;

        check_cancelled_flag(&self.cancelled)?;

        // Topic discovery — search social media to find new accounts
        // Merge expansion-derived social topics with LLM-generated topics
        let mut all_social_topics = social_topics;
        all_social_topics.extend(ctx.social_expansion_topics.drain(..));
        run.phase
            .discover_from_topics(&all_social_topics, ctx, run_log)
            .await;

        self.promote_collected_links(run, ctx).await;

        Ok(())
    }

    /// Record source metrics, update weights/cadence, deactivate dead sources.
    pub(crate) async fn update_source_metrics(&self, run: &ScheduledRun, ctx: &RunContext) {
        let pipe_deps = crate::pipeline::state::PipelineDeps {
            store: self.store.clone() as std::sync::Arc<dyn crate::traits::SignalReader>,
            embedder: self.embedder.clone(),
            region: Some(self.region.clone()),
            run_id: self.run_id.clone(),
            fetcher: None,
            anthropic_api_key: None,
        };
        let metrics = Metrics::new(&self.writer, &self.engine, &pipe_deps, &self.region.name);
        metrics
            .update(
                &run.all_sources,
                &ctx.source_signal_counts,
                &ctx.query_api_errors,
                Utc::now(),
            )
            .await;

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
        let expansion = Expansion::new(&self.writer, &*self.embedder, &self.region.name);
        let expansion_sources = expansion.run(ctx, run_log).await;
        if !expansion_sources.is_empty() {
            run.phase
                .register_sources(expansion_sources, "signal_expansion", ctx)
                .await?;
        }

        check_cancelled_flag(&self.cancelled)?;

        // End-of-run discovery — find new sources for next run
        let end_discoverer = crate::discovery::source_finder::SourceFinder::new(
            &self.writer,
            &self.region.name,
            &self.region.name,
            Some(&self.anthropic_api_key),
            self.budget,
        )
        .with_embedder(&*self.embedder);
        let (end_discovery_stats, end_social_topics, end_discovery_sources) =
            end_discoverer.run().await;
        if !end_discovery_sources.is_empty() {
            run.phase
                .register_sources(end_discovery_sources, "source_finder", ctx)
                .await?;
        }
        if end_discovery_stats.actor_sources + end_discovery_stats.gap_sources > 0 {
            info!("{end_discovery_stats}");
        }
        if !end_social_topics.is_empty() {
            info!(
                count = end_social_topics.len(),
                "Consuming end-of-run social topics"
            );
            run.phase
                .discover_from_topics(&end_social_topics, ctx, run_log)
                .await;
            self.promote_collected_links(run, ctx).await;
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
        let run_log = RunLogger::new(
            self.run_id.clone(),
            self.region.name.clone(),
            self.pg_pool.clone(),
        )
        .await;

        self.reap_expired_signals(&run_log).await;

        let (run, mut ctx) = self.load_and_schedule_sources(&run_log).await?;

        self.scrape_tension_sources(&run, &mut ctx, &run_log).await;
        check_cancelled_flag(&self.cancelled)?;

        let (_, social_topics, mid_run_sources) = self.discover_mid_run_sources().await;
        if !mid_run_sources.is_empty() {
            run.phase
                .register_sources(mid_run_sources, "source_finder", &mut ctx)
                .await?;
        }
        check_cancelled_flag(&self.cancelled)?;

        self.scrape_response_sources(&run, social_topics, &mut ctx, &run_log)
            .await?;

        // Delete consumed pins now that their sources have been scraped
        if !run.consumed_pin_ids.is_empty() {
            match self.writer.delete_pins(&run.consumed_pin_ids).await {
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
        let actor_deps = crate::pipeline::state::PipelineDeps {
            store: self.store.clone() as std::sync::Arc<dyn crate::traits::SignalReader>,
            embedder: self.embedder.clone(),
            region: Some(self.region.clone()),
            run_id: self.run_id.clone(),
            fetcher: None,
            anthropic_api_key: Some(self.anthropic_api_key.clone()),
        };
        let actor_stats = crate::enrichment::actor_extractor::run_actor_extraction(
            self.store.as_ref() as &dyn crate::traits::SignalReader,
            &self.graph_client,
            &self.anthropic_api_key,
            &*self.engine,
            &actor_deps,
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
            min_lat,
            max_lat,
            min_lng,
            max_lng,
        )
        .await
        {
            Ok(stats) => info!(?stats, "Metric enrichment complete"),
            Err(e) => warn!(error = %e, "Metric enrichment failed, continuing"),
        }

        self.update_source_metrics(&run, &ctx).await;
        check_cancelled_flag(&self.cancelled)?;

        self.expand_and_discover(&run, &mut ctx, &run_log).await?;

        Ok(self.finalize(ctx, run_log).await)
    }
}
