#![cfg(feature = "test-utils")]
//! Shared integration test binary for rootsignal-graph.
//!
//! All Neo4j-dependent test modules share a single container (booted once).
//! Each test gets a clean graph via DETACH DELETE in setup().
//!
//! Tests run serially (shared Mutex) because they share one Neo4j database.
//!
//! Run with: cargo test -p rootsignal-graph --features test-utils --test integration

mod enrich;
mod helpers;
mod litmus;
mod pipeline;
mod projection_edges;
mod projection_lifecycle;
mod projection_sources;
mod source_region;

use rootsignal_graph::{query, GraphClient};
use tokio::sync::{Mutex, MutexGuard, OnceCell};

struct TestContainer {
    _handle: Box<dyn std::any::Any + Send>,
    client: GraphClient,
}

// Safety: GraphClient (neo4rs::Graph) is Arc-wrapped and Send+Sync.
// The _handle Box<dyn Any + Send> is only kept alive, never accessed concurrently.
unsafe impl Sync for TestContainer {}

static CONTAINER: OnceCell<TestContainer> = OnceCell::const_new();
static SERIAL: Mutex<()> = Mutex::const_new(());

/// Acquire the shared Neo4j client. First caller boots the container + migrates.
/// Every caller gets a clean graph (all nodes deleted, schema preserved).
/// Returns a MutexGuard to ensure tests run serially — hold it for the test's lifetime.
async fn setup() -> (MutexGuard<'static, ()>, GraphClient) {
    let guard = SERIAL.lock().await;

    let tc = CONTAINER
        .get_or_init(|| async {
            let (handle, client) = rootsignal_graph::testutil::neo4j_container().await;
            rootsignal_graph::migrate::migrate(&client)
                .await
                .expect("migration failed");
            TestContainer {
                _handle: handle,
                client,
            }
        })
        .await;

    // Wipe all data (schema/indexes survive DETACH DELETE)
    tc.client
        .run(query("MATCH (n) DETACH DELETE n"))
        .await
        .expect("cleanup failed");

    (guard, tc.client.clone())
}
