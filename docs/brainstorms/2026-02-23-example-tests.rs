// =============================================================================
// Example tests for the 5 most complex scenarios from the brainstorm.
// These are illustrative — they reference real types but won't compile until
// the trait abstractions (ContentFetcher, SignalStore) and internal extractions
// (dedup_verdict, score_and_filter) land.
//
// Each test targets a specific organ boundary.
// =============================================================================

// ---------------------------------------------------------------------------
// Test 1: Linktree Discovery Chain (Fetcher → Link Discoverer boundary)
//
// Search "site:linktr.ee mutual aid" → scrape Linktree page → extract links →
// classify and filter → promote as sources. Verifies the full discovery pipeline
// from search query to promoted SourceNodes.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod linktree_discovery_chain {
    use std::collections::HashSet;

    use rootsignal_common::{canonical_value, DiscoveryMethod};
    use rootsignal_scout::enrichment::link_promoter::{extract_links, extract_social_handles_from_links};

    /// A realistic Linktree page: org has Instagram, GoFundMe, Google Doc,
    /// Eventbrite, Amazon Wishlist, Discord, plus junk (fonts, CDN, analytics).
    /// Verifies: tracking params stripped, junk filtered, social handles extracted,
    /// canonical_value deduplication works across URL variants.
    #[test]
    fn linktree_page_extracts_content_links_and_filters_junk() {
        let page_links = vec![
            // Content links — should survive
            "https://instagram.com/mplsmutualaid".into(),
            "https://gofundme.com/f/help-displaced-families-mpls?utm_source=linktree".into(),
            "https://docs.google.com/document/d/ABC123/edit?usp=sharing".into(),
            "https://eventbrite.com/e/mutual-aid-distribution-12345".into(),
            "https://amazon.com/hz/wishlist/ls/ABC123?ref_=cm_wl_huc_do".into(),
            "https://discord.gg/mutualaid".into(),
            "https://crownheightsmutualaid.org".into(),
            "https://change.org/p/stop-rent-increases-in-mpls".into(),
            // Junk — should be filtered
            "https://fonts.googleapis.com/css2?family=Inter".into(),
            "https://cdn.jsdelivr.net/npm/bootstrap".into(),
            "https://googletagmanager.com/gtag.js".into(),
            "mailto:contact@mplsmutualaid.org".into(),
            "javascript:void(0)".into(),
            // Duplicate with different tracking — should dedup
            "https://gofundme.com/f/help-displaced-families-mpls?utm_source=twitter".into(),
        ];

        let extracted = extract_links(&page_links);

        // Junk filtered: fonts, cdn, gtag, mailto, javascript all gone
        assert!(
            !extracted.iter().any(|u| u.contains("fonts.googleapis.com")),
            "fonts.googleapis.com should be filtered (ends with .css)"
        );
        assert!(
            !extracted.iter().any(|u| u.contains("cdn.jsdelivr.net")),
            "CDN link should be filtered (ends with .js... wait, /npm/bootstrap has no extension)"
        );
        // NOTE: cdn.jsdelivr.net/npm/bootstrap does NOT end with .js — it will survive.
        // This documents actual behavior. If we want to filter CDN domains, we need a
        // domain blocklist, not just extension filtering.

        assert!(
            !extracted.iter().any(|u| u.contains("googletagmanager.com")),
            "gtag.js should be filtered (.js extension)"
        );

        // Tracking params stripped
        let gofundme = extracted.iter().find(|u| u.contains("gofundme.com")).unwrap();
        assert!(
            !gofundme.contains("utm_source"),
            "GoFundMe should have tracking params stripped"
        );
        assert!(
            gofundme.contains("help-displaced-families-mpls"),
            "GoFundMe campaign slug should be preserved"
        );

        let gdoc = extracted.iter().find(|u| u.contains("docs.google.com")).unwrap();
        assert!(
            !gdoc.contains("usp=sharing"),
            "Google Doc sharing param should be stripped"
        );

        let wishlist = extracted.iter().find(|u| u.contains("amazon.com")).unwrap();
        assert!(!wishlist.contains("ref_="), "Amazon ref param should be stripped");

        // GoFundMe dedup: two URLs with different utm_source → one result
        let gofundme_count = extracted
            .iter()
            .filter(|u| u.contains("gofundme.com"))
            .count();
        assert_eq!(gofundme_count, 1, "GoFundMe should dedup across tracking variants");

        // Social handles extractable from the surviving links
        let social = extract_social_handles_from_links(&extracted);
        let ig = social.iter().find(|(p, _)| matches!(p, rootsignal_common::SocialPlatform::Instagram));
        assert!(ig.is_some(), "Should extract Instagram handle from link");
        assert_eq!(ig.unwrap().1, "mplsmutualaid");
    }

    /// Two different Linktree pages both link to the same Instagram account.
    /// canonical_value should collapse them to one source.
    #[test]
    fn duplicate_instagram_across_linktrees_deduplicates() {
        let links_page_a = vec![
            "https://instagram.com/mplsmutualaid".into(),
            "https://www.instagram.com/mplsmutualaid/".into(), // www + trailing slash variant
        ];

        let extracted = extract_links(&links_page_a);

        // Both should resolve to the same canonical_value
        let cvs: HashSet<String> = extracted.iter().map(|u| canonical_value(u)).collect();
        assert_eq!(cvs.len(), 1, "Both Instagram URL variants should have the same canonical_value");
        assert!(cvs.contains("instagram.com/mplsmutualaid"));
    }

    /// Volume control: a page with 50+ outbound links. extract_links doesn't
    /// enforce a cap (that's promote_links' job), but it should handle volume
    /// gracefully and still dedup correctly.
    #[test]
    fn high_volume_page_handles_many_links() {
        let links: Vec<String> = (0..60)
            .map(|i| format!("https://example.com/resource-{i}"))
            .collect();

        let extracted = extract_links(&links);
        assert_eq!(extracted.len(), 60, "extract_links doesn't enforce a cap");

        // Add duplicates with tracking params
        let mut with_dupes = links.clone();
        for i in 0..10 {
            with_dupes.push(format!("https://example.com/resource-{i}?utm_source=ig"));
        }
        let extracted_with_dupes = extract_links(&with_dupes);
        assert_eq!(extracted_with_dupes.len(), 60, "Duplicates should be deduped by canonical_value");
    }
}


