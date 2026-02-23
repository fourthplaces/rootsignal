// =============================================================================
// Chain test examples — testing organs of the pipeline end-to-end with mocks.
//
// Shape of every test:
//
//   1. Set up mocks (the fake external world)
//   2. Call the ACTUAL ORGAN (run_web, run_social, scrape_tension_sources)
//   3. Assert against output state (ctx + MockSignalStore)
//
// We never reach into the organ and call its internal functions. The organ
// does the wiring. We just set up the world, press go, and check what came out.
//
// Won't compile until Phase 2 lands (ContentFetcher/SignalStore traits).
// =============================================================================


// ---------------------------------------------------------------------------
// Shared test helpers
// ---------------------------------------------------------------------------

#[cfg(test)]
mod helpers {
    use rootsignal_common::{ScoutScope, SourceNode, DiscoveryMethod, SourceRole};

    pub fn mpls_region() -> ScoutScope {
        ScoutScope {
            center_lat: 44.9778,
            center_lng: -93.2650,
            radius_km: 30.0,
            name: "minneapolis".into(),
            geo_terms: vec![
                "minneapolis".into(),
                "minnesota".into(),
                "hennepin".into(),
            ],
        }
    }

    pub fn web_query_source(query: &str) -> SourceNode {
        SourceNode::new(
            query.to_string(),
            query.to_string(),
            None,
            DiscoveryMethod::Curated,
            1.0,
            SourceRole::Tension,
            None,
        )
    }

    pub fn page_source(url: &str) -> SourceNode {
        use rootsignal_common::canonical_value;
        SourceNode::new(
            canonical_value(url),
            canonical_value(url),
            Some(url.to_string()),
            DiscoveryMethod::Curated,
            1.0,
            SourceRole::Mixed,
            None,
        )
    }

    pub fn social_source(platform_url: &str) -> SourceNode {
        use rootsignal_common::canonical_value;
        SourceNode::new(
            canonical_value(platform_url),
            canonical_value(platform_url),
            Some(platform_url.to_string()),
            DiscoveryMethod::Curated,
            0.8,
            SourceRole::Mixed,
            None,
        )
    }
}


// ---------------------------------------------------------------------------
// Chain Test 1: Linktree Discovery
//
// search "site:linktr.ee mutual aid" → results → fetch each result → extract links
//
// Organ under test: ScrapePhase::run_web
// Input: query source + mocked search results + mocked Linktree pages
// Output: ctx.collected_links (ready for promotion)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod chain_linktree_discovery {
    use std::sync::Arc;

    use rootsignal_common::*;

    use super::helpers::*;

    #[tokio::test]
    async fn search_finds_linktrees_and_collects_their_outbound_links() {
        // --- MOCK: the external world ---

        let fetcher = MockFetcher::new()
            .on_search("site:linktr.ee mutual aid Minneapolis", vec![
                SearchResult { url: "https://linktr.ee/mplsmutualaid".into(), title: "MPLS Mutual Aid".into(), snippet: "".into() },
                SearchResult { url: "https://linktr.ee/northsideaid".into(), title: "Northside Aid".into(), snippet: "".into() },
            ])
            .on_page("https://linktr.ee/mplsmutualaid", ArchivedPage {
                markdown: "Minneapolis Mutual Aid".into(),
                links: vec![
                    "https://instagram.com/mplsmutualaid".into(),
                    "https://gofundme.com/f/help-families?utm_source=linktree".into(),
                    "https://localorg.org/resources".into(),
                    "https://fonts.googleapis.com/css2?family=Inter".into(),
                ],
                ..Default::default()
            })
            .on_page("https://linktr.ee/northsideaid", ArchivedPage {
                markdown: "Northside Aid".into(),
                links: vec![
                    "https://www.instagram.com/mplsmutualaid/".into(), // same IG, www + trailing slash
                    "https://northsideaid.org/volunteer".into(),
                ],
                ..Default::default()
            });

        // Extractor returns nothing for Linktree pages (no signals, just links)
        let extractor = MockExtractor::empty();
        let store = MockSignalStore::new();
        let embedder = FixedEmbedder::new();
        let region = mpls_region();

        // --- ORGAN: call the real thing ---

        let phase = ScrapePhase::new(
            store.into(),         // via SignalStore trait
            Arc::new(extractor),  // via SignalExtractor trait
            Arc::new(embedder),   // via TextEmbedder trait
            Arc::new(fetcher),    // via ContentFetcher trait
            region,
            "test-run".into(),
        );

        let sources = vec![web_query_source("site:linktr.ee mutual aid Minneapolis")];
        let source_refs: Vec<&SourceNode> = sources.iter().collect();
        let mut ctx = RunContext::new(&sources);
        let mut run_log = RunLog::new("test-run".into(), "minneapolis".into());

        phase.run_web(&source_refs, &mut ctx, &mut run_log).await;

        // --- OUTPUT: assert what came out ---

        // Collected links should contain content URLs from both Linktree pages
        let collected_urls: Vec<&str> = ctx.collected_links.iter().map(|(u, _)| u.as_str()).collect();

        // Instagram collected (from both pages — dedup happens at promote_links, not here)
        assert!(collected_urls.iter().any(|u| u.contains("instagram.com/mplsmutualaid")));

        // GoFundMe collected with tracking stripped
        let gf = collected_urls.iter().find(|u| u.contains("gofundme.com"));
        assert!(gf.is_some(), "GoFundMe should be collected");
        assert!(!gf.unwrap().contains("utm_source"), "Tracking params should be stripped");

        // Org sites collected
        assert!(collected_urls.iter().any(|u| u.contains("localorg.org/resources")));
        assert!(collected_urls.iter().any(|u| u.contains("northsideaid.org/volunteer")));

        // Junk filtered out
        assert!(!collected_urls.iter().any(|u| u.contains("fonts.googleapis.com")));
    }
}


