//! ScrapePipeline — one dispatch, handler chain.
//!
//! `run_all()` dispatches `EngineStarted` and the seesaw handler chain
//! drives the entire scout run through phase lifecycle events.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::Result;
use sqlx::PgPool;
use tracing::warn;

use crate::domains::lifecycle::events::LifecycleEvent;
use crate::core::engine::ScoutEngine;
use crate::traits::SignalReader;

use rootsignal_common::ScoutScope;
use rootsignal_events::EventStore;
use rootsignal_graph::{GraphClient, GraphProjector, GraphWriter};

use rootsignal_archive::Archive;

use crate::store::event_sourced::EventSourcedReader;

use crate::infra::embedder::TextEmbedder;
use crate::infra::run_log::{EventKind, EventLogger, RunLogger};
use crate::core::extractor::SignalExtractor;
use crate::core::stats::ScoutStats;
use crate::scheduling::budget::BudgetTracker;

/// Bundles the shared dependencies for the scrape pipeline.
pub struct ScrapePipeline {
    budget: Arc<BudgetTracker>,
    run_id: String,
    region_name: String,
    pg_pool: PgPool,
    engine: Arc<ScoutEngine>,
}

impl ScrapePipeline {
    pub fn new(
        writer: GraphWriter,
        graph_client: GraphClient,
        event_store: EventStore,
        extractor: Arc<dyn SignalExtractor>,
        embedder: Arc<dyn TextEmbedder>,
        archive: Arc<Archive>,
        anthropic_api_key: String,
        region: ScoutScope,
        budget: Arc<BudgetTracker>,
        cancelled: Arc<AtomicBool>,
        run_id: String,
        pg_pool: PgPool,
    ) -> Self {
        let store = Arc::new(EventSourcedReader::new(writer));
        let engine_projector = GraphProjector::new(graph_client.clone());
        let engine = Arc::new(crate::core::engine::build_engine(
            crate::core::engine::ScoutEngineDeps {
                store: store as Arc<dyn SignalReader>,
                embedder,
                region: Some(region.clone()),
                fetcher: Some(archive as Arc<dyn crate::traits::ContentFetcher>),
                anthropic_api_key: Some(anthropic_api_key),
                graph_client: Some(graph_client),
                extractor: Some(extractor),
                state: Arc::new(tokio::sync::RwLock::new(
                    crate::core::aggregate::PipelineState::default(),
                )),
                graph_projector: Some(engine_projector),
                event_store: Some(event_store),
                run_id: run_id.clone(),
                captured_events: None,
                budget: Some(budget.clone()),
                cancelled: Some(cancelled),
                pg_pool: Some(pg_pool.clone()),
                archive: None,
            },
        ));
        Self {
            budget,
            run_id,
            region_name: region.name,
            pg_pool,
            engine,
        }
    }

    /// Run all phases via a single `EngineStarted` emit.
    ///
    /// The handler chain drives the entire run:
    /// EngineStarted → bootstrap + reap → schedule → tension_scrape →
    /// link_promotion + mid_run → response_scrape → link_promotion +
    /// actor_location + post_scrape → metrics → expansion →
    /// link_promotion + finalize → RunCompleted
    pub async fn run_all(self) -> Result<ScoutStats> {
        self.engine
            .emit(LifecycleEvent::EngineStarted {
                run_id: self.run_id.clone(),
            })
            .settled()
            .await?;

        // Save run stats
        let run_log = RunLogger::new(
            self.run_id.clone(),
            self.region_name.clone(),
            self.pg_pool.clone(),
        )
        .await;
        run_log.log(EventKind::BudgetCheckpoint {
            spent_cents: self.budget.total_spent(),
            remaining_cents: self.budget.remaining(),
        });
        let stats = self.engine.deps().state.read().await.stats.clone();
        if let Err(e) = run_log.save_stats(&self.pg_pool, &stats).await {
            warn!(error = %e, "Failed to save scout run log");
        }

        Ok(stats)
    }
}
