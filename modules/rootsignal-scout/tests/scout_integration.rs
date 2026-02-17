//! Integration test: runs Scout with fixture services against a real Memgraph instance.
//! Requires Docker. Skipped in environments without Docker.

use rootsignal_graph::testutil::memgraph_container;
use rootsignal_scout::fixtures::{
    FixtureEmbedder, FixtureExtractor, FixtureScraper, FixtureSearcher, FixtureSocialScraper,
};
use rootsignal_scout::scraper::SearchResult;
use rootsignal_scout::scout::Scout;

#[tokio::test]
async fn scout_stores_signal_in_graph() {
    // 1. Start Memgraph
    let (_container, client) = memgraph_container().await;

    // 2. Run migrations to create schema
    rootsignal_graph::migrate::migrate(&client)
        .await
        .expect("Migration failed");

    // 3. Build Scout with all-fixture deps
    let scout = Scout::with_deps(
        client,
        Box::new(FixtureExtractor::single_event()),
        Box::new(FixtureEmbedder),
        Box::new(FixtureScraper::new("Some civic content about a garden volunteer day.")),
        Box::new(FixtureSearcher::new(vec![SearchResult {
            url: "https://example.com/garden".to_string(),
            title: "Community Garden".to_string(),
            snippet: "Volunteer day at the community garden.".to_string(),
        }])),
        Box::new(FixtureSocialScraper::empty()),
        "test-api-key",
        "twincities",
    );

    // 4. Run scout
    let stats = scout.run().await.expect("Scout run failed");

    // 5. Assert at least one signal was stored
    assert!(
        stats.signals_stored >= 1,
        "Expected at least 1 signal stored, got {}",
        stats.signals_stored
    );
}