// ---------------------------------------------------------------------------
// Chain Test 2: Page → Signal → Actors → Evidence
//
// Organ under test: ScrapePhase::run_web
// Input: page source + mocked page content + mocked extraction result
// Output: MockSignalStore state (signals, actors, edges, evidence)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod chain_page_to_signal {
    use std::sync::Arc;

    use rootsignal_common::*;

    use super::helpers::*;

    #[tokio::test]
    async fn page_produces_signal_with_actors_and_evidence() {
        // --- MOCK ---

        let fetcher = MockFetcher::new()
            .on_page("https://localorg.org/resources", ArchivedPage {
                markdown: "Free legal clinic every Tuesday at Sabathani Center...".into(),
                links: vec![
                    "https://facebook.com/localorg".into(),
                    "https://sabathani.org/events".into(),
                ],
                ..Default::default()
            });

        let extractor = MockExtractor::new()
            .on_url("https://localorg.org/resources", vec![
                Node::Aid(AidNode {
                    meta: NodeMeta {
                        title: "Free Legal Clinic at Sabathani".into(),
                        location: Some(GeoPoint { lat: 44.9341, lng: -93.2619, precision: GeoPrecision::Exact }),
                        mentioned_actors: vec!["Volunteer Lawyers Network".into()],
                        author_actor: Some("Sabathani Community Center".into()),
                        confidence: 0.8,
                        ..test_meta_defaults("https://localorg.org/resources")
                    },
                    is_free: true,
                    ..Default::default()
                }),
            ]);

        let store = MockSignalStore::new();
        let embedder = FixedEmbedder::new();

        // --- ORGAN ---

        let phase = ScrapePhase::new(
            store.clone().into(),
            Arc::new(extractor),
            Arc::new(embedder),
            Arc::new(fetcher),
            mpls_region(),
            "test-run".into(),
        );

        let sources = vec![page_source("https://localorg.org/resources")];
        let source_refs: Vec<&SourceNode> = sources.iter().collect();
        let mut ctx = RunContext::new(&sources);
        let mut run_log = RunLog::new("test-run".into(), "minneapolis".into());

        phase.run_web(&source_refs, &mut ctx, &mut run_log).await;

        // --- OUTPUT ---

        // Signal created
        assert_eq!(store.signals_created(), 1);
        assert!(store.has_signal_titled("Free Legal Clinic at Sabathani"));

        // Actors wired
        assert!(store.has_actor("Sabathani Community Center"));
        assert!(store.has_actor("Volunteer Lawyers Network"));
        assert!(store.actor_linked_to_signal("Sabathani Community Center", "Free Legal Clinic", "authored"));
        assert!(store.actor_linked_to_signal("Volunteer Lawyers Network", "Free Legal Clinic", "mentioned"));

        // Evidence trail
        assert_eq!(store.evidence_count_for("Free Legal Clinic"), 1);

        // Outbound links collected for promotion
        assert!(ctx.collected_links.iter().any(|(u, _)| u.contains("facebook.com/localorg")));
        assert!(ctx.collected_links.iter().any(|(u, _)| u.contains("sabathani.org/events")));
    }

    /// Signal in Dallas extracted from a page. Minneapolis scout filters it.
    /// Nothing stored — no signal, no actors, no evidence.
    #[tokio::test]
    async fn out_of_region_signal_produces_nothing() {
        let fetcher = MockFetcher::new()
            .on_page("https://texasorg.org/events", ArchivedPage {
                markdown: "Dallas community dinner...".into(),
                links: vec![],
                ..Default::default()
            });

        let extractor = MockExtractor::new()
            .on_url("https://texasorg.org/events", vec![
                Node::Aid(AidNode {
                    meta: NodeMeta {
                        title: "Dallas Community Dinner".into(),
                        location: Some(GeoPoint { lat: 32.7767, lng: -96.7970, precision: GeoPrecision::Exact }),
                        confidence: 0.8,
                        ..test_meta_defaults("https://texasorg.org/events")
                    },
                    ..Default::default()
                }),
            ]);

        let store = MockSignalStore::new();
        let embedder = FixedEmbedder::new();

        let phase = ScrapePhase::new(
            store.clone().into(),
            Arc::new(extractor),
            Arc::new(embedder),
            Arc::new(fetcher),
            mpls_region(),
            "test-run".into(),
        );

        let sources = vec![page_source("https://texasorg.org/events")];
        let source_refs: Vec<&SourceNode> = sources.iter().collect();
        let mut ctx = RunContext::new(&sources);
        let mut run_log = RunLog::new("test-run".into(), "minneapolis".into());

        phase.run_web(&source_refs, &mut ctx, &mut run_log).await;

        // Geo filter killed it — nothing stored
        assert_eq!(store.signals_created(), 0);
        assert_eq!(store.actors_created(), 0);
    }
}