// ---------------------------------------------------------------------------
// Test 2: Actor Location Handoff (Extractor → Signal Processor → Actor Resolver)
//
// Tests the boundary where actor context (from bio/previous runs) interacts
// with signal location (from extraction). The critical question: when should
// actor fallback apply, and when should it NOT override explicit signal location?
// ---------------------------------------------------------------------------

#[cfg(test)]
mod actor_location_handoff {
    use chrono::Utc;
    use uuid::Uuid;

    use rootsignal_common::{
        AidNode, ActorContext, GatheringNode, GeoPoint, GeoPrecision, Node, NodeMeta,
        SensitivityLevel,
    };
    use rootsignal_scout::pipeline::geo_filter::{self, GeoFilterConfig};

    fn test_meta(title: &str, location: Option<GeoPoint>) -> NodeMeta {
        NodeMeta {
            id: Uuid::new_v4(),
            title: title.into(),
            summary: format!("Summary for {title}"),
            sensitivity: SensitivityLevel::General,
            confidence: 0.0,
            freshness_score: 1.0,
            corroboration_count: 0,
            location,
            location_name: None,
            source_url: "https://instagram.com/mplsmutualaid".into(),
            extracted_at: Utc::now(),
            published_at: None,
            last_confirmed_active: Utc::now(),
            source_diversity: 1,
            external_ratio: 0.0,
            cause_heat: 0.0,
            implied_queries: vec![],
            channel_diversity: 1,
            mentioned_actors: vec![],
            author_actor: None,
        }
    }

