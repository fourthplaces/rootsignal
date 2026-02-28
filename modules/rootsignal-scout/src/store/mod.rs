pub mod event_sourced;

use std::sync::Arc;

use rootsignal_events::EventStore;
use rootsignal_graph::{GraphClient, GraphProjector, GraphWriter};
use sqlx::PgPool;

use crate::traits::SignalReader;

/// Build the production SignalReader: read-only queries via Neo4j.
///
/// Pure assembly — no logic, no side effects.
pub fn build_signal_reader(graph_client: GraphClient) -> event_sourced::EventSourcedReader {
    event_sourced::EventSourcedReader::new(GraphWriter::new(graph_client))
}

/// Factory for creating per-operation SignalReader instances.
///
/// Production: each call creates a new EventSourcedReader.
///
/// Tests: wraps a shared MockSignalReader via `fixed()`.
pub struct SignalReaderFactory {
    create_fn: Box<dyn Fn() -> Arc<dyn SignalReader> + Send + Sync>,
}

impl SignalReaderFactory {
    /// Production factory: each `create()` yields a new EventSourcedReader.
    pub fn new(graph_client: GraphClient) -> Self {
        Self {
            create_fn: Box::new(move || Arc::new(build_signal_reader(graph_client.clone()))),
        }
    }

    /// Wrap a fixed store instance. Every `create()` returns the same Arc.
    /// Useful for tests with MockSignalReader.
    pub fn fixed(store: Arc<dyn SignalReader>) -> Self {
        Self {
            create_fn: Box::new(move || store.clone()),
        }
    }

    /// Create a SignalReader for a single operation.
    pub fn create(&self) -> Arc<dyn SignalReader> {
        (self.create_fn)()
    }
}

/// Factory for creating per-operation engines.
///
/// Used by API mutations to dispatch `SourceDiscovered` through the engine
/// instead of calling `upsert_source` directly.
pub struct EngineFactory {
    create_fn: Box<dyn Fn() -> crate::pipeline::ScoutEngine + Send + Sync>,
}

impl EngineFactory {
    /// Production factory: each `create()` yields a new engine wired to Postgres + Neo4j.
    pub fn new(graph_client: GraphClient, pg_pool: PgPool) -> Self {
        Self {
            create_fn: Box::new(move || {
                let run_id = format!("api-{}", uuid::Uuid::new_v4());
                let event_store = EventStore::new(pg_pool.clone());
                let projector = GraphProjector::new(graph_client.clone());
                let store =
                    Arc::new(build_signal_reader(graph_client.clone())) as Arc<dyn SignalReader>;
                let embedder = Arc::new(crate::infra::embedder::NoOpEmbedder)
                    as Arc<dyn crate::infra::embedder::TextEmbedder>;
                crate::core::engine::build_engine(crate::core::engine::ScoutEngineDeps {
                    store,
                    embedder,
                    region: None,
                    fetcher: None,
                    anthropic_api_key: None,
                    graph_client: Some(graph_client.clone()),
                    extractor: None,
                    state: Arc::new(tokio::sync::RwLock::new(
                        crate::core::aggregate::PipelineState::default(),
                    )),
                    graph_projector: Some(projector),
                    event_store: Some(event_store),
                    run_id,
                    captured_events: None,
                    budget: None,
                    cancelled: None,
                    pg_pool: None,
                    archive: None,
                })
            }),
        }
    }

    /// Test factory: engine with no event store and no projector, mock store in deps.
    pub fn fixed(store: Arc<dyn SignalReader>) -> Self {
        Self {
            create_fn: Box::new(move || {
                let run_id = format!("test-{}", uuid::Uuid::new_v4());
                let embedder = Arc::new(crate::infra::embedder::NoOpEmbedder)
                    as Arc<dyn crate::infra::embedder::TextEmbedder>;
                crate::core::engine::build_engine(crate::core::engine::ScoutEngineDeps {
                    store: store.clone(),
                    embedder,
                    region: None,
                    fetcher: None,
                    anthropic_api_key: None,
                    graph_client: None,
                    extractor: None,
                    state: Arc::new(tokio::sync::RwLock::new(
                        crate::core::aggregate::PipelineState::default(),
                    )),
                    graph_projector: None,
                    event_store: None,
                    run_id,
                    captured_events: None,
                    budget: None,
                    cancelled: None,
                    pg_pool: None,
                    archive: None,
                })
            }),
        }
    }

    /// Create an engine for a single operation.
    pub fn create(&self) -> crate::pipeline::ScoutEngine {
        (self.create_fn)()
    }
}
