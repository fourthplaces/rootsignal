//! Chain tests — end-to-end with mocks.
//!
//! Each test follows MOCK → FUNCTION → OUTPUT:
//! set up the fake external world, call the ACTUAL organ, assert what came out.
//! We never reach into the organ and call its internal functions.

use std::sync::Arc;

use rootsignal_common::types::ActorContext;
use rootsignal_common::canonical_value;

use crate::infra::run_log::RunLog;
use crate::pipeline::extractor::ExtractionResult;
use crate::pipeline::scrape_phase::{RunContext, ScrapePhase};
use crate::testing::*;

fn run_log() -> RunLog {
    RunLog::new("test-run".to_string(), "Minneapolis".to_string())
}

// ---------------------------------------------------------------------------
// Chain Test 1: Linktree Discovery
//
// search "site:linktr.ee mutual aid Minneapolis" → results → fetch each
// result page → extract links. Junk filtered, tracking stripped.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn linktree_search_collects_outbound_links() {
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
    let embedder = Arc::new(FixedEmbedder::new(64));

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
async fn page_produces_signal_with_actors_and_evidence() {
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
    let embedder = Arc::new(FixedEmbedder::new(64));

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

    // Actors wired
    assert!(store.has_actor("Sabathani Community Center"), "author actor created");
    assert!(store.has_actor("Volunteer Lawyers Network"), "mentioned actor created");
    assert!(
        store.actor_linked_to_signal("Sabathani Community Center", "Free Legal Clinic at Sabathani"),
        "author actor linked to signal"
    );
    assert!(
        store.actor_linked_to_signal("Volunteer Lawyers Network", "Free Legal Clinic at Sabathani"),
        "mentioned actor linked to signal"
    );

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

/// Signal in Dallas extracted from a page. Minneapolis scout filters it.
#[tokio::test]
async fn out_of_region_signal_produces_nothing() {
    let url = "https://texasorg.org/events";

    let fetcher = MockFetcher::new()
        .on_page(url, archived_page(url, "Dallas community dinner..."));

    let extractor = MockExtractor::new()
        .on_url(url, ExtractionResult {
            nodes: vec![tension_at("Dallas Community Dinner", 32.7767, -96.7970)],
            implied_queries: vec![],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
        });

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

    let source = page_source(url);
    let sources: Vec<&_> = vec![&source];
    let mut ctx = RunContext::new(&[source.clone()]);
    let mut log = run_log();

    phase.run_web(&sources, &mut ctx, &mut log).await;

    // Geo filter killed it
    assert_eq!(store.signals_created(), 0);
}

// ---------------------------------------------------------------------------
// Chain Test 3: Multi-Source Corroboration
//
// 3 pages describe the same event → run_web → 1 signal, corroborations,
// evidence trails from each source.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn three_sources_corroborate_to_one_signal() {
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
        FixedEmbedder::new(64)
            .on_text("Community Garden Cleanup ", vec![0.5f32; 64]),
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
async fn instagram_posts_get_actor_fallback_and_mentions_collected() {
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
        meta.location_name = Some("Minneapolis, MN".to_string());
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
    let embedder = Arc::new(FixedEmbedder::new(64));

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

/// Actor in NYC, signal has no location. Fallback puts it in NYC → filtered.
#[tokio::test]
async fn out_of_region_actor_fallback_gets_filtered() {
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
    let embedder = Arc::new(FixedEmbedder::new(64));

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
            location_lat: Some(40.7128),
            location_lng: Some(-74.0060),
        },
    );

    let mut log = run_log();
    phase.run_social(&sources, &mut ctx, &mut log).await;

    // Fallback put it in NYC → filtered by Minneapolis scout
    assert_eq!(store.signals_created(), 0, "NYC signal should be geo-filtered");
}

// ---------------------------------------------------------------------------
// Chain Test 5: Content Unchanged → Skip Extraction
//
// Hash match → skip extraction → links still collected.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unchanged_content_skips_extraction_but_collects_links() {
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
    let embedder = Arc::new(FixedEmbedder::new(64));

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
async fn phase_a_discovers_source_phase_b_scrapes_it() {
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
    let embedder = Arc::new(FixedEmbedder::new(64));

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
    assert!(store.has_actor("Minneapolis Mutual Aid"));

    // Phase B also collected facebook link for future promotion
    assert!(
        ctx_b.collected_links.iter().any(|l| l.url.contains("facebook.com/localorg")),
        "facebook link should be collected in Phase B"
    );
}