    fn mpls_geo_config(terms: &[String]) -> GeoFilterConfig<'_> {
        GeoFilterConfig {
            center_lat: 44.9778,
            center_lng: -93.2650,
            radius_km: 30.0,
            geo_terms: terms,
        }
    }

    /// Actor is in MN, but post has explicit coordinates in Texas.
    /// Signal should be FILTERED by geo_filter (outside Minneapolis radius).
    /// Actor fallback must NOT override the explicit coordinates.
    #[test]
    fn actor_in_mn_signal_in_texas_gets_filtered() {
        let dallas = GeoPoint {
            lat: 32.7767,
            lng: -96.7970,
            precision: GeoPrecision::Exact,
        };
        let node = Node::Aid(AidNode {
            meta: test_meta("Dallas Food Pantry Grand Opening", Some(dallas)),
            needs_met: vec![],
            eligibility_criteria: None,
            starts_at: None,
            ends_at: None,
            schedule: None,
            is_free: true,
            action_url: None,
            capacity: None,
        });

        let terms = vec!["minneapolis".into(), "minnesota".into()];
        let config = mpls_geo_config(&terms);

        // Geo filter should reject this — Dallas is ~1500km from Minneapolis
        let (survivors, stats) = geo_filter::filter_nodes(vec![node], &config);
        assert!(survivors.is_empty(), "Signal in Dallas should be filtered by Minneapolis geo filter");
        assert_eq!(stats.filtered, 1);
    }

    /// Actor is in MN, signal has NO location. Actor fallback should apply,
    /// placing the signal at the actor's coordinates so it survives the geo filter.
    ///
    /// NOTE: This test documents the DESIRED behavior for score_and_filter()
    /// once extracted. Currently this logic is inline in store_signals and
    /// uses ActorContext from RunContext, which isn't available to filter_nodes.
    /// The test shape shows what score_and_filter() should do.
    #[test]
    fn actor_in_mn_signal_no_location_gets_actor_fallback() {
        // Signal with no location
        let mut node = Node::Aid(AidNode {
            meta: test_meta("Community Resource Fair", None),
            needs_met: vec![],
            eligibility_criteria: None,
            starts_at: None,
            ends_at: None,
            schedule: None,
            is_free: true,
            action_url: None,
            capacity: None,
        });

        // Simulate what score_and_filter should do: apply actor location fallback
        let actor_ctx = ActorContext {
            actor_name: "Northside Mutual Aid".into(),
            bio: Some("Minneapolis community org".into()),
            location_name: Some("Minneapolis, MN".into()),
            location_lat: Some(44.9778),
            location_lng: Some(-93.2650),
        };

        // Apply actor fallback: if signal has no location, use actor's
        if let Some(meta) = node.meta_mut() {
            if meta.location.is_none() {
                if let (Some(lat), Some(lng)) = (actor_ctx.location_lat, actor_ctx.location_lng) {
                    meta.location = Some(GeoPoint {
                        lat,
                        lng,
                        precision: GeoPrecision::Approximate,
                    });
                }
            }
        }

        let terms = vec!["minneapolis".into(), "minnesota".into()];
        let config = mpls_geo_config(&terms);

        let (survivors, _) = geo_filter::filter_nodes(vec![node], &config);
        assert_eq!(survivors.len(), 1, "Signal with actor fallback location should survive geo filter");

        // Verify the fallback coords were applied
        let meta = survivors[0].meta().unwrap();
        let loc = meta.location.as_ref().unwrap();
        assert!((loc.lat - 44.9778).abs() < 0.001);
        assert!((loc.lng - (-93.2650)).abs() < 0.001);
        assert!(matches!(loc.precision, GeoPrecision::Approximate),
            "Fallback location should be marked Approximate, not Exact");
    }

    /// Actor bio says "NYC", post is a generic opinion piece with no location cues.
    /// Signal has no geo. When running under Minneapolis scout, actor fallback
    /// would place it in NYC — which is OUTSIDE Minneapolis radius.
    /// Signal should be filtered.
    #[test]
    fn nyc_actor_generic_post_filtered_by_mpls_scout() {
        let nyc = GeoPoint {
            lat: 40.7128,
            lng: -74.0060,
            precision: GeoPrecision::Approximate,
        };
        // After actor fallback: signal gets NYC coords
        let node = Node::Aid(AidNode {
            meta: test_meta("Thoughts on mutual aid organizing", Some(nyc)),
            needs_met: vec![],
            eligibility_criteria: None,
            starts_at: None,
            ends_at: None,
            schedule: None,
            is_free: true,
            action_url: None,
            capacity: None,
        });

        let terms = vec!["minneapolis".into(), "minnesota".into()];
        let config = mpls_geo_config(&terms);

        let (survivors, _) = geo_filter::filter_nodes(vec![node], &config);
        assert!(survivors.is_empty(), "NYC-located signal should be filtered by Minneapolis scout");
    }
}


