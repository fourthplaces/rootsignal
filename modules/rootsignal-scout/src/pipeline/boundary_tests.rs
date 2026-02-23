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

// ---------------------------------------------------------------------------
// Fetcher → Link Discoverer boundary
//
// Page links flow into ctx.collected_links, then promote_links creates sources.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn page_links_collected_for_promotion() {
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
    let embedder = Arc::new(FixedEmbedder::new(64));

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
    let collected_urls: Vec<&str> = ctx.collected_links.iter().map(|(u, _)| u.as_str()).collect();
    assert!(collected_urls.contains(&"https://localorg.org/events"));
    assert!(collected_urls.contains(&"https://foodshelf.org/volunteer"));
    assert!(
        !collected_urls.iter().any(|u| u.starts_with("javascript:")),
        "javascript: links should be filtered"
    );
}

#[tokio::test]
async fn promote_links_creates_source_nodes() {
    use crate::enrichment::link_promoter::{self, PromotionConfig};

    let store = Arc::new(MockSignalStore::new());

    let links = vec![
        ("https://localorg.org/events".to_string(), "https://linktree.org".to_string()),
        ("https://foodshelf.org/volunteer".to_string(), "https://linktree.org".to_string()),
    ];

    let config = PromotionConfig { max_per_source: 10, max_per_run: 50 };
    let region = mpls_region();

    let promoted = link_promoter::promote_links(
        &links,
        store.as_ref(),
        &config,
        region.center_lat,
        region.center_lng,
    )
    .await
    .unwrap();

    assert_eq!(promoted, 2);
    assert_eq!(store.sources_promoted(), 2);
    assert!(store.has_source_url("https://localorg.org/events"));
    assert!(store.has_source_url("https://foodshelf.org/volunteer"));
}

#[tokio::test]
async fn promote_links_deduplicates_same_url() {
    use crate::enrichment::link_promoter::{self, PromotionConfig};

    let store = Arc::new(MockSignalStore::new());

    let links = vec![
        ("https://localorg.org/events".to_string(), "https://page-a.org".to_string()),
        ("https://localorg.org/events".to_string(), "https://page-b.org".to_string()), // same URL, different source
        ("https://other.org/page".to_string(), "https://page-c.org".to_string()),
    ];

    let config = PromotionConfig { max_per_source: 10, max_per_run: 50 };
    let region = mpls_region();

    let promoted = link_promoter::promote_links(
        &links,
        store.as_ref(),
        &config,
        region.center_lat,
        region.center_lng,
    )
    .await
    .unwrap();

    assert_eq!(promoted, 2, "duplicate URLs should be deduped to 2 unique sources");
}

#[tokio::test]
async fn promote_links_respects_max_per_run() {
    use crate::enrichment::link_promoter::{self, PromotionConfig};

    let store = Arc::new(MockSignalStore::new());

    let links: Vec<(String, String)> = (0..10)
        .map(|i| (format!("https://site-{i}.org"), "https://source.org".to_string()))
        .collect();

    let config = PromotionConfig { max_per_source: 10, max_per_run: 3 };
    let region = mpls_region();

    let promoted = link_promoter::promote_links(
        &links,
        store.as_ref(),
        &config,
        region.center_lat,
        region.center_lng,
    )
    .await
    .unwrap();

    assert_eq!(promoted, 3, "should respect max_per_run cap");
}

#[tokio::test]
async fn end_to_end_page_links_to_promoted_sources() {
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
    let embedder = Arc::new(FixedEmbedder::new(64));

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
    let region = mpls_region();
    let promoted = link_promoter::promote_links(
        &ctx.collected_links,
        store.as_ref(),
        &config,
        region.center_lat,
        region.center_lng,
    )
    .await
    .unwrap();

    assert!(promoted >= 2, "at least 2 links should be promoted");
    assert!(store.has_source_url("https://partner-a.org/programs"));
    assert!(store.has_source_url("https://partner-b.org/events"));
}

// ---------------------------------------------------------------------------
// Source location bug (TDD red phase)
//
// BUG: promote_links stamps every promoted source with the region center
// coordinates, regardless of where the discovering source actually is.
// These tests assert CORRECT behavior and are expected to FAIL until fixed.
// ---------------------------------------------------------------------------

