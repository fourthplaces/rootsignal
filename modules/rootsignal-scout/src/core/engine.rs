//! Seesaw engine setup for scout.
//!
//! Three engine variants share the same deps and infrastructure handlers:
//!
//! - **Scrape engine** (`build_engine`): reap → schedule → scrape → enrichment →
//!   expansion → synthesis. Used by standalone scrape/bootstrap workflows.
//!
//! - **Full engine** (`build_full_engine`): extends the scrape chain with
//!   situation_weaving → supervisor. Used by full_run and
//!   standalone synthesis/situation_weaver/supervisor workflows.
//!
//! - **News engine** (`build_news_engine`): NewsScanRequested → scan RSS → extract signals.
//!   Used by the news scanner workflow.

use std::sync::Arc;

use uuid::Uuid;
use ai_client::Agent;
use seesaw_utils::Batcher;
use rootsignal_common::EmbeddingLookup;
use rootsignal_graph::{EmbeddingStore, GraphClient, GraphProjector, GraphReader};

use sqlx::PgPool;

use crate::core::aggregate::pipeline_aggregators;
use crate::core::embedding_cache::EmbeddingCache;
use crate::core::pipeline_events::PipelineEvent;
use crate::core::postgres_store::PostgresStore;
use crate::core::projection;
use crate::domains::{
    curiosity, discovery, enrichment, expansion, lifecycle, news_scanning, scrape, signals,
    situation_weaving, supervisor, synthesis,
};
use crate::infra::embedder::TextEmbedder;
use crate::infra::util::EMBEDDING_MODEL;
use crate::core::extractor::SignalExtractor;
use crate::traits::{ContentFetcher, SignalReader};

/// Dependencies shared by all seesaw handlers.
pub struct ScoutEngineDeps {
    // --- Fields from PipelineDeps (previously behind Arc<RwLock<Option>>) ---
    pub store: Arc<dyn SignalReader>,
    pub embedder: Arc<dyn TextEmbedder>,
    pub fetcher: Option<Arc<dyn ContentFetcher>>,
    pub ai: Option<Arc<dyn Agent>>,
    /// Raw API key — only used by out-of-scope callers (supervisor, news_scanner)
    /// that haven't been migrated to `dyn Agent` yet.
    pub anthropic_api_key: Option<String>,
    pub graph: Option<GraphReader>,
    pub extractor: Option<Arc<dyn SignalExtractor>>,
    /// In-memory embedding cache for cross-batch dedup (layer 1 of 4).
    pub embed_cache: EmbeddingCache,
    // --- Engine infrastructure ---
    /// Current run ID for event tagging.
    pub run_id: Uuid,
    /// Test-only: capture all dispatched events for inspection.
    /// None in production, Some in tests that need event inspection.
    pub captured_events: Option<Arc<std::sync::Mutex<Vec<seesaw_core::AnyEvent>>>>,
    /// Budget tracker for LLM/API cost tracking.
    pub budget: Option<Arc<crate::domains::scheduling::activities::budget::BudgetTracker>>,
    /// Postgres connection pool — used by projections to save run stats.
    pub pg_pool: Option<PgPool>,
    /// Archive for web search/page reading in synthesis finders.
    pub archive: Option<Arc<rootsignal_archive::Archive>>,
    /// Batcher for grouping items across handler invocations (e.g. signal review).
    pub batcher: Batcher,
}

impl ScoutEngineDeps {
    /// Create deps with required fields; all optional fields default to None.
    pub fn new(
        store: Arc<dyn SignalReader>,
        embedder: Arc<dyn TextEmbedder>,
        run_id: Uuid,
    ) -> Self {
        Self {
            store,
            embedder,
            fetcher: None,
            ai: None,
            anthropic_api_key: None,
            graph: None,
            extractor: None,
            embed_cache: EmbeddingCache::new(),
            run_id,
            captured_events: None,
            budget: None,
            pg_pool: None,
            archive: None,
            batcher: Batcher::new(),
        }
    }
}