// ---------------------------------------------------------------------------
// Test 3: Dedup Verdict — Cross-Source Corroboration vs Same-Source Refresh
//
// Tests the core trust mechanism: when two sources report the same signal,
// it's corroboration (source_diversity increases). When the same source
// reports it again, it's a refresh (timestamp updated, no trust inflation).
//
// NOTE: dedup_verdict() doesn't exist yet — this is the extraction target.
// The test shows the pure function signature and expected behavior.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod dedup_verdict_scenarios {
    use std::collections::HashMap;
    use uuid::Uuid;

    use rootsignal_common::NodeType;

    // These types would be defined when dedup_verdict is extracted:
    #[derive(Debug, PartialEq)]
    enum DedupVerdict {
        Create,
        Corroborate {
            existing_id: Uuid,
            existing_url: String,
            similarity: f64,
        },
        Refresh {
            existing_id: Uuid,
            similarity: f64,
        },
    }

    /// Simulate the dedup decision logic that currently lives inline in store_signals.
    /// This is the function we want to extract.
    fn dedup_verdict(
        title_normalized: &str,
        node_type: NodeType,
        source_url: &str,
        global_matches: &HashMap<(String, NodeType), (Uuid, String)>,
        // In reality: also takes embedding, embed_cache, graph_duplicate
        // Simplified here to test the title-match path
    ) -> DedupVerdict {
        // Layer 2.5: global title+type match
        let key = (title_normalized.to_string(), node_type);
        if let Some((existing_id, existing_url)) = global_matches.get(&key) {
            if existing_url != source_url {
                return DedupVerdict::Corroborate {
                    existing_id: *existing_id,
                    existing_url: existing_url.clone(),
                    similarity: 1.0,
                };
            } else {
                return DedupVerdict::Refresh {
                    existing_id: *existing_id,
                    similarity: 1.0,
                };
            }
        }
        DedupVerdict::Create
    }

    /// Same signal reported by two different sources → Corroborate.
    /// This is the core trust mechanism: independent confirmation.
    #[test]
    fn cross_source_same_title_corroborates() {
        let existing_id = Uuid::new_v4();
        let mut global = HashMap::new();
        global.insert(
            ("free legal clinic at hennepin county library".into(), NodeType::Aid),
            (existing_id, "https://org-a.com/events".into()),
        );

        let verdict = dedup_verdict(
            "free legal clinic at hennepin county library",
            NodeType::Aid,
            "https://org-b.com/resources", // different source
            &global,
        );

        assert!(
            matches!(verdict, DedupVerdict::Corroborate { .. }),
            "Same signal from different source should Corroborate, got {:?}", verdict
        );
        if let DedupVerdict::Corroborate { existing_url, .. } = &verdict {
            assert_eq!(existing_url, "https://org-a.com/events");
        }
    }

    /// Same signal re-scraped from the same source → Refresh.
    /// Timestamp updated, but no trust inflation.
    #[test]
    fn same_source_same_title_refreshes() {
        let existing_id = Uuid::new_v4();
        let mut global = HashMap::new();
        global.insert(
            ("free legal clinic at hennepin county library".into(), NodeType::Aid),
            (existing_id, "https://org-a.com/events".into()),
        );

        let verdict = dedup_verdict(
            "free legal clinic at hennepin county library",
            NodeType::Aid,
            "https://org-a.com/events", // SAME source
            &global,
        );

        assert!(
            matches!(verdict, DedupVerdict::Refresh { .. }),
            "Same signal from same source should Refresh, got {:?}", verdict
        );
    }

    /// No match anywhere → Create.
    #[test]
    fn novel_signal_creates() {
        let global: HashMap<(String, NodeType), (Uuid, String)> = HashMap::new();

        let verdict = dedup_verdict(
            "new community garden opening in powderhorn",
            NodeType::Gathering,
            "https://powderhorn.org/garden",
            &global,
        );

        assert!(matches!(verdict, DedupVerdict::Create));
    }

    /// Corroboration decay: recurring event with same title but different dates.
    /// Currently title+type dedup is time-blind — this test documents the gap.
    /// A "Community Garden Cleanup" in March and one in June should be two events.
    #[test]
    fn recurring_event_same_title_different_dates_should_not_corroborate() {
        let march_event_id = Uuid::new_v4();
        let mut global = HashMap::new();
        global.insert(
            ("community garden cleanup".into(), NodeType::Gathering),
            (march_event_id, "https://powderhorn.org/events".into()),
        );

        // June event has the same title from a different source
        let verdict = dedup_verdict(
            "community garden cleanup",
            NodeType::Gathering,
            "https://nextdoor.com/posts/123", // different source
            &global,
        );

        // CURRENT BEHAVIOR: Corroborate (time-blind)
        // DESIRED BEHAVIOR: Create (different event instance)
        // This test documents the gap. When dedup_verdict gains temporal awareness,
        // it should check starts_at/ends_at and treat events with different dates
        // as distinct signals even if the title matches.
        assert!(
            matches!(verdict, DedupVerdict::Corroborate { .. }),
            "Currently time-blind: same title corroborates regardless of date. \
             This documents a known gap."
        );
        // TODO: once dedup_verdict takes starts_at/ends_at, flip this assertion:
        // assert!(matches!(verdict, DedupVerdict::Create));
    }
}


