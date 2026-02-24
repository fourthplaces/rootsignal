//! Boundary tests — one organ handoff at a time.
//!
//! Each test follows MOCK → FUNCTION → OUTPUT:
//! set up mocks, call ONE real pipeline method, assert the output.

use std::sync::Arc;

use crate::pipeline::scrape_phase::{CollectedLink, RunContext, ScrapePhase};
use crate::testing::*;

use rootsignal_common::types::SourceNode;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a CollectedLink for testing.
fn link(url: &str, discovered_on: &str) -> CollectedLink {
    CollectedLink { url: url.to_string(), discovered_on: discovered_on.to_string() }
}

// ---------------------------------------------------------------------------
// Fetcher → Extractor boundary
//
// ArchivedPage.markdown flows through to extractor, signals get stored.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn page_with_content_produces_signal() {
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
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

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
async fn empty_page_produces_nothing() {
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
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

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
async fn unreachable_page_does_not_crash() {
    // MockFetcher has no page registered for this URL → returns Err
    let fetcher = MockFetcher::new();
    let extractor = MockExtractor::new();

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

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
async fn page_with_multiple_issues_produces_multiple_signals() {
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
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

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
async fn same_title_extracted_twice_produces_one_signal() {
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
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

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
async fn mentioned_actors_are_linked_to_their_signal() {
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
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

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
async fn same_actor_in_two_signals_appears_once_linked_to_both() {
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
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

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
// All signals stored regardless of location (no geo-filter).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn all_signals_stored_regardless_of_region() {
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
                    tension_at("NYC subway delay", NYC.0, NYC.1), // New York
                    tension_at("Local pothole", 44.960, -93.265),      // Minneapolis
                ],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            },
        );

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

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

    assert_eq!(store.signals_created(), 2, "all signals stored regardless of location");
    assert!(store.has_signal_titled("Local pothole"));
    assert!(store.has_signal_titled("NYC subway delay"));
}

#[tokio::test]
async fn blocked_url_produces_nothing() {
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
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

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
async fn unchanged_content_is_not_re_extracted() {
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
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

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

// ---------------------------------------------------------------------------
// Fetcher → Link Discoverer boundary
//
// Page links flow into ctx.collected_links, then promote_links creates sources.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn outbound_links_on_page_are_collected() {
    let mut page = archived_page("https://linktree.org", "# Links page");
    page.links = vec![
        "https://localorg.org/events".to_string(),
        "https://foodshelf.org/volunteer".to_string(),
        "javascript:void(0)".to_string(), // should be filtered by extract_links
    ];

    let fetcher = MockFetcher::new()
        .on_page("https://linktree.org", page);

    let extractor = MockExtractor::new()
        .on_url(
            "https://linktree.org",
            crate::pipeline::extractor::ExtractionResult {
                nodes: vec![tension_at("Community Links", 44.975, -93.270)],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            },
        );

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://linktree.org");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    // javascript: links should be filtered out
    assert!(
        ctx.collected_links.len() >= 2,
        "at least 2 content links should be collected, got {}",
        ctx.collected_links.len()
    );
    let collected_urls: Vec<&str> = ctx.collected_links.iter().map(|l| l.url.as_str()).collect();
    assert!(collected_urls.contains(&"https://localorg.org/events"));
    assert!(collected_urls.contains(&"https://foodshelf.org/volunteer"));
    assert!(
        !collected_urls.iter().any(|u| u.starts_with("javascript:")),
        "javascript: links should be filtered"
    );
}

#[tokio::test]
async fn discovered_links_become_new_sources() {
    use crate::enrichment::link_promoter::{self, PromotionConfig};

    let store = Arc::new(MockSignalStore::new());

    let links = vec![
        link("https://localorg.org/events", "https://linktree.org"),
        link("https://foodshelf.org/volunteer", "https://linktree.org"),
    ];

    let config = PromotionConfig { max_per_source: 10, max_per_run: 50 };

    let promoted = link_promoter::promote_links(
        &links,
        store.as_ref(),
        &config,
    )
    .await
    .unwrap();

    assert_eq!(promoted, 2);
    assert_eq!(store.sources_promoted(), 2);
    assert!(store.has_source_url("https://localorg.org/events"));
    assert!(store.has_source_url("https://foodshelf.org/volunteer"));
}

#[tokio::test]
async fn same_link_from_two_pages_becomes_one_source() {
    use crate::enrichment::link_promoter::{self, PromotionConfig};

    let store = Arc::new(MockSignalStore::new());

    let links = vec![
        link("https://localorg.org/events", "https://page-a.org"),
        link("https://localorg.org/events", "https://page-b.org"), // same URL, different source
        link("https://other.org/page", "https://page-c.org"),
    ];

    let config = PromotionConfig { max_per_source: 10, max_per_run: 50 };

    let promoted = link_promoter::promote_links(
        &links,
        store.as_ref(),
        &config,
    )
    .await
    .unwrap();

    assert_eq!(promoted, 2, "duplicate URLs should be deduped to 2 unique sources");
}

#[tokio::test]
async fn link_promotion_stops_at_configured_cap() {
    use crate::enrichment::link_promoter::{self, PromotionConfig};

    let store = Arc::new(MockSignalStore::new());

    let links: Vec<CollectedLink> = (0..10)
        .map(|i| link(&format!("https://site-{i}.org"), "https://source.org"))
        .collect();

    let config = PromotionConfig { max_per_source: 10, max_per_run: 3 };

    let promoted = link_promoter::promote_links(
        &links,
        store.as_ref(),
        &config,
    )
    .await
    .unwrap();

    assert_eq!(promoted, 3, "should respect max_per_run cap");
}

#[tokio::test]
async fn scrape_then_promote_creates_new_sources() {
    use crate::enrichment::link_promoter::{self, PromotionConfig};

    // Full flow: fetch page with links → run_web → collected_links → promote_links

    let mut page = archived_page("https://hub.org", "# Hub page");
    page.links = vec![
        "https://partner-a.org/programs".to_string(),
        "https://partner-b.org/events".to_string(),
    ];

    let fetcher = MockFetcher::new()
        .on_page("https://hub.org", page);

    let extractor = MockExtractor::new()
        .on_url(
            "https://hub.org",
            crate::pipeline::extractor::ExtractionResult {
                nodes: vec![tension_at("Hub Signal", 44.975, -93.270)],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            },
        );

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://hub.org");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    // Step 1: run_web collects links
    phase.run_web(&sources, &mut ctx, &mut log).await;
    assert!(!ctx.collected_links.is_empty(), "links should be collected");

    // Step 2: promote_links creates source nodes
    let config = PromotionConfig { max_per_source: 10, max_per_run: 50 };
    let promoted = link_promoter::promote_links(
        &ctx.collected_links,
        store.as_ref(),
        &config,
    )
    .await
    .unwrap();

    assert!(promoted >= 2, "at least 2 links should be promoted");
    assert!(store.has_source_url("https://partner-a.org/programs"));
    assert!(store.has_source_url("https://partner-b.org/events"));
}

// ---------------------------------------------------------------------------
// Error-path tests
//
// Verify graceful handling when components fail.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unreachable_page_produces_no_signals() {
    // MockFetcher has NO page registered → returns Err.
    // Pipeline should skip without panic and create no signals.
    let fetcher = MockFetcher::new();
    let extractor = MockExtractor::new();

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://unreachable.org/page");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 0, "fetcher error → no signals");
}

#[tokio::test]
async fn page_with_no_extractable_content_produces_nothing() {
    let fetcher = MockFetcher::new()
        .on_page(
            "https://example.com/empty-extract",
            archived_page("https://example.com/empty-extract", "Some content here"),
        );

    // Extractor returns zero nodes (empty extraction)
    let extractor = MockExtractor::new()
        .on_url(
            "https://example.com/empty-extract",
            crate::pipeline::extractor::ExtractionResult {
                nodes: vec![],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            },
        );

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://example.com/empty-extract");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 0, "empty extraction → no signals, no panic");
}

#[tokio::test]
async fn database_write_failure_does_not_crash() {
    let fetcher = MockFetcher::new()
        .on_page(
            "https://example.com/store-fail",
            archived_page("https://example.com/store-fail", "Content about local issues"),
        );

    let extractor = MockExtractor::new()
        .on_url(
            "https://example.com/store-fail",
            crate::pipeline::extractor::ExtractionResult {
                nodes: vec![tension_at("Signal That Fails To Store", 44.975, -93.270)],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            },
        );

    let store = Arc::new(MockSignalStore::new().failing_creates());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://example.com/store-fail");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    // Should not panic even when store.create_node fails
    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 0, "failing store → no signals persisted");
}

#[tokio::test]
async fn blocked_url_produces_no_signals() {
    // URL is pre-blocked in the store. Pipeline should skip it entirely.
    // Register a page + extractor that WOULD produce a signal — but it should
    // never be reached because the URL is blocked before fetching.
    let fetcher = MockFetcher::new()
        .on_page(
            "https://spam-site.org/page",
            archived_page("https://spam-site.org/page", "Spam content"),
        );

    let extractor = MockExtractor::new()
        .on_url(
            "https://spam-site.org/page",
            crate::pipeline::extractor::ExtractionResult {
                nodes: vec![tension_at("Spam Signal", 44.975, -93.270)],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            },
        );

    let store = Arc::new(MockSignalStore::new().block_url("spam-site.org"));
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://spam-site.org/page");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 0, "blocked URL → zero signals");
    assert!(!store.has_signal_titled("Spam Signal"), "blocked URL signal must not appear");
}

// ---------------------------------------------------------------------------
// Edge case tests — probing corners of the pipeline logic
// ---------------------------------------------------------------------------

#[tokio::test]
async fn all_signal_types_are_stored() {
    // Verify non-Tension/Need node types are stored correctly.
    let fetcher = MockFetcher::new()
        .on_page(
            "https://example.com/mixed-types",
            archived_page("https://example.com/mixed-types", "# Mixed signal types"),
        );

    let extractor = MockExtractor::new()
        .on_url(
            "https://example.com/mixed-types",
            crate::pipeline::extractor::ExtractionResult {
                nodes: vec![
                    gathering_at("Community Potluck", 44.975, -93.270),
                    aid_at("Free Legal Clinic", 44.960, -93.265),
                    notice_at("Park Closure Notice", 44.950, -93.260),
                ],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            },
        );

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://example.com/mixed-types");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 3, "all 3 node types should be created");
    assert!(store.has_signal_titled("Community Potluck"));
    assert!(store.has_signal_titled("Free Legal Clinic"));
    assert!(store.has_signal_titled("Park Closure Notice"));
}

#[tokio::test]
async fn unicode_and_emoji_titles_are_preserved() {
    let fetcher = MockFetcher::new()
        .on_page(
            "https://example.com/unicode",
            archived_page("https://example.com/unicode", "# Événements communautaires 🎉"),
        );

    let extractor = MockExtractor::new()
        .on_url(
            "https://example.com/unicode",
            crate::pipeline::extractor::ExtractionResult {
                nodes: vec![
                    tension_at("Événements communautaires 🎉", 44.975, -93.270),
                    tension_at("日本語のタイトル", 44.960, -93.265),
                ],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            },
        );

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://example.com/unicode");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 2);
    assert!(store.has_signal_titled("Événements communautaires 🎉"));
    assert!(store.has_signal_titled("日本語のタイトル"));
}

#[tokio::test]
async fn signal_at_zero_zero_is_still_stored() {
    // Coords (0.0, 0.0) — no geo-filter, so even null island signals are stored.
    let fetcher = MockFetcher::new()
        .on_page(
            "https://example.com/null-island",
            archived_page("https://example.com/null-island", "# Null island"),
        );

    let extractor = MockExtractor::new()
        .on_url(
            "https://example.com/null-island",
            crate::pipeline::extractor::ExtractionResult {
                nodes: vec![tension_at("Null Island Signal", 0.0, 0.0)],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            },
        );

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://example.com/null-island");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 1, "null island signal is stored (no geo-filter)");
}

#[tokio::test]
async fn broken_extraction_skips_page_gracefully() {
    // Page fetches fine, but extractor returns Err for the URL.
    let fetcher = MockFetcher::new()
        .on_page(
            "https://example.com/extract-fail",
            archived_page("https://example.com/extract-fail", "Valid content here"),
        );

    // MockExtractor has no URL registered → returns Err
    let extractor = MockExtractor::new();

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://example.com/extract-fail");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 0, "extractor error → no signals, no panic");
}

#[tokio::test]
async fn blank_author_name_does_not_create_actor() {
    // author_actor = Some("  ") should be treated as empty and not create an actor.
    let fetcher = MockFetcher::new()
        .on_page(
            "https://example.com/ws-author",
            archived_page("https://example.com/ws-author", "# Content"),
        );

    let mut node = tension_at("Signal With Blank Author", 44.975, -93.270);
    if let Some(meta) = node.meta_mut() {
        meta.author_actor = Some("   ".to_string()); // whitespace-only
    }

    let extractor = MockExtractor::new()
        .on_url(
            "https://example.com/ws-author",
            crate::pipeline::extractor::ExtractionResult {
                nodes: vec![node],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            },
        );

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://example.com/ws-author");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 1, "signal should still be created");
    // Whitespace-only author should NOT create an actor
    assert!(!store.has_actor("   "), "whitespace-only author should not create actor");
}

#[tokio::test]
async fn signal_with_resource_needs_gets_resource_edge() {
    // Verify that resource_tags in ExtractionResult flow through to resource edges.
    let fetcher = MockFetcher::new()
        .on_page(
            "https://example.com/resources",
            archived_page("https://example.com/resources", "# Needs vehicles"),
        );

    let node = tension_at("Need Drivers", 44.975, -93.270);
    let node_id = node.meta().unwrap().id;

    let extractor = MockExtractor::new()
        .on_url(
            "https://example.com/resources",
            crate::pipeline::extractor::ExtractionResult {
                nodes: vec![node],
                implied_queries: vec![],
                resource_tags: vec![(
                    node_id,
                    vec![crate::pipeline::extractor::ResourceTag {
                        slug: "vehicle".to_string(),
                        role: "requires".to_string(),
                        confidence: 0.9,
                        context: Some("pickup truck".to_string()),
                    }],
                )],
                signal_tags: vec![(
                    node_id,
                    vec!["mutual-aid".to_string(), "transportation".to_string()],
                )],
            },
        );

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://example.com/resources");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 1);
    assert!(store.has_signal_titled("Need Drivers"));
    assert!(store.has_resource_edge("Need Drivers", "vehicle"), "resource edge should be created");
    assert!(store.has_tag("Need Drivers", "mutual-aid"), "signal tag should be created");
    assert!(store.has_tag("Need Drivers", "transportation"), "signal tag should be created");
}

#[tokio::test]
async fn zero_sources_produces_nothing() {
    let fetcher = MockFetcher::new();
    let extractor = MockExtractor::new();
    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let sources: Vec<&SourceNode> = vec![];
    let dummy_source = page_source("https://dummy.org");
    let mut ctx = RunContext::new(&[dummy_source]);
    let mut log = run_log();

    // Should not panic with empty sources
    phase.run_web(&sources, &mut ctx, &mut log).await;
    assert_eq!(store.signals_created(), 0);
}

#[tokio::test]
async fn outbound_links_collected_despite_extraction_failure() {
    // Page has outbound links, but extractor fails. Links should still be collected.
    let mut page = archived_page("https://example.com/links-but-error", "Content");
    page.links = vec![
        "https://partner-a.org/events".to_string(),
        "https://partner-b.org/programs".to_string(),
    ];

    let fetcher = MockFetcher::new()
        .on_page("https://example.com/links-but-error", page);

    // No extractor mapping → returns Err
    let extractor = MockExtractor::new();

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://example.com/links-but-error");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 0, "no signals from failed extraction");
    // But links should still be collected
    let collected_urls: Vec<&str> = ctx.collected_links.iter().map(|l| l.url.as_str()).collect();
    assert!(
        collected_urls.iter().any(|u| u.contains("partner-a.org")),
        "links should be collected even when extraction fails"
    );
    assert!(
        collected_urls.iter().any(|u| u.contains("partner-b.org")),
        "links should be collected even when extraction fails"
    );
}

#[tokio::test]
async fn empty_social_account_produces_nothing() {
    // Social source returns 0 posts → no signals, no crash.
    let ig_url = "https://www.instagram.com/empty_account";

    let fetcher = MockFetcher::new()
        .on_posts(ig_url, vec![]); // zero posts

    let extractor = MockExtractor::new();

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_social(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 0, "zero posts → no signals");
}

#[tokio::test]
async fn image_only_posts_produce_no_signals() {
    // Posts exist but have None text → combined_text is empty → early return.
    let ig_url = "https://www.instagram.com/image_only";

    let mut post = test_post("");
    post.text = None; // image-only post

    let fetcher = MockFetcher::new()
        .on_posts(ig_url, vec![post]);

    let extractor = MockExtractor::new();

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_social(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 0, "text-less posts → no signals");
}

#[tokio::test]
async fn empty_mentioned_actor_name_is_not_created() {
    // Empty and whitespace-only strings in mentioned_actors should be filtered out.
    // Only real actor names should create actor nodes.
    let fetcher = MockFetcher::new()
        .on_page(
            "https://example.com/empty-actor",
            archived_page("https://example.com/empty-actor", "# Content"),
        );

    let mut node = tension_at("Signal With Empty Mention", 44.975, -93.270);
    if let Some(meta) = node.meta_mut() {
        meta.mentioned_actors = vec![
            "".to_string(),
            "   ".to_string(),  // whitespace-only
            "Real Org".to_string(),
        ];
    }

    let extractor = MockExtractor::new()
        .on_url(
            "https://example.com/empty-actor",
            crate::pipeline::extractor::ExtractionResult {
                nodes: vec![node],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            },
        );

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://example.com/empty-actor");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 1, "signal should still be created");
    assert!(store.has_actor("Real Org"), "real actor should be created");
    assert!(!store.has_actor(""), "empty string actor should NOT be created");
    assert!(!store.has_actor("   "), "whitespace-only actor should NOT be created");
}

#[tokio::test]
async fn empty_markdown_page_still_collects_outbound_links() {
    // Page fetches successfully but has empty markdown. Links on the page
    // should still be collected for promotion, even though extraction is skipped.
    let mut page = archived_page("https://example.com/empty-md", "");
    // Manually clear the markdown (archived_page sets it from the content arg)
    page.markdown = String::new();
    page.links = vec![
        "https://partner.org/events".to_string(),
        "https://foodshelf.org".to_string(),
    ];

    let fetcher = MockFetcher::new()
        .on_page("https://example.com/empty-md", page);

    let extractor = MockExtractor::new();

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://example.com/empty-md");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 0, "no signals from empty markdown");
    // Links should still be collected even from empty-markdown pages
    let collected_urls: Vec<&str> = ctx.collected_links.iter().map(|l| l.url.as_str()).collect();
    assert!(
        collected_urls.iter().any(|u| u.contains("partner.org")),
        "links from empty-markdown page should still be collected"
    );
}

#[tokio::test]
async fn mixed_outcome_pages_each_handled_independently() {
    // Three pages in one run: one succeeds, one has empty markdown, one fails fetch.
    // Only the successful page should produce a signal.
    let fetcher = MockFetcher::new()
        .on_page(
            "https://good.org/events",
            archived_page("https://good.org/events", "# Community dinner"),
        )
        .on_page(
            "https://empty.org/page",
            {
                let mut p = archived_page("https://empty.org/page", "");
                p.markdown = String::new();
                p
            },
        );
    // https://fail.org/page is NOT registered → returns Err

    let extractor = MockExtractor::new()
        .on_url(
            "https://good.org/events",
            crate::pipeline::extractor::ExtractionResult {
                nodes: vec![tension_at("Community Dinner", 44.975, -93.270)],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            },
        );

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let s1 = page_source("https://good.org/events");
    let s2 = page_source("https://empty.org/page");
    let s3 = page_source("https://fail.org/page");
    let all = vec![s1.clone(), s2.clone(), s3.clone()];
    let sources: Vec<&SourceNode> = vec![&s1, &s2, &s3];
    let mut ctx = RunContext::new(&all);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 1, "only the good page produces a signal");
    assert!(store.has_signal_titled("Community Dinner"));
}

