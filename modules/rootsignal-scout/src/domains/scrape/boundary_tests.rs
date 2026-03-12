//! Boundary tests — one organ handoff at a time.
//!
//! Each test follows MOCK → FUNCTION → OUTPUT:
//! set up mocks, call ONE real pipeline method, assert the output.

use std::sync::{Arc, Mutex};

use crate::core::extractor::{ExtractionResult, ResourceRole, ResourceTag};
use crate::domains::enrichment::activities::link_promoter::CollectedLink;
use crate::core::aggregate::PipelineState;
use crate::domains::scrape::activities::ScrapeOutput;
use crate::testing::*;

use rootsignal_common::types::SourceNode;
use crate::domains::signals::events::SignalEvent;
use crate::domains::enrichment::activities::link_promoter::{self, PromotionConfig};
use rootsignal_common::canonical_value;
use chrono::TimeZone;
use crate::domains::enrichment::activities::actor_location::{triangulate_all_actors, ActorLocationUpdate};
use crate::traits::SignalReader;
use chrono::Utc;
use rootsignal_common::ActorType;
use uuid::Uuid;
use rootsignal_common::events::SystemEvent;


// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Dispatch collected events through a test engine, updating state.
async fn dispatch_events(
    events: causal::Events,
    ctx: &mut PipelineState,
    store: &Arc<MockSignalReader>,
) {
    let store_arc = store.clone() as Arc<dyn SignalReader>;
    let engine = test_engine_for_store(store_arc);
    for output in events.into_outputs() {
        let _ = engine.emit_output(output).settled().await;
    }
    let state = engine.singleton::<crate::core::aggregate::PipelineState>();
    ctx.stats = state.stats.clone();
}

/// Take events from scrape output, apply state, and dispatch through engine.
///
/// Mirrors what the scrape handler does: dispatches freshness events, then
/// constructs a WebScrapeCompleted carrying extracted_batches so the dedup
/// handler triggers and processes new signals through the engine.
async fn scrape_and_dispatch(
    output: ScrapeOutput,
    ctx: &mut PipelineState,
    store: &Arc<MockSignalReader>,
) {
    use crate::domains::scrape::events::ScrapeEvent;

    let mut output = output;
    let events = output.take_events();
    let extracted_batches = std::mem::take(&mut output.extracted_batches);
    ctx.apply_scrape_output(output);

    let store_arc = store.clone() as Arc<dyn SignalReader>;
    let engine = test_engine_for_store(store_arc);
    for out in events.into_outputs() {
        let _ = engine.emit_output(out).settled().await;
    }

    if !extracted_batches.is_empty() {
        let _ = engine
            .emit(ScrapeEvent::from(TestWebScrapeCompleted::builder()
                .is_tension(true)
                .extracted_batches(extracted_batches)
                .build()))
            .settled()
            .await;
    }

    let state = engine.singleton::<PipelineState>();
    ctx.stats = state.stats.clone();
}

