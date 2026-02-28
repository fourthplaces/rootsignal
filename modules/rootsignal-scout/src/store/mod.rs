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

/// Factory for creating per-operation engine + deps pairs.
///
/// Used by API mutations to dispatch `SourceDiscovered` through the engine
/// instead of calling `upsert_source` directly.
pub struct EngineFactory {
    create_fn: Box<
        dyn Fn() -> (crate::pipeline::ScoutEngine, crate::core::deps::PipelineDeps) + Send + Sync,
    >,
}

impl EngineFactory {
    /// Production factory: each `create()` yields a new engine wired to Postgres + Neo4j.
    pub fn new(graph_client: GraphClient, pg_pool: PgPool) -> Self {
        Self {
            create_fn: Box::new(move || {
                let run_id = format!("api-{}", uuid::Uuid::new_v4());
                let event_store = EventStore::new(pg_pool.clone());
                let projector = GraphProjector::new(graph_client.clone());
                let engine = crate::core::engine::CompatEngine::new(
                    crate::core::engine::ScoutEngineDeps {
                        pipeline_deps: Arc::new(tokio::sync::RwLock::new(None)),
                        state: Arc::new(tokio::sync::RwLock::new(
                            crate::core::aggregate::PipelineState::default(),
                        )),
                        graph_projector: Some(projector),
                        event_store: Some(event_store),
                        run_id: run_id.clone(),
                        captured_events: None,
                    },
                );
                let store =
                    Arc::new(build_signal_reader(graph_client.clone())) as Arc<dyn SignalReader>;
                let embedder = Arc::new(crate::infra::embedder::NoOpEmbedder)
                    as Arc<dyn crate::infra::embedder::TextEmbedder>;
                let deps = crate::core::deps::PipelineDeps {
                    store,
                    embedder,
                    region: None,
                    run_id,
                    fetcher: None,
                    anthropic_api_key: None,
                };
                (engine, deps)
            }),
        }
    }

    /// Test factory: engine with no event store and no projector, mock store in deps.
    pub fn fixed(store: Arc<dyn SignalReader>) -> Self {
        Self {
            create_fn: Box::new(move || {
                let run_id = format!("test-{}", uuid::Uuid::new_v4());
                let engine = crate::core::engine::CompatEngine::new(
                    crate::core::engine::ScoutEngineDeps {
                        pipeline_deps: Arc::new(tokio::sync::RwLock::new(None)),
                        state: Arc::new(tokio::sync::RwLock::new(
                            crate::core::aggregate::PipelineState::default(),
                        )),
                        graph_projector: None,
                        event_store: None,
                        run_id: run_id.clone(),
                        captured_events: None,
                    },
                );
                let embedder = Arc::new(crate::infra::embedder::NoOpEmbedder)
                    as Arc<dyn crate::infra::embedder::TextEmbedder>;
                let deps = crate::core::deps::PipelineDeps {
                    store: store.clone(),
                    embedder,
                    region: None,
                    run_id,
                    fetcher: None,
                    anthropic_api_key: None,
                };
                (engine, deps)
            }),
        }
    }

    /// Create an engine + minimal PipelineDeps for a single operation.
    pub fn create(&self) -> (crate::pipeline::ScoutEngine, crate::core::deps::PipelineDeps) {
        (self.create_fn)()
    }
}
