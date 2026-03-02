//! Seesaw engine setup for scout.
//!
//! Three engine variants share the same deps and infrastructure handlers:
//!
//! - **Scrape engine** (`build_engine`): reap → schedule → scrape → enrichment →
//!   expansion → synthesis → finalize. Used by standalone scrape/bootstrap workflows.
//!
//! - **Full engine** (`build_full_engine`): extends the scrape chain with
//!   situation_weaving → supervisor before finalize. Used by full_run and
//!   standalone synthesis/situation_weaver/supervisor workflows.
//!
//! - **News engine** (`build_news_engine`): NewsScanRequested → scan RSS → BeaconDetected.
//!   Used by the news scanner workflow.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use rootsignal_common::{EmbeddingLookup, ScoutScope};
use rootsignal_graph::{EmbeddingStore, GraphClient, GraphProjector};
use sqlx::PgPool;

use crate::core::aggregate::pipeline_aggregators;
use crate::core::embedding_cache::EmbeddingCache;
use crate::core::projection;
use crate::core::seesaw_event_store::SeesawEventStoreAdapter;
use crate::domains::{
    discovery, enrichment, expansion, lifecycle, news_scanning, scrape, signals,
    situation_weaving, supervisor, synthesis,
};
use crate::infra::embedder::TextEmbedder;
use crate::core::extractor::SignalExtractor;
use crate::traits::{ContentFetcher, SignalReader};

/// Dependencies shared by all seesaw handlers.
pub struct ScoutEngineDeps {
    // --- Fields from PipelineDeps (previously behind Arc<RwLock<Option>>) ---
    pub store: Arc<dyn SignalReader>,
    pub embedder: Arc<dyn TextEmbedder>,
    pub region: Option<ScoutScope>,
    pub fetcher: Option<Arc<dyn ContentFetcher>>,
    pub anthropic_api_key: Option<String>,
    pub graph_client: Option<GraphClient>,
    pub extractor: Option<Arc<dyn SignalExtractor>>,
    /// In-memory embedding cache for cross-batch dedup (layer 1 of 4).
    pub embed_cache: EmbeddingCache,
    // --- Engine infrastructure ---
    /// Current run ID for event tagging.
    pub run_id: String,
    /// Test-only: capture all dispatched events for inspection.
    /// None in production, Some in tests that need event inspection.
    pub captured_events: Option<Arc<std::sync::Mutex<Vec<seesaw_core::AnyEvent>>>>,
    /// Budget tracker for LLM/API cost tracking.
    pub budget: Option<Arc<crate::domains::scheduling::activities::budget::BudgetTracker>>,
    /// Cancellation flag — checked by handlers between phases.
    pub cancelled: Option<Arc<AtomicBool>>,
    /// Postgres connection pool — used by finalize handler to save run stats.
    pub pg_pool: Option<PgPool>,
    /// Archive for web search/page reading in synthesis finders.
    pub archive: Option<Arc<rootsignal_archive::Archive>>,
}

impl ScoutEngineDeps {
    /// Create deps with required fields; all optional fields default to None.
    pub fn new(
        store: Arc<dyn SignalReader>,
        embedder: Arc<dyn TextEmbedder>,
        run_id: impl Into<String>,
    ) -> Self {
        Self {
            store,
            embedder,
            region: None,
            fetcher: None,
            anthropic_api_key: None,
            graph_client: None,
            extractor: None,
            embed_cache: EmbeddingCache::new(),
            run_id: run_id.into(),
            captured_events: None,
            budget: None,
            cancelled: None,
            pg_pool: None,
            archive: None,
        }
    }
}

/// The seesaw-backed scout engine type.
pub type SeesawEngine = seesaw_core::Engine<ScoutEngineDeps>;

/// Public alias — canonical name for the scout engine.
pub type ScoutEngine = SeesawEngine;