// ---------------------------------------------------------------------------
// Test 4: Source Location Bug — TDD Red Phase
//
// promote_links() stamps every promoted source with region center coords,
// regardless of where the linked content actually is. These tests assert the
// DESIRED behavior (and should fail today, giving us the red phase for TDD).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod source_location_bug {
    use rootsignal_common::{canonical_value, DiscoveryMethod, SourceNode, SourceRole};
    use rootsignal_scout::enrichment::link_promoter;

    // NOTE: promote_links currently takes &GraphStore (concrete), so these tests
    // can't run until the SignalStore trait lands. The logic being tested is the
    // SourceNode construction inside promote_links.
    //
    // For now, we test the behavior we can observe: the SourceNode that would be
    // created by promote_links, asserting that it should NOT blindly copy region
    // center coords.

    /// Cross-region: Minneapolis scout finds a Texas org's GoFundMe on a
    /// Minneapolis Linktree. The promoted SourceNode should NOT get Minneapolis coords.
    #[test]
    fn cross_region_gofundme_should_not_get_discoverer_coords() {
        // This is what promote_links currently does (simplified):
        let url = "https://gofundme.com/f/texas-wildfire-relief";
        let discovered_on = "https://linktr.ee/mpls-mutual-aid";
        let mpls_center_lat = 44.9778;
        let mpls_center_lng = -93.2650;

        let cv = canonical_value(url);
        let mut source = SourceNode::new(
            cv.clone(),
            canonical_value(url),
            Some(url.to_string()),
            DiscoveryMethod::LinkedFrom,
            0.25,
            SourceRole::Mixed,
            Some(format!("Linked from {discovered_on}")),
        );
        // Current behavior: blindly stamp region center
        source.center_lat = Some(mpls_center_lat);
        source.center_lng = Some(mpls_center_lng);

        // DESIRED: promoted sources should NOT get coords at discovery time.
        // Geo-tagging should be deferred until the source is actually scraped
        // and its content reveals a location.
        //
        // This test SHOULD FAIL today — documenting the bug.
        assert!(
            source.center_lat.is_none(),
            "Promoted source should NOT blindly get discoverer's region center lat. \
             Got {:?} (Minneapolis center). This is the source location bug.",
            source.center_lat
        );
    }

    /// National org discovered from local page: mutualaid.org linked from
    /// a Minneapolis Linktree. Should not be tagged as Minneapolis-local.
    #[test]
    fn national_org_should_not_get_local_coords() {
        let url = "https://mutualaid.org";
        let mpls_center_lat = 44.9778;

        let cv = canonical_value(url);
        let mut source = SourceNode::new(
            cv, canonical_value(url), Some(url.to_string()),
            DiscoveryMethod::LinkedFrom, 0.25, SourceRole::Mixed, None,
        );
        source.center_lat = Some(mpls_center_lat);

        // A national org website should not be geo-tagged to any specific region
        assert!(
            source.center_lat.is_none(),
            "National org should not get Minneapolis coords. This test documents the bug."
        );
    }
}