#[tokio::test]
async fn social_scrape_failure_does_not_crash() {
    // Social source fetcher returns Err → no panic, no signals.
    let ig_url = "https://www.instagram.com/broken_account";

    // MockFetcher has no posts registered → returns Err
    let fetcher = MockFetcher::new();
    let extractor = MockExtractor::new();

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    // Should not panic
    phase.run_social(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 0, "social fetch error → no signals");
}

#[tokio::test]
async fn batch_title_dedup_is_case_insensitive() {
    // "Housing Crisis" and "housing crisis" should be deduped to one signal.
    let fetcher = MockFetcher::new()
        .on_page(
            "https://example.com/case-dedup",
            archived_page("https://example.com/case-dedup", "# Case dedup test"),
        );

    let extractor = MockExtractor::new()
        .on_url(
            "https://example.com/case-dedup",
            crate::pipeline::extractor::ExtractionResult {
                nodes: vec![
                    tension_at("Housing Crisis", 44.975, -93.270),
                    tension_at("housing crisis", 44.960, -93.265),
                    tension_at("HOUSING CRISIS", 44.950, -93.260),
                    tension_at("Different Signal", 44.940, -93.255),
                ],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            },
        );

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source("https://example.com/case-dedup");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 2, "case-insensitive dedup should produce 2 signals");
    assert!(store.has_signal_titled("Different Signal"));
}

