//! Seesaw engine setup for scout.
//!
//! `ScoutEngineDeps` holds everything handlers need. The engine is built
//! via `build_engine()`, which registers all domain handlers plus the
//! persist/reduce/project infrastructure.
//!
//! `CompatEngine` wraps the seesaw engine with the same `dispatch()` signature
//! as the old `rootsignal_engine::Engine`, so existing call sites need no changes.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::Result;
use rootsignal_events::EventStore as RsEventStore;
use rootsignal_graph::GraphProjector;
use sqlx::PgPool;
use tokio::sync::RwLock;

use crate::core::aggregate::PipelineState;
use crate::core::deps::PipelineDeps;
use crate::core::events::ScoutEvent;
use crate::core::projection;
use crate::domains::{discovery, enrichment, expansion, lifecycle, scrape, signals};

/// Dependencies shared by all seesaw handlers.
pub struct ScoutEngineDeps {
    /// Pipeline-level deps (store, embedder, region, etc.)
    /// Behind RwLock so CompatEngine can swap them in at dispatch time.
    /// None until the first dispatch() call sets it.
    pub pipeline_deps: Arc<RwLock<Option<PipelineDeps>>>,
    /// Shared mutable state — updated by the state_updater handler.
    pub state: Arc<RwLock<PipelineState>>,
    /// Neo4j graph projector (None in tests).
    pub graph_projector: Option<GraphProjector>,
    /// Event persistence to rootsignal's Postgres event store (None in tests).
    pub event_store: Option<RsEventStore>,
    /// Current run ID for event tagging.
    pub run_id: String,
    /// Test-only: capture all dispatched events for inspection.
    /// None in production, Some in tests that need event inspection.
    pub captured_events: Option<Arc<std::sync::Mutex<Vec<ScoutEvent>>>>,
    /// Budget tracker for LLM/API cost tracking.
    pub budget: Option<Arc<crate::scheduling::budget::BudgetTracker>>,
    /// Cancellation flag — checked by handlers between phases.
    pub cancelled: Option<Arc<AtomicBool>>,
    /// Postgres connection pool — used by finalize handler to save run stats.
    pub pg_pool: Option<PgPool>,
}

/// The seesaw-backed scout engine type.
pub type SeesawEngine = seesaw_core::Engine<ScoutEngineDeps>;

/// Build a fully-wired seesaw engine for a scout run.
///
/// Handler registration order matches the old dispatch loop:
/// 1. **persist** (priority 0) — persist event to rootsignal event store
/// 2. **state_updater** (priority 1) — apply event to shared PipelineState
/// 3. **neo4j_projection** (priority 2) — project to graph (projectable events only)
/// 4. **domain handlers** (default priority) — react to events, emit children
pub(crate) fn build_seesaw_engine(deps: ScoutEngineDeps) -> SeesawEngine {
    let capture_sink = deps.captured_events.clone();

    let mut engine = seesaw_core::Engine::new(deps)
        // Infrastructure handlers (priority 0–2)
        .with_handler(projection::persist_handler())
        .with_handler(projection::state_updater())
        .with_handler(projection::neo4j_handler())
        // Signal domain handlers
        .with_handler(signals::handlers::dedup_handler())
        .with_handler(signals::handlers::create_handler())
        .with_handler(signals::handlers::corroborate_handler())
        .with_handler(signals::handlers::refresh_handler())
        .with_handler(signals::handlers::signal_stored_handler())
        // Lifecycle domain handlers
        .with_handler(lifecycle::handlers::reap_handler())
        .with_handler(lifecycle::handlers::schedule_handler())
        .with_handler(lifecycle::handlers::finalize_handler())
        // Scrape domain handlers
        .with_handler(scrape::handlers::tension_scrape_handler())
        .with_handler(scrape::handlers::response_scrape_handler())
        // Discovery domain handlers
        .with_handler(discovery::handlers::bootstrap_handler())
        .with_handler(discovery::handlers::link_promotion_handler())
        .with_handler(discovery::handlers::mid_run_handler())
        // Enrichment domain handlers
        .with_handler(enrichment::handlers::actor_location_handler())
        .with_handler(enrichment::handlers::post_scrape_handler())
        .with_handler(enrichment::handlers::metrics_handler())
        // Expansion domain handlers
        .with_handler(expansion::handlers::expansion_handler());

    // Test-only: register capture handler when sink is provided
    if let Some(sink) = capture_sink {
        engine = engine.with_handler(projection::capture_handler(sink));
    }

    engine
}

/// Compatibility wrapper around the seesaw engine.
///
/// Provides the same `dispatch(event, &mut state, &deps)` signature as the old
/// `rootsignal_engine::Engine`, so existing call sites need zero changes.
///
/// Internally, it swaps the caller's state into the shared `Arc<RwLock<PipelineState>>`
/// before dispatch and swaps it back after settlement (O(1) via `std::mem::swap`).
pub struct CompatEngine {
    seesaw: SeesawEngine,
    /// Shared state — same Arc as ScoutEngineDeps.state, kept here for swap access.
    state: Arc<RwLock<PipelineState>>,
    /// Shared pipeline deps — same Arc as ScoutEngineDeps.pipeline_deps.
    pipeline_deps: Arc<RwLock<Option<PipelineDeps>>>,
    /// Run ID for read-only access.
    run_id: String,
}

impl CompatEngine {
    /// Build a new compat engine from the given deps.
    pub fn new(deps: ScoutEngineDeps) -> Self {
        let state = deps.state.clone();
        let pipeline_deps = deps.pipeline_deps.clone();
        let run_id = deps.run_id.clone();
        Self {
            seesaw: build_seesaw_engine(deps),
            state,
            pipeline_deps,
            run_id,
        }
    }

    /// Dispatch an event through seesaw with the same signature as the old engine.
    ///
    /// 1. Swap caller's state + deps into shared storage (O(1))
    /// 2. Process through seesaw with synchronous settlement
    /// 3. Swap shared state back to caller (O(1))
    pub async fn dispatch(
        &self,
        event: ScoutEvent,
        state: &mut PipelineState,
        deps: &PipelineDeps,
    ) -> Result<()> {
        // Move caller's state into shared storage
        {
            let mut shared = self.state.write().await;
            std::mem::swap(state, &mut *shared);
        }
        // Set pipeline deps (clone Arcs — cheap)
        {
            let mut shared = self.pipeline_deps.write().await;
            *shared = Some(deps.clone());
        }

        // Process through seesaw — settled() drives the full causal tree
        let result: Result<_> = self.seesaw.dispatch(event).settled().await;

        // Move shared state back to caller (even on error, so caller has latest state)
        {
            let mut shared = self.state.write().await;
            std::mem::swap(state, &mut *shared);
        }

        result.map(|_| ())
    }

    /// Read-only access to the run ID.
    pub fn run_id(&self) -> &str {
        &self.run_id
    }
}