// ---------------------------------------------------------------------------
// Chain Test 3: Multi-Source Corroboration
//
// Organ under test: ScrapePhase::run_web (called with 3 page sources)
// Input: 3 pages describing the same event
// Output: 1 signal, 2 corroborations, 3 evidence trails
// ---------------------------------------------------------------------------

#[cfg(test)]
mod chain_corroboration {
    use std::sync::Arc;

    use rootsignal_common::*;

    use super::helpers::*;

    #[tokio::test]
    async fn three_sources_corroborate_to_one_signal() {
        let fetcher = MockFetcher::new()
            .on_page("https://org-a.org/events", ArchivedPage {
                markdown: "Community garden cleanup at MLK Park...".into(),
                links: vec![],
                ..Default::default()
            })
            .on_page("https://org-b.org/calendar", ArchivedPage {
                markdown: "Garden cleanup event this Saturday...".into(),
                links: vec![],
                ..Default::default()
            })
            .on_page("https://nextdoor.com/post/xyz", ArchivedPage {
                markdown: "Anyone going to the garden cleanup?".into(),
                links: vec![],
                ..Default::default()
            });

        // All three extract a Gathering with the same title and location
        let make_gathering = |url: &str| -> Vec<Node> {
            vec![Node::Gathering(GatheringNode {
                meta: NodeMeta {
                    title: "Community Garden Cleanup".into(),
                    location: Some(GeoPoint { lat: 44.9489, lng: -93.2654, precision: GeoPrecision::Exact }),
                    confidence: 0.8,
                    ..test_meta_defaults(url)
                },
                starts_at: Some(chrono::NaiveDate::from_ymd_opt(2026, 3, 7).unwrap().and_hms_opt(9, 0, 0).unwrap()),
                is_free: true,
                ..Default::default()
            })]
        };

        let extractor = MockExtractor::new()
            .on_url("https://org-a.org/events", make_gathering("https://org-a.org/events"))
            .on_url("https://org-b.org/calendar", make_gathering("https://org-b.org/calendar"))
            .on_url("https://nextdoor.com/post/xyz", make_gathering("https://nextdoor.com/post/xyz"));

        // Embedder: near-identical vectors so embedding dedup also fires
        let base_vec = vec![0.5f32; 1024]; // all three get ~same vector
        let embedder = FixedEmbedder::new()
            .on_text("Community Garden Cleanup", base_vec.clone());

        let store = MockSignalStore::new();

        let phase = ScrapePhase::new(
            store.clone().into(),
            Arc::new(extractor),
            Arc::new(embedder),
            Arc::new(fetcher),
            mpls_region(),
            "test-run".into(),
        );

        let sources = vec![
            page_source("https://org-a.org/events"),
            page_source("https://org-b.org/calendar"),
            page_source("https://nextdoor.com/post/xyz"),
        ];
        let source_refs: Vec<&SourceNode> = sources.iter().collect();
        let mut ctx = RunContext::new(&sources);
        let mut run_log = RunLog::new("test-run".into(), "minneapolis".into());

        phase.run_web(&source_refs, &mut ctx, &mut run_log).await;

        // ONE signal, not three
        assert_eq!(store.signals_created(), 1);

        // Corroborated by the other two
        assert_eq!(store.corroborations_for("Community Garden Cleanup"), 2);

        // Three evidence trails (one per source)
        assert_eq!(store.evidence_count_for("Community Garden Cleanup"), 3);
    }