// ---------------------------------------------------------------------------
// Location metadata through the full pipeline
//
// Verify about_location and from_location survive into StoredSignal.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn web_source_without_actor_stores_content_location_only() {
    let fetcher = MockFetcher::new()
        .on_page(
            "https://localorg.org/events",
            archived_page("https://localorg.org/events", "# Event at Powderhorn"),
        );

    let extractor = MockExtractor::new()
        .on_url(
            "https://localorg.org/events",
            crate::pipeline::extractor::ExtractionResult {
                nodes: vec![tension_at("Powderhorn Cleanup", 44.9489, -93.2583)],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            },
        );

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

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

    let stored = store.signal_by_title("Powderhorn Cleanup").expect("signal should exist");
    let about = stored.about_location.expect("about_location should be set from content");
    assert!((about.lat - 44.9489).abs() < 0.001);
    assert!(stored.from_location.is_none(), "no actor → no from_location");
}

#[tokio::test]
async fn signal_without_content_location_falls_back_to_actor() {
    use rootsignal_common::canonical_value;

    let ig_url = "https://www.instagram.com/localorg";

    let fetcher = MockFetcher::new()
        .on_posts(ig_url, vec![test_post("Thoughts on community organizing")]);

    // Signal with NO about_location
    let extractor = MockExtractor::new()
        .on_url(ig_url, crate::pipeline::extractor::ExtractionResult {
            nodes: vec![tension("Community Organizing Thoughts")],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
        });

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);

    ctx.actor_contexts.insert(
        canonical_value(ig_url),
        rootsignal_common::ActorContext {
            actor_name: "Local Org".to_string(),
            bio: None,
            location_name: Some("Minneapolis".to_string()),
            location_lat: Some(44.9778),
            location_lng: Some(-93.2650),
        },
    );

    let mut log = run_log();
    phase.run_social(&sources, &mut ctx, &mut log).await;

    let stored = store.signal_by_title("Community Organizing Thoughts").expect("signal should exist");
    let about = stored.about_location.expect("about_location should fall back to actor");
    assert!((about.lat - 44.9778).abs() < 0.001, "about_location should be actor coords");
    let from = stored.from_location.expect("from_location should be set from actor");
    assert!((from.lat - 44.9778).abs() < 0.001, "from_location should be actor coords");
}