/// The seesaw-backed scout engine type.
pub type SeesawEngine = seesaw_core::Engine<ScoutEngineDeps>;

/// Public alias — canonical name for the scout engine.
pub type ScoutEngine = SeesawEngine;

/// Build a scrape-chain engine: reap → schedule → scrape → enrichment →
/// expansion → synthesis.
///
/// Terminal event: SeverityInferred (after similarity + response mapping).
/// Does NOT include situation_weaving or supervisor handlers.
///
/// When `seesaw_store` is provided, it replaces the default in-memory store
/// for durable crash recovery. Pass `None` for tests.
pub fn build_engine(deps: ScoutEngineDeps, seesaw_store: Option<Arc<dyn seesaw_core::Store>>) -> SeesawEngine {
    let capture_sink = deps.captured_events.clone();
    let embedding_store: Option<Arc<dyn EmbeddingLookup>> =
        deps.pg_pool.as_ref().map(|pool| {
            Arc::new(EmbeddingStore::new(
                pool.clone(),
                deps.embedder.clone(),
                EMBEDDING_MODEL.to_string(),
            )) as Arc<dyn EmbeddingLookup>
        });
    let graph_projector = deps.graph.as_ref().map(|gr| {
        let mut projector = GraphProjector::new(gr.client().clone());
        if let Some(store) = embedding_store.clone() {
            projector = projector.with_embedding_store(store);
        }
        projector
    });
    let run_id = deps.run_id;

    let mut engine = seesaw_core::Engine::new(deps)
        // Aggregators — PipelineState maintained by seesaw
        .with_aggregators(pipeline_aggregators::aggregators())
        .with_aggregators(curiosity::aggregates::curiosity_aggregators::aggregators())
        .with_handlers(signals::handlers::handlers())
        .with_handlers(lifecycle::handlers::handlers())
        .with_handlers(scrape::handlers::handlers())
        .with_handlers(discovery::handlers::handlers())
        .with_handlers(enrichment::handlers::handlers())
        .with_handlers(expansion::handlers::handlers())
        .with_handlers(synthesis::handlers::handlers())
        .with_handlers(curiosity::handlers::handlers())
        // Surface DLQ'd handlers as events in the causal chain
        .on_dlq(|info: seesaw_core::DlqTerminalInfo| PipelineEvent::HandlerFailed {
            handler_id: info.handler_id.clone(),
            source_event_type: info.source_event_type.clone(),
            error: info.error.clone(),
            attempts: info.attempts,
        });

    if let Some(s) = seesaw_store {
        engine = engine
            .with_store(s)
            .with_event_metadata(serde_json::json!({
                "run_id": run_id,
                "schema_v": 1
            }))
            .snapshot_every(100);
    }

    // Neo4j projection — captured via closure, not on deps
    if let Some(projector) = graph_projector {
        engine = engine.with_handler(projection::neo4j_projection_handler(projector));
    }

    // Infrastructure projections
    engine = engine.with_projection(projection::scout_runs_projection());
    engine = engine.with_projection(projection::system_log_projection());

    // Test-only: register capture handler when sink is provided
    if let Some(sink) = capture_sink {
        engine = engine.with_handler(projection::capture_handler(sink));
    }

    engine
}