/// Build a scrape-chain engine: reap → schedule → scrape → enrichment →
/// expansion → synthesis → finalize.
///
/// Finalize triggers on PhaseCompleted(Synthesis). Does NOT include
/// situation_weaving or supervisor handlers.
pub fn build_engine(deps: ScoutEngineDeps) -> SeesawEngine {
    let capture_sink = deps.captured_events.clone();
    let snapshot_store = deps.pg_pool.as_ref().map(|pool| {
        Arc::new(rootsignal_events::PostgresSnapshotStore::new(pool.clone()))
            as Arc<dyn seesaw_core::SnapshotStore>
    });

    // Construct infrastructure from deps — not stored on ScoutEngineDeps
    let event_store_adapter = deps.pg_pool.as_ref().map(|pool| {
        Arc::new(SeesawEventStoreAdapter::new(
            rootsignal_events::EventStore::new(pool.clone()),
        )) as Arc<dyn seesaw_core::event_store::EventStore>
    });
    let embedding_store: Option<Arc<dyn EmbeddingLookup>> =
        deps.pg_pool.as_ref().map(|pool| {
            Arc::new(EmbeddingStore::new(
                pool.clone(),
                deps.embedder.clone(),
                "voyage-3-large".to_string(),
            )) as Arc<dyn EmbeddingLookup>
        });
    let graph_projector = deps.graph_client.as_ref().map(|gc| {
        let mut projector = GraphProjector::new(gc.clone());
        if let Some(store) = embedding_store.clone() {
            projector = projector.with_embedding_store(store);
        }
        projector
    });
    let run_id = deps.run_id.clone();

    let mut engine = seesaw_core::Engine::new(deps)
        // Aggregators — PipelineState maintained by seesaw
        .with_aggregators(pipeline_aggregators::aggregators())
        // Domain handlers
        .with_handlers(signals::handlers::handlers())
        .with_handlers(lifecycle::handlers::handlers())
        .with_handlers(scrape::handlers::handlers())
        .with_handlers(discovery::handlers::handlers())
        .with_handlers(enrichment::handlers::handlers())
        .with_handlers(expansion::handlers::handlers())
        .with_handlers(synthesis::handlers::handlers())
        // Scrape chain finalize — triggers on PhaseCompleted(Synthesis)
        .with_handler(lifecycle::__seesaw_effect_scrape_finalize());

    // Wire seesaw's built-in event persistence
    if let Some(store) = event_store_adapter {
        engine = engine
            .with_event_store(store)
            .with_event_metadata(serde_json::json!({
                "run_id": run_id,
                "schema_v": 1
            }));
    }

    // Neo4j projection — captured via closure, not on deps
    if let Some(projector) = graph_projector {
        engine = engine.with_handler(projection::neo4j_projection_handler(projector));
    }

    if let Some(store) = snapshot_store {
        engine = engine.with_snapshot_store(store).snapshot_every(100);
    }

    // Test-only: register capture handler when sink is provided
    if let Some(sink) = capture_sink {
        engine = engine.with_handler(projection::capture_handler(sink));
    }

    engine
}