#[tokio::test]
async fn explicit_content_location_not_overwritten_by_actor() {
    use rootsignal_common::canonical_value;

    let ig_url = "https://www.instagram.com/nycorg";

    let fetcher = MockFetcher::new()
        .on_posts(ig_url, vec![test_post("Great event in St Paul!")]);

    // Signal explicitly located in St Paul
    let extractor = MockExtractor::new()
        .on_url(ig_url, crate::pipeline::extractor::ExtractionResult {
            nodes: vec![tension_at("St Paul Event", ST_PAUL.0, ST_PAUL.1)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
        });

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);

    // Actor in Minneapolis — should NOT overwrite St Paul about_location
    ctx.actor_contexts.insert(
        canonical_value(ig_url),
        rootsignal_common::ActorContext {
            actor_name: "Minneapolis Org".to_string(),
            bio: None,
            location_name: None,
            location_lat: Some(44.9778),
            location_lng: Some(-93.2650),
        },
    );

    let mut log = run_log();
    phase.run_social(&sources, &mut ctx, &mut log).await;

    let stored = store.signal_by_title("St Paul Event").expect("signal should exist");
    let about = stored.about_location.expect("about_location should be preserved");
    assert!((about.lat - ST_PAUL.0).abs() < 0.001, "about_location should stay St Paul, not Minneapolis");
    let from = stored.from_location.expect("from_location should be set from actor");
    assert!((from.lat - 44.9778).abs() < 0.001, "from_location should be Minneapolis actor coords");
}