    /// Same source re-scraped — should Refresh, not Corroborate.
    #[tokio::test]
    async fn same_source_rescraped_refreshes_not_corroborates() {
        let fetcher = MockFetcher::new()
            .on_page("https://org-a.org/events", ArchivedPage {
                markdown: "Community garden cleanup...".into(),
                links: vec![],
                ..Default::default()
            });

        let gathering = vec![Node::Gathering(GatheringNode {
            meta: NodeMeta {
                title: "Community Garden Cleanup".into(),
                location: Some(GeoPoint { lat: 44.9489, lng: -93.2654, precision: GeoPrecision::Exact }),
                confidence: 0.8,
                ..test_meta_defaults("https://org-a.org/events")
            },
            starts_at: Some(chrono::NaiveDate::from_ymd_opt(2026, 3, 7).unwrap().and_hms_opt(9, 0, 0).unwrap()),
            is_free: true,
            ..Default::default()
        })];

        let extractor = MockExtractor::new()
            .on_url("https://org-a.org/events", gathering);

        let store = MockSignalStore::new();
        let embedder = FixedEmbedder::new();

        let phase = ScrapePhase::new(
            store.clone().into(),
            Arc::new(extractor),
            Arc::new(embedder),
            Arc::new(fetcher),
            mpls_region(),
            "test-run".into(),
        );

        let sources = vec![page_source("https://org-a.org/events")];
        let source_refs: Vec<&SourceNode> = sources.iter().collect();

        // --- First run: creates the signal ---
        let mut ctx = RunContext::new(&sources);
        let mut run_log = RunLog::new("run-1".into(), "minneapolis".into());
        phase.run_web(&source_refs, &mut ctx, &mut run_log).await;
        assert_eq!(store.signals_created(), 1);

        // --- Second run: same source, same content ---
        let mut ctx2 = RunContext::new(&sources);
        let mut run_log2 = RunLog::new("run-2".into(), "minneapolis".into());
        phase.run_web(&source_refs, &mut ctx2, &mut run_log2).await;

        // Still one signal — no duplicate created
        assert_eq!(store.signals_created(), 1);
        // Corroboration count did NOT increase (same source = Refresh)
        assert_eq!(store.corroborations_for("Community Garden Cleanup"), 0);
    }
}


