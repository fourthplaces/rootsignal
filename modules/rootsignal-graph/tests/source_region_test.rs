//! Integration tests for get_sources_for_region filtering.
//!
//! Verifies that:
//! - Never-scraped sources are included (deserve a chance)
//! - Scraped sources with signals geolocated in region are included (proven relevant)
//! - Scraped-but-unproductive sources are excluded (scheduler handles their lifecycle)
//!
//! Requirements: Docker (for Neo4j via testcontainers)
//!
//! Run with: cargo test -p rootsignal-graph --features test-utils --test source_region_test

#![cfg(feature = "test-utils")]

use uuid::Uuid;

use rootsignal_graph::{query, GraphClient, GraphWriter};

async fn setup() -> (impl std::any::Any, GraphClient) {
    rootsignal_graph::testutil::neo4j_container().await
}

/// Create a Source node in Neo4j with the given properties.
async fn create_source(
    client: &GraphClient,
    canonical_key: &str,
    canonical_value: &str,
    active: bool,
    signals_produced: u32,
    last_scraped: bool,
) {
    let id = Uuid::new_v4();
    let last_scraped_clause = if last_scraped {
        ", last_scraped: datetime()"
    } else {
        ""
    };
    let cypher = format!(
        "CREATE (:Source {{
            id: $id,
            canonical_key: $key,
            canonical_value: $value,
            active: $active,
            signals_produced: $signals,
            signals_corroborated: 0,
            consecutive_empty_runs: 0,
            weight: 0.5,
            discovery_method: 'curated',
            created_at: datetime(),
            source_role: 'mixed',
            scrape_count: 0
            {last_scraped_clause}
        }})"
    );
    let q = query(&cypher)
        .param("id", id.to_string())
        .param("key", canonical_key)
        .param("value", canonical_value)
        .param("active", active)
        .param("signals", signals_produced as i64);
    client.inner().run(q).await.expect("Failed to create source");
}

/// Create a signal node at a given lat/lng with a source_url linking it to a source.
async fn create_signal_at(client: &GraphClient, source_url: &str, lat: f64, lng: f64) {
    let id = Uuid::new_v4();
    let q = query(
        "CREATE (:Gathering {
            id: $id,
            title: 'test signal',
            summary: 'test',
            source_url: $source_url,
            lat: $lat,
            lng: $lng,
            sensitivity: 'general',
            confidence: 0.8,
            extracted_at: datetime()
        })",
    )
    .param("id", id.to_string())
    .param("source_url", source_url)
    .param("lat", lat)
    .param("lng", lng);
    client.inner().run(q).await.expect("Failed to create signal");
}

// Minneapolis center
const MPLS_LAT: f64 = 44.9778;
const MPLS_LNG: f64 = -93.2650;
const RADIUS_KM: f64 = 30.0;

#[tokio::test]
async fn never_scraped_source_included() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    create_source(&client, "src:virgin", "https://virgin.example.com", true, 0, false).await;

    let sources = writer.get_sources_for_region(MPLS_LAT, MPLS_LNG, RADIUS_KM).await.unwrap();
    assert_eq!(sources.len(), 1, "Never-scraped source should be returned");
    assert_eq!(sources[0].canonical_key, "src:virgin");
}

#[tokio::test]
async fn scraped_source_with_signal_in_region_included() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    create_source(&client, "src:productive", "https://productive.example.com", true, 3, true).await;
    create_signal_at(&client, "https://productive.example.com", MPLS_LAT, MPLS_LNG).await;

    let sources = writer.get_sources_for_region(MPLS_LAT, MPLS_LNG, RADIUS_KM).await.unwrap();
    assert_eq!(sources.len(), 1, "Source with signal in region should be returned");
    assert_eq!(sources[0].canonical_key, "src:productive");
}

#[tokio::test]
async fn scraped_unproductive_source_excluded() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    // Scraped but produced zero signals — should NOT appear
    create_source(&client, "src:dud", "https://dud.example.com", true, 0, true).await;

    let sources = writer.get_sources_for_region(MPLS_LAT, MPLS_LNG, RADIUS_KM).await.unwrap();
    assert!(
        sources.is_empty(),
        "Scraped-but-unproductive source should not be returned, got {} sources",
        sources.len()
    );
}

#[tokio::test]
async fn inactive_source_excluded() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    // Inactive, never scraped — should NOT appear (active: false)
    create_source(&client, "src:dead", "https://dead.example.com", false, 0, false).await;

    let sources = writer.get_sources_for_region(MPLS_LAT, MPLS_LNG, RADIUS_KM).await.unwrap();
    assert!(
        sources.is_empty(),
        "Inactive source should not be returned"
    );
}

#[tokio::test]
async fn source_with_signal_outside_region_excluded() {
    let (_container, client) = setup().await;
    let writer = GraphWriter::new(client.clone());

    // Scraped, produced signals, but signals are in Miami — not in Minneapolis region
    create_source(&client, "src:miami", "https://miami.example.com", true, 2, true).await;
    create_signal_at(&client, "https://miami.example.com", 25.76, -80.19).await;

    let sources = writer.get_sources_for_region(MPLS_LAT, MPLS_LNG, RADIUS_KM).await.unwrap();
    assert!(
        sources.is_empty(),
        "Source with signals only in Miami should not appear in Minneapolis region"
    );
}
