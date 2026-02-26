pub mod event_sourced_store;
pub mod expansion;
pub mod extractor;
pub mod news_scanner;
pub mod scrape_phase;
pub mod scrape_pipeline;
pub mod stats;
pub mod traits;

use rootsignal_events::EventStore;
use rootsignal_graph::{GraphClient, GraphProjector, GraphWriter};
use sqlx::PgPool;

/// Build the production SignalStore: events → Postgres, projections → Neo4j.
///
/// Pure assembly — no logic, no side effects. Caller owns the run_id and
/// decides when to construct.
pub fn build_signal_store(
    graph_client: GraphClient,
    pg_pool: PgPool,
    run_id: String,
) -> event_sourced_store::EventSourcedStore {
    event_sourced_store::EventSourcedStore::new(
        GraphWriter::new(graph_client.clone()),
        GraphProjector::new(graph_client),
        EventStore::new(pg_pool),
        run_id,
    )
}
#[cfg(test)]
pub mod simweb_adapter;
#[cfg(test)]
mod boundary_tests;
#[cfg(test)]
mod chain_tests;
