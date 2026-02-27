pub mod event_sourced;

use std::sync::Arc;

use rootsignal_events::EventStore;
use rootsignal_graph::{GraphClient, GraphProjector, GraphWriter};
use sqlx::PgPool;

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