/// Build a full-chain engine: extends the scrape chain with situation_weaving →
/// supervisor.
///
/// Terminal events: SupervisionCompleted or NothingToSupervise.
pub fn build_full_engine(deps: ScoutEngineDeps, seesaw_store: Option<Arc<dyn seesaw_core::Store>>) -> SeesawEngine {
    let capture_sink = deps.captured_events.clone();
    let embedding_store: Option<Arc<dyn EmbeddingLookup>> =
        deps.pg_pool.as_ref().map(|pool| {
            Arc::new(EmbeddingStore::new(
                pool.clone(),
                deps.embedder.clone(),
                EMBEDDING_MODEL.to_string(),
            )) as Arc<dyn EmbeddingLookup>
        });
    let graph_projector = deps.graph.as_ref().map(|gr| {
        let mut projector = GraphProjector::new(gr.client().clone());
        if let Some(store) = embedding_store.clone() {
            projector = projector.with_embedding_store(store);
        }
        projector
    });
    let run_id = deps.run_id;

    let mut engine = seesaw_core::Engine::new(deps)
        // Aggregators — PipelineState maintained by seesaw
        .with_aggregators(pipeline_aggregators::aggregators())
        .with_aggregators(curiosity::aggregates::curiosity_aggregators::aggregators())
        .with_handlers(signals::handlers::handlers())
        .with_handlers(lifecycle::handlers::handlers())
        .with_handlers(scrape::handlers::handlers())
        .with_handlers(discovery::handlers::handlers())
        .with_handlers(enrichment::handlers::handlers())
        .with_handlers(expansion::handlers::handlers())
        .with_handlers(synthesis::handlers::handlers())
        .with_handlers(curiosity::handlers::handlers())
        .with_handlers(situation_weaving::handlers::handlers())
        .with_handlers(supervisor::handlers::handlers())
        // Surface DLQ'd handlers as events in the causal chain
        .on_dlq(|info: seesaw_core::DlqTerminalInfo| PipelineEvent::HandlerFailed {
            handler_id: info.handler_id.clone(),
            source_event_type: info.source_event_type.clone(),
            error: info.error.clone(),
            attempts: info.attempts,
        });

    if let Some(s) = seesaw_store {
        engine = engine
            .with_store(s)
            .with_event_metadata(serde_json::json!({
                "run_id": run_id,
                "schema_v": 1
            }))
            .snapshot_every(100);
    }

    // Neo4j projection — captured via closure, not on deps
    if let Some(projector) = graph_projector {
        engine = engine.with_handler(projection::neo4j_projection_handler(projector));
    }

    // Infrastructure projections
    engine = engine.with_projection(projection::scout_runs_projection());
    engine = engine.with_projection(projection::system_log_projection());

    // Test-only: register capture handler when sink is provided
    if let Some(sink) = capture_sink {
        engine = engine.with_handler(projection::capture_handler(sink));
    }

    engine
}

/// Build a weave-only engine: cross-signal synthesis at any region level.
///
/// Includes: lifecycle, signals, synthesis, situation_weaving, supervisor.
/// Excludes: scrape, discovery, enrichment, expansion (those are scrape-time only).
///
/// Terminal events: SupervisionCompleted or NothingToSupervise.
///
/// NOTE: Weave engine is non-functional until a proper weave kickoff event
/// is designed (separate PR). The old `start_weave` trampoline that emitted
/// fake ExpansionCompleted has been deleted.
pub fn build_weave_engine(deps: ScoutEngineDeps, seesaw_store: Option<Arc<dyn seesaw_core::Store>>) -> SeesawEngine {
    let capture_sink = deps.captured_events.clone();
    let embedding_store: Option<Arc<dyn EmbeddingLookup>> =
        deps.pg_pool.as_ref().map(|pool| {
            Arc::new(EmbeddingStore::new(
                pool.clone(),
                deps.embedder.clone(),
                EMBEDDING_MODEL.to_string(),
            )) as Arc<dyn EmbeddingLookup>
        });
    let graph_projector = deps.graph.as_ref().map(|gr| {
        let mut projector = GraphProjector::new(gr.client().clone());
        if let Some(store) = embedding_store.clone() {
            projector = projector.with_embedding_store(store);
        }
        projector
    });
    let run_id = deps.run_id;

    let mut engine = seesaw_core::Engine::new(deps)
        .with_aggregators(pipeline_aggregators::aggregators())
        .with_handlers(signals::handlers::handlers())
        .with_handlers(lifecycle::handlers::handlers())
        .with_handlers(synthesis::handlers::handlers())
        .with_handlers(situation_weaving::handlers::handlers())
        .with_handlers(supervisor::handlers::handlers())
        .on_dlq(|info: seesaw_core::DlqTerminalInfo| PipelineEvent::HandlerFailed {
            handler_id: info.handler_id.clone(),
            source_event_type: info.source_event_type.clone(),
            error: info.error.clone(),
            attempts: info.attempts,
        });

    if let Some(s) = seesaw_store {
        engine = engine
            .with_store(s)
            .with_event_metadata(serde_json::json!({
                "run_id": run_id,
                "schema_v": 1
            }))
            .snapshot_every(100);
    }

    if let Some(projector) = graph_projector {
        engine = engine.with_handler(projection::neo4j_projection_handler(projector));
    }

    engine = engine.with_projection(projection::scout_runs_projection());
    engine = engine.with_projection(projection::system_log_projection());

    if let Some(sink) = capture_sink {
        engine = engine.with_handler(projection::capture_handler(sink));
    }

    engine
}

