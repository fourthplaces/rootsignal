pub mod event_sourced;

use std::sync::Arc;

use rootsignal_events::EventStore;
use rootsignal_graph::{GraphClient, GraphProjector, GraphWriter};
use sqlx::PgPool;

use crate::pipeline::ScoutEngine;
use crate::traits::SignalStore;

/// Build the production SignalStore: events → Postgres, projections → Neo4j.
///
/// Pure assembly — no logic, no side effects. Caller owns the run_id and
/// decides when to construct.
pub fn build_signal_store(
    graph_client: GraphClient,
    pg_pool: PgPool,
    run_id: String,
) -> event_sourced::EventSourcedStore {
    event_sourced::EventSourcedStore::new(
        GraphWriter::new(graph_client.clone()),
        GraphProjector::new(graph_client),
        EventStore::new(pg_pool),
        run_id,
    )
}

/// Factory for creating per-operation SignalStore instances.
///
/// Production: each call creates a new EventSourcedStore with a unique run_id,
/// giving proper event correlation per API mutation.
///
/// Tests: wraps a shared MockSignalStore via `fixed()`.
pub struct SignalStoreFactory {
    create_fn: Box<dyn Fn() -> Arc<dyn SignalStore> + Send + Sync>,
}

impl SignalStoreFactory {
    /// Production factory: each `create()` yields a new EventSourcedStore
    /// with a unique run_id.
    pub fn new(graph_client: GraphClient, pg_pool: PgPool) -> Self {
        Self {
            create_fn: Box::new(move || {
                let run_id = format!("api-{}", uuid::Uuid::new_v4());
                Arc::new(build_signal_store(
                    graph_client.clone(),
                    pg_pool.clone(),
                    run_id,
                ))
            }),
        }
    }

    /// Wrap a fixed store instance. Every `create()` returns the same Arc.
    /// Useful for tests with MockSignalStore.
    pub fn fixed(store: Arc<dyn SignalStore>) -> Self {
        Self {
            create_fn: Box::new(move || store.clone()),
        }
    }

    /// Create a SignalStore for a single operation.
    pub fn create(&self) -> Arc<dyn SignalStore> {
        (self.create_fn)()
    }
}

/// Factory for creating per-operation engine + deps pairs.
///
/// Used by API mutations to dispatch `SourceDiscovered` through the engine
/// instead of calling `upsert_source` directly.
pub struct EngineFactory {
    create_fn: Box<dyn Fn() -> (ScoutEngine, crate::pipeline::state::PipelineDeps) + Send + Sync>,
}

impl EngineFactory {
    /// Production factory: each `create()` yields a new engine wired to Postgres + Neo4j.
    pub fn new(graph_client: GraphClient, pg_pool: PgPool) -> Self {
        Self {
            create_fn: Box::new(move || {
                let run_id = format!("api-{}", uuid::Uuid::new_v4());
                let event_store = EventStore::new(pg_pool.clone());
                let projector = GraphProjector::new(graph_client.clone());
                let engine = rootsignal_engine::Engine::new(
                    crate::pipeline::reducer::ScoutReducer,
                    crate::pipeline::router::ScoutRouter::new(Some(projector)),
                    Arc::new(event_store) as Arc<dyn rootsignal_engine::EventPersister>,
                    run_id.clone(),
                );
                let store = Arc::new(build_signal_store(
                    graph_client.clone(),
                    pg_pool.clone(),
                    run_id.clone(),
                )) as Arc<dyn SignalStore>;
                let embedder = Arc::new(crate::infra::embedder::NoOpEmbedder)
                    as Arc<dyn crate::infra::embedder::TextEmbedder>;
                let deps = crate::pipeline::state::PipelineDeps {
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

    /// Test factory: engine with MemoryEventSink and no projector, mock store in deps.
    pub fn fixed(store: Arc<dyn SignalStore>) -> Self {
        Self {
            create_fn: Box::new(move || {
                let run_id = format!("test-{}", uuid::Uuid::new_v4());
                let engine = rootsignal_engine::Engine::new(
                    crate::pipeline::reducer::ScoutReducer,
                    crate::pipeline::router::ScoutRouter::new(None),
                    Arc::new(rootsignal_engine::MemoryEventSink::new())
                        as Arc<dyn rootsignal_engine::EventPersister>,
                    run_id.clone(),
                );
                let embedder = Arc::new(crate::infra::embedder::NoOpEmbedder)
                    as Arc<dyn crate::infra::embedder::TextEmbedder>;
                let deps = crate::pipeline::state::PipelineDeps {
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
    pub fn create(&self) -> (ScoutEngine, crate::pipeline::state::PipelineDeps) {
        (self.create_fn)()
    }
}
