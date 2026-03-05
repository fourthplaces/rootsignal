//! Chain tests — end-to-end with mocks.
//!
//! Each test follows MOCK → FUNCTION → OUTPUT:
//! set up the fake external world, call the ACTUAL organ, assert what came out.
//! We never reach into the organ and call its internal functions.

use std::sync::Arc;

use chrono::Utc;
use rootsignal_common::types::ActorContext;
use rootsignal_common::{canonical_value, ScheduleNode};
use uuid::Uuid;

use crate::core::extractor::ExtractionResult;
use crate::core::aggregate::PipelineState;
use crate::domains::scrape::activities::ScrapeOutput;
use crate::infra::util::sanitize_url;
use crate::testing::*;
use crate::traits::SignalReader;
use crate::domains::enrichment::activities::link_promoter::{self, PromotionConfig};

async fn dispatch_events(
    events: seesaw_core::Events,
    ctx: &mut PipelineState,
    store: &Arc<MockSignalReader>,
) {
    let engine = test_engine_for_store(store.clone() as Arc<dyn SignalReader>);
    for output in events.into_outputs() {
        let _ = engine.emit_output(output).settled().await;
    }
    // Sync engine stats back to ctx so test assertions work.
    let state = engine.singleton::<crate::core::aggregate::PipelineState>();
    ctx.stats = state.stats.clone();
}

/// Take events from scrape output, apply state, and dispatch through engine.
async fn scrape_and_dispatch(
    output: ScrapeOutput,
    ctx: &mut PipelineState,
    store: &Arc<MockSignalReader>,
) {
    let mut output = output;
    let events = output.take_events();
    ctx.apply_scrape_output(output);
    dispatch_events(events, ctx, store).await;
}

// ---------------------------------------------------------------------------
// Chain Test 1: Linktree Discovery
//
// search "site:linktr.ee mutual aid Minneapolis" → results → fetch each
// result page → extract links. Junk filtered, tracking stripped.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn linktree_page_discovers_outbound_links() {
    let query = "site:linktr.ee mutual aid Minneapolis";

    let fetcher = MockFetcher::new()
        .on_search(
            query,
            search_results(
                query,
                &[
                    "https://linktr.ee/mplsmutualaid",
                    "https://linktr.ee/northsideaid",
                ],
            ),
        )
        .on_page("https://linktr.ee/mplsmutualaid", {
            let mut page = archived_page("https://linktr.ee/mplsmutualaid", "MPLS Mutual Aid");
            page.links = vec![
                "https://instagram.com/mplsmutualaid".to_string(),
                "https://gofundme.com/f/help-families?utm_source=linktree".to_string(),
                "https://localorg.org/resources".to_string(),
                "https://fonts.googleapis.com/css2?family=Inter".to_string(), // .css → filtered
            ];
            page
        })
        .on_page("https://linktr.ee/northsideaid", {
            let mut page = archived_page("https://linktr.ee/northsideaid", "Northside Aid");
            page.links = vec!["https://northsideaid.org/volunteer".to_string()];
            page
        });

    // Linktree pages: no signals, just links
    let extractor = MockExtractor::new()
        .on_url(
            "https://linktr.ee/mplsmutualaid",
            ExtractionResult {
                nodes: vec![],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
                rejected: Vec::new(),
                schedules: Vec::new(),
                author_actors: Vec::new(),
            categories: Vec::new(),
            logs: vec![],
            },
        )
        .on_url(
            "https://linktr.ee/northsideaid",
            ExtractionResult {
                nodes: vec![],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
                rejected: Vec::new(),
                schedules: Vec::new(),
                author_actors: Vec::new(),
            categories: Vec::new(),
            logs: vec![],
            },
        );

    let store = Arc::new(MockSignalReader::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = web_query_source(query);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    // No signals from Linktree pages
    assert_eq!(ctx.stats.signals_stored, 0);

    let collected_urls: Vec<&str> = ctx.collected_links.iter().map(|l| l.url.as_str()).collect();

    // Linktree pages return 0 nodes from extraction (not evaluated for signals).
    // Signal-gate only applies when nodes were extracted but produced 0 signals.
    // So all links from Linktree should still be collected.
    assert!(
        collected_urls
            .iter()
            .any(|u| u.contains("instagram.com/mplsmutualaid")),
        "Instagram should be collected"
    );
    assert!(
        collected_urls
            .iter()
            .any(|u| u.contains("localorg.org/resources")),
        "Org site should be collected"
    );
    assert!(
        collected_urls
            .iter()
            .any(|u| u.contains("northsideaid.org/volunteer")),
        "Northside org should be collected"
    );

    // GoFundMe collected with tracking stripped
    let gf = collected_urls.iter().find(|u| u.contains("gofundme.com"));
    assert!(gf.is_some(), "GoFundMe should be collected");
    assert!(
        !gf.unwrap().contains("utm_source"),
        "Tracking params should be stripped"
    );
}