/// Build an infrastructure-only engine: event persistence + Neo4j projector.
///
/// No domain handlers, no aggregators, no production deps — used for emitting
/// system events from error paths where the main engine is dead.
/// Takes only the two infrastructure handles it actually needs.
pub fn build_infra_only_engine(
    pg_pool: PgPool,
    graph_client: GraphClient,
    run_id: Option<Uuid>,
) -> SeesawEngine {
    let run_id = run_id.unwrap_or_else(Uuid::new_v4);

    let store = Arc::new(PostgresStore::new(pg_pool.clone(), run_id))
        as Arc<dyn seesaw_core::Store>;

    let projector = GraphProjector::new(graph_client.clone());

    let deps = ScoutEngineDeps::new(
        Arc::new(crate::traits::NoOpSignalReader),
        Arc::new(crate::infra::embedder::NoOpEmbedder),
        run_id,
    );

    seesaw_core::Engine::new(deps)
        .with_store(store)
        .with_event_metadata(serde_json::json!({
            "run_id": run_id,
            "schema_v": 1
        }))
        .with_handler(projection::neo4j_projection_handler(projector))
        .with_projection(projection::scout_runs_projection())
        .with_projection(projection::system_log_projection())
}

/// Build a news-scan engine: NewsScanRequested → scan RSS → extract signals.
///
/// Minimal handler set — only news scanning domain + infrastructure.
pub fn build_news_engine(deps: ScoutEngineDeps, seesaw_store: Option<Arc<dyn seesaw_core::Store>>) -> SeesawEngine {
    let capture_sink = deps.captured_events.clone();
    let embedding_store: Option<Arc<dyn EmbeddingLookup>> =
        deps.pg_pool.as_ref().map(|pool| {
            Arc::new(EmbeddingStore::new(
                pool.clone(),
                deps.embedder.clone(),
                EMBEDDING_MODEL.to_string(),
            )) as Arc<dyn EmbeddingLookup>
        });
    let graph_projector = deps.graph.as_ref().map(|gr| {
        let mut projector = GraphProjector::new(gr.client().clone());
        if let Some(store) = embedding_store.clone() {
            projector = projector.with_embedding_store(store);
        }
        projector
    });
    let run_id = deps.run_id;

    let mut engine = seesaw_core::Engine::new(deps)
        .with_handlers(news_scanning::handlers::handlers());

    if let Some(s) = seesaw_store {
        engine = engine
            .with_store(s)
            .with_event_metadata(serde_json::json!({
                "run_id": run_id,
                "schema_v": 1
            }))
            .snapshot_every(100);
    }

    if let Some(projector) = graph_projector {
        engine = engine.with_handler(projection::neo4j_projection_handler(projector));
    }

    engine = engine.with_projection(projection::scout_runs_projection());
    engine = engine.with_projection(projection::system_log_projection());

    if let Some(sink) = capture_sink {
        engine = engine.with_handler(projection::capture_handler(sink));
    }

    engine
}