// ---------------------------------------------------------------------------
// Content date fallback
//
// RSS pub_date and social published_at flow into content_date when the
// LLM didn't extract one.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rss_pub_date_becomes_content_date_when_llm_omits_it() {
    use chrono::TimeZone;

    let feed_url = "https://localorg.org/feed";
    let article_url = "https://localorg.org/article-1";
    let pub_date = chrono::Utc.with_ymd_and_hms(2026, 2, 20, 12, 0, 0).unwrap();

    let feed = rootsignal_common::ArchivedFeed {
        id: uuid::Uuid::new_v4(),
        source_id: uuid::Uuid::new_v4(),
        fetched_at: chrono::Utc::now(),
        content_hash: String::new(),
        items: vec![rootsignal_common::FeedItem {
            url: article_url.to_string(),
            title: Some("Article Title".to_string()),
            pub_date: Some(pub_date),
        }],
        title: Some("Local Org Blog".to_string()),
    };

    let fetcher = MockFetcher::new()
        .on_feed(feed_url, feed)
        .on_page(article_url, archived_page(article_url, "# Community event recap"));

    // Extractor returns signal with NO content_date
    let extractor = MockExtractor::new()
        .on_url(article_url, crate::pipeline::extractor::ExtractionResult {
            nodes: vec![tension_at("Community Event Recap", 44.975, -93.270)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
        });

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source(feed_url);
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    let stored = store.signal_by_title("Community Event Recap").expect("signal should exist");
    let content_date = stored.content_date.expect("content_date should be backfilled from RSS pub_date");
    assert_eq!(content_date, pub_date, "content_date should match RSS pub_date");
}