// ---------------------------------------------------------------------------
// Chain Test 4: Social Scrape with Actor Context
//
// Organ under test: ScrapePhase::run_social
// Input: Instagram source linked to known actor + mocked posts
// Output: signals with actor fallback location, @mentions promoted
// ---------------------------------------------------------------------------

#[cfg(test)]
mod chain_social_with_actor {
    use std::sync::Arc;

    use rootsignal_common::*;

    use super::helpers::*;

    #[tokio::test]
    async fn instagram_posts_get_actor_fallback_and_mentions_collected() {
        let fetcher = MockFetcher::new()
            .on_posts("https://www.instagram.com/northsidemutualaid", vec![
                Post {
                    text: Some("Food distribution this Saturday! Thanks @mplsfoodshelf for the supplies.".into()),
                    permalink: Some("https://instagram.com/p/abc123".into()),
                    mentions: vec!["mplsfoodshelf".into()],
                    ..Default::default()
                },
                Post {
                    text: Some("Know your rights workshop next Tuesday @hennepincounty".into()),
                    permalink: Some("https://instagram.com/p/def456".into()),
                    mentions: vec!["hennepincounty".into()],
                    ..Default::default()
                },
            ]);

        let extractor = MockExtractor::new()
            .on_url("https://www.instagram.com/northsidemutualaid", vec![
                Node::Aid(AidNode {
                    meta: NodeMeta {
                        title: "Food Distribution at MLK Park".into(),
                        location: None,  // no location from extraction
                        mentioned_actors: vec!["Minneapolis Food Shelf".into()],
                        author_actor: Some("Northside Mutual Aid".into()),
                        confidence: 0.7,
                        ..test_meta_defaults("https://www.instagram.com/northsidemutualaid")
                    },
                    is_free: true,
                    ..Default::default()
                }),
            ]);

        let store = MockSignalStore::new();
        let embedder = FixedEmbedder::new();

        let phase = ScrapePhase::new(
            store.clone().into(),
            Arc::new(extractor),
            Arc::new(embedder),
            Arc::new(fetcher),
            mpls_region(),
            "test-run".into(),
        );

        let sources = vec![social_source("https://www.instagram.com/northsidemutualaid")];
        let source_refs: Vec<&SourceNode> = sources.iter().collect();
        let mut ctx = RunContext::new(&sources);

        // Inject actor context (in production, loaded from graph in load_and_schedule_sources)
        ctx.actor_contexts.insert(
            canonical_value("https://www.instagram.com/northsidemutualaid"),
            ActorContext {
                actor_name: "Northside Mutual Aid".into(),
                bio: Some("Community org serving North Minneapolis".into()),
                location_name: Some("North Minneapolis, MN".into()),
                location_lat: Some(45.0118),
                location_lng: Some(-93.2885),
            },
        );

        let mut run_log = RunLog::new("test-run".into(), "minneapolis".into());

        phase.run_social(&source_refs, &mut ctx, &mut run_log).await;

        // Signal stored (actor fallback gave it Minneapolis coords → survives geo filter)
        assert_eq!(store.signals_created(), 1);
        assert!(store.has_signal_titled("Food Distribution at MLK Park"));

        // Actor wired
        assert!(store.has_actor("Northside Mutual Aid"));
        assert!(store.actor_linked_to_signal("Northside Mutual Aid", "Food Distribution", "authored"));

        // @mentions collected for promotion
        let mention_urls: Vec<&str> = ctx.collected_links.iter().map(|(u, _)| u.as_str()).collect();
        assert!(mention_urls.iter().any(|u| u.contains("instagram.com/mplsfoodshelf")));
        assert!(mention_urls.iter().any(|u| u.contains("instagram.com/hennepincounty")));
    }