#[tokio::test]
#[should_panic]
async fn promoted_source_should_not_get_hardcoded_region_center() {
    // A link discovered from a page should not blindly receive the region
    // center coordinates. The promoted source hasn't been scraped yet — we
    // don't know its actual coverage area.
    use crate::enrichment::link_promoter::{self, PromotionConfig};

    let store = Arc::new(MockSignalStore::new());
    let links = vec![
        ("https://discovered.org/page".to_string(), "https://stpaul-source.org".to_string()),
    ];
    let config = PromotionConfig { max_per_source: 10, max_per_run: 50 };
    let region = mpls_region();

    link_promoter::promote_links(
        &links,
        store.as_ref(),
        &config,
        region.center_lat,
        region.center_lng,
    )
    .await
    .unwrap();

    let (lat, lng) = store.get_source_coords("https://discovered.org/page");

    // BUG: promoted source gets region center (44.9778, -93.2650).
    // Correct behavior: coords should come from the discovering source,
    // or be None until the promoted source is actually scraped.
    assert_ne!(
        lat,
        Some(region.center_lat),
        "promoted source should not blindly get region center lat"
    );
    assert_ne!(
        lng,
        Some(region.center_lng),
        "promoted source should not blindly get region center lng"
    );
}

#[tokio::test]
#[should_panic]
async fn links_from_different_sources_should_get_different_coords() {
    // Two links discovered from sources at different locations should NOT
    // both end up with the same region-center coordinates.
    use crate::enrichment::link_promoter::{self, PromotionConfig};

    let store = Arc::new(MockSignalStore::new());

    // Link A discovered from a St. Paul source, Link B from a Duluth source.
    // Currently collected_links only carries (url, source_url) — no coords.
    // promote_links stamps both with identical region center.
    let links = vec![
        ("https://stpaul-event.org".to_string(), "https://stpaul-news.org".to_string()),
        ("https://duluth-event.org".to_string(), "https://duluth-news.org".to_string()),
    ];
    let config = PromotionConfig { max_per_source: 10, max_per_run: 50 };
    let region = mpls_region();

    link_promoter::promote_links(
        &links,
        store.as_ref(),
        &config,
        region.center_lat,
        region.center_lng,
    )
    .await
    .unwrap();

    let (lat_a, lng_a) = store.get_source_coords("https://stpaul-event.org");
    let (lat_b, lng_b) = store.get_source_coords("https://duluth-event.org");

    // BUG: both get identical region center (44.9778, -93.2650).
    // Correct behavior: each should inherit coords from its discovering source,
    // so a St. Paul link ≠ a Duluth link.
    assert_ne!(
        lat_a, lat_b,
        "promoted sources from different-location discoverers should have different coords"
    );
    assert_ne!(
        lng_a, lng_b,
        "promoted sources from different-location discoverers should have different coords"
    );
}

#[tokio::test]
#[should_panic]
async fn collected_links_should_carry_discovering_source_location() {
    // After run_web, collected_links only stores (url, source_url).
    // It should also carry the discovering source's coordinates so that
    // promote_links can assign per-link coords instead of region center.

    let fetcher = MockFetcher::new()
        .on_page(
            "https://stpaul-hub.org",
            {
                let mut page = archived_page("https://stpaul-hub.org", "# St. Paul Resources");
                page.links = vec!["https://stpaul-foodshelf.org".to_string()];
                page
            },
        );

    let extractor = MockExtractor::new()
        .on_url(
            "https://stpaul-hub.org",
            crate::pipeline::extractor::ExtractionResult {
                nodes: vec![tension_at("St. Paul Food Drive", 44.9537, -93.0900)],
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

    // Source with St. Paul coords, NOT Minneapolis center
    let mut source = page_source("https://stpaul-hub.org");
    source.center_lat = Some(44.9537);
    source.center_lng = Some(-93.0900);

    let sources: Vec<&SourceNode> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    // BUG: collected_links is Vec<(String, String)> — only (url, source_url).
    // It loses the discovering source's coordinates entirely.
    // Correct behavior: collected_links should carry source location so
    // promote_links can stamp per-link coords.
    assert!(
        !ctx.collected_links.is_empty(),
        "should have collected a link"
    );

    // This will fail because collected_links tuples only have 2 elements.
    // When fixed, the tuple should be (url, source_url, Option<f64>, Option<f64>).
    let first_link = &ctx.collected_links[0];
    let link_url = &first_link.0;
    assert!(link_url.starts_with("https://stpaul-foodshelf.org"), "unexpected link: {link_url}");

    // Can't even access coords — they're not in the tuple.
    // For now, assert that collected_links length is wrong (it's 2-tuples, should be 4-tuples).
    // This is a compile-time limitation we document with a runtime check.
    let tuple_size = std::mem::size_of_val(first_link);
    let two_strings_size = std::mem::size_of::<(String, String)>();
    assert!(
        tuple_size > two_strings_size,
        "collected_links tuples should carry more than just (url, source_url) — need coords too"
    );
}