#[tokio::test]
async fn llm_content_date_not_overwritten_by_rss_pub_date() {
    use chrono::TimeZone;

    let feed_url = "https://localorg.org/feed";
    let article_url = "https://localorg.org/article-2";
    let rss_date = chrono::Utc.with_ymd_and_hms(2026, 2, 20, 12, 0, 0).unwrap();
    let llm_date = chrono::Utc.with_ymd_and_hms(2026, 3, 1, 10, 0, 0).unwrap();

    let feed = rootsignal_common::ArchivedFeed {
        id: uuid::Uuid::new_v4(),
        source_id: uuid::Uuid::new_v4(),
        fetched_at: chrono::Utc::now(),
        content_hash: String::new(),
        items: vec![rootsignal_common::FeedItem {
            url: article_url.to_string(),
            title: None,
            pub_date: Some(rss_date),
        }],
        title: None,
    };

    let fetcher = MockFetcher::new()
        .on_feed(feed_url, feed)
        .on_page(article_url, archived_page(article_url, "# Upcoming event"));

    // Extractor returns signal WITH an explicit content_date
    let mut node = tension_at("Upcoming Event", 44.975, -93.270);
    if let Some(meta) = node.meta_mut() {
        meta.content_date = Some(llm_date);
    }

    let extractor = MockExtractor::new()
        .on_url(article_url, crate::pipeline::extractor::ExtractionResult {
            nodes: vec![node],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
        });

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source(feed_url);
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    let stored = store.signal_by_title("Upcoming Event").expect("signal should exist");
    let content_date = stored.content_date.expect("content_date should exist");
    assert_eq!(content_date, llm_date, "LLM content_date should NOT be overwritten by RSS pub_date");
}

#[tokio::test]
async fn social_published_at_becomes_content_date_fallback() {
    use chrono::TimeZone;

    let ig_url = "https://www.instagram.com/localorg";
    let post_date = chrono::Utc.with_ymd_and_hms(2026, 2, 15, 18, 30, 0).unwrap();

    let mut post = test_post("Big community event coming up!");
    post.published_at = Some(post_date);

    let fetcher = MockFetcher::new()
        .on_posts(ig_url, vec![post]);

    // Signal with NO content_date
    let extractor = MockExtractor::new()
        .on_url(ig_url, crate::pipeline::extractor::ExtractionResult {
            nodes: vec![tension("Big Community Event")],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
        });

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_social(&sources, &mut ctx, &mut log).await;

    let stored = store.signal_by_title("Big Community Event").expect("signal should exist");
    let content_date = stored.content_date.expect("content_date should be backfilled from post published_at");
    assert_eq!(content_date, post_date);
}