    /// Actor is in NYC, signal has no location. Fallback puts it in NYC.
    /// Minneapolis scout filters it. Nothing stored.
    #[tokio::test]
    async fn out_of_region_actor_fallback_gets_filtered() {
        let fetcher = MockFetcher::new()
            .on_posts("https://www.instagram.com/nycorg", vec![
                Post {
                    text: Some("Thoughts on organizing...".into()),
                    mentions: vec![],
                    ..Default::default()
                },
            ]);

        let extractor = MockExtractor::new()
            .on_url("https://www.instagram.com/nycorg", vec![
                Node::Aid(AidNode {
                    meta: NodeMeta {
                        title: "Organizing Reflections".into(),
                        location: None, // no location
                        confidence: 0.5,
                        ..test_meta_defaults("https://www.instagram.com/nycorg")
                    },
                    ..Default::default()
                }),
            ]);

        let store = MockSignalStore::new();
        let embedder = FixedEmbedder::new();

        let phase = ScrapePhase::new(
            store.clone().into(),
            Arc::new(extractor),
            Arc::new(embedder),
            Arc::new(fetcher),
            mpls_region(),
            "test-run".into(),
        );

        let sources = vec![social_source("https://www.instagram.com/nycorg")];
        let source_refs: Vec<&SourceNode> = sources.iter().collect();
        let mut ctx = RunContext::new(&sources);

        // Actor in NYC
        ctx.actor_contexts.insert(
            canonical_value("https://www.instagram.com/nycorg"),
            ActorContext {
                actor_name: "NYC Org".into(),
                bio: None,
                location_name: Some("New York, NY".into()),
                location_lat: Some(40.7128),
                location_lng: Some(-74.0060),
            },
        );

        let mut run_log = RunLog::new("test-run".into(), "minneapolis".into());

        phase.run_social(&source_refs, &mut ctx, &mut run_log).await;

        // Fallback put it in NYC → filtered by Minneapolis scout
        assert_eq!(store.signals_created(), 0);
    }
}


// ---------------------------------------------------------------------------
// Chain Test 5: Content Unchanged → Skip Extraction
//
// Organ under test: ScrapePhase::run_web
// Input: page source, MockSignalStore says content hash already processed
// Output: no extraction (no LLM call), existing signals refreshed,
//         outbound links still collected
// ---------------------------------------------------------------------------

#[cfg(test)]
mod chain_unchanged_content {
    use std::sync::Arc;

    use rootsignal_common::*;

    use super::helpers::*;

    #[tokio::test]
    async fn unchanged_content_skips_extraction_but_collects_links() {
        let fetcher = MockFetcher::new()
            .on_page("https://localorg.org/resources", ArchivedPage {
                markdown: "Free legal clinic every Tuesday...".into(),
                links: vec![
                    "https://facebook.com/localorg".into(),
                    "https://newpartner.org".into(),
                ],
                ..Default::default()
            });

        // Extractor should NOT be called — but if it is, it returns signals
        // (which would show up as store.signals_created() > 0, failing the test)
        let extractor = MockExtractor::new()
            .on_url("https://localorg.org/resources", vec![
                Node::Aid(AidNode {
                    meta: NodeMeta {
                        title: "SHOULD NOT APPEAR".into(),
                        confidence: 0.8,
                        ..test_meta_defaults("https://localorg.org/resources")
                    },
                    ..Default::default()
                }),
            ]);

        // Store says this content hash was already processed
        let store = MockSignalStore::new()
            .with_processed_hash("https://localorg.org/resources", "Free legal clinic every Tuesday...");

        let embedder = FixedEmbedder::new();

        let phase = ScrapePhase::new(
            store.clone().into(),
            Arc::new(extractor),
            Arc::new(embedder),
            Arc::new(fetcher),
            mpls_region(),
            "test-run".into(),
        );

        let sources = vec![page_source("https://localorg.org/resources")];
        let source_refs: Vec<&SourceNode> = sources.iter().collect();
        let mut ctx = RunContext::new(&sources);
        let mut run_log = RunLog::new("test-run".into(), "minneapolis".into());

        phase.run_web(&source_refs, &mut ctx, &mut run_log).await;

        // No new signals (extraction skipped)
        assert_eq!(store.signals_created(), 0);

        // But outbound links still collected
        assert!(ctx.collected_links.iter().any(|(u, _)| u.contains("newpartner.org")));

        // Stats show URL was unchanged
        assert_eq!(ctx.stats.urls_unchanged, 1);
        assert_eq!(ctx.stats.urls_scraped, 0);
    }
}