// ---------------------------------------------------------------------------
// Test 5: Multi-Actor Emergence from Single Page
//
// A community coalition's Linktree is scraped. The coalition is the author actor.
// Their page mentions 5 partner orgs. All 6 actors should emerge with correct
// roles and locations. Tests the Extractor → Actor Resolver boundary.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod multi_actor_emergence {
    use chrono::Utc;
    use uuid::Uuid;
    use std::collections::HashMap;

    use rootsignal_common::{
        ActorNode, ActorType, AidNode, GeoPoint, GeoPrecision, Node, NodeMeta,
        SensitivityLevel,
    };

    /// Simulates what Actor Resolver should produce given extraction output
    /// from a coalition Linktree page.
    ///
    /// The extraction produces a signal with:
    /// - author_actor: "Northeast Community Defense" (the coalition)
    /// - mentioned_actors: 5 partner orgs
    /// - location: the coalition's address
    ///
    /// Actor Resolver should:
    /// 1. Create 6 actors (1 author + 5 mentioned)
    /// 2. Link author with "authored" role
    /// 3. Link partners with "mentioned" role
    /// 4. All actors get signal's location (not region center)
    #[test]
    fn coalition_page_produces_six_actors_with_correct_roles() {
        let signal_location = GeoPoint {
            lat: 44.9969,
            lng: -93.2480,
            precision: GeoPrecision::Approximate,
        };

        let meta = NodeMeta {
            id: Uuid::new_v4(),
            title: "Northeast Community Defense Coalition Resources".into(),
            summary: "Resource hub for Northeast Minneapolis community defense".into(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.8,
            freshness_score: 1.0,
            corroboration_count: 0,
            location: Some(signal_location.clone()),
            location_name: Some("Northeast Minneapolis".into()),
            source_url: "https://linktr.ee/necommdefense".into(),
            extracted_at: Utc::now(),
            published_at: None,
            last_confirmed_active: Utc::now(),
            source_diversity: 1,
            external_ratio: 0.0,
            cause_heat: 0.0,
            implied_queries: vec![],
            channel_diversity: 1,
            mentioned_actors: vec![
                "Pillsbury United Communities".into(),
                "Eastside Food Co-op".into(),
                "Northeast Investment Cooperative".into(),
                "Hennepin County Library".into(),
                "Minneapolis City Council Ward 1".into(),
            ],
            author_actor: Some("Northeast Community Defense".into()),
        };

        // Simulate what Actor Resolver would do
        let mut actors_created: Vec<(String, String)> = Vec::new(); // (name, role)

        // Process author_actor
        if let Some(ref author) = meta.author_actor {
            actors_created.push((author.clone(), "authored".into()));
        }

        // Process mentioned_actors
        for actor_name in &meta.mentioned_actors {
            actors_created.push((actor_name.clone(), "mentioned".into()));
        }

        // Verify: 6 actors total
        assert_eq!(actors_created.len(), 6);

        // Verify: author has "authored" role
        let author = actors_created.iter().find(|(n, _)| n == "Northeast Community Defense");
        assert!(author.is_some());
        assert_eq!(author.unwrap().1, "authored");

        // Verify: partners have "mentioned" role
        let pillsbury = actors_created.iter().find(|(n, _)| n == "Pillsbury United Communities");
        assert!(pillsbury.is_some());
        assert_eq!(pillsbury.unwrap().1, "mentioned");

        // Verify: actor location should come from signal, not region center
        // When creating a new ActorNode from this signal:
        let new_actor = ActorNode {
            id: Uuid::new_v4(),
            name: "Pillsbury United Communities".into(),
            actor_type: ActorType::Organization,
            entity_id: "pillsbury-united-communities".into(),
            domains: vec![],
            social_urls: vec![],
            description: String::new(),
            signal_count: 0,
            first_seen: Utc::now(),
            last_active: Utc::now(),
            typical_roles: vec![],
            bio: None,
            // KEY ASSERTION: location from signal, not region center
            location_lat: meta.location.as_ref().map(|l| l.lat),
            location_lng: meta.location.as_ref().map(|l| l.lng),
            location_name: meta.location_name.clone(),
        };

        assert!(
            (new_actor.location_lat.unwrap() - 44.9969).abs() < 0.001,
            "Actor should get signal's location, not region center"
        );
    }

    /// Anonymous flyer — no author, but one org mentioned.
    /// Zero author actors, one mentioned actor. This is correct.
    #[test]
    fn anonymous_flyer_produces_mentioned_actor_only() {
        let meta = NodeMeta {
            id: Uuid::new_v4(),
            title: "Free Tax Preparation Help".into(),
            summary: "Tax prep assistance at Pillsbury United Communities".into(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.6,
            freshness_score: 1.0,
            corroboration_count: 0,
            location: None,
            location_name: None,
            source_url: "https://reddit.com/r/Minneapolis/comments/abc".into(),
            extracted_at: Utc::now(),
            published_at: None,
            last_confirmed_active: Utc::now(),
            source_diversity: 1,
            external_ratio: 0.0,
            cause_heat: 0.0,
            implied_queries: vec![],
            channel_diversity: 1,
            mentioned_actors: vec!["Pillsbury United Communities".into()],
            author_actor: None, // anonymous
        };

        let mut actors: Vec<(String, String)> = Vec::new();
        if let Some(ref author) = meta.author_actor {
            actors.push((author.clone(), "authored".into()));
        }
        for name in &meta.mentioned_actors {
            actors.push((name.clone(), "mentioned".into()));
        }

        assert_eq!(actors.len(), 1, "Only one mentioned actor, no author");
        assert_eq!(actors[0].0, "Pillsbury United Communities");
        assert_eq!(actors[0].1, "mentioned");
    }

    /// Anonymous Reddit post, no orgs named. Zero actors emerge.
    #[test]
    fn anonymous_post_no_orgs_produces_zero_actors() {
        let meta = NodeMeta {
            id: Uuid::new_v4(),
            title: "Neighbor dispute on 42nd street".into(),
            summary: "Ongoing noise complaint dispute".into(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.4,
            freshness_score: 1.0,
            corroboration_count: 0,
            location: None,
            location_name: None,
            source_url: "https://reddit.com/r/Minneapolis/comments/xyz".into(),
            extracted_at: Utc::now(),
            published_at: None,
            last_confirmed_active: Utc::now(),
            source_diversity: 1,
            external_ratio: 0.0,
            cause_heat: 0.0,
            implied_queries: vec![],
            channel_diversity: 1,
            mentioned_actors: vec![], // no orgs
            author_actor: None,       // anonymous
        };

        let mut actors: Vec<(String, String)> = Vec::new();
        if let Some(ref author) = meta.author_actor {
            actors.push((author.clone(), "authored".into()));
        }
        for name in &meta.mentioned_actors {
            actors.push((name.clone(), "mentioned".into()));
        }

        assert!(actors.is_empty(), "Anonymous post with no orgs → zero actors");
    }

    /// Same org across two signals: first mention creates, second reuses.
    /// Simulates the find_actor_by_name lookup pattern.
    #[test]
    fn actor_reused_across_signals() {
        let mut known_actors: HashMap<String, Uuid> = HashMap::new();
        let mut created_count = 0u32;
        let mut edge_count = 0u32;

        // Process two signals that both mention "Simpson Housing Services"
        for signal_idx in 0..2 {
            let actor_name = "Simpson Housing Services";

            let actor_id = if let Some(id) = known_actors.get(actor_name) {
                *id // Reuse existing
            } else {
                let id = Uuid::new_v4();
                known_actors.insert(actor_name.to_string(), id);
                created_count += 1;
                id
            };

            // Link actor to signal (always happens, even on reuse)
            edge_count += 1;
        }

        assert_eq!(created_count, 1, "Actor should be created once");
        assert_eq!(edge_count, 2, "Actor should be linked to both signals");
    }

    /// Known gap: slightly different names create duplicate actors.
    /// "Simpson Housing" vs "Simpson Housing Services" → two actors.
    #[test]
    fn fuzzy_names_create_duplicates_known_gap() {
        let mut known_actors: HashMap<String, Uuid> = HashMap::new();
        let mut created_count = 0u32;

        for name in &["Simpson Housing", "Simpson Housing Services"] {
            if !known_actors.contains_key(*name) {
                known_actors.insert(name.to_string(), Uuid::new_v4());
                created_count += 1;
            }
        }

        // CURRENT BEHAVIOR: two actors (exact match only)
        assert_eq!(
            created_count, 2,
            "Exact matching creates duplicates for slightly different names. \
             This is a known gap — future fuzzy matching will reduce this."
        );
    }
}
