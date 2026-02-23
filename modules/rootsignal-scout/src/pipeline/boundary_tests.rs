//! Boundary tests — one organ handoff at a time.
//!
//! Each test follows MOCK → FUNCTION → OUTPUT:
//! set up mocks, call ONE real pipeline method, assert the output.

use std::sync::Arc;

use crate::infra::run_log::RunLog;
use crate::pipeline::scrape_phase::{RunContext, ScrapePhase};
use crate::testing::*;

use rootsignal_common::types::SourceNode;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn run_log() -> RunLog {
    RunLog::new("test-run".to_string(), "Minneapolis".to_string())
}

// ---------------------------------------------------------------------------
// Fetcher → Extractor boundary
//
// ArchivedPage.markdown flows through to extractor, signals get stored.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn page_content_flows_to_extractor_and_creates_signals() {
    let fetcher = MockFetcher::new()
        .on_page(
            "https://localorg.org/events",
            archived_page("https://localorg.org/events", "# Community Dinner\nFree dinner at Powderhorn Park"),
        );

    let extractor = MockExtractor::new()
        .on_url(
            "https://localorg.org/events",
            crate::pipeline::extractor::ExtractionResult {
                nodes: vec![tension_at("Community Dinner at Powderhorn", 44.9489, -93.2583)],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            },
        );

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(64));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://localorg.org/events");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 1, "one signal should be created");
    assert!(store.has_signal_titled("Community Dinner at Powderhorn"));
}

#[tokio::test]
async fn empty_page_creates_no_signals() {
    let fetcher = MockFetcher::new()
        .on_page(
            "https://empty.org",
            archived_page("https://empty.org", ""),
        );

    let extractor = MockExtractor::new()
        .on_url(
            "https://empty.org",
            crate::pipeline::extractor::ExtractionResult {
                nodes: vec![],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            },
        );

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(64));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://empty.org");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 0);
}

#[tokio::test]
async fn unfetchable_page_does_not_crash() {
    // MockFetcher has no page registered for this URL → returns Err
    let fetcher = MockFetcher::new();
    let extractor = MockExtractor::new();

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(64));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://doesnt-exist.org");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    // Should not panic
    phase.run_web(&sources, &mut ctx, &mut log).await;
    assert_eq!(store.signals_created(), 0);
}

// ---------------------------------------------------------------------------
// Extractor → Signal Processor boundary
//
// Multiple extracted nodes → store_signals → correct signals created,
// dedup works, evidence linked.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn multiple_signals_from_one_page() {
    let fetcher = MockFetcher::new()
        .on_page(
            "https://news.org/article",
            archived_page("https://news.org/article", "# Multiple issues\nHousing and transit"),
        );

    let extractor = MockExtractor::new()
        .on_url(
            "https://news.org/article",
            crate::pipeline::extractor::ExtractionResult {
                nodes: vec![
                    tension_at("Housing Crisis Downtown", 44.975, -93.270),
                    tension_at("Bus Route 5 Cuts", 44.960, -93.265),
                    need_at("Volunteer Drivers Needed", 44.955, -93.260),
                ],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            },
        );

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(64));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://news.org/article");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 3, "all three signals should be created");
    assert!(store.has_signal_titled("Housing Crisis Downtown"));
    assert!(store.has_signal_titled("Bus Route 5 Cuts"));
    assert!(store.has_signal_titled("Volunteer Drivers Needed"));
}

#[tokio::test]
async fn duplicate_titles_within_batch_deduped() {
    let fetcher = MockFetcher::new()
        .on_page(
            "https://news.org/dupe",
            archived_page("https://news.org/dupe", "# Repeated story"),
        );

    let extractor = MockExtractor::new()
        .on_url(
            "https://news.org/dupe",
            crate::pipeline::extractor::ExtractionResult {
                nodes: vec![
                    tension_at("Housing Crisis", 44.975, -93.270),
                    tension_at("Housing Crisis", 44.975, -93.270), // same title+type
                    tension_at("Different Signal", 44.960, -93.265),
                ],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            },
        );

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(64));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://news.org/dupe");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 2, "duplicate title+type should be deduped to 1");
    assert!(store.has_signal_titled("Housing Crisis"));
    assert!(store.has_signal_titled("Different Signal"));
}