/// Build a full-chain engine: extends the scrape chain with situation_weaving →
/// supervisor → finalize.
///
/// Finalize triggers on PhaseCompleted(Supervisor).
pub fn build_full_engine(deps: ScoutEngineDeps) -> SeesawEngine {
    let capture_sink = deps.captured_events.clone();
    let snapshot_store = deps.pg_pool.as_ref().map(|pool| {
        Arc::new(rootsignal_events::PostgresSnapshotStore::new(pool.clone()))
            as Arc<dyn seesaw_core::SnapshotStore>
    });

    // Construct infrastructure from deps — not stored on ScoutEngineDeps
    let event_store_adapter = deps.pg_pool.as_ref().map(|pool| {
        Arc::new(SeesawEventStoreAdapter::new(
            rootsignal_events::EventStore::new(pool.clone()),
        )) as Arc<dyn seesaw_core::event_store::EventStore>
    });
    let embedding_store: Option<Arc<dyn EmbeddingLookup>> =
        deps.pg_pool.as_ref().map(|pool| {
            Arc::new(EmbeddingStore::new(
                pool.clone(),
                deps.embedder.clone(),
                "voyage-3-large".to_string(),
            )) as Arc<dyn EmbeddingLookup>
        });
    let graph_projector = deps.graph_client.as_ref().map(|gc| {
        let mut projector = GraphProjector::new(gc.clone());
        if let Some(store) = embedding_store.clone() {
            projector = projector.with_embedding_store(store);
        }
        projector
    });
    let run_id = deps.run_id.clone();

    let mut engine = seesaw_core::Engine::new(deps)
        // Aggregators — PipelineState maintained by seesaw
        .with_aggregators(pipeline_aggregators::aggregators())
        // Domain handlers — scrape chain
        .with_handlers(signals::handlers::handlers())
        .with_handlers(lifecycle::handlers::handlers())
        .with_handlers(scrape::handlers::handlers())
        .with_handlers(discovery::handlers::handlers())
        .with_handlers(enrichment::handlers::handlers())
        .with_handlers(expansion::handlers::handlers())
        .with_handlers(synthesis::handlers::handlers())
        // Full chain — situation weaving + supervisor
        .with_handlers(situation_weaving::handlers::handlers())
        .with_handlers(supervisor::handlers::handlers())
        // Full chain finalize — triggers on PhaseCompleted(Supervisor)
        .with_handler(lifecycle::__seesaw_effect_full_finalize());

    // Wire seesaw's built-in event persistence
    if let Some(store) = event_store_adapter {
        engine = engine
            .with_event_store(store)
            .with_event_metadata(serde_json::json!({
                "run_id": run_id,
                "schema_v": 1
            }));
    }

    // Neo4j projection — captured via closure, not on deps
    if let Some(projector) = graph_projector {
        engine = engine.with_handler(projection::neo4j_projection_handler(projector));
    }

    if let Some(store) = snapshot_store {
        engine = engine.with_snapshot_store(store).snapshot_every(100);
    }

    // Test-only: register capture handler when sink is provided
    if let Some(sink) = capture_sink {
        engine = engine.with_handler(projection::capture_handler(sink));
    }

    engine
}

/// Build a news-scan engine: NewsScanRequested → scan RSS → BeaconDetected.
///
/// Minimal handler set — only news scanning domain + infrastructure.
pub fn build_news_engine(deps: ScoutEngineDeps) -> SeesawEngine {
    let capture_sink = deps.captured_events.clone();
    let snapshot_store = deps.pg_pool.as_ref().map(|pool| {
        Arc::new(rootsignal_events::PostgresSnapshotStore::new(pool.clone()))
            as Arc<dyn seesaw_core::SnapshotStore>
    });
    let event_store_adapter = deps.pg_pool.as_ref().map(|pool| {
        Arc::new(SeesawEventStoreAdapter::new(
            rootsignal_events::EventStore::new(pool.clone()),
        )) as Arc<dyn seesaw_core::event_store::EventStore>
    });
    let embedding_store: Option<Arc<dyn EmbeddingLookup>> =
        deps.pg_pool.as_ref().map(|pool| {
            Arc::new(EmbeddingStore::new(
                pool.clone(),
                deps.embedder.clone(),
                "voyage-3-large".to_string(),
            )) as Arc<dyn EmbeddingLookup>
        });
    let graph_projector = deps.graph_client.as_ref().map(|gc| {
        let mut projector = GraphProjector::new(gc.clone());
        if let Some(store) = embedding_store.clone() {
            projector = projector.with_embedding_store(store);
        }
        projector
    });
    let run_id = deps.run_id.clone();

    let mut engine = seesaw_core::Engine::new(deps)
        .with_aggregators(news_scanning::aggregate::news_aggregators::aggregators())
        .with_handlers(news_scanning::handlers::handlers());

    if let Some(store) = event_store_adapter {
        engine = engine
            .with_event_store(store)
            .with_event_metadata(serde_json::json!({
                "run_id": run_id,
                "schema_v": 1
            }));
    }

    if let Some(projector) = graph_projector {
        engine = engine.with_handler(projection::neo4j_projection_handler(projector));
    }

    if let Some(store) = snapshot_store {
        engine = engine.with_snapshot_store(store).snapshot_every(100);
    }

    if let Some(sink) = capture_sink {
        engine = engine.with_handler(projection::capture_handler(sink));
    }

    engine
}
