//! Seesaw engine setup for scout.
//!
//! Two engine variants share the same deps and infrastructure handlers:
//!
//! - **Scrape engine** (`build_engine`): reap → schedule → scrape → enrichment →
//!   expansion → synthesis → finalize. Used by standalone scrape/bootstrap workflows.
//!
//! - **Full engine** (`build_full_engine`): extends the scrape chain with
//!   situation_weaving → supervisor before finalize. Used by full_run and
//!   standalone synthesis/situation_weaver/supervisor workflows.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use rootsignal_common::ScoutScope;
use rootsignal_events::EventStore as RsEventStore;
use rootsignal_graph::{GraphClient, GraphProjector};
use sqlx::PgPool;
use tokio::sync::RwLock;

use crate::core::aggregate::PipelineState;
use crate::core::projection;
use crate::domains::{
    discovery, enrichment, expansion, lifecycle, scrape, signals, situation_weaving, supervisor,
    synthesis,
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
            state: Arc::new(RwLock::new(PipelineState::default())),
            graph_projector: None,
            event_store: None,
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

    let mut engine = seesaw_core::Engine::new(deps)
        // Infrastructure handlers (priority 0–2)
        .with_handlers(projection::handlers::handlers())
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

    let mut engine = seesaw_core::Engine::new(deps)
        // Infrastructure handlers (priority 0–2)
        .with_handlers(projection::handlers::handlers())
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

    // Test-only: register capture handler when sink is provided
    if let Some(sink) = capture_sink {
        engine = engine.with_handler(projection::capture_handler(sink));
    }

    engine
}
