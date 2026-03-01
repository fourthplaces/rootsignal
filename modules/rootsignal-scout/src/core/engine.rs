//! Seesaw engine setup for scout.
//!
//! `ScoutEngineDeps` holds everything handlers need. The engine is built
//! via `build_engine()`, which registers all domain handlers plus the
//! persist/reduce/project infrastructure.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use rootsignal_common::ScoutScope;
use rootsignal_events::EventStore as RsEventStore;
use rootsignal_graph::{GraphClient, GraphProjector};
use sqlx::PgPool;
use tokio::sync::RwLock;

use crate::core::aggregate::PipelineState;
use crate::core::projection;
use crate::domains::{discovery, enrichment, expansion, lifecycle, scrape, signals, synthesis};
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
    // --- Engine infrastructure ---
    /// Aggregate state — updated by the apply_to_aggregate handler via apply_* methods.
    pub state: Arc<RwLock<PipelineState>>,
    /// Neo4j graph projector (None in tests).
    pub graph_projector: Option<GraphProjector>,
    /// Event persistence to rootsignal's Postgres event store (None in tests).
    pub event_store: Option<RsEventStore>,
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

/// The seesaw-backed scout engine type.
pub type SeesawEngine = seesaw_core::Engine<ScoutEngineDeps>;

/// Public alias — canonical name for the scout engine.
pub type ScoutEngine = SeesawEngine;

/// Build a fully-wired seesaw engine for a scout run.
///
/// Handler registration order matches the old dispatch loop:
/// 1. **persist** (priority 0) — persist event to rootsignal event store
/// 2. **apply_to_aggregate** (priority 1) — apply event to shared PipelineState
/// 3. **neo4j_projection** (priority 2) — project to graph (projectable events only)
/// 4. **domain handlers** (default priority) — react to events, emit children
pub fn build_engine(deps: ScoutEngineDeps) -> SeesawEngine {
    let capture_sink = deps.captured_events.clone();

    let mut engine = seesaw_core::Engine::new(deps)
        // Infrastructure handlers (priority 0–2)
        .with_handler(projection::persist_handler())
        .with_handler(projection::apply_to_aggregate_handler())
        .with_handler(projection::project_to_graph_handler())
        // Domain handlers
        .with_handlers(signals::handlers::handlers())
        .with_handlers(lifecycle::handlers::handlers())
        .with_handlers(scrape::handlers::handlers())
        .with_handlers(discovery::handlers::handlers())
        .with_handlers(enrichment::handlers::handlers())
        .with_handlers(expansion::handlers::handlers())
        .with_handlers(synthesis::handlers::handlers());

    // Test-only: register capture handler when sink is provided
    if let Some(sink) = capture_sink {
        engine = engine.with_handler(projection::capture_handler(sink));
    }

    engine
}