// ---------------------------------------------------------------------------
// Extractor → Actor Resolver boundary
//
// mentioned_actors in NodeMeta → actor upsert + link to signal.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mentioned_actors_create_actor_nodes_and_edges() {
    let fetcher = MockFetcher::new()
        .on_page(
            "https://example.com/actors",
            archived_page("https://example.com/actors", "# Actor story"),
        );

    let mut node = tension_at("Free Legal Clinic", 44.975, -93.270);
    if let Some(meta) = node.meta_mut() {
        meta.mentioned_actors = vec!["Legal Aid Society".to_string(), "City Council".to_string()];
    }

    let extractor = MockExtractor::new()
        .on_url(
            "https://example.com/actors",
            crate::pipeline::extractor::ExtractionResult {
                nodes: vec![node],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            },
        );

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(64));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://example.com/actors");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 1);
    assert!(store.has_actor("Legal Aid Society"), "mentioned actor should be created");
    assert!(store.has_actor("City Council"), "mentioned actor should be created");
    assert!(
        store.actor_linked_to_signal("Legal Aid Society", "Free Legal Clinic"),
        "actor should be linked to signal"
    );
    assert!(
        store.actor_linked_to_signal("City Council", "Free Legal Clinic"),
        "actor should be linked to signal"
    );
}

#[tokio::test]
async fn same_actor_mentioned_twice_creates_one_actor() {
    let fetcher = MockFetcher::new()
        .on_page(
            "https://example.com/repeat-actor",
            archived_page("https://example.com/repeat-actor", "# Actor repeat"),
        );

    let mut node1 = tension_at("Event A", 44.975, -93.270);
    if let Some(meta) = node1.meta_mut() {
        meta.mentioned_actors = vec!["Local Org".to_string()];
    }
    let mut node2 = tension_at("Event B", 44.960, -93.265);
    if let Some(meta) = node2.meta_mut() {
        meta.mentioned_actors = vec!["Local Org".to_string()];
    }

    let extractor = MockExtractor::new()
        .on_url(
            "https://example.com/repeat-actor",
            crate::pipeline::extractor::ExtractionResult {
                nodes: vec![node1, node2],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            },
        );

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(64));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://example.com/repeat-actor");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 2);
    assert!(store.has_actor("Local Org"));
    assert!(
        store.actor_linked_to_signal("Local Org", "Event A"),
        "actor linked to first signal"
    );
    assert!(
        store.actor_linked_to_signal("Local Org", "Event B"),
        "actor linked to second signal"
    );
}

// ---------------------------------------------------------------------------
// Location handoff boundary
//
// Geo-filter + actor fallback interaction.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn signal_outside_region_filtered_out() {
    let fetcher = MockFetcher::new()
        .on_page(
            "https://news.org/far-away",
            archived_page("https://news.org/far-away", "# Far away story"),
        );

    let extractor = MockExtractor::new()
        .on_url(
            "https://news.org/far-away",
            crate::pipeline::extractor::ExtractionResult {
                nodes: vec![
                    tension_at("NYC subway delay", 40.7128, -74.0060), // New York
                    tension_at("Local pothole", 44.960, -93.265),      // Minneapolis
                ],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            },
        );

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(64));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://news.org/far-away");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 1, "only the local signal should survive");
    assert!(store.has_signal_titled("Local pothole"));
    assert!(!store.has_signal_titled("NYC subway delay"));
}

#[tokio::test]
async fn blocked_url_skipped_entirely() {
    let fetcher = MockFetcher::new()
        .on_page(
            "https://blocked.org/page",
            archived_page("https://blocked.org/page", "# Blocked content"),
        );

    let extractor = MockExtractor::new()
        .on_url(
            "https://blocked.org/page",
            crate::pipeline::extractor::ExtractionResult {
                nodes: vec![tension_at("Should not appear", 44.975, -93.270)],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            },
        );

    let store = Arc::new(MockSignalStore::new().block_url("blocked.org"));
    let embedder = Arc::new(FixedEmbedder::new(64));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://blocked.org/page");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 0, "blocked URL should produce no signals");
}

// ---------------------------------------------------------------------------
// Embedder → Signal Processor boundary
//
// Content-unchanged skip: same hash → no re-extraction.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn content_unchanged_skips_extraction() {
    // Pre-populate the hash so content is "already processed"
    let page = archived_page("https://news.org/same", "Same content as before");
    let hash = page.content_hash.clone();

    let fetcher = MockFetcher::new()
        .on_page("https://news.org/same", page);

    // Extractor should NOT be called, so register nothing
    let extractor = MockExtractor::new();

    let store = Arc::new(
        MockSignalStore::new()
            .with_processed_hash(&hash, "https://news.org/same"),
    );
    let embedder = Arc::new(FixedEmbedder::new(64));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://news.org/same");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 0, "unchanged content should skip extraction");
}
