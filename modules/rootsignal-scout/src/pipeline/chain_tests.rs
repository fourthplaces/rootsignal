//! Chain tests — end-to-end with mocks.
//!
//! Each test follows MOCK → FUNCTION → OUTPUT:
//! set up the fake external world, call the ACTUAL organ, assert what came out.
//! We never reach into the organ and call its internal functions.

use std::sync::Arc;

use rootsignal_common::types::ActorContext;
use rootsignal_common::canonical_value;

use crate::pipeline::extractor::ExtractionResult;
use crate::pipeline::scrape_phase::{RunContext, ScrapePhase};
use crate::testing::*;

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
        .on_search(query, search_results(query, &[
            "https://linktr.ee/mplsmutualaid",
            "https://linktr.ee/northsideaid",
        ]))
        .on_page(
            "https://linktr.ee/mplsmutualaid",
            {
                let mut page = archived_page("https://linktr.ee/mplsmutualaid", "MPLS Mutual Aid");
                page.links = vec![
                    "https://instagram.com/mplsmutualaid".to_string(),
                    "https://gofundme.com/f/help-families?utm_source=linktree".to_string(),
                    "https://localorg.org/resources".to_string(),
                    "https://fonts.googleapis.com/css2?family=Inter".to_string(), // .css → filtered
                ];
                page
            },
        )
        .on_page(
            "https://linktr.ee/northsideaid",
            {
                let mut page = archived_page("https://linktr.ee/northsideaid", "Northside Aid");
                page.links = vec![
                    "https://northsideaid.org/volunteer".to_string(),
                ];
                page
            },
        );

    // Linktree pages: no signals, just links
    let extractor = MockExtractor::new()
        .on_url("https://linktr.ee/mplsmutualaid", ExtractionResult {
            nodes: vec![],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
        })
        .on_url("https://linktr.ee/northsideaid", ExtractionResult {
            nodes: vec![],
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

    let source = web_query_source(query);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    // No signals from Linktree pages
    assert_eq!(store.signals_created(), 0);

    let collected_urls: Vec<&str> = ctx.collected_links.iter().map(|l| l.url.as_str()).collect();

    // Content links collected
    assert!(
        collected_urls.iter().any(|u| u.contains("instagram.com/mplsmutualaid")),
        "Instagram should be collected"
    );
    assert!(
        collected_urls.iter().any(|u| u.contains("localorg.org/resources")),
        "Org site should be collected"
    );
    assert!(
        collected_urls.iter().any(|u| u.contains("northsideaid.org/volunteer")),
        "Northside org should be collected"
    );

    // GoFundMe collected with tracking stripped
    let gf = collected_urls.iter().find(|u| u.contains("gofundme.com"));
    assert!(gf.is_some(), "GoFundMe should be collected");
    assert!(!gf.unwrap().contains("utm_source"), "Tracking params should be stripped");
}

// ---------------------------------------------------------------------------
// Chain Test 2: Page → Signal → Actors → Evidence
//
// page source → run_web → signal created, actors wired, evidence linked.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn page_creates_signal_wires_actors_and_records_evidence() {
    let url = "https://localorg.org/resources";

    let fetcher = MockFetcher::new()
        .on_page(url, {
            let mut page = archived_page(url, "Free legal clinic every Tuesday at Sabathani Center...");
            page.links = vec![
                "https://facebook.com/localorg".to_string(),
                "https://sabathani.org/events".to_string(),
            ];
            page
        });

    let mut node = tension_at("Free Legal Clinic at Sabathani", 44.9341, -93.2619);
    if let Some(meta) = node.meta_mut() {
        meta.mentioned_actors = vec!["Volunteer Lawyers Network".to_string()];
        meta.author_actor = Some("Sabathani Community Center".to_string());
    }

    let extractor = MockExtractor::new()
        .on_url(url, ExtractionResult {
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

    let source = page_source(url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    // Signal created
    assert_eq!(store.signals_created(), 1);
    assert!(store.has_signal_titled("Free Legal Clinic at Sabathani"));

    // Web page is NOT an owned source — no actor nodes created.
    // Mentioned actors stay as metadata strings; author_actor ignored for non-owned sources.
    assert!(!store.has_actor("Sabathani Community Center"), "web page source should not create author actor");
    assert!(!store.has_actor("Volunteer Lawyers Network"), "mentioned actors should not create nodes");

    // Evidence trail
    assert_eq!(
        store.evidence_count_for_title("Free Legal Clinic at Sabathani"),
        1,
        "one evidence record for the signal"
    );

    // Outbound links collected for promotion
    let collected_urls: Vec<&str> = ctx.collected_links.iter().map(|l| l.url.as_str()).collect();
    assert!(collected_urls.iter().any(|u| u.contains("facebook.com/localorg")));
    assert!(collected_urls.iter().any(|u| u.contains("sabathani.org/events")));
}

/// Signal in Dallas extracted from a page. No geo-filter — stored regardless.
#[tokio::test]
async fn dallas_signal_is_stored_by_minneapolis_scout() {
    let url = "https://texasorg.org/events";

    let fetcher = MockFetcher::new()
        .on_page(url, archived_page(url, "Dallas community dinner..."));

    let extractor = MockExtractor::new()
        .on_url(url, ExtractionResult {
            nodes: vec![tension_at("Dallas Community Dinner", DALLAS.0, DALLAS.1)],
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

    let source = page_source(url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    // No geo-filter — all signals stored
    assert_eq!(store.signals_created(), 1);
}

// ---------------------------------------------------------------------------
// Chain Test 3: Multi-Source Corroboration
//
// 3 pages describe the same event → run_web → 1 signal, corroborations,
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
        extractor = extractor.on_url(url, ExtractionResult {
            nodes: vec![tension_at("Community Garden Cleanup", 44.9489, -93.2654)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
        });
    }

    // All three signals get near-identical embeddings → vector dedup fires
    let embedder = Arc::new(
        FixedEmbedder::new(TEST_EMBEDDING_DIM)
            .on_text("Community Garden Cleanup ", vec![0.5f32; TEST_EMBEDDING_DIM]),
    );

    let store = Arc::new(MockSignalStore::new());

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source_nodes: Vec<_> = urls.iter().map(|u| page_source(u)).collect();
    let sources: Vec<&_> = source_nodes.iter().collect();
    let mut ctx = RunContext::new(&source_nodes);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    // ONE signal, not three
    assert_eq!(store.signals_created(), 1, "should dedup to 1 signal");

    // Corroborated by the other two
    assert_eq!(
        store.corroborations_for("Community Garden Cleanup"),
        2,
        "two corroborations (first creates, second and third corroborate)"
    );
}

// ---------------------------------------------------------------------------
// Chain Test 4: Social Scrape with Actor Context
//
// Instagram posts + actor_ctx → run_social → signal with fallback location,
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

    let fetcher = MockFetcher::new()
        .on_posts(ig_url, vec![post1, post2]);

    // Extractor returns a signal with no coordinates but a location_name matching
    // a geo-term. This lets it survive geo-filter via name match. Actor fallback
    // then backfills exact coordinates from the actor context.
    let mut node = tension("Food Distribution at MLK Park");
    if let Some(meta) = node.meta_mut() {
        meta.about_location_name = Some("Minneapolis, MN".to_string());
        meta.mentioned_actors = vec!["Minneapolis Food Shelf".to_string()];
        meta.author_actor = Some("Northside Mutual Aid".to_string());
        meta.confidence = 0.7;
    }

    let extractor = MockExtractor::new()
        .on_url(ig_url, ExtractionResult {
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

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);

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

    let mut log = run_log();
    phase.run_social(&sources, &mut ctx, &mut log).await;

    // Signal stored (actor fallback gave it Minneapolis coords → survives geo filter)
    assert_eq!(store.signals_created(), 1);
    assert!(store.has_signal_titled("Food Distribution at MLK Park"));

    // Author actor wired
    assert!(store.has_actor("Northside Mutual Aid"), "author actor created");
    assert!(
        store.actor_linked_to_signal("Northside Mutual Aid", "Food Distribution at MLK Park"),
        "author actor linked to signal"
    );

    // @mentions collected for promotion
    let mention_urls: Vec<&str> = ctx.collected_links.iter().map(|l| l.url.as_str()).collect();
    assert!(
        mention_urls.iter().any(|u| u.contains("instagram.com/mplsfoodshelf")),
        "mplsfoodshelf mention should be promoted"
    );
    assert!(
        mention_urls.iter().any(|u| u.contains("instagram.com/hennepincounty")),
        "hennepincounty mention should be promoted"
    );
}

/// Actor in NYC, signal has no content location. Fallback populates from_location
/// and about_location from actor coords. Signal is stored (no geo-filter).
#[tokio::test]
async fn nyc_actor_fallback_stores_signal_with_actor_location() {
    let ig_url = "https://www.instagram.com/nycorg";

    let fetcher = MockFetcher::new()
        .on_posts(ig_url, vec![test_post("Thoughts on organizing...")]);

    let mut node = tension("Organizing Reflections");
    if let Some(meta) = node.meta_mut() {
        meta.confidence = 0.5;
    }

    let extractor = MockExtractor::new()
        .on_url(ig_url, ExtractionResult {
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

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);

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

    let mut log = run_log();
    phase.run_social(&sources, &mut ctx, &mut log).await;

    // No geo-filter — signal stored with actor location as fallback
    assert_eq!(store.signals_created(), 1, "signal should be stored regardless of location");

    // Location metadata: no backfill at write time (derived at query time via actor graph)
    let stored = store.signal_by_title("Organizing Reflections").unwrap();
    assert!(stored.about_location.is_none(), "about_location not backfilled from actor at write time");
    assert!(stored.from_location.is_none(), "from_location not set at write time");
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

    let fetcher = MockFetcher::new()
        .on_posts(ig_url, vec![test_post("Amazing event in Dallas!")]);

    let extractor = MockExtractor::new()
        .on_url(ig_url, ExtractionResult {
            nodes: vec![tension_at("Dallas Fundraiser", DALLAS.0, DALLAS.1)],
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

    let mut log = run_log();
    phase.run_social(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 1);
    let stored = store.signal_by_title("Dallas Fundraiser").unwrap();

    // about_location = Dallas (from content, NOT overwritten by actor)
    let about = stored.about_location.expect("about_location should be Dallas");
    assert!((about.lat - DALLAS.0).abs() < 0.001, "about_location should be Dallas, not Minneapolis");

    // from_location not set at write time (derived at query time via actor graph)
    assert!(stored.from_location.is_none(), "from_location not set at write time");
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

    let fetcher = MockFetcher::new()
        .on_posts(ig_url, vec![post1, post2, post3]);

    // LLM extracts three signals with different location states:
    // 1. Powderhorn Park — explicit local coords
    // 2. Community resilience — no location at all (geo-neutral content)
    // 3. Chicago farm — explicit Chicago coords
    let extractor = MockExtractor::new()
        .on_url(ig_url, ExtractionResult {
            nodes: vec![
                tension_at("Powderhorn Spring Planting", 44.9489, -93.2583),
                tension("Community Resilience Reflections"),
                tension_at("Chicago Urban Farm Network", 41.8781, -87.6298),
            ],
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

    let mut log = run_log();
    phase.run_social(&sources, &mut ctx, &mut log).await;

    // All three signals stored — no geo-filter rejection
    assert_eq!(store.signals_created(), 3, "all three posts should produce signals");

    // --- Signal 1: Powderhorn (explicit local location) ---
    let powderhorn = store.signal_by_title("Powderhorn Spring Planting")
        .expect("powderhorn signal should exist");
    let about = powderhorn.about_location.expect("about_location should be Powderhorn coords");
    assert!(
        (about.lat - 44.9489).abs() < 0.001,
        "about_location should be Powderhorn, not actor fallback"
    );
    assert!(powderhorn.from_location.is_none(), "from_location not set at write time");

    // --- Signal 2: Geo-neutral (no content location, no backfill) ---
    let reflections = store.signal_by_title("Community Resilience Reflections")
        .expect("reflections signal should exist");
    assert!(
        reflections.about_location.is_none(),
        "geo-neutral post: about_location not backfilled from actor at write time"
    );
    assert!(reflections.from_location.is_none(), "from_location not set at write time");

    // --- Signal 3: Chicago (explicit out-of-town location) ---
    let chicago = store.signal_by_title("Chicago Urban Farm Network")
        .expect("chicago signal should exist");
    let about = chicago.about_location.expect("about_location should be Chicago coords");
    assert!(
        (about.lat - 41.8781).abs() < 0.001,
        "about_location should be Chicago, NOT overwritten by Minneapolis actor"
    );
    assert!(chicago.from_location.is_none(), "from_location not set at write time");
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
            "https://facebook.com/localorg".to_string(),
            "https://newpartner.org".to_string(),
        ];
        p
    };

    // Must match the FNV-1a hash that run_web computes from the markdown
    let hash = format!("{:x}", rootsignal_common::content_hash(content));

    let fetcher = MockFetcher::new().on_page(url, page);

    // Extractor returns a signal — but if extraction is skipped (hash match),
    // it won't be called and no signals appear.
    let extractor = MockExtractor::new()
        .on_url(url, ExtractionResult {
            nodes: vec![tension_at("SHOULD NOT APPEAR", 44.975, -93.270)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
        });

    // run_web sanitizes the URL before checking — pre-populate with sanitized URL
    let clean_url = crate::infra::util::sanitize_url(url);
    let store = Arc::new(MockSignalStore::new().with_processed_hash(&hash, &clean_url));
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    let phase = ScrapePhase::new(
        store.clone(),
        Arc::new(extractor),
        embedder,
        Arc::new(fetcher),
        mpls_region(),
        "test-run".to_string(),
    );

    let source = page_source(url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    // No new signals (extraction skipped)
    assert_eq!(store.signals_created(), 0, "unchanged content should skip extraction");
    assert!(!store.has_signal_titled("SHOULD NOT APPEAR"));

    // But outbound links still collected
    assert!(
        ctx.collected_links.iter().any(|l| l.url.contains("newpartner.org")),
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
    use crate::enrichment::link_promoter::{self, PromotionConfig};

    let fetcher = Arc::new(
        MockFetcher::new()
            .on_page("https://linktr.ee/mplsmutualaid", {
                let mut page = archived_page("https://linktr.ee/mplsmutualaid", "MPLS Mutual Aid");
                page.links = vec!["https://localorg.org/resources".to_string()];
                page
            })
            .on_page("https://localorg.org/resources", {
                let mut page = archived_page("https://localorg.org/resources", "Free groceries every Saturday at MLK Park...");
                page.links = vec!["https://facebook.com/localorg".to_string()];
                page
            }),
    );

    let mut org_node = tension_at("Free Groceries at MLK Park", 44.9489, -93.2654);
    if let Some(meta) = org_node.meta_mut() {
        meta.author_actor = Some("Minneapolis Mutual Aid".to_string());
    }

    let extractor = Arc::new(
        MockExtractor::new()
            // Linktree: no signals, just links
            .on_url("https://linktr.ee/mplsmutualaid", ExtractionResult {
                nodes: vec![],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            })
            // Org site: one signal
            .on_url("https://localorg.org/resources", ExtractionResult {
                nodes: vec![org_node],
                implied_queries: vec![],
                resource_tags: Vec::new(),
                signal_tags: Vec::new(),
            }),
    );

    let store = Arc::new(MockSignalStore::new());
    let embedder = Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM));

    // --- Phase A: scrape the Linktree ---
    let phase_a = ScrapePhase::new(
        store.clone(),
        extractor.clone(),
        embedder.clone(),
        fetcher.clone(),
        mpls_region(),
        "test-run".to_string(),
    );

    let linktree_source = page_source("https://linktr.ee/mplsmutualaid");
    let sources_a: Vec<&_> = vec![&linktree_source];
    let mut ctx = RunContext::new(&[linktree_source.clone()]);
    let mut log = run_log();

    phase_a.run_web(&sources_a, &mut ctx, &mut log).await;

    // After Phase A: localorg.org discovered in collected_links
    assert!(
        ctx.collected_links.iter().any(|l| l.url.contains("localorg.org")),
        "org site should be in collected_links"
    );
    assert_eq!(store.signals_created(), 0, "no signals from Linktree");

    // Promote collected links → creates SourceNodes in store
    let config = PromotionConfig { max_per_source: 10, max_per_run: 50 };
    let promoted = link_promoter::promote_links(
        &ctx.collected_links,
        store.as_ref(),
        &config,
    )
    .await
    .unwrap();
    assert!(promoted >= 1, "at least 1 link promoted");
    assert!(store.has_source_url("https://localorg.org/resources"));

    // --- Phase B: scrape the discovered org site ---
    let phase_b = ScrapePhase::new(
        store.clone(),
        extractor,
        embedder,
        fetcher,
        mpls_region(),
        "test-run".to_string(),
    );

    let org_source = page_source("https://localorg.org/resources");
    let sources_b: Vec<&_> = vec![&org_source];
    let mut ctx_b = RunContext::new(&[org_source.clone()]);
    let mut log_b = run_log();

    phase_b.run_web(&sources_b, &mut ctx_b, &mut log_b).await;

    // Signal from Phase B
    assert_eq!(store.signals_created(), 1, "one signal from org site");
    assert!(store.has_signal_titled("Free Groceries at MLK Park"));
    // Web page is not an owned source — no actor node created
    assert!(!store.has_actor("Minneapolis Mutual Aid"));

    // Phase B also collected facebook link for future promotion
    assert!(
        ctx_b.collected_links.iter().any(|l| l.url.contains("facebook.com/localorg")),
        "facebook link should be collected in Phase B"
    );
}

// ---------------------------------------------------------------------------
// Chain Test 7: Social scrape wires actor, HAS_SOURCE, and PRODUCED_BY
//
// Full flywheel wiring: social source → fetch posts → extract → store.
// The signal gets a PRODUCED_BY edge to the source. The author actor gets
// an entity_id derived from the source URL, a HAS_SOURCE edge, and an
// ACTED_IN ("authored") link to the signal.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn social_scrape_creates_actor_with_has_source_and_produced_by() {
    let ig_url = "https://www.instagram.com/friendsfalls";

    let fetcher = MockFetcher::new()
        .on_posts(ig_url, vec![test_post("Spring cleanup at Minnehaha Falls!")]);

    let mut node = tension_at("Minnehaha Falls Spring Cleanup", 44.9154, -93.2114);
    if let Some(meta) = node.meta_mut() {
        meta.author_actor = Some("Friends of the Falls".to_string());
    }

    let extractor = MockExtractor::new()
        .on_url(ig_url, ExtractionResult {
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

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_social(&sources, &mut ctx, &mut log).await;

    // Signal created
    assert_eq!(store.signals_created(), 1);
    assert!(store.has_signal_titled("Minnehaha Falls Spring Cleanup"));

    // PRODUCED_BY: signal → source
    assert!(
        store.signal_has_source("Minnehaha Falls Spring Cleanup", source.id),
        "signal should have PRODUCED_BY edge to its source"
    );

    // Actor created with URL-based entity_id
    assert!(store.has_actor("Friends of the Falls"), "owned source should create author actor");
    let entity_id = store.actor_entity_id("Friends of the Falls")
        .expect("actor should have entity_id");
    assert_eq!(entity_id, canonical_value(ig_url), "entity_id should be canonical source URL");

    // HAS_SOURCE: actor → source
    assert!(
        store.actor_has_source("Friends of the Falls", source.id),
        "actor should have HAS_SOURCE edge to its source"
    );

    // ACTED_IN: actor → signal (role: "authored")
    assert!(
        store.actor_linked_to_signal("Friends of the Falls", "Minnehaha Falls Spring Cleanup"),
        "actor should be linked to signal with authored role"
    );
}

// (Chain Test 8 deleted — tested function directly, not through the organ)

// ---------------------------------------------------------------------------
// Chain Test 9: Blank author name does not create actor on owned source
//
// Even on a social (owned) source, a blank or whitespace-only author name
// should not produce an actor node.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn blank_author_on_owned_source_does_not_create_actor() {
    let ig_url = "https://www.instagram.com/someorg";

    let fetcher = MockFetcher::new()
        .on_posts(ig_url, vec![test_post("Event happening!")]);

    let mut node = tension("Community Event");
    if let Some(meta) = node.meta_mut() {
        meta.author_actor = Some("  ".to_string());
    }

    let extractor = MockExtractor::new()
        .on_url(ig_url, ExtractionResult {
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

    let source = social_source(ig_url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_social(&sources, &mut ctx, &mut log).await;

    // Signal still created
    assert_eq!(store.signals_created(), 1);

    // But no actor — blank name
    assert_eq!(store.actor_count(), 0, "blank author name should not create actor");
}

// ---------------------------------------------------------------------------
// Chain Test: Scrape social source → enrich → actor gets location
//
// Full pipeline chain: social scrape produces actor + signals with
// about_location, then enrichment triangulates actor location from mode.
// MOCK → ORGAN (run_social, then enrich_actors) → OUTPUT.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn flywheel_scrape_then_enrich_updates_actor_location() {
    use rootsignal_common::types::GeoPoint;
    use rootsignal_common::GeoPrecision;

    let ig_url = "https://www.instagram.com/phillipsorg";

    // MOCK: post content doesn't matter — extractor produces the signals
    let fetcher = MockFetcher::new()
        .on_posts(ig_url, vec![
            test_post("Community event in Phillips"),
            test_post("Another Phillips event"),
            test_post("Powderhorn park day"),
        ]);

    // Extractor produces 3 signals: 2× Phillips, 1× Powderhorn
    // All have about_location + author_actor for actor creation
    let mut sig1 = tension("Phillips Community Dinner");
    if let Some(meta) = sig1.meta_mut() {
        meta.about_location = Some(GeoPoint { lat: 44.9489, lng: -93.2601, precision: GeoPrecision::Approximate });
        meta.about_location_name = Some("Phillips".to_string());
        meta.author_actor = Some("Phillips Community Org".to_string());
    }

    let mut sig2 = tension("Phillips Art Walk");
    if let Some(meta) = sig2.meta_mut() {
        meta.about_location = Some(GeoPoint { lat: 44.9489, lng: -93.2601, precision: GeoPrecision::Approximate });
        meta.about_location_name = Some("Phillips".to_string());
        meta.author_actor = Some("Phillips Community Org".to_string());
    }

    let mut sig3 = tension("Powderhorn Picnic");
    if let Some(meta) = sig3.meta_mut() {
        meta.about_location = Some(GeoPoint { lat: 44.9367, lng: -93.2393, precision: GeoPrecision::Approximate });
        meta.about_location_name = Some("Powderhorn".to_string());
        meta.author_actor = Some("Phillips Community Org".to_string());
    }

    let extractor = MockExtractor::new()
        .on_url(ig_url, ExtractionResult {
            nodes: vec![sig1, sig2, sig3],
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

    // ORGAN: scrape creates actor + signals
    phase.run_social(&sources, &mut ctx, &mut log).await;

    assert_eq!(store.signals_created(), 3, "three signals should be created");
    assert!(store.has_actor("Phillips Community Org"), "actor should be created from author_actor");

    // Actor should have NO location yet (scrape doesn't triangulate)
    assert_eq!(
        store.actor_location_name("Phillips Community Org"),
        None,
        "actor should not have location before enrichment"
    );

    // ORGAN: enrich — phase finds actors and triangulates locations
    phase.enrich_actors().await;

    // OUTPUT: actor's location should now reflect signal mode
    assert_eq!(
        store.actor_location_name("Phillips Community Org"),
        Some("Phillips".to_string()),
        "actor should be placed in Phillips (2 Phillips vs 1 Powderhorn)"
    );
}