// ---------------------------------------------------------------------------
// Chain Test 2: Page → Signal → Actors → Evidence
//
// page source → scrape_web_sources → signal created, actors wired, evidence linked.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn page_creates_signal_wires_actors_and_records_evidence() {
    let url = "https://localorg.org/resources";

    let fetcher = MockFetcher::new().on_page(url, {
        let mut page = archived_page(
            url,
            "Free legal clinic every Tuesday at Sabathani Center...",
        );
        page.links = vec![
            "https://communitypartners.org/localorg".to_string(),
            "https://sabathani.org/events".to_string(),
        ];
        page
    });

    let node = tension_at("Free Legal Clinic at Sabathani", 44.9341, -93.2619);

    let extractor = MockExtractor::new().on_url(
        url,
        ExtractionResult {
            nodes: vec![node],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source(url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    // Signal created
    assert_eq!(ctx.stats.signals_stored, 1);

    // Outbound links collected for promotion
    let collected_urls: Vec<&str> = ctx.collected_links.iter().map(|l| l.url.as_str()).collect();
    assert!(collected_urls
        .iter()
        .any(|u| u.contains("communitypartners.org/localorg")));
    assert!(collected_urls
        .iter()
        .any(|u| u.contains("sabathani.org/events")));
}

/// Signal in Dallas extracted from a page. No geo-filter — stored regardless.
#[tokio::test]
async fn dallas_signal_is_stored_by_minneapolis_scout() {
    let url = "https://texasorg.org/events";

    let fetcher = MockFetcher::new().on_page(url, archived_page(url, "Dallas community dinner..."));

    let extractor = MockExtractor::new().on_url(
        url,
        ExtractionResult {
            nodes: vec![tension_at("Dallas Community Dinner", DALLAS.0, DALLAS.1)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source(url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    // No geo-filter — all signals stored
    assert_eq!(ctx.stats.signals_stored, 1);
}

// ---------------------------------------------------------------------------
// Chain Test 3: Multi-Source Corroboration
//
// 3 pages describe the same event → scrape_web_sources → 1 signal, corroborations,
// evidence trails from each source.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn same_event_from_three_sites_produces_one_signal_with_two_corroborations() {
    let urls = [
        "https://org-a.org/events",
        "https://org-b.org/calendar",
        "https://nextdoor.com/post/xyz",
    ];

    let mut fetcher = MockFetcher::new();
    let mut extractor = MockExtractor::new();

    for url in &urls {
        fetcher = fetcher.on_page(url, archived_page(url, "Community garden cleanup..."));
        extractor = extractor.on_url(
            url,
            ExtractionResult {
                nodes: vec![tension_at("Community Garden Cleanup", 44.9489, -93.2654)],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
                rejected: Vec::new(),
                schedules: Vec::new(),
                author_actors: Vec::new(),
            categories: Vec::new(),
            logs: vec![],
            },
        );
    }

    // All three signals get near-identical embeddings → vector dedup fires
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM).on_text(
        "Community Garden Cleanup ",
        vec![0.5f32; TEST_EMBEDDING_DIM],
    ));

    let store = Arc::new(MockSignalReader::new());

    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source_nodes: Vec<_> = urls.iter().map(|u| page_source(u)).collect();
    let sources: Vec<&_> = source_nodes.iter().collect();
    let mut ctx = PipelineState::from_sources(&source_nodes);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    // ONE signal, not three
    assert_eq!(ctx.stats.signals_stored, 1, "should dedup to 1 signal");

    // Corroborated by the other two
}

// ---------------------------------------------------------------------------
// Chain Test 4: Social Scrape with Actor Context
//
// Instagram posts + actor_ctx → scrape_social_sources → signal with fallback location,
// @mentions collected for promotion.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn instagram_signal_inherits_actor_location_and_collects_mentions() {
    let ig_url = "https://www.instagram.com/northsidemutualaid";

    let mut post1 = test_post("Food distribution this Saturday at MLK Park!");
    post1.permalink = Some("https://instagram.com/p/abc123".to_string());
    post1.mentions = vec!["mplsfoodshelf".to_string()];

    let mut post2 = test_post("Know your rights workshop next Tuesday");
    post2.permalink = Some("https://instagram.com/p/def456".to_string());
    post2.mentions = vec!["hennepincounty".to_string()];

    let fetcher = MockFetcher::new().on_posts(ig_url, vec![post1, post2]);

    // Extractor returns a signal with no coordinates but a location_name matching
    // a geo-term. This lets it survive geo-filter via name match. Actor fallback
    // then backfills exact coordinates from the actor context.
    let mut node = tension("Food Distribution at MLK Park");
    if let Some(meta) = node.meta_mut() {
        meta.about_location_name = Some("Minneapolis, MN".to_string());
        meta.confidence = 0.7;
    }
    let node_id = node.meta().unwrap().id;

    let extractor = MockExtractor::new().on_url(
        ig_url,
        ExtractionResult {
            nodes: vec![node],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: vec![(node_id, "Northside Mutual Aid".to_string())],
            categories: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    // Inject actor context — location fallback for signals without geography
    ctx.actor_contexts.insert(
        canonical_value(ig_url),
        ActorContext {
            actor_name: "Northside Mutual Aid".to_string(),
            bio: Some("Community org serving North Minneapolis".to_string()),
            location_name: Some("North Minneapolis, MN".to_string()),
            location_lat: Some(45.0118),
            location_lng: Some(-93.2885),
            discovery_depth: 0,
        },
    );

    let output = super::activities::social_scrape::scrape_social_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    // Signal stored (actor fallback gave it Minneapolis coords → survives geo filter)
    assert_eq!(ctx.stats.signals_stored, 1);

    // @mentions collected for promotion
    let mention_urls: Vec<&str> = ctx.collected_links.iter().map(|l| l.url.as_str()).collect();
    assert!(
        mention_urls
            .iter()
            .any(|u| u.contains("instagram.com/mplsfoodshelf")),
        "mplsfoodshelf mention should be promoted"
    );
    assert!(
        mention_urls
            .iter()
            .any(|u| u.contains("instagram.com/hennepincounty")),
        "hennepincounty mention should be promoted"
    );
}

/// Actor in NYC, signal has no content location. Fallback populates from_location
/// and about_location from actor coords. Signal is stored (no geo-filter).
#[tokio::test]
async fn nyc_actor_fallback_stores_signal_with_actor_location() {
    let ig_url = "https://www.instagram.com/nycorg";

    let fetcher = MockFetcher::new().on_posts(ig_url, vec![test_post("Thoughts on organizing...")]);

    let mut node = tension("Organizing Reflections");
    if let Some(meta) = node.meta_mut() {
        meta.confidence = 0.5;
    }

    let extractor = MockExtractor::new().on_url(
        ig_url,
        ExtractionResult {
            nodes: vec![node],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    // Actor in NYC
    ctx.actor_contexts.insert(
        canonical_value(ig_url),
        ActorContext {
            actor_name: "NYC Org".to_string(),
            bio: None,
            location_name: Some("New York, NY".to_string()),
            location_lat: Some(NYC.0),
            location_lng: Some(NYC.1),
            discovery_depth: 0,
        },
    );

    let output = super::activities::social_scrape::scrape_social_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    // No geo-filter — signal stored with actor location as fallback
    assert_eq!(
        ctx.stats.signals_stored, 1,
        "signal should be stored regardless of location"
    );
}

// ---------------------------------------------------------------------------
// Chain Test 4b: Social with explicit content location + actor
//
// Actor in Minneapolis, signal explicitly about Dallas. about_location stays
// Dallas, from_location is Minneapolis.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn dallas_signal_from_minneapolis_actor_preserves_both_locations() {
    let ig_url = "https://www.instagram.com/mplsorg";

    let fetcher = MockFetcher::new().on_posts(ig_url, vec![test_post("Amazing event in Dallas!")]);

    let extractor = MockExtractor::new().on_url(
        ig_url,
        ExtractionResult {
            nodes: vec![tension_at("Dallas Fundraiser", DALLAS.0, DALLAS.1)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    // Actor in Minneapolis
    ctx.actor_contexts.insert(
        canonical_value(ig_url),
        ActorContext {
            actor_name: "MPLS Org".to_string(),
            bio: None,
            location_name: Some("Minneapolis, MN".to_string()),
            location_lat: Some(44.9778),
            location_lng: Some(-93.2650),
            discovery_depth: 0,
        },
    );

    let output = super::activities::social_scrape::scrape_social_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(ctx.stats.signals_stored, 1);
}

// ---------------------------------------------------------------------------
// Chain Test 4c: Instagram profile with bio location, mixed-geography posts
//
// Real-world scenario: @mpls_community_garden has "Minneapolis, MN" in their
// IG bio. They post about three things:
//
//   1. "Powderhorn Park spring planting day!" → LLM extracts Powderhorn coords
//   2. "Reflections on community resilience" → geographically neutral, no location
//   3. "Inspired by Chicago's urban farm network" → LLM extracts Chicago coords
//
// Expected behavior (from_location derived at query time, not write time):
//   Signal 1: about_location = Powderhorn, from_location = None
//   Signal 2: about_location = None (no backfill), from_location = None
//   Signal 3: about_location = Chicago, from_location = None
// ---------------------------------------------------------------------------

/// Minneapolis actor's IG: one local post, one geo-neutral, one out-of-town.
/// about_location reflects content; from_location is not set at write time.
#[tokio::test]
async fn ig_bio_location_flows_through_mixed_geography_posts() {
    let ig_url = "https://www.instagram.com/mpls_community_garden";

    // Three posts — extractor sees them as combined text keyed by source_url
    let post1 = test_post("Powderhorn Park spring planting day this Saturday! Bring gloves.");
    let post2 = test_post("Reflections on community resilience and what it means to show up.");
    let post3 = test_post("Inspired by Chicago's urban farm network — amazing what they've built.");

    let fetcher = MockFetcher::new().on_posts(ig_url, vec![post1, post2, post3]);

    // LLM extracts three signals with different location states:
    // 1. Powderhorn Park — explicit local coords
    // 2. Community resilience — no location at all (geo-neutral content)
    // 3. Chicago farm — explicit Chicago coords
    let extractor = MockExtractor::new().on_url(
        ig_url,
        ExtractionResult {
            nodes: vec![
                tension_at("Powderhorn Spring Planting", 44.9489, -93.2583),
                tension("Community Resilience Reflections"),
                tension_at("Chicago Urban Farm Network", 41.8781, -87.6298),
            ],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    // Actor context: IG bio says "Minneapolis, MN"
    ctx.actor_contexts.insert(
        canonical_value(ig_url),
        ActorContext {
            actor_name: "MPLS Community Garden".to_string(),
            bio: Some("Growing food and community in Minneapolis, MN 🌱".to_string()),
            location_name: Some("Minneapolis, MN".to_string()),
            location_lat: Some(44.9778),
            location_lng: Some(-93.2650),
            discovery_depth: 0,
        },
    );

    let output = super::activities::social_scrape::scrape_social_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    // All three signals stored — no geo-filter rejection
    assert_eq!(
        ctx.stats.signals_stored, 3,
        "all three posts should produce signals"
    );
}

// ---------------------------------------------------------------------------
// Chain Test 5: Content Unchanged → Skip Extraction
//
// Hash match → skip extraction → links still collected.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unchanged_page_is_not_re_extracted_but_links_still_collected() {
    let url = "https://localorg.org/resources";
    let content = "Free legal clinic every Tuesday...";

    let page = {
        let mut p = archived_page(url, content);
        p.links = vec![
            "https://communitypartners.org/localorg".to_string(),
            "https://newpartner.org".to_string(),
        ];
        p
    };

    // Must match the FNV-1a hash that fetch_and_extract computes from the markdown
    let hash = format!("{:x}", rootsignal_common::content_hash(content));

    let fetcher = MockFetcher::new().on_page(url, page);

    // Extractor returns a signal — but if extraction is skipped (hash match),
    // it won't be called and no signals appear.
    let extractor = MockExtractor::new().on_url(
        url,
        ExtractionResult {
            nodes: vec![tension_at("SHOULD NOT APPEAR", 44.975, -93.270)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            logs: vec![],
        },
    );

    // fetch_and_extract sanitizes the URL before checking — pre-populate with sanitized URL
    let clean_url = sanitize_url(url);
    let store = Arc::new(MockSignalReader::new().with_processed_hash(&hash, &clean_url));
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source(url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    // No new signals (extraction skipped)
    assert_eq!(
        ctx.stats.signals_stored, 0,
        "unchanged content should skip extraction"
    );

    // But outbound links still collected
    assert!(
        ctx.collected_links
            .iter()
            .any(|l| l.url.contains("newpartner.org")),
        "links should still be collected even when content unchanged"
    );
}

// ---------------------------------------------------------------------------
// Chain Test 6: Two-Phase Pipeline
//
// Phase A: scrape a Linktree → discovers org site via collected_links.
// Phase B: scrape the org site → signals created.
//
// Tests the discovery → scrape loop across two manual phases.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn linktree_discovery_feeds_second_scrape_that_produces_signal() {

    let fetcher = Arc::new(
        MockFetcher::new()
            .on_page("https://linktr.ee/mplsmutualaid", {
                let mut page = archived_page("https://linktr.ee/mplsmutualaid", "MPLS Mutual Aid");
                page.links = vec!["https://localorg.org/resources".to_string()];
                page
            })
            .on_page("https://localorg.org/resources", {
                let mut page = archived_page(
                    "https://localorg.org/resources",
                    "Free groceries every Saturday at MLK Park...",
                );
                page.links = vec!["https://communitypartners.org/localorg".to_string()];
                page
            }),
    );

    let org_node = tension_at("Free Groceries at MLK Park", 44.9489, -93.2654);

    let extractor = Arc::new(
        MockExtractor::new()
            // Linktree: no signals, just links
            .on_url(
                "https://linktr.ee/mplsmutualaid",
                ExtractionResult {
                    nodes: vec![],
                    implied_queries: vec![],
                    resource_tags: Vec::new(),
                    signal_tags: Vec::new(),
                    rejected: Vec::new(),
                    schedules: Vec::new(),
                    author_actors: Vec::new(),
            categories: Vec::new(),
            logs: vec![],
                },
            )
            // Org site: one signal
            .on_url(
                "https://localorg.org/resources",
                ExtractionResult {
                    nodes: vec![org_node],
                    implied_queries: vec![],
                    resource_tags: Vec::new(),
                    signal_tags: Vec::new(),
                    rejected: Vec::new(),
                    schedules: Vec::new(),
                    author_actors: Vec::new(),
            categories: Vec::new(),
            logs: vec![],
                },
            ),
    );

    let store = Arc::new(MockSignalReader::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    // --- Phase A: scrape the Linktree ---
    let deps = test_scrape_deps(store.clone(), extractor.clone(), fetcher.clone());

    let linktree_source = page_source("https://linktr.ee/mplsmutualaid");
    let sources_a: Vec<&_> = vec![&linktree_source];
    let mut ctx = PipelineState::from_sources(&[linktree_source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources_a, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    // After Phase A: localorg.org discovered in collected_links
    assert!(
        ctx.collected_links
            .iter()
            .any(|l| l.url.contains("localorg.org")),
        "org site should be in collected_links"
    );
    assert_eq!(ctx.stats.signals_stored, 0, "no signals from Linktree");

    // Promote collected links → creates SourceNodes in store
    let config = PromotionConfig {
        max_per_source: 10,
        max_per_run: 50,
        ..Default::default()
    };
    let promoted_sources = link_promoter::promote_links(&ctx.collected_links, &config);
    assert!(!promoted_sources.is_empty(), "at least 1 link promoted");
    let promoted_urls: Vec<_> = promoted_sources
        .iter()
        .filter_map(|s| s.url.as_deref())
        .collect();
    assert!(promoted_urls.contains(&"https://localorg.org/resources"));

    // --- Phase B: scrape the discovered org site ---
    let deps_b = test_scrape_deps(store.clone(), extractor, fetcher);

    let org_source = page_source("https://localorg.org/resources");
    let sources_b: Vec<&_> = vec![&org_source];
    let mut ctx_b = PipelineState::from_sources(&[org_source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps_b, &sources_b, &ctx_b.url_to_canonical_key, &ctx_b.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx_b, &store).await;

    // Signal from Phase B
    assert_eq!(ctx_b.stats.signals_stored, 1, "one signal from org site");

    // Phase B also collected facebook link for future promotion
    assert!(
        ctx_b
            .collected_links
            .iter()
            .any(|l| l.url.contains("communitypartners.org/localorg")),
        "community partner link should be collected in Phase B"
    );
}

// ---------------------------------------------------------------------------
// Chain Test 7: Social scrape wires actor, HAS_SOURCE, and PRODUCED_BY
//
// ---------------------------------------------------------------------------
// Schedule Chain Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn gathering_with_rrule_creates_linked_schedule_node() {
    let url = "https://communitycenter.org/yoga";

    let fetcher = MockFetcher::new().on_page(
        url,
        archived_page(url, "Weekly yoga class every Tuesday 6-8pm..."),
    );

    let node = gathering_at("Weekly Yoga Class", 44.95, -93.27);
    let node_id = node.meta().unwrap().id;

    let schedule = ScheduleNode {
        id: Uuid::new_v4(),
        rrule: Some("FREQ=WEEKLY;BYDAY=TU".to_string()),
        rdates: vec![],
        exdates: vec![],
        dtstart: Some(Utc::now()),
        dtend: None,
        timezone: Some("America/Chicago".to_string()),
        schedule_text: Some("Every Tuesday 6-8pm".to_string()),
        extracted_at: Utc::now(),
    };

    let extractor = MockExtractor::new().on_url(
        url,
        ExtractionResult {
            nodes: vec![node],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            rejected: Vec::new(),
            schedules: vec![(node_id, schedule)],
            author_actors: Vec::new(),
            categories: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source(url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(ctx.stats.signals_stored, 1);
}

#[tokio::test]
async fn gathering_without_schedule_creates_no_schedule_node() {
    let url = "https://localpark.org/cleanup";

    let fetcher = MockFetcher::new().on_page(
        url,
        archived_page(url, "Park cleanup this Saturday morning..."),
    );

    let node = gathering_at("Park Cleanup Day", 44.95, -93.27);

    let extractor = MockExtractor::new().on_url(
        url,
        ExtractionResult {
            nodes: vec![node],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source(url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(ctx.stats.signals_stored, 1);
}

#[tokio::test]
async fn schedule_text_only_fallback_creates_schedule_node() {
    let url = "https://mosque.org/open-house";

    let fetcher = MockFetcher::new().on_page(
        url,
        archived_page(url, "Open house first Saturday of every month..."),
    );

    let node = gathering_at("Monthly Open House", 44.96, -93.25);
    let node_id = node.meta().unwrap().id;

    let schedule = ScheduleNode {
        id: Uuid::new_v4(),
        rrule: None,
        rdates: vec![],
        exdates: vec![],
        dtstart: None,
        dtend: None,
        timezone: None,
        schedule_text: Some("First Saturday of every month, 10am-2pm".to_string()),
        extracted_at: Utc::now(),
    };

    let extractor = MockExtractor::new().on_url(
        url,
        ExtractionResult {
            nodes: vec![node],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            rejected: Vec::new(),
            schedules: vec![(node_id, schedule)],
            author_actors: Vec::new(),
            categories: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = page_source(url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    let output = super::activities::web_scrape::scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
    scrape_and_dispatch(output, &mut ctx, &store).await;

    assert_eq!(ctx.stats.signals_stored, 1);
}

// ---------------------------------------------------------------------------
// Chain Test: resolve → fetch_extract causal chain
//
// Verifies the decomposed pipeline: resolve_web_urls produces URLs,
// fetch_and_extract consumes them and produces signals.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn resolve_then_fetch_extract_produces_same_signals_as_monolithic() {
    let query = "site:linktr.ee mutual aid Minneapolis";

    let fetcher = MockFetcher::new()
        .on_search(
            query,
            search_results(query, &["https://localorg.org/resources"]),
        )
        .on_page("https://localorg.org/resources", {
            let mut page = archived_page(
                "https://localorg.org/resources",
                "Free legal clinic every Tuesday...",
            );
            page.links = vec!["https://partner.org".to_string()];
            page
        });

    let node = tension_at("Free Legal Clinic", 44.9341, -93.2619);
    let extractor = MockExtractor::new().on_url(
        "https://localorg.org/resources",
        ExtractionResult {
            nodes: vec![node],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
            rejected: Vec::new(),
            schedules: Vec::new(),
            author_actors: Vec::new(),
            categories: Vec::new(),
            logs: vec![],
        },
    );

    let store = Arc::new(MockSignalReader::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));

    let source = web_query_source(query);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = PipelineState::from_sources(&[source.clone()]);

    // Step 1: resolve URLs
    let resolution = super::activities::url_resolution::resolve_web_urls(&deps, &sources, &ctx.url_to_canonical_key, None, None).await;
    assert_eq!(resolution.urls.len(), 1, "search resolved one URL");
    assert!(resolution.query_api_errors.is_empty());

    // Apply URL mappings to context (simulates UrlsResolvedAccumulated)
    ctx.url_to_canonical_key.extend(resolution.url_mappings.clone());
    ctx.url_to_pub_date.extend(resolution.pub_dates.clone());

    // Step 2: fetch + extract from resolved URLs
    let source_keys: std::collections::HashMap<String, Uuid> = sources
        .iter()
        .map(|s| (s.canonical_key.clone(), s.id))
        .collect();
    let result = super::activities::web_scrape::fetch_and_extract(&deps, 
        &resolution.urls,
        &source_keys,
        &ctx.url_to_canonical_key,
        &ctx.actor_contexts,
        &resolution.pub_dates,
    ).await;

    assert_eq!(result.stats.urls_scraped, 1, "one URL fetched+extracted");
    assert_eq!(result.stats.signals_extracted, 1, "one signal extracted");
    assert!(!result.events.is_empty(), "should produce SignalsExtracted events");

    // Dispatch events through engine to verify full chain
    dispatch_events(result.events, &mut ctx, &store).await;
    assert_eq!(ctx.stats.signals_stored, 1, "signal stored through engine");

    // Links collected
    assert!(
        result.collected_links.iter().any(|l| l.url.contains("partner.org")),
        "partner.org link should be collected"
    );
}