/// Build a CollectedLink for testing.
fn link(url: &str, discovered_on: &str) -> CollectedLink {
    CollectedLink {
        url: url.to_string(),
        discovered_on: discovered_on.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Fetcher → Extractor boundary
//
// ArchivedPage.markdown flows through to extractor, signals get stored.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn page_with_content_produces_signal() {
    let fetcher = MockFetcher::new().on_page(
        "https://localorg.org/events",
        archived_page(
            "https://localorg.org/events",
            "# Community Dinner\nFree dinner at Powderhorn Park",
        ),
    );

    let extractor = MockExtractor::new().on_url(
        "https://localorg.org/events",
        ExtractionResult {
            nodes: vec![tension_at(
                "Community Dinner at Powderhorn",
                44.9489,
                -93.2583,
            )],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://localorg.org/events");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(ctx.stats.signals_stored, 1, "one signal should be created");
}

#[tokio::test]
async fn empty_page_produces_nothing() {
    let fetcher =
        MockFetcher::new().on_page("https://empty.org", archived_page("https://empty.org", ""));

    let extractor = MockExtractor::new().on_url(
        "https://empty.org",
        ExtractionResult {
            nodes: vec![],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://empty.org");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(ctx.stats.signals_stored, 0);
}

#[tokio::test]
async fn unreachable_page_does_not_crash() {
    // MockFetcher has no page registered for this URL → returns Err
    let fetcher = MockFetcher::new();
    let extractor = MockExtractor::new();

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://doesnt-exist.org");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    // Should not panic
    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;
    assert_eq!(ctx.stats.signals_stored, 0);
}

// ---------------------------------------------------------------------------
// Extractor → Signal Processor boundary
//
// Multiple extracted nodes → store_signals → correct signals created,
// dedup works, evidence linked.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn page_with_multiple_issues_produces_multiple_signals() {
    let fetcher = MockFetcher::new().on_page(
        "https://news.org/article",
        archived_page(
            "https://news.org/article",
            "# Multiple issues\nHousing and transit",
        ),
    );

    let extractor = MockExtractor::new().on_url(
        "https://news.org/article",
        ExtractionResult {
            nodes: vec![
                tension_at("Housing Crisis Downtown", 44.975, -93.270),
                tension_at("Bus Route 5 Cuts", 44.960, -93.265),
                need_at("Volunteer Drivers Needed", 44.955, -93.260),
            ],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://news.org/article");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 3,
        "all three signals should be created"
    );
}

#[tokio::test]
async fn same_title_extracted_twice_produces_one_signal() {
    let fetcher = MockFetcher::new().on_page(
        "https://news.org/dupe",
        archived_page("https://news.org/dupe", "# Repeated story"),
    );

    let extractor = MockExtractor::new().on_url(
        "https://news.org/dupe",
        ExtractionResult {
            nodes: vec![
                tension_at("Housing Crisis", 44.975, -93.270),
                tension_at("Housing Crisis", 44.975, -93.270), // same title+type
                tension_at("Different Signal", 44.960, -93.265),
            ],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://news.org/dupe");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 2,
        "duplicate title+type should be deduped to 1"
    );
}

// NOTE: Tests `mentioned_actors_are_linked_to_their_signal` and
// `same_actor_in_two_signals_appears_once_linked_to_both` were removed.
// Mentioned entities no longer create Actor nodes — see
// `mentioned_entities_do_not_create_actor_nodes` below.

// ---------------------------------------------------------------------------
// Location handoff boundary
//
// All signals stored regardless of location (no geo-filter).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn all_signals_stored_regardless_of_region() {
    let fetcher = MockFetcher::new().on_page(
        "https://news.org/far-away",
        archived_page("https://news.org/far-away", "# Far away story"),
    );

    let extractor = MockExtractor::new().on_url(
        "https://news.org/far-away",
        ExtractionResult {
            nodes: vec![
                tension_at("NYC subway delay", NYC.0, NYC.1), // New York
                tension_at("Local pothole", 44.960, -93.265), // Minneapolis
            ],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://news.org/far-away");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 2,
        "all signals stored regardless of location"
    );
}

#[tokio::test]
async fn blocked_url_produces_nothing() {
    let fetcher = MockFetcher::new().on_page(
        "https://blocked.org/page",
        archived_page("https://blocked.org/page", "# Blocked content"),
    );

    let extractor = MockExtractor::new().on_url(
        "https://blocked.org/page",
        ExtractionResult {
            nodes: vec![tension_at("Should not appear", 44.975, -93.270)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new().block_url("blocked.org"));
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://blocked.org/page");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 0,
        "blocked URL should produce no signals"
    );
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

    let fetcher = MockFetcher::new().on_page("https://news.org/same", page);

    // Extractor should NOT be called, so register nothing
    let extractor = MockExtractor::new();

    let store =
        Arc::new(MockSignalReader::new().with_processed_hash(&hash, "https://news.org/same"));
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://news.org/same");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 0,
        "unchanged content should skip extraction"
    );
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

    let fetcher = MockFetcher::new().on_page("https://linktree.org", page);

    let extractor = MockExtractor::new().on_url(
        "https://linktree.org",
        ExtractionResult {
            nodes: vec![tension_at("Community Links", 44.975, -93.270)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://linktree.org");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

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

    let links = vec![
        link("https://localorg.org/events", "https://linktree.org"),
        link("https://foodshelf.org/volunteer", "https://linktree.org"),
    ];

    let config = PromotionConfig {
        max_per_source: 10,
        max_per_run: 50,
        ..Default::default()
    };

    let sources = link_promoter::promote_links(&links, &config);

    assert_eq!(sources.len(), 2);
    let urls: Vec<_> = sources.iter().filter_map(|s| s.url.as_deref()).collect();
    assert!(urls.contains(&"https://localorg.org/events"));
    assert!(urls.contains(&"https://foodshelf.org/volunteer"));
}

#[tokio::test]
async fn same_link_from_two_pages_becomes_one_source() {

    let links = vec![
        link("https://localorg.org/events", "https://page-a.org"),
        link("https://localorg.org/events", "https://page-b.org"), // same URL, different source
        link("https://other.org/page", "https://page-c.org"),
    ];

    let config = PromotionConfig {
        max_per_source: 10,
        max_per_run: 50,
        ..Default::default()
    };

    let sources = link_promoter::promote_links(&links, &config);

    assert_eq!(
        sources.len(),
        2,
        "duplicate URLs should be deduped to 2 unique sources"
    );
}

#[tokio::test]
async fn link_promotion_stops_at_configured_cap() {

    let links: Vec<CollectedLink> = (0..10)
        .map(|i| link(&format!("https://site-{i}.org"), "https://source.org"))
        .collect();

    let config = PromotionConfig {
        max_per_source: 10,
        max_per_run: 3,
        ..Default::default()
    };

    let sources = link_promoter::promote_links(&links, &config);

    assert_eq!(sources.len(), 3, "should respect max_per_run cap");
}

#[tokio::test]
async fn scrape_then_promote_creates_new_sources() {

    // Full flow: fetch page with links → scrape_web_sources → collected_links → promote_links

    let mut page = archived_page("https://hub.org", "# Hub page");
    page.links = vec![
        "https://partner-a.org/programs".to_string(),
        "https://partner-b.org/events".to_string(),
    ];

    let fetcher = MockFetcher::new().on_page("https://hub.org", page);

    let extractor = MockExtractor::new().on_url(
        "https://hub.org",
        ExtractionResult {
            nodes: vec![tension_at("Hub Signal", 44.975, -93.270)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://hub.org");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    // Step 1: scrape_web_sources collects links
    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;
    assert!(!ctx.collected_links.is_empty(), "links should be collected");

    // Step 2: promote_links creates source nodes
    let config = PromotionConfig {
        max_per_source: 10,
        max_per_run: 50,
        ..Default::default()
    };
    let sources = link_promoter::promote_links(&ctx.collected_links, &config);

    assert!(sources.len() >= 2, "at least 2 links should be promoted");
    let urls: Vec<_> = sources.iter().filter_map(|s| s.url.as_deref()).collect();
    assert!(urls.contains(&"https://partner-a.org/programs"));
    assert!(urls.contains(&"https://partner-b.org/events"));
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

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://unreachable.org/page");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(ctx.stats.signals_stored, 0, "fetcher error → no signals");
}

#[tokio::test]
async fn page_with_no_extractable_content_produces_nothing() {
    let fetcher = MockFetcher::new().on_page(
        "https://example.com/empty-extract",
        archived_page("https://example.com/empty-extract", "Some content here"),
    );

    // Extractor returns zero nodes (empty extraction)
    let extractor = MockExtractor::new().on_url(
        "https://example.com/empty-extract",
        ExtractionResult {
            nodes: vec![],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://example.com/empty-extract");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 0,
        "empty extraction → no signals, no panic"
    );
}

#[tokio::test]
async fn database_write_failure_does_not_crash() {
    let fetcher = MockFetcher::new().on_page(
        "https://example.com/store-fail",
        archived_page(
            "https://example.com/store-fail",
            "Content about local issues",
        ),
    );

    let extractor = MockExtractor::new().on_url(
        "https://example.com/store-fail",
        ExtractionResult {
            nodes: vec![tension_at("Signal That Fails To Store", 44.975, -93.270)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new().failing_creates());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://example.com/store-fail");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    // Should not panic even when store.create_node fails
    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 1,
        "signal event is still emitted (store failure only affects projection)"
    );
}

#[tokio::test]
async fn blocked_url_produces_no_signals() {
    // URL is pre-blocked in the store. Pipeline should skip it entirely.
    // Register a page + extractor that WOULD produce a signal — but it should
    // never be reached because the URL is blocked before fetching.
    let fetcher = MockFetcher::new().on_page(
        "https://spam-site.org/page",
        archived_page("https://spam-site.org/page", "Spam content"),
    );

    let extractor = MockExtractor::new().on_url(
        "https://spam-site.org/page",
        ExtractionResult {
            nodes: vec![tension_at("Spam Signal", 44.975, -93.270)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new().block_url("spam-site.org"));
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://spam-site.org/page");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(ctx.stats.signals_stored, 0, "blocked URL → zero signals");
}

// ---------------------------------------------------------------------------
// Edge case tests — probing corners of the pipeline logic
// ---------------------------------------------------------------------------

#[tokio::test]
async fn all_signal_types_are_stored() {
    // Verify non-Tension/Need node types are stored correctly.
    let fetcher = MockFetcher::new().on_page(
        "https://example.com/mixed-types",
        archived_page("https://example.com/mixed-types", "# Mixed signal types"),
    );

    let extractor = MockExtractor::new().on_url(
        "https://example.com/mixed-types",
        ExtractionResult {
            nodes: vec![
                gathering_at("Community Potluck", 44.975, -93.270),
                aid_at("Free Legal Clinic", 44.960, -93.265),
                notice_at("Park Closure Notice", 44.950, -93.260),
            ],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://example.com/mixed-types");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 3,
        "all 3 node types should be created"
    );
}

#[tokio::test]
async fn unicode_and_emoji_titles_are_preserved() {
    let fetcher = MockFetcher::new().on_page(
        "https://example.com/unicode",
        archived_page(
            "https://example.com/unicode",
            "# Événements communautaires 🎉",
        ),
    );

    let extractor = MockExtractor::new().on_url(
        "https://example.com/unicode",
        ExtractionResult {
            nodes: vec![
                tension_at("Événements communautaires 🎉", 44.975, -93.270),
                tension_at("日本語のタイトル", 44.960, -93.265),
            ],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://example.com/unicode");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(ctx.stats.signals_stored, 2);
}

#[tokio::test]
async fn signal_at_zero_zero_is_still_stored() {
    // Coords (0.0, 0.0) — no geo-filter, so even null island signals are stored.
    let fetcher = MockFetcher::new().on_page(
        "https://example.com/null-island",
        archived_page("https://example.com/null-island", "# Null island"),
    );

    let extractor = MockExtractor::new().on_url(
        "https://example.com/null-island",
        ExtractionResult {
            nodes: vec![tension_at("Null Island Signal", 0.0, 0.0)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://example.com/null-island");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 1,
        "null island signal is stored (no geo-filter)"
    );
}

#[tokio::test]
async fn broken_extraction_skips_page_gracefully() {
    // Page fetches fine, but extractor returns Err for the URL.
    let fetcher = MockFetcher::new().on_page(
        "https://example.com/extract-fail",
        archived_page("https://example.com/extract-fail", "Valid content here"),
    );

    // MockExtractor has no URL registered → returns Err
    let extractor = MockExtractor::new();

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://example.com/extract-fail");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 0,
        "extractor error → no signals, no panic"
    );
}

#[tokio::test]
async fn blank_author_name_does_not_create_actor() {
    // author_actor = Some("  ") should be treated as empty and not create an actor.
    let fetcher = MockFetcher::new().on_page(
        "https://example.com/ws-author",
        archived_page("https://example.com/ws-author", "# Content"),
    );

    let node = tension_at("Signal With Blank Author", 44.975, -93.270);
    // NOTE: author_actor field removed from NodeMeta; blank-author path is now a no-op.

    let extractor = MockExtractor::new().on_url(
        "https://example.com/ws-author",
        ExtractionResult {
            nodes: vec![node],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://example.com/ws-author");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 1,
        "signal should still be created"
    );
}

#[tokio::test]
async fn signal_with_resource_needs_gets_resource_edge() {
    // Verify that resource_tags in ExtractionResult flow through to resource edges.
    let fetcher = MockFetcher::new().on_page(
        "https://example.com/resources",
        archived_page("https://example.com/resources", "# Needs vehicles"),
    );

    let node = tension_at("Need Drivers", 44.975, -93.270);
    let node_id = node.meta().unwrap().id;

    let extractor = MockExtractor::new().on_url(
        "https://example.com/resources",
        ExtractionResult {
            nodes: vec![node],
            implied_queries: vec![],
            resource_tags: vec![(
                node_id,
                vec![ResourceTag {
                    slug: "vehicle".to_string(),
                    role: ResourceRole::Requires,
                    confidence: 0.9,
                    context: Some("pickup truck".to_string()),
                }],
            )],
            signal_tags: vec![(
                node_id,
                vec!["mutual-aid".to_string(), "transportation".to_string()],
            )],
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://example.com/resources");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(ctx.stats.signals_stored, 1);
}

#[tokio::test]
async fn zero_sources_produces_nothing() {
    let fetcher = MockFetcher::new();
    let extractor = MockExtractor::new();
    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let sources: Vec<&SourceNode> = vec![];
    let dummy_source = page_source("https://dummy.org");
    let mut ctx = PipelineState::from_sources(&[dummy_source]);

    // Should not panic with empty sources
    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;
    assert_eq!(ctx.stats.signals_stored, 0);
}

#[tokio::test]
async fn outbound_links_collected_despite_extraction_failure() {
    // Page has outbound links, but extractor fails. Links should still be collected.
    let mut page = archived_page("https://example.com/links-but-error", "Content");
    page.links = vec![
        "https://partner-a.org/events".to_string(),
        "https://partner-b.org/programs".to_string(),
    ];

    let fetcher = MockFetcher::new().on_page("https://example.com/links-but-error", page);

    // No extractor mapping → returns Err
    let extractor = MockExtractor::new();

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://example.com/links-but-error");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 0,
        "no signals from failed extraction"
    );
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

    let fetcher = MockFetcher::new().on_posts(ig_url, vec![]); // zero posts

    let extractor = MockExtractor::new();

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::social_scrape::scrape_social_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts, &causal::Logger::new()).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(ctx.stats.signals_stored, 0, "zero posts → no signals");
}

#[tokio::test]
async fn image_only_posts_produce_no_signals() {
    // Posts exist but have None text → combined_text is empty → early return.
    let ig_url = "https://www.instagram.com/image_only";

    let mut post = test_post("");
    post.text = None; // image-only post

    let fetcher = MockFetcher::new().on_posts(ig_url, vec![post]);

    let extractor = MockExtractor::new();

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::social_scrape::scrape_social_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts, &causal::Logger::new()).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(ctx.stats.signals_stored, 0, "text-less posts → no signals");
}

// NOTE: Test `empty_mentioned_actor_name_is_not_created` was removed.
// Mentioned actors no longer create Actor nodes at all.

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

    let fetcher = MockFetcher::new().on_page("https://example.com/empty-md", page);

    let extractor = MockExtractor::new();

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://example.com/empty-md");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 0,
        "no signals from empty markdown"
    );
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
        .on_page("https://empty.org/page", {
            let mut p = archived_page("https://empty.org/page", "");
            p.markdown = String::new();
            p
        });
    // https://fail.org/page is NOT registered → returns Err

    let extractor = MockExtractor::new().on_url(
        "https://good.org/events",
        ExtractionResult {
            nodes: vec![tension_at("Community Dinner", 44.975, -93.270)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let s1 = page_source("https://good.org/events");
    let s2 = page_source("https://empty.org/page");
    let s3 = page_source("https://fail.org/page");
    let all = vec![s1.clone(), s2.clone(), s3.clone()];
    let sources: Vec<&SourceNode> = vec![&s1, &s2, &s3];
    let mut ctx = PipelineState::from_sources(&all);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 1,
        "only the good page produces a signal"
    );
}

#[tokio::test]
async fn social_scrape_failure_does_not_crash() {
    // Social source fetcher returns Err → no panic, no signals.
    let ig_url = "https://www.instagram.com/broken_account";

    // MockFetcher has no posts registered → returns Err
    let fetcher = MockFetcher::new();
    let extractor = MockExtractor::new();

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    // Should not panic
    let output = super::activities::social_scrape::scrape_social_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts, &causal::Logger::new()).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 0,
        "social fetch error → no signals"
    );
}

#[tokio::test]
async fn batch_title_dedup_is_case_insensitive() {
    // "Housing Crisis" and "housing crisis" should be deduped to one signal.
    let fetcher = MockFetcher::new().on_page(
        "https://example.com/case-dedup",
        archived_page("https://example.com/case-dedup", "# Case dedup test"),
    );

    let extractor = MockExtractor::new().on_url(
        "https://example.com/case-dedup",
        ExtractionResult {
            nodes: vec![
                tension_at("Housing Crisis", 44.975, -93.270),
                tension_at("housing crisis", 44.960, -93.265),
                tension_at("HOUSING CRISIS", 44.950, -93.260),
                tension_at("Different Signal", 44.940, -93.255),
            ],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://example.com/case-dedup");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 2,
        "case-insensitive dedup should produce 2 signals"
    );
}

// ---------------------------------------------------------------------------
// Location metadata through the full pipeline
//
// Verify about_location and from_location survive into StoredSignal.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn web_source_without_actor_stores_content_location_only() {
    let fetcher = MockFetcher::new().on_page(
        "https://localorg.org/events",
        archived_page("https://localorg.org/events", "# Event at Powderhorn"),
    );

    let extractor = MockExtractor::new().on_url(
        "https://localorg.org/events",
        ExtractionResult {
            nodes: vec![tension_at("Powderhorn Cleanup", 44.9489, -93.2583)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://localorg.org/events");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;
}

#[tokio::test]
async fn signal_without_content_location_does_not_backfill_from_actor() {

    let ig_url = "https://www.instagram.com/localorg";

    let fetcher =
        MockFetcher::new().on_posts(ig_url, vec![test_post("Thoughts on community organizing")]);

    // Signal with NO about_location
    let extractor = MockExtractor::new().on_url(
        ig_url,
        ExtractionResult {
            nodes: vec![tension("Community Organizing Thoughts")],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    ctx.actor_contexts.insert(
        canonical_value(ig_url),
        rootsignal_common::ActorContext {
            actor_name: "Local Org".to_string(),
            bio: None,
            location_name: Some("Minneapolis".to_string()),
            location_lat: Some(44.9778),
            location_lng: Some(-93.2650),
            discovery_depth: 0,
        },
    );

    let output = super::activities::social_scrape::scrape_social_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts, &causal::Logger::new()).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;
}

#[tokio::test]
async fn explicit_content_location_not_overwritten_by_actor() {

    let ig_url = "https://www.instagram.com/nycorg";

    let fetcher = MockFetcher::new().on_posts(ig_url, vec![test_post("Great event in St Paul!")]);

    // Signal explicitly located in St Paul
    let extractor = MockExtractor::new().on_url(
        ig_url,
        ExtractionResult {
            nodes: vec![tension_at("St Paul Event", ST_PAUL.0, ST_PAUL.1)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    // Actor in Minneapolis — should NOT overwrite St Paul about_location
    ctx.actor_contexts.insert(
        canonical_value(ig_url),
        rootsignal_common::ActorContext {
            actor_name: "Minneapolis Org".to_string(),
            bio: None,
            location_name: None,
            location_lat: Some(44.9778),
            location_lng: Some(-93.2650),
            discovery_depth: 0,
        },
    );

    let output = super::activities::social_scrape::scrape_social_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts, &causal::Logger::new()).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;
}

// ---------------------------------------------------------------------------
// Discovery depth inheritance
//
// Actors discovered from a source inherit parent_depth + 1.
// Bootstrap actors (no actor context) get depth 0.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn new_actor_inherits_parent_depth_plus_one() {

    let ig_url = "https://www.instagram.com/depthorg";

    let fetcher = MockFetcher::new().on_posts(ig_url, vec![test_post("Depth test post")]);

    let node = tension_at("Depth Signal", 44.9778, -93.2650);
    let node_id = node.meta().unwrap().id;

    let extractor = MockExtractor::new().on_url(
        ig_url,
        ExtractionResult {
            nodes: vec![node],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: vec![(node_id, "Depth Child Org".to_string())],
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    // Parent actor at depth 1
    ctx.actor_contexts.insert(
        canonical_value(ig_url),
        rootsignal_common::ActorContext {
            actor_name: "Depth Org".to_string(),
            bio: None,
            location_name: Some("Minneapolis".to_string()),
            location_lat: Some(44.9778),
            location_lng: Some(-93.2650),
            discovery_depth: 1,
        },
    );

    let output = super::activities::social_scrape::scrape_social_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts, &causal::Logger::new()).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 1,
        "signal should be created for discovered actor"
    );
}

#[tokio::test]
async fn bootstrap_actor_gets_depth_zero() {
    let ig_url = "https://www.instagram.com/bootstraporg";

    let fetcher = MockFetcher::new().on_posts(ig_url, vec![test_post("Bootstrap post")]);

    let node = tension_at("Bootstrap Signal", 44.9778, -93.2650);
    let node_id = node.meta().unwrap().id;

    let extractor = MockExtractor::new().on_url(
        ig_url,
        ExtractionResult {
            nodes: vec![node],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: vec![(node_id, "Bootstrap Org".to_string())],
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);
    // No actor context — this is a bootstrap source

    let output = super::activities::social_scrape::scrape_social_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts, &causal::Logger::new()).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 1,
        "bootstrap actor signal should be created"
    );
}

// ---------------------------------------------------------------------------
// Content date fallback
//
// RSS pub_date and social published_at flow into published_at when the
// LLM didn't extract one.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rss_pub_date_becomes_published_at_when_llm_omits_it() {

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

    let fetcher = MockFetcher::new().on_feed(feed_url, feed).on_page(
        article_url,
        archived_page(article_url, "# Community event recap"),
    );

    // Extractor returns signal with NO published_at
    let extractor = MockExtractor::new().on_url(
        article_url,
        ExtractionResult {
            nodes: vec![tension_at("Community Event Recap", 44.975, -93.270)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source(feed_url);
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;
}

#[tokio::test]
async fn llm_published_at_not_overwritten_by_rss_pub_date() {

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

    // Extractor returns signal WITH an explicit published_at
    let mut node = tension_at("Upcoming Event", 44.975, -93.270);
    if let Some(meta) = node.meta_mut() {
        meta.published_at = Some(llm_date);
    }

    let extractor = MockExtractor::new().on_url(
        article_url,
        ExtractionResult {
            nodes: vec![node],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source(feed_url);
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;
}

#[tokio::test]
async fn social_published_at_becomes_published_at_fallback() {

    let ig_url = "https://www.instagram.com/localorg";
    let post_date = chrono::Utc
        .with_ymd_and_hms(2026, 2, 15, 18, 30, 0)
        .unwrap();

    let mut post = test_post("Big community event coming up!");
    post.published_at = Some(post_date);

    let fetcher = MockFetcher::new().on_posts(ig_url, vec![post]);

    // Signal with NO published_at
    let extractor = MockExtractor::new().on_url(
        ig_url,
        ExtractionResult {
            nodes: vec![tension("Big Community Event")],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::social_scrape::scrape_social_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts, &causal::Logger::new()).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;
}

// ---------------------------------------------------------------------------
// Edge case: Ecological signals at ocean/non-land coordinates
//
// Principle #11: "Life, Not Just People" — ecological signal is first-class.
// Oil spill in the Pacific, reef damage, etc. should store fine.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ocean_coordinates_store_ecological_signal() {
    // Mid-Pacific oil spill at (-15.0, -170.0) — valid ecological signal, no land
    let fetcher = MockFetcher::new().on_page(
        "https://news.org/oil-spill",
        archived_page(
            "https://news.org/oil-spill",
            "# Pacific Oil Spill Emergency",
        ),
    );

    let extractor = MockExtractor::new().on_url(
        "https://news.org/oil-spill",
        ExtractionResult {
            nodes: vec![
                tension_at("Pacific Oil Spill Threatening Coral Reef", -15.0, -170.0),
                need_at("Volunteer Boats Needed for Cleanup", -15.1, -170.1),
            ],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://news.org/oil-spill");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 2,
        "ocean-coordinate ecological signals should be stored"
    );
}

#[tokio::test]
async fn antarctic_coordinates_store_signal() {
    // Research station environmental monitoring at Antarctica
    let fetcher = MockFetcher::new().on_page(
        "https://science.org/antarctic",
        archived_page(
            "https://science.org/antarctic",
            "# Antarctic ice shelf collapse",
        ),
    );

    let extractor = MockExtractor::new().on_url(
        "https://science.org/antarctic",
        ExtractionResult {
            nodes: vec![tension_at(
                "Ice Shelf Collapse Accelerating",
                -77.85,
                166.67,
            )],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://science.org/antarctic");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 1,
        "Antarctic signal should be stored"
    );
}

// ---------------------------------------------------------------------------
// Edge case: Out-of-bounds coordinates (LLM hallucination)
//
// Kill Test #5: Geo-localization failures. lat=999 is physically impossible.
// Pipeline should still store the signal — bad coords don't crash anything.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn out_of_bounds_coordinates_do_not_crash_pipeline() {
    let fetcher = MockFetcher::new().on_page(
        "https://example.com/hallucinated-geo",
        archived_page(
            "https://example.com/hallucinated-geo",
            "# Hallucinated location",
        ),
    );

    let extractor = MockExtractor::new().on_url(
        "https://example.com/hallucinated-geo",
        ExtractionResult {
            nodes: vec![tension_at("Signal With Impossible Coords", 999.0, -999.0)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://example.com/hallucinated-geo");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    // Pipeline must not panic on absurd coordinates
    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    // Signal is stored — we don't validate coordinate ranges at pipeline level.
    // Downstream display/query layers are responsible for geo-bounds checks.
    assert_eq!(
        ctx.stats.signals_stored, 1,
        "out-of-bounds coords should not crash pipeline"
    );
}

// ---------------------------------------------------------------------------
// Edge case: Environmental disaster — full signal-type spectrum
//
// Crisis scenario produces Tension + Need + Aid + Gathering from same URL.
// All types should flow through and be stored.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn environmental_disaster_produces_all_signal_types() {
    let fetcher = MockFetcher::new().on_page(
        "https://news.org/hurricane-response",
        archived_page(
            "https://news.org/hurricane-response",
            "# Hurricane Response Underway",
        ),
    );

    let extractor = MockExtractor::new().on_url(
        "https://news.org/hurricane-response",
        ExtractionResult {
            nodes: vec![
                tension_at("Category 4 Hurricane Hits Gulf Coast", 29.95, -90.07),
                need_at("Emergency Blood Donations Needed", 29.96, -90.08),
                aid_at("Red Cross Shelter Open at Convention Center", 29.94, -90.06),
                gathering_at("Volunteer Deployment Briefing 8AM Tomorrow", 29.97, -90.05),
                notice_at("Mandatory Evacuation Order Zone A", 29.93, -90.09),
            ],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://news.org/hurricane-response");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 5,
        "all 5 signal types should be stored in crisis"
    );
}

// ---------------------------------------------------------------------------
// Edge case: Hallucinated dates
//
// Kill Test #3: Extraction hallucinations. Future and epoch dates.
// Pipeline should not crash or reject — dates are metadata, not filters.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn hallucinated_future_date_does_not_crash() {

    let fetcher = MockFetcher::new().on_page(
        "https://example.com/future-date",
        archived_page("https://example.com/future-date", "# Far future event"),
    );

    let mut node = tension_at("Signal From Year 2099", 44.975, -93.270);
    if let Some(meta) = node.meta_mut() {
        meta.published_at = Some(
            chrono::Utc
                .with_ymd_and_hms(2099, 12, 31, 23, 59, 59)
                .unwrap(),
        );
    }

    let extractor = MockExtractor::new().on_url(
        "https://example.com/future-date",
        ExtractionResult {
            nodes: vec![node],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://example.com/future-date");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 1,
        "future date should not prevent storage"
    );
}

#[tokio::test]
async fn epoch_zero_date_does_not_crash() {

    let fetcher = MockFetcher::new().on_page(
        "https://example.com/epoch-date",
        archived_page("https://example.com/epoch-date", "# Epoch date"),
    );

    let mut node = tension_at("Signal With Epoch Date", 44.975, -93.270);
    if let Some(meta) = node.meta_mut() {
        meta.published_at = Some(chrono::Utc.with_ymd_and_hms(1970, 1, 1, 0, 0, 0).unwrap());
    }

    let extractor = MockExtractor::new().on_url(
        "https://example.com/epoch-date",
        ExtractionResult {
            nodes: vec![node],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://example.com/epoch-date");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 1,
        "epoch date should not prevent storage"
    );
}

// ---------------------------------------------------------------------------
// Edge case: Extremely long title (LLM hallucination)
//
// Kill Test #3: LLM sometimes outputs paragraph-length titles.
// Pipeline should handle without panic or truncation crash.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn extremely_long_title_survives_pipeline() {
    let long_title = "A".repeat(2000);

    let fetcher = MockFetcher::new().on_page(
        "https://example.com/long-title",
        archived_page("https://example.com/long-title", "# Long title page"),
    );

    let extractor = MockExtractor::new().on_url(
        "https://example.com/long-title",
        ExtractionResult {
            nodes: vec![tension_at(&long_title, 44.975, -93.270)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://example.com/long-title");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 1,
        "long title should not crash pipeline"
    );
}

// ---------------------------------------------------------------------------
// Edge case: Cross-source corroboration
//
// Kill Test #4: Same signal from two different sources.
// First source creates, second source corroborates.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn same_signal_from_two_sources_corroborates() {
    let fetcher = MockFetcher::new()
        .on_page(
            "https://source-a.org/article",
            archived_page("https://source-a.org/article", "# Housing Crisis Report"),
        )
        .on_page(
            "https://source-b.org/story",
            archived_page("https://source-b.org/story", "# Housing Crisis Coverage"),
        );

    // Both sources extract a signal with the SAME title and type
    let extractor = MockExtractor::new()
        .on_url(
            "https://source-a.org/article",
            ExtractionResult {
                nodes: vec![tension_at("Housing Crisis in Uptown", 44.948, -93.298)],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
                raw_signal_count: 0,
                rejected: Vec::new(),
                schedules: Vec::new(),
                author_actors: Vec::new(),
                categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
            },
        )
        .on_url(
            "https://source-b.org/story",
            ExtractionResult {
                nodes: vec![tension_at("Housing Crisis in Uptown", 44.949, -93.297)],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
                raw_signal_count: 0,
                rejected: Vec::new(),
                schedules: Vec::new(),
                author_actors: Vec::new(),
                categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
            },
        );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    // Process source A first
    let source_a = page_source("https://source-a.org/article");
    let sources_a: Vec<&SourceNode> = vec![&source_a];
    let mut ctx = PipelineState::from_sources(&[source_a.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources_a, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(ctx.stats.signals_stored, 1, "first source creates signal");

    // Process source B — should corroborate, not duplicate
    let source_b = page_source("https://source-b.org/story");
    let sources_b: Vec<&SourceNode> = vec![&source_b];
    let mut ctx2 = PipelineState::from_sources(&[source_b.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources_b, &ctx2.url_to_canonical_key, &ctx2.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx2, &store).await;

    assert_eq!(
        ctx2.stats.signals_stored, 1,
        "second source should corroborate (counted as stored)"
    );
}

// ---------------------------------------------------------------------------
// Edge case: Mixed social posts (some text, some image-only, some empty)
//
// IG account with diverse post types. Pipeline should extract from
// text posts and gracefully skip image-only/empty posts.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mixed_text_and_image_posts_produce_correct_signals() {
    let ig_url = "https://www.instagram.com/community_org";

    let mut text_post_1 = test_post("Community cleanup at Lake Harriet this Saturday!");
    text_post_1.published_at = Some(chrono::Utc::now());

    let mut text_post_2 = test_post("Volunteers needed for food shelf restocking");
    text_post_2.published_at = Some(chrono::Utc::now());

    let mut image_only_1 = test_post("");
    image_only_1.text = None; // pure image post

    let mut image_only_2 = test_post("");
    image_only_2.text = None; // another image post

    let empty_text = test_post(""); // empty string text

    let fetcher = MockFetcher::new().on_posts(
        ig_url,
        vec![
            text_post_1,
            text_post_2,
            image_only_1,
            image_only_2,
            empty_text,
        ],
    );

    // Extractor sees combined text of the text posts (image-only posts have None text)
    let extractor = MockExtractor::new().on_url(
        ig_url,
        ExtractionResult {
            nodes: vec![
                gathering_at("Lake Harriet Cleanup", 44.921, -93.306),
                need_at("Food Shelf Volunteers Needed", 44.948, -93.280),
            ],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::social_scrape::scrape_social_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts, &causal::Logger::new()).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 2,
        "only text posts should produce signals"
    );
}

// ---------------------------------------------------------------------------
// Edge case: Minimum viable signal (no location, no action URL, no date)
//
// Signal with just a title and summary — bare minimum from LLM.
// System should still store it, not reject it.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn minimum_viable_signal_with_no_optional_fields() {
    let fetcher = MockFetcher::new().on_page(
        "https://example.com/bare-signal",
        archived_page("https://example.com/bare-signal", "# Bare signal"),
    );

    // tension() creates a node with no location (vs tension_at which has coords)
    let extractor = MockExtractor::new().on_url(
        "https://example.com/bare-signal",
        ExtractionResult {
            nodes: vec![tension("Community Tension Without Details")],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://example.com/bare-signal");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 1,
        "bare signal should still be stored"
    );
}

// ---------------------------------------------------------------------------
// Group B: Actor creation on owned sources
//
// Social accounts and web pages are "owned" — the account holder IS the actor.
// Aggregator sources (RSS, web query) do not create actor nodes.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn owned_source_author_creates_actor_with_url_canonical_key() {
    let ig_url = "https://www.instagram.com/friendsfalls";

    let fetcher =
        MockFetcher::new().on_posts(ig_url, vec![test_post("Waterfall cleanup this Saturday!")]);

    let node = tension_at("Falls Cleanup Day", 44.92, -93.21);
    let node_id = node.meta().unwrap().id;

    let extractor = MockExtractor::new().on_url(
        ig_url,
        ExtractionResult {
            nodes: vec![node],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: vec![(node_id, "Friends of the Falls".to_string())],
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::social_scrape::scrape_social_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts, &causal::Logger::new()).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 1,
        "signal should be created for owned source"
    );
}

#[tokio::test]
async fn aggregator_source_author_does_not_create_actor_node() {
    let fetcher = MockFetcher::new().on_page(
        "https://aggregator.com/news",
        archived_page("https://aggregator.com/news", "# Local News Roundup"),
    );

    let node = tension_at("Community Event Coverage", 44.975, -93.270);

    let extractor = MockExtractor::new().on_url(
        "https://aggregator.com/news",
        ExtractionResult {
            nodes: vec![node],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://aggregator.com/news");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;
    assert_eq!(ctx.stats.signals_stored, 1, "signal should still be stored");
}

// ---------------------------------------------------------------------------
// Group C: Mentioned entities no longer create nodes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mentioned_entities_do_not_create_actor_nodes() {
    let fetcher = MockFetcher::new().on_page(
        "https://example.com/mentions",
        archived_page("https://example.com/mentions", "# Article with mentions"),
    );

    let node = tension_at("Free Legal Clinic", 44.975, -93.270);

    let extractor = MockExtractor::new().on_url(
        "https://example.com/mentions",
        ExtractionResult {
            nodes: vec![node],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://example.com/mentions");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 1,
        "signal should still be stored with mentions in metadata"
    );
}

// ---------------------------------------------------------------------------
// Group D: PRODUCED_BY edge (Signal → Source)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn signal_has_produced_by_edge_to_its_source() {
    let fetcher = MockFetcher::new().on_page(
        "https://localorg.org/events",
        archived_page("https://localorg.org/events", "# Community Events"),
    );

    let extractor = MockExtractor::new().on_url(
        "https://localorg.org/events",
        ExtractionResult {
            nodes: vec![tension_at("Community Dinner", 44.975, -93.270)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://localorg.org/events");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(ctx.stats.signals_stored, 1);
}

#[tokio::test]
async fn social_signal_has_produced_by_edge() {
    let ig_url = "https://www.instagram.com/communityorg";

    let fetcher =
        MockFetcher::new().on_posts(ig_url, vec![test_post("Park cleanup this weekend!")]);

    let extractor = MockExtractor::new().on_url(
        ig_url,
        ExtractionResult {
            nodes: vec![tension_at("Park Cleanup", 44.95, -93.26)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::social_scrape::scrape_social_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts, &causal::Logger::new()).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(ctx.stats.signals_stored, 1);
}

// ---------------------------------------------------------------------------
// Actor location enrichment — boundary tests
//
// MOCK → triangulate_all_actors → OUTPUT
// ---------------------------------------------------------------------------

/// Wrapper that calls triangulate_all_actors directly.
/// Returns the updates vec (len = updated count) for inspection.
async fn enrich_with_sink(
    store: &dyn SignalReader,
    actors: &[(
        rootsignal_common::ActorNode,
        Vec<rootsignal_common::SourceNode>,
    )],
) -> (u32, Vec<ActorLocationUpdate>) {
    let updates = triangulate_all_actors(store, actors).await;
    let count = updates.len() as u32;
    (count, updates)
}

/// Wrapper for tests that only need the updated count.
async fn enrich_with_engine(
    store: &dyn SignalReader,
    actors: &[(
        rootsignal_common::ActorNode,
        Vec<rootsignal_common::SourceNode>,
    )],
) -> u32 {
    let (updated, _) = enrich_with_sink(store, actors).await;
    updated
}

/// Extract the location name for an actor from updates.
fn dispatched_location_name(
    updates: &[ActorLocationUpdate],
    actor_id: uuid::Uuid,
) -> Option<String> {
    updates.iter().find_map(|u| {
        if u.actor_id == actor_id {
            Some(u.name.clone().unwrap_or_default())
        } else {
            None
        }
    })
}

/// Extract the location coordinates for an actor from updates.
fn dispatched_location_coords(
    updates: &[ActorLocationUpdate],
    actor_id: uuid::Uuid,
) -> Option<(f64, f64)> {
    updates.iter().find_map(|u| {
        if u.actor_id == actor_id {
            Some((u.lat, u.lng))
        } else {
            None
        }
    })
}

/// Phillips neighborhood coordinates.
const PHILLIPS: (f64, f64) = (44.9489, -93.2601);
/// Powderhorn neighborhood coordinates.
const POWDERHORN: (f64, f64) = (44.9367, -93.2393);
/// Whittier neighborhood coordinates.
const WHITTIER: (f64, f64) = (44.9505, -93.2776);

fn test_actor(name: &str) -> rootsignal_common::ActorNode {
    rootsignal_common::ActorNode {
        id: Uuid::new_v4(),
        name: name.to_string(),
        actor_type: ActorType::Organization,
        canonical_key: format!("{}-entity", name.to_lowercase().replace(' ', "-")),
        domains: vec![],
        social_urls: vec![],
        description: String::new(),
        signal_count: 0,
        first_seen: Utc::now(),
        last_active: Utc::now(),
        typical_roles: vec![],
        bio: None,
        location_lat: None,
        location_lng: None,
        location_name: None,
        external_url: None,
        discovery_depth: 0,
    }
}

fn test_actor_at(name: &str, lat: f64, lng: f64, loc_name: &str) -> rootsignal_common::ActorNode {
    let mut actor = test_actor(name);
    actor.location_lat = Some(lat);
    actor.location_lng = Some(lng);
    actor.location_name = Some(loc_name.to_string());
    actor
}

/// Create a tension node with about_location and about_location_name set.
fn tension_at_named(title: &str, lat: f64, lng: f64, loc_name: &str) -> rootsignal_common::Node {
    let mut node = tension_at(title, lat, lng);
    if let Some(meta) = node.meta_mut() {
        if let Some(loc) = meta.locations.first_mut() {
            loc.name = Some(loc_name.to_string());
        }
    }
    node
}

/// Seed a signal linked to an actor and return the signal ID.
async fn seed_signal(
    store: &MockSignalReader,
    actor_id: Uuid,
    title: &str,
    lat: f64,
    lng: f64,
    loc_name: &str,
) -> Uuid {
    let node = tension_at_named(title, lat, lng, loc_name);
    let sig_id = store
        .create_node(&node, &[0.1], "test", "run-1")
        .await
        .unwrap();
    store
        .link_actor_to_signal(actor_id, sig_id, "authored")
        .await
        .unwrap();
    sig_id
}

// ---------------------------------------------------------------------------
// Happy path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn enrichment_updates_actor_to_signal_mode_location() {
    let store = Arc::new(MockSignalReader::new());
    let actor = test_actor("Northside Collective");
    store.upsert_actor(&actor).await.unwrap();

    // 2× Phillips, 1× Powderhorn → Phillips wins
    seed_signal(
        &store, actor.id, "Signal A", PHILLIPS.0, PHILLIPS.1, "Phillips",
    )
    .await;
    seed_signal(
        &store, actor.id, "Signal B", PHILLIPS.0, PHILLIPS.1, "Phillips",
    )
    .await;
    seed_signal(
        &store,
        actor.id,
        "Signal C",
        POWDERHORN.0,
        POWDERHORN.1,
        "Powderhorn",
    )
    .await;

    let actor_id = actor.id;
    let actors = vec![(actor, vec![])];
    let (updated, captured) = enrich_with_sink(&*store, &actors).await;

    assert_eq!(updated, 1, "one actor should have been updated");
    assert_eq!(
        dispatched_location_name(&captured, actor_id),
        Some("Phillips".to_string()),
        "actor should be placed in Phillips (mode of signals)"
    );
}

// ---------------------------------------------------------------------------
// Minimum evidence threshold
// ---------------------------------------------------------------------------

#[tokio::test]
async fn enrichment_single_signal_insufficient_to_set_location() {
    let store = Arc::new(MockSignalReader::new());
    let actor = test_actor("Lone Signal Org");
    store.upsert_actor(&actor).await.unwrap();

    seed_signal(
        &store,
        actor.id,
        "Only Signal",
        PHILLIPS.0,
        PHILLIPS.1,
        "Phillips",
    )
    .await;

    let actors = vec![(actor, vec![])];
    let updated = enrich_with_engine(&*store, &actors).await;

    assert_eq!(
        updated, 0,
        "one signal is not enough evidence to set location"
    );
    assert_eq!(
        store.actor_location_name("Lone Signal Org"),
        None,
        "actor location should remain unset"
    );
}

// ---------------------------------------------------------------------------
// No signals
// ---------------------------------------------------------------------------

#[tokio::test]
async fn enrichment_no_signals_leaves_actor_unchanged() {
    let store = Arc::new(MockSignalReader::new());
    let actor = test_actor_at("Rooted Org", PHILLIPS.0, PHILLIPS.1, "Phillips");
    store.upsert_actor(&actor).await.unwrap();

    // No signals linked — actor should keep its current location
    let actors = vec![(actor, vec![])];
    let updated = enrich_with_engine(&*store, &actors).await;

    assert_eq!(updated, 0, "no signals means no change");
    assert_eq!(
        store.actor_location_name("Rooted Org"),
        Some("Phillips".to_string()),
        "existing location should be preserved"
    );
}

// ---------------------------------------------------------------------------
// Empty actor list
// ---------------------------------------------------------------------------

#[tokio::test]
async fn enrichment_empty_actor_list_returns_zero() {
    let store = Arc::new(MockSignalReader::new());

    let updated = enrich_with_engine(&*store, &[]).await;

    assert_eq!(updated, 0);
}

// ---------------------------------------------------------------------------
// Tie preserves inertia
// ---------------------------------------------------------------------------

#[tokio::test]
async fn enrichment_tie_preserves_current_location() {
    let store = Arc::new(MockSignalReader::new());
    let actor = test_actor_at("Tied Org", PHILLIPS.0, PHILLIPS.1, "Phillips");
    store.upsert_actor(&actor).await.unwrap();

    // 2× Phillips, 2× Powderhorn → tie → keep Phillips (inertia)
    seed_signal(&store, actor.id, "T1", PHILLIPS.0, PHILLIPS.1, "Phillips").await;
    seed_signal(&store, actor.id, "T2", PHILLIPS.0, PHILLIPS.1, "Phillips").await;
    seed_signal(
        &store,
        actor.id,
        "T3",
        POWDERHORN.0,
        POWDERHORN.1,
        "Powderhorn",
    )
    .await;
    seed_signal(
        &store,
        actor.id,
        "T4",
        POWDERHORN.0,
        POWDERHORN.1,
        "Powderhorn",
    )
    .await;

    let actors = vec![(actor, vec![])];
    let updated = enrich_with_engine(&*store, &actors).await;

    assert_eq!(updated, 0, "tie should not change location");
    assert_eq!(
        store.actor_location_name("Tied Org"),
        Some("Phillips".to_string()),
        "inertia keeps Phillips on a tie"
    );
}

// ---------------------------------------------------------------------------
// Idempotent — already-correct location
// ---------------------------------------------------------------------------

#[tokio::test]
async fn enrichment_does_not_update_when_location_already_matches() {
    let store = Arc::new(MockSignalReader::new());
    let actor = test_actor_at("Stable Org", PHILLIPS.0, PHILLIPS.1, "Phillips");
    store.upsert_actor(&actor).await.unwrap();

    // 3× Phillips — mode is Phillips, actor already at Phillips → no-op
    seed_signal(&store, actor.id, "S1", PHILLIPS.0, PHILLIPS.1, "Phillips").await;
    seed_signal(&store, actor.id, "S2", PHILLIPS.0, PHILLIPS.1, "Phillips").await;
    seed_signal(&store, actor.id, "S3", PHILLIPS.0, PHILLIPS.1, "Phillips").await;

    let actors = vec![(actor, vec![])];
    let updated = enrich_with_engine(&*store, &actors).await;

    assert_eq!(updated, 0, "location already correct — no update needed");
}

// ---------------------------------------------------------------------------
// Multiple actors — independent triangulation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn enrichment_processes_each_actor_independently() {
    let store = Arc::new(MockSignalReader::new());

    let actor_a = test_actor("Actor Alpha");
    let actor_b = test_actor("Actor Beta");
    store.upsert_actor(&actor_a).await.unwrap();
    store.upsert_actor(&actor_b).await.unwrap();

    // Alpha → Phillips (2 signals)
    seed_signal(&store, actor_a.id, "A1", PHILLIPS.0, PHILLIPS.1, "Phillips").await;
    seed_signal(&store, actor_a.id, "A2", PHILLIPS.0, PHILLIPS.1, "Phillips").await;

    // Beta → Powderhorn (3 signals)
    seed_signal(
        &store,
        actor_b.id,
        "B1",
        POWDERHORN.0,
        POWDERHORN.1,
        "Powderhorn",
    )
    .await;
    seed_signal(
        &store,
        actor_b.id,
        "B2",
        POWDERHORN.0,
        POWDERHORN.1,
        "Powderhorn",
    )
    .await;
    seed_signal(
        &store,
        actor_b.id,
        "B3",
        POWDERHORN.0,
        POWDERHORN.1,
        "Powderhorn",
    )
    .await;

    let actor_a_id = actor_a.id;
    let actor_b_id = actor_b.id;
    let actors = vec![(actor_a, vec![]), (actor_b, vec![])];
    let (updated, captured) = enrich_with_sink(&*store, &actors).await;

    assert_eq!(updated, 2, "both actors should be updated");
    assert_eq!(
        dispatched_location_name(&captured, actor_a_id),
        Some("Phillips".to_string())
    );
    assert_eq!(
        dispatched_location_name(&captured, actor_b_id),
        Some("Powderhorn".to_string())
    );
}

// ---------------------------------------------------------------------------
// Mixed results — only changed actors counted
// ---------------------------------------------------------------------------

#[tokio::test]
async fn enrichment_counts_only_actors_whose_location_changed() {
    let store = Arc::new(MockSignalReader::new());

    // Actor 1: no location → will be updated
    let actor_1 = test_actor("Updatable Org");
    store.upsert_actor(&actor_1).await.unwrap();
    seed_signal(&store, actor_1.id, "U1", PHILLIPS.0, PHILLIPS.1, "Phillips").await;
    seed_signal(&store, actor_1.id, "U2", PHILLIPS.0, PHILLIPS.1, "Phillips").await;

    // Actor 2: already at Phillips, signals say Phillips → not counted
    let actor_2 = test_actor_at("Already There", PHILLIPS.0, PHILLIPS.1, "Phillips");
    store.upsert_actor(&actor_2).await.unwrap();
    seed_signal(
        &store, actor_2.id, "AT1", PHILLIPS.0, PHILLIPS.1, "Phillips",
    )
    .await;
    seed_signal(
        &store, actor_2.id, "AT2", PHILLIPS.0, PHILLIPS.1, "Phillips",
    )
    .await;

    // Actor 3: only 1 signal → not enough evidence
    let actor_3 = test_actor("Insufficient Org");
    store.upsert_actor(&actor_3).await.unwrap();
    seed_signal(
        &store,
        actor_3.id,
        "I1",
        POWDERHORN.0,
        POWDERHORN.1,
        "Powderhorn",
    )
    .await;

    let actors = vec![(actor_1, vec![]), (actor_2, vec![]), (actor_3, vec![])];
    let updated = enrich_with_engine(&*store, &actors).await;

    assert_eq!(
        updated, 1,
        "only the one actor that actually changed should be counted"
    );
}

// ---------------------------------------------------------------------------
// Overwrites wrong location
// ---------------------------------------------------------------------------

#[tokio::test]
async fn enrichment_overwrites_wrong_location_with_signal_mode() {
    let store = Arc::new(MockSignalReader::new());
    // Actor thinks they're in Powderhorn, but signals say Phillips
    let actor = test_actor_at("Mislocated Org", POWDERHORN.0, POWDERHORN.1, "Powderhorn");
    store.upsert_actor(&actor).await.unwrap();

    seed_signal(&store, actor.id, "M1", PHILLIPS.0, PHILLIPS.1, "Phillips").await;
    seed_signal(&store, actor.id, "M2", PHILLIPS.0, PHILLIPS.1, "Phillips").await;
    seed_signal(&store, actor.id, "M3", PHILLIPS.0, PHILLIPS.1, "Phillips").await;

    let actor_id = actor.id;
    let actors = vec![(actor, vec![])];
    let (updated, captured) = enrich_with_sink(&*store, &actors).await;

    assert_eq!(updated, 1, "wrong location should be corrected");
    assert_eq!(
        dispatched_location_name(&captured, actor_id),
        Some("Phillips".to_string()),
        "should move from Powderhorn to Phillips"
    );
}

// ---------------------------------------------------------------------------
// Three neighborhoods — plurality wins
// ---------------------------------------------------------------------------

#[tokio::test]
async fn enrichment_three_neighborhoods_plurality_wins() {
    let store = Arc::new(MockSignalReader::new());
    let actor = test_actor("Spread Org");
    store.upsert_actor(&actor).await.unwrap();

    // 3× Phillips, 2× Powderhorn, 1× Whittier
    seed_signal(&store, actor.id, "P1", PHILLIPS.0, PHILLIPS.1, "Phillips").await;
    seed_signal(&store, actor.id, "P2", PHILLIPS.0, PHILLIPS.1, "Phillips").await;
    seed_signal(&store, actor.id, "P3", PHILLIPS.0, PHILLIPS.1, "Phillips").await;
    seed_signal(
        &store,
        actor.id,
        "PW1",
        POWDERHORN.0,
        POWDERHORN.1,
        "Powderhorn",
    )
    .await;
    seed_signal(
        &store,
        actor.id,
        "PW2",
        POWDERHORN.0,
        POWDERHORN.1,
        "Powderhorn",
    )
    .await;
    seed_signal(&store, actor.id, "W1", WHITTIER.0, WHITTIER.1, "Whittier").await;

    let actor_id = actor.id;
    let actors = vec![(actor, vec![])];
    let (updated, captured) = enrich_with_sink(&*store, &actors).await;

    assert_eq!(updated, 1);
    assert_eq!(
        dispatched_location_name(&captured, actor_id),
        Some("Phillips".to_string()),
        "Phillips has plurality (3 of 6)"
    );
}

// ---------------------------------------------------------------------------
// Coordinates flow through — not just the name
// ---------------------------------------------------------------------------

#[tokio::test]
async fn enrichment_sets_coordinates_not_just_name() {
    let store = Arc::new(MockSignalReader::new());
    let actor = test_actor("Coord Org");
    store.upsert_actor(&actor).await.unwrap();

    seed_signal(&store, actor.id, "C1", PHILLIPS.0, PHILLIPS.1, "Phillips").await;
    seed_signal(&store, actor.id, "C2", PHILLIPS.0, PHILLIPS.1, "Phillips").await;

    let actor_id = actor.id;
    let actors = vec![(actor, vec![])];
    let (_updated, captured) = enrich_with_sink(&*store, &actors).await;

    let coords = dispatched_location_coords(&captured, actor_id);
    assert!(coords.is_some(), "coordinates should be set");
    let (lat, lng) = coords.unwrap();
    assert!(
        (lat - PHILLIPS.0).abs() < 0.01,
        "latitude should match Phillips: got {lat}"
    );
    assert!(
        (lng - PHILLIPS.1).abs() < 0.01,
        "longitude should match Phillips: got {lng}"
    );
}

// ---------------------------------------------------------------------------
// Actor with no location and no signals — completely untouched
// ---------------------------------------------------------------------------

#[tokio::test]
async fn enrichment_blank_actor_with_no_signals_stays_blank() {
    let store = Arc::new(MockSignalReader::new());
    let actor = test_actor("Ghost Org");
    store.upsert_actor(&actor).await.unwrap();

    let actors = vec![(actor, vec![])];
    let updated = enrich_with_engine(&*store, &actors).await;

    assert_eq!(updated, 0);
    assert_eq!(store.actor_location_name("Ghost Org"), None);
    assert_eq!(store.actor_location_coords("Ghost Org"), None);
}

// ---------------------------------------------------------------------------
// Signals from "mentioned" role should NOT count — only "authored"
// ---------------------------------------------------------------------------

#[tokio::test]
async fn enrichment_ignores_mentioned_signals() {
    let store = Arc::new(MockSignalReader::new());
    let actor = test_actor("Mentioned Only");
    store.upsert_actor(&actor).await.unwrap();

    // Link signals as "mentioned" not "authored"
    let node1 = tension_at_named("M1", PHILLIPS.0, PHILLIPS.1, "Phillips");
    let sig1 = store
        .create_node(&node1, &[0.1], "test", "run-1")
        .await
        .unwrap();
    store
        .link_actor_to_signal(actor.id, sig1, "mentioned")
        .await
        .unwrap();

    let node2 = tension_at_named("M2", PHILLIPS.0, PHILLIPS.1, "Phillips");
    let sig2 = store
        .create_node(&node2, &[0.2], "test", "run-1")
        .await
        .unwrap();
    store
        .link_actor_to_signal(actor.id, sig2, "mentioned")
        .await
        .unwrap();

    let actors = vec![(actor, vec![])];
    let updated = enrich_with_engine(&*store, &actors).await;

    assert_eq!(
        updated, 0,
        "mentioned signals should not count for location"
    );
    assert_eq!(store.actor_location_name("Mentioned Only"), None);
}

// ---------------------------------------------------------------------------
// Mix of authored and mentioned — only authored count
// ---------------------------------------------------------------------------

#[tokio::test]
async fn enrichment_only_authored_signals_count_for_location() {
    let store = Arc::new(MockSignalReader::new());
    let actor = test_actor("Mixed Roles Org");
    store.upsert_actor(&actor).await.unwrap();

    // 1 authored Phillips + 5 mentioned Powderhorn → only 1 authored signal, not enough
    seed_signal(
        &store, actor.id, "Auth1", PHILLIPS.0, PHILLIPS.1, "Phillips",
    )
    .await;

    for i in 0..5 {
        let node = tension_at_named(
            &format!("Ment{i}"),
            POWDERHORN.0,
            POWDERHORN.1,
            "Powderhorn",
        );
        let sig = store
            .create_node(&node, &[0.1], "test", "run-1")
            .await
            .unwrap();
        store
            .link_actor_to_signal(actor.id, sig, "mentioned")
            .await
            .unwrap();
    }

    let actors = vec![(actor, vec![])];
    let updated = enrich_with_engine(&*store, &actors).await;

    assert_eq!(updated, 0, "only 1 authored signal — not enough evidence");
}

// ---------------------------------------------------------------------------
// Exactly 2 signals — the minimum threshold
// ---------------------------------------------------------------------------

#[tokio::test]
async fn enrichment_exactly_two_signals_is_sufficient() {
    let store = Arc::new(MockSignalReader::new());
    let actor = test_actor("Bare Minimum Org");
    store.upsert_actor(&actor).await.unwrap();

    seed_signal(
        &store,
        actor.id,
        "BM1",
        POWDERHORN.0,
        POWDERHORN.1,
        "Powderhorn",
    )
    .await;
    seed_signal(
        &store,
        actor.id,
        "BM2",
        POWDERHORN.0,
        POWDERHORN.1,
        "Powderhorn",
    )
    .await;

    let actor_id = actor.id;
    let actors = vec![(actor, vec![])];
    let (updated, captured) = enrich_with_sink(&*store, &actors).await;

    assert_eq!(updated, 1, "2 signals should be enough");
    assert_eq!(
        dispatched_location_name(&captured, actor_id),
        Some("Powderhorn".to_string())
    );
}

// ---------------------------------------------------------------------------
// Resource edge wiring — boundary tests
//
// MOCK → scrape_web_sources → OUTPUT
// Validates confidence filtering, role wiring, and multiple resources.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn low_confidence_resource_tag_does_not_create_edge() {
    let fetcher = MockFetcher::new().on_page(
        "https://example.com/low-conf",
        archived_page("https://example.com/low-conf", "# Low confidence resources"),
    );

    let node = need_at("Need Blankets", 44.975, -93.270);
    let node_id = node.meta().unwrap().id;

    let extractor = MockExtractor::new().on_url(
        "https://example.com/low-conf",
        ExtractionResult {
            nodes: vec![node],
            implied_queries: vec![],
            resource_tags: vec![(
                node_id,
                vec![ResourceTag {
                    slug: "clothing".to_string(),
                    role: ResourceRole::Requires,
                    confidence: 0.2, // below 0.3 threshold
                    context: None,
                }],
            )],
            signal_tags: vec![],
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://example.com/low-conf");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(
        ctx.stats.signals_stored, 1,
        "signal should still be created"
    );
}

#[tokio::test]
async fn resource_roles_wire_to_correct_edge_types() {
    let fetcher = MockFetcher::new().on_page(
        "https://example.com/multi-role",
        archived_page("https://example.com/multi-role", "# Multi-role resources"),
    );

    let node = aid_at("Community Kitchen", 44.975, -93.270);
    let node_id = node.meta().unwrap().id;

    let extractor = MockExtractor::new().on_url(
        "https://example.com/multi-role",
        ExtractionResult {
            nodes: vec![node],
            implied_queries: vec![],
            resource_tags: vec![(
                node_id,
                vec![
                    ResourceTag {
                        slug: "vehicle".to_string(),
                        role: ResourceRole::Requires,
                        confidence: 0.9,
                        context: Some("pickup truck".to_string()),
                    },
                    ResourceTag {
                        slug: "bilingual-spanish".to_string(),
                        role: ResourceRole::Prefers,
                        confidence: 0.8,
                        context: None,
                    },
                    ResourceTag {
                        slug: "food".to_string(),
                        role: ResourceRole::Offers,
                        confidence: 0.7,
                        context: Some("hot meals".to_string()),
                    },
                ],
            )],
            signal_tags: vec![],
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://example.com/multi-role");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(ctx.stats.signals_stored, 1);
}

#[tokio::test]
async fn multiple_resources_on_one_signal_all_create_edges() {
    let fetcher = MockFetcher::new().on_page(
        "https://example.com/multi-res",
        archived_page("https://example.com/multi-res", "# Multi-resource signal"),
    );

    let node = need_at("Winter Coat Drive", 44.975, -93.270);
    let node_id = node.meta().unwrap().id;

    let extractor = MockExtractor::new().on_url(
        "https://example.com/multi-res",
        ExtractionResult {
            nodes: vec![node],
            implied_queries: vec![],
            resource_tags: vec![(
                node_id,
                vec![
                    ResourceTag {
                        slug: "clothing".to_string(),
                        role: ResourceRole::Requires,
                        confidence: 0.9,
                        context: Some("winter coats".to_string()),
                    },
                    ResourceTag {
                        slug: "storage-space".to_string(),
                        role: ResourceRole::Requires,
                        confidence: 0.8,
                        context: None,
                    },
                    ResourceTag {
                        slug: "vehicle".to_string(),
                        role: ResourceRole::Requires,
                        confidence: 0.7,
                        context: Some("for delivery".to_string()),
                    },
                ],
            )],
            signal_tags: vec![],
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://example.com/multi-res");
    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(ctx.stats.signals_stored, 1);
}

// ---------------------------------------------------------------------------
// Topic discovery → mention collection (signal-gated)
//
// MOCK → discover_from_topics (the organ) → OUTPUT
// Two authors found via topic search. Author A produces signals and has
// mentions; Author B produces zero signals and has mentions.
// Only Author A's mentions should appear in collected_links.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn topic_discovery_collects_mentions_only_from_signal_producing_authors() {
    // Author A: produces signals, mentions @friend_a
    let mut post_a = test_post("Free legal clinic in Phillips this Saturday");
    post_a.author = Some("signal_author".to_string());
    post_a.mentions = vec!["friend_a".to_string()];

    // Author B: produces no signals, mentions @friend_b
    let mut post_b = test_post("Just a regular day, nothing community-related");
    post_b.author = Some("noise_author".to_string());
    post_b.mentions = vec!["friend_b".to_string()];

    // Register topic search for Instagram only (others return Err → skipped)
    let fetcher = MockFetcher::new()
        .on_topic_search("https://www.instagram.com/topics", vec![post_a, post_b]);

    // Author A's URL produces a signal; Author B's produces nothing
    let extractor = MockExtractor::new()
        .on_url(
            "https://www.instagram.com/signal_author/",
            ExtractionResult {
                nodes: vec![tension_at("Free Legal Clinic Phillips", 44.9489, -93.2583)],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
                raw_signal_count: 0,
                rejected: Vec::new(),
                schedules: Vec::new(),
                author_actors: Vec::new(),
                categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
            },
        )
        .on_url(
            "https://www.instagram.com/noise_author/",
            ExtractionResult {
                nodes: vec![],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
                raw_signal_count: 0,
                rejected: Vec::new(),
                schedules: Vec::new(),
                author_actors: Vec::new(),
                categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
            },
        );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let mut ctx = PipelineState::from_sources(&[]);

    let topics = vec!["legal clinic".to_string()];
    let output = super::activities::topic_discovery::discover_from_topics(
        &deps, &topics, &ctx.url_to_canonical_key, &ctx.actor_contexts)
        .await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    // Signal-producing author's mentions should be collected
    let mention_urls: Vec<&str> = ctx.collected_links.iter().map(|l| l.url.as_str()).collect();
    assert!(
        mention_urls.iter().any(|u| u.contains("friend_a")),
        "mention from signal-producing author should be collected, got: {mention_urls:?}"
    );

    // Noise author's mentions should NOT be collected
    assert!(
        !mention_urls.iter().any(|u| u.contains("friend_b")),
        "mention from zero-signal author should not be collected, got: {mention_urls:?}"
    );
}

// ---------------------------------------------------------------------------
// Bio location enrichment — TDD RED test
//
// MOCK → triangulate_all_actors → OUTPUT
// Actor has bio text matching a signal's location name.
// Bio corroborated by 1 signal should win.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn actor_bio_location_corroborated_by_signal_wins() {
    let store = Arc::new(MockSignalReader::new());
    let mut actor = test_actor("Phillips Pantry");
    actor.bio = Some("Based in Phillips, Minneapolis".to_string());
    store.upsert_actor(&actor).await.unwrap();

    // Only ONE signal in Phillips — not enough on its own (need 2),
    // but bio corroboration should make it sufficient.
    seed_signal(
        &store,
        actor.id,
        "Food Drive",
        PHILLIPS.0,
        PHILLIPS.1,
        "Phillips",
    )
    .await;

    let actor_id = actor.id;
    let actors = vec![(actor, vec![])];
    let (updated, captured) = enrich_with_sink(&*store, &actors).await;

    assert_eq!(
        updated, 1,
        "bio corroborated by one signal should update location"
    );
    assert_eq!(
        dispatched_location_name(&captured, actor_id),
        Some("Phillips".to_string()),
        "bio location corroborated by signal should win"
    );
}

// ---------------------------------------------------------------------------
// resolve_web_urls boundary tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn resolve_web_urls_collects_search_result_urls() {
    let query = "mutual aid Minneapolis";
    let fetcher = MockFetcher::new().on_search(
        query,
        search_results(query, &["https://org-a.org", "https://org-b.org"]),
    );

    let store = Arc::new(MockSignalReader::new());
    let extractor = MockExtractor::new();

    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = web_query_source(query);
    let sources: Vec<&_> = vec![&source];
    let ctx = PipelineState::from_sources(&[source.clone()]);

    let resolution = super::activities::url_resolution::resolve_web_urls(&deps, &sources, &ctx.url_to_canonical_key).await;

    assert_eq!(resolution.urls.len(), 2, "should resolve 2 URLs from search");
    assert!(resolution.query_api_errors.is_empty(), "no API errors");
    assert!(
        resolution.url_mappings.values().any(|v| v == &source.canonical_key),
        "URL mappings should map to source canonical_key"
    );
}

#[tokio::test]
async fn resolve_web_urls_records_api_errors() {
    // MockFetcher with no search configured → returns Err
    let fetcher = MockFetcher::new();
    let store = Arc::new(MockSignalReader::new());
    let extractor = MockExtractor::new();

    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = web_query_source("failing query");
    let sources: Vec<&_> = vec![&source];
    let ctx = PipelineState::from_sources(&[source.clone()]);

    let resolution = super::activities::url_resolution::resolve_web_urls(&deps, &sources, &ctx.url_to_canonical_key).await;

    assert!(resolution.urls.is_empty(), "no URLs on API error");
    assert!(
        resolution.query_api_errors.contains(&source.canonical_key),
        "API error should be recorded"
    );
}

#[tokio::test]
async fn resolve_web_urls_includes_page_source_urls() {
    let fetcher = MockFetcher::new();
    let store = Arc::new(MockSignalReader::new());
    let extractor = MockExtractor::new();

    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source("https://localorg.org/events");
    let sources: Vec<&_> = vec![&source];
    let ctx = PipelineState::from_sources(&[source.clone()]);

    let resolution = super::activities::url_resolution::resolve_web_urls(&deps, &sources, &ctx.url_to_canonical_key).await;

    assert_eq!(resolution.urls.len(), 1);
    assert_eq!(resolution.urls[0], "https://localorg.org/events");
}

// ---------------------------------------------------------------------------
// fetch_and_extract boundary tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fetch_and_extract_produces_signals_and_stats() {
    let url = "https://localorg.org/events";
    let fetcher = MockFetcher::new().on_page(
        url,
        archived_page(url, "Community dinner at Powderhorn Park"),
    );

    let extractor = MockExtractor::new().on_url(
        url,
        ExtractionResult {
            nodes: vec![tension_at("Community Dinner", 44.9489, -93.2583)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source(url);
    let source_keys: std::collections::HashMap<String, Uuid> =
        vec![(source.canonical_key.clone(), source.id)].into_iter().collect();
    let ctx = PipelineState::from_sources(&[source.clone()]);
    let urls = vec![url.to_string()];

    let result = super::activities::web_scrape::fetch_and_extract(&deps, 
        &urls,
        &source_keys,
        &ctx.url_to_canonical_key,
        &ctx.actor_contexts,
        &std::collections::HashMap::new(),
        &causal::Logger::new(),
    ).await;

    assert_eq!(result.stats.urls_scraped, 1, "one URL scraped");
    assert_eq!(result.stats.urls_failed, 0, "no failures");
    assert_eq!(result.stats.signals_extracted, 1, "one signal extracted");
    assert!(!result.extracted_batches.is_empty(), "should produce extracted batches");
}

#[tokio::test]
async fn fetch_and_extract_with_empty_urls_returns_empty() {
    let fetcher = MockFetcher::new();
    let store = Arc::new(MockSignalReader::new());
    let extractor = MockExtractor::new();

    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let ctx = PipelineState::default();
    let urls: Vec<String> = vec![];
    let source_keys = std::collections::HashMap::new();

    let result = super::activities::web_scrape::fetch_and_extract(&deps, 
        &urls,
        &source_keys,
        &ctx.url_to_canonical_key,
        &ctx.actor_contexts,
        &std::collections::HashMap::new(),
        &causal::Logger::new(),
    ).await;

    assert_eq!(result.stats.urls_scraped, 0);
    assert_eq!(result.stats.urls_failed, 0);
    assert!(result.events.is_empty());
}

#[tokio::test]
async fn fetch_and_extract_counts_failed_urls() {
    // MockFetcher with no page configured → returns Err
    let fetcher = MockFetcher::new();
    let store = Arc::new(MockSignalReader::new());
    let extractor = MockExtractor::new();

    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let ctx = PipelineState::default();
    let urls = vec!["https://unreachable.org".to_string()];
    let source_keys = std::collections::HashMap::new();

    let result = super::activities::web_scrape::fetch_and_extract(&deps, 
        &urls,
        &source_keys,
        &ctx.url_to_canonical_key,
        &ctx.actor_contexts,
        &std::collections::HashMap::new(),
        &causal::Logger::new(),
    ).await;

    assert_eq!(result.stats.urls_failed, 1, "one URL should fail");
    assert_eq!(result.stats.urls_scraped, 0);
}

// ---------------------------------------------------------------------------
// Channel weight gating — social handler skips/includes channels by weight
// ---------------------------------------------------------------------------

#[tokio::test]
async fn feed_channel_off_skips_post_fetch() {
    let ig_url = "https://www.instagram.com/gated_account";

    let fetcher = MockFetcher::new()
        .on_posts(ig_url, vec![test_post("Should not be fetched")]);

    let extractor = MockExtractor::new().on_url(
        ig_url,
        ExtractionResult {
            nodes: vec![tension_at("Gated Signal", 44.95, -93.27)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let mut source = social_source(ig_url);
    source.channel_weights.feed = 0.0;
    source.channel_weights.media = 0.0;
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::social_scrape::scrape_social_sources(
        &deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts,
        &causal::Logger::new(),
    ).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(ctx.stats.signals_stored, 0, "all channels off → no signals");
}

#[tokio::test]
async fn media_channel_on_fetches_stories_and_videos() {
    let ig_url = "https://www.instagram.com/media_account";

    let story = test_story("Free yoga in the park tomorrow morning");
    let video = test_short_video("Neighborhood mural project reveal");

    let fetcher = MockFetcher::new()
        .on_stories(ig_url, vec![story])
        .on_short_videos(ig_url, vec![video]);

    let extractor = MockExtractor::new().on_url(
        ig_url,
        ExtractionResult {
            nodes: vec![
                gathering_at("Free Yoga in Park", 44.95, -93.27),
                tension_at("Mural Project Reveal", 44.96, -93.26),
            ],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let mut source = social_source(ig_url);
    source.channel_weights.feed = 0.0;
    source.channel_weights.media = 1.0;
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::social_scrape::scrape_social_sources(
        &deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts,
        &causal::Logger::new(),
    ).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(ctx.stats.signals_stored, 2, "media channel on → signals from stories/videos");
}

#[tokio::test]
async fn feed_and_media_combined_in_single_extraction() {
    let ig_url = "https://www.instagram.com/both_channels";

    let fetcher = MockFetcher::new()
        .on_posts(ig_url, vec![test_post("New community garden plot signups")])
        .on_stories(ig_url, vec![test_story("Storm damage on Elm Street")])
        .on_short_videos(ig_url, vec![test_short_video("Block party this weekend")]);

    let extractor = MockExtractor::new().on_url(
        ig_url,
        ExtractionResult {
            nodes: vec![
                gathering_at("Community Garden Signups", 44.95, -93.27),
                tension_at("Storm Damage Elm Street", 44.96, -93.26),
                gathering_at("Block Party Weekend", 44.94, -93.28),
            ],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let mut source = social_source(ig_url);
    source.channel_weights.feed = 1.0;
    source.channel_weights.media = 1.0;
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::social_scrape::scrape_social_sources(
        &deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts,
        &causal::Logger::new(),
    ).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(ctx.stats.signals_stored, 3, "posts + stories + videos → combined extraction");
}

#[tokio::test]
async fn media_fetch_failure_does_not_block_feed() {
    let ig_url = "https://www.instagram.com/partial_fail";

    let fetcher = MockFetcher::new()
        .on_posts(ig_url, vec![test_post("Farmers market returns next week")]);
    // stories/short_videos not registered → will return error, soft-fail to empty

    let extractor = MockExtractor::new().on_url(
        ig_url,
        ExtractionResult {
            nodes: vec![gathering_at("Farmers Market Returns", 44.95, -93.27)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            raw_signal_count: 0,
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            source_ids: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let mut source = social_source(ig_url);
    source.channel_weights.feed = 1.0;
    source.channel_weights.media = 1.0;
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::social_scrape::scrape_social_sources(
        &deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts,
        &causal::Logger::new(),
    ).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(ctx.stats.signals_stored, 1, "media failure → feed signals still extracted");
}

// ---------------------------------------------------------------------------
// Scheduled scrapes — unenriched media triggers deferred re-scrape
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unenriched_story_media_emits_scrape_scheduled() {
    let ig_url = "https://www.instagram.com/unenriched_test";

    let fetcher = MockFetcher::new()
        .on_stories(ig_url, vec![test_story_with_unenriched_media("Story with image")]);

    let extractor = MockExtractor::new().on_url(
        ig_url,
        ExtractionResult {
            nodes: vec![tension_at("Image Story Signal", 44.95, -93.27)],
            ..Default::default()
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let mut source = social_source(ig_url);
    source.channel_weights.feed = 0.0;
    source.channel_weights.media = 1.0;
    let sources: Vec<&_> = vec![&source];
    let ctx = PipelineState::from_sources(&[source.clone()]);

    let mut output = super::activities::social_scrape::scrape_social_sources(
        &deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts,
        &causal::Logger::new(),
    ).await;

    let events = output.take_events();
    let has_scheduled = events.into_outputs().into_iter().any(|out| {
        out.event_type.contains("SchedulingEvent")
    });

    assert!(has_scheduled, "unenriched media attachment → ScrapeScheduled event emitted");
}

#[tokio::test]
async fn enriched_story_media_does_not_emit_scrape_scheduled() {
    let ig_url = "https://www.instagram.com/enriched_test";

    let fetcher = MockFetcher::new()
        .on_stories(ig_url, vec![test_story_with_enriched_media("Story with OCR text")]);

    let extractor = MockExtractor::new().on_url(
        ig_url,
        ExtractionResult {
            nodes: vec![tension_at("Enriched Story Signal", 44.95, -93.27)],
            ..Default::default()
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let mut source = social_source(ig_url);
    source.channel_weights.feed = 0.0;
    source.channel_weights.media = 1.0;
    let sources: Vec<&_> = vec![&source];
    let ctx = PipelineState::from_sources(&[source.clone()]);

    let mut output = super::activities::social_scrape::scrape_social_sources(
        &deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts,
        &causal::Logger::new(),
    ).await;

    let events = output.take_events();
    let has_scheduled = events.into_outputs().into_iter().any(|out| {
        out.event_type.contains("SchedulingEvent")
    });

    assert!(!has_scheduled, "fully enriched media → no ScrapeScheduled event");
}

#[tokio::test]
async fn media_channel_off_does_not_emit_scrape_scheduled() {
    let ig_url = "https://www.instagram.com/no_media_test";

    let fetcher = MockFetcher::new()
        .on_posts(ig_url, vec![test_post("Regular post")])
        .on_stories(ig_url, vec![test_story_with_unenriched_media("Should not be fetched")]);

    let extractor = MockExtractor::new().on_url(
        ig_url,
        ExtractionResult {
            nodes: vec![tension_at("Post Signal", 44.95, -93.27)],
            ..Default::default()
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let mut source = social_source(ig_url);
    source.channel_weights.feed = 1.0;
    source.channel_weights.media = 0.0;
    let sources: Vec<&_> = vec![&source];
    let ctx = PipelineState::from_sources(&[source.clone()]);

    let mut output = super::activities::social_scrape::scrape_social_sources(
        &deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts,
        &causal::Logger::new(),
    ).await;

    let events = output.take_events();
    let has_scheduled = events.into_outputs().into_iter().any(|out| {
        out.event_type.contains("SchedulingEvent")
    });

    assert!(!has_scheduled, "media channel off → no ScrapeScheduled even with unenriched stories");
}

// ---------------------------------------------------------------------------
// ScoutRunTest harness — proof-of-concept
// ---------------------------------------------------------------------------

#[tokio::test]
async fn harness_page_with_content_produces_signal() {
    let url = "https://localorg.org/events";
    let harness = ScoutRunTest::new()
        .source(url, archived_page(url, "# Community Dinner\nFree dinner at Powderhorn Park"))
        .extraction(url, ExtractionResult {
            nodes: vec![tension_at("Community Dinner at Powderhorn", 44.9489, -93.2583)],
            ..Default::default()
        })
        .build();

    harness.run().await;

    assert_eq!(harness.stats().signals_stored, 1, "one signal should be created");
}

#[tokio::test]
async fn harness_empty_page_produces_nothing() {
    let url = "https://empty.org";
    let harness = ScoutRunTest::new()
        .source(url, archived_page(url, ""))
        .extraction(url, ExtractionResult::default())
        .build();

    harness.run().await;

    assert_eq!(harness.stats().signals_stored, 0);
}

#[tokio::test]
async fn harness_unreachable_page_does_not_crash() {
    let url = "https://broken.org";
    let harness = ScoutRunTest::new()
        .source_only(url)
        .build();

    harness.run().await;

    assert_eq!(harness.stats().signals_stored, 0);
}

#[tokio::test]
async fn harness_multiple_signals_from_one_page() {
    let url = "https://news.org/article";
    let harness = ScoutRunTest::new()
        .source(url, archived_page(url, "# Multiple issues\nHousing and transit"))
        .extraction(url, ExtractionResult {
            nodes: vec![
                tension_at("Housing Crisis Downtown", 44.975, -93.270),
                tension_at("Bus Route 5 Cuts", 44.960, -93.265),
                need_at("Volunteer Drivers Needed", 44.955, -93.260),
            ],
            ..Default::default()
        })
        .build();

    harness.run().await;

    assert_eq!(harness.stats().signals_stored, 3, "all three signals should be created");
}

#[tokio::test]
async fn harness_duplicate_title_deduped_within_batch() {
    let url = "https://news.org/dupe";
    let harness = ScoutRunTest::new()
        .source(url, archived_page(url, "# Repeated story"))
        .extraction(url, ExtractionResult {
            nodes: vec![
                tension_at("Housing Crisis", 44.975, -93.270),
                tension_at("Housing Crisis", 44.975, -93.270),
                tension_at("Different Signal", 44.960, -93.265),
            ],
            ..Default::default()
        })
        .build();

    harness.run().await;

    assert_eq!(harness.stats().signals_stored, 2, "duplicate title+type deduped to 1");
}

#[tokio::test]
async fn harness_far_away_signal_still_stored() {
    let url = "https://news.org/far-away";
    let harness = ScoutRunTest::new()
        .region(mpls_region())
        .source(url, archived_page(url, "# Far Away Signal"))
        .extraction(url, ExtractionResult {
            nodes: vec![tension_at("Issue in Duluth", DULUTH.0, DULUTH.1)],
            ..Default::default()
        })
        .build();

    harness.run().await;

    assert_eq!(harness.stats().signals_stored, 1, "signals stored regardless of region");
}

#[tokio::test]
async fn harness_two_sources_two_signals() {
    let url1 = "https://org1.org/page";
    let url2 = "https://org2.org/page";
    let harness = ScoutRunTest::new()
        .source(url1, archived_page(url1, "Page 1"))
        .source(url2, archived_page(url2, "Page 2"))
        .extraction(url1, ExtractionResult {
            nodes: vec![tension_at("Signal One", 44.97, -93.27)],
            ..Default::default()
        })
        .extraction(url2, ExtractionResult {
            nodes: vec![tension_at("Signal Two", 44.96, -93.26)],
            ..Default::default()
        })
        .build();

    harness.run().await;

    assert_eq!(harness.stats().signals_stored, 2);
}