// ---------------------------------------------------------------------------
// Chain Test 6: Two-Phase Pipeline (Tension → Discovery → Response)
//
// Organ under test: ScrapePipeline::scrape_tension_sources + scrape_response_sources
// Input: tension source (Linktree) discovers org site → response phase scrapes it
// Output: promoted sources from Phase A, signals from Phase B
// ---------------------------------------------------------------------------

#[cfg(test)]
mod chain_two_phase_pipeline {
    use std::sync::Arc;

    use rootsignal_common::*;

    use super::helpers::*;

    /// Phase A scrapes a Linktree, discovers localorg.org via link promotion.
    /// Phase B scrapes localorg.org (now a source), extracting signals.
    /// Tests the full discovery → scrape loop across two phases.
    #[tokio::test]
    async fn phase_a_discovers_source_phase_b_scrapes_it() {
        let fetcher = MockFetcher::new()
            // Phase A: Linktree
            .on_page("https://linktr.ee/mplsmutualaid", ArchivedPage {
                markdown: "MPLS Mutual Aid".into(),
                links: vec!["https://localorg.org/resources".into()],
                ..Default::default()
            })
            // Phase B: org site (discovered from Linktree)
            .on_page("https://localorg.org/resources", ArchivedPage {
                markdown: "Free groceries every Saturday at MLK Park...".into(),
                links: vec!["https://facebook.com/localorg".into()],
                ..Default::default()
            });

        let extractor = MockExtractor::new()
            // Linktree: no signals (just links)
            .on_url("https://linktr.ee/mplsmutualaid", vec![])
            // Org site: one Aid signal
            .on_url("https://localorg.org/resources", vec![
                Node::Aid(AidNode {
                    meta: NodeMeta {
                        title: "Free Groceries at MLK Park".into(),
                        location: Some(GeoPoint { lat: 44.9489, lng: -93.2654, precision: GeoPrecision::Exact }),
                        author_actor: Some("Minneapolis Mutual Aid".into()),
                        confidence: 0.85,
                        ..test_meta_defaults("https://localorg.org/resources")
                    },
                    is_free: true,
                    ..Default::default()
                }),
            ]);

        let store = MockSignalStore::new();
        let embedder = FixedEmbedder::new();

        let pipeline = ScrapePipeline::new(
            store.clone().into(),
            Arc::new(extractor),
            Arc::new(embedder),
            Arc::new(fetcher),
            mpls_region(),
            "test-run".into(),
        );

        // Phase A: tension sources (Linktree is a web query or page source)
        let tension_sources = vec![page_source("https://linktr.ee/mplsmutualaid")];
        let mut ctx = RunContext::new(&tension_sources);
        let mut run_log = RunLog::new("test-run".into(), "minneapolis".into());

        pipeline.scrape_tension_sources_with(&tension_sources, &mut ctx, &mut run_log).await;

        // After Phase A: localorg.org discovered in collected_links
        assert!(ctx.collected_links.iter().any(|(u, _)| u.contains("localorg.org")));

        // Promote collected links (creates SourceNodes in store)
        pipeline.promote_collected_links(&mut ctx).await;
        assert!(store.has_source_url("https://localorg.org/resources"));

        // Phase B: response sources (the newly promoted org site)
        let response_sources = vec![page_source("https://localorg.org/resources")];
        pipeline.scrape_response_sources_with(&response_sources, &mut ctx, &mut run_log).await;

        // Signal from Phase B
        assert_eq!(store.signals_created(), 1);
        assert!(store.has_signal_titled("Free Groceries at MLK Park"));
        assert!(store.has_actor("Minneapolis Mutual Aid"));

        // Phase B also collected facebook link for future promotion
        assert!(ctx.collected_links.iter().any(|(u, _)| u.contains("facebook.com/localorg")));
    }
}
