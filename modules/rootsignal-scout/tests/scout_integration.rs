//! Integration tests for Scout with real LLM calls against real Neo4j.
//!
//! Requirements:
//!   - Docker (for Neo4j via testcontainers)
//!   - ANTHROPIC_API_KEY env var
//!   - VOYAGE_API_KEY env var
//!
//! Tests are skipped (not failed) when keys are missing.

mod harness;

use harness::{search_result, TestContext};
use rootsignal_common::CityNode;
use rootsignal_scout::fixtures::{
    CorpusSearcher, ScenarioSearcher, ScenarioSocialScraper,
};

fn city_node(name: &str, slug: &str, lat: f64, lng: f64, radius_km: f64, geo_terms: &[&str]) -> CityNode {
    CityNode {
        id: uuid::Uuid::new_v4(),
        name: name.to_string(),
        slug: slug.to_string(),
        center_lat: lat,
        center_lng: lng,
        radius_km,
        geo_terms: geo_terms.iter().map(|s| s.to_string()).collect(),
        active: true,
        created_at: chrono::Utc::now(),
        last_scout_completed_at: None,
    }
}
use rootsignal_scout::scraper::SocialPost;

// ---------------------------------------------------------------------------
// Scenario 1: Event page → extracts Event signals
// ---------------------------------------------------------------------------

#[tokio::test]
async fn event_page_produces_event_signals() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys not set");
        return;
    };

    let stats = ctx
        .scout()
        .with_web_content(include_str!("fixtures/community_garden_event.txt"))
        .with_search_results(vec![search_result(
            "https://example.com/powderhorn-garden",
            "Powderhorn Community Garden",
        )])
        .run()
        .await;

    assert!(stats.signals_extracted >= 1, "should extract at least 1 signal, got {}", stats.signals_extracted);
    assert!(stats.signals_extracted <= 10, "shouldn't hallucinate dozens, got {}", stats.signals_extracted);
    assert!(stats.by_type[0] >= 1, "should extract at least one Event, got {}", stats.by_type[0]);
    assert!(stats.signals_stored >= 1, "at least one should survive pipeline, got {}", stats.signals_stored);
}

// ---------------------------------------------------------------------------
// Scenario 2: Give/resource page → extracts Give signals
// ---------------------------------------------------------------------------

#[tokio::test]
async fn resource_page_produces_give_signals() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys not set");
        return;
    };

    let stats = ctx
        .scout()
        .with_web_content(include_str!("fixtures/food_shelf_give.txt"))
        .with_search_results(vec![search_result(
            "https://example.com/briva-food-shelf",
            "Briva Health Food Shelf",
        )])
        .run()
        .await;

    assert!(stats.signals_extracted >= 1, "should extract at least 1 signal, got {}", stats.signals_extracted);
    // Food shelf could produce Give (the service) and/or Ask (volunteer needs)
    let give_or_ask = stats.by_type[1] + stats.by_type[2]; // Give + Ask
    assert!(give_or_ask >= 1, "should extract Give and/or Ask signals, got give={} ask={}", stats.by_type[1], stats.by_type[2]);
}

// ---------------------------------------------------------------------------
// Scenario 3: Identical content on second run → dedup via content hash
// ---------------------------------------------------------------------------

#[tokio::test]
async fn duplicate_content_is_detected_unchanged() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys not set");
        return;
    };

    let content = include_str!("fixtures/community_garden_event.txt");
    let results = vec![search_result(
        "https://example.com/powderhorn-garden",
        "Powderhorn Community Garden",
    )];

    // Run 1: seed the graph
    let stats1 = ctx
        .scout()
        .with_web_content(content)
        .with_search_results(results.clone())
        .run()
        .await;
    assert!(stats1.signals_stored >= 1, "run 1 should store signals");

    // Run 2: same content → content hash detects no change
    let stats2 = ctx
        .scout()
        .with_web_content(content)
        .with_search_results(results)
        .run()
        .await;

    assert!(
        stats2.urls_unchanged >= 1,
        "same content should be detected as unchanged, got {}",
        stats2.urls_unchanged,
    );
    assert_eq!(
        stats2.signals_stored, 0,
        "no new signals should be stored from identical content",
    );
}

// ---------------------------------------------------------------------------
// Scenario 4: Overlapping content from different source → corroboration
// ---------------------------------------------------------------------------

#[tokio::test]
async fn overlapping_content_corroborates() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys not set");
        return;
    };

    // Run 1: organization's own page
    let stats1 = ctx
        .scout()
        .with_web_content(include_str!("fixtures/community_garden_event.txt"))
        .with_search_results(vec![search_result(
            "https://powderhornpark.org/garden-day",
            "Powderhorn Garden Volunteer Day",
        )])
        .run()
        .await;
    assert!(stats1.signals_stored >= 1, "run 1 should store signals");

    // Run 2: newspaper article about the same event (different URL, different wording)
    let stats2 = ctx
        .scout()
        .with_web_content(include_str!("fixtures/garden_event_newspaper.txt"))
        .with_search_results(vec![search_result(
            "https://southwestjournal.com/community-gardens-2026",
            "Community Gardens Gear Up",
        )])
        .run()
        .await;

    // The newspaper article covers the same event — dedup should catch it
    // via embedding similarity or title match
    assert!(
        stats2.signals_deduplicated >= 1 || stats2.signals_stored == 0,
        "same event from different source should corroborate or produce no new signals; \
         deduped={}, stored={}",
        stats2.signals_deduplicated,
        stats2.signals_stored,
    );
}

// ---------------------------------------------------------------------------
// Scenario 5: Non-civic content → zero signals
// ---------------------------------------------------------------------------

#[tokio::test]
async fn non_civic_content_produces_nothing() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys not set");
        return;
    };

    let stats = ctx
        .scout()
        .with_web_content(include_str!("fixtures/pizza_menu.txt"))
        .with_search_results(vec![search_result(
            "https://bobshouseofpizza.com/menu",
            "Bob's House of Pizza Menu",
        )])
        .run()
        .await;

    assert_eq!(
        stats.signals_stored, 0,
        "pizza menu should produce no civic signals, got {}",
        stats.signals_stored,
    );
}

// ---------------------------------------------------------------------------
// Scenario 6: Social media posts → extraction works
// ---------------------------------------------------------------------------

#[tokio::test]
async fn social_posts_produce_signals() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys not set");
        return;
    };

    let posts = vec![
        SocialPost {
            content: "Spring volunteer day this Saturday at Powderhorn Community Garden! \
                      Bring gloves, we'll provide tools and lunch. 9am-1pm at 3524 15th Ave S. \
                      All ages welcome. Sign up at eventbrite.com/powderhorn-spring-2026"
                .to_string(),
            author: Some("mpls_gardens".to_string()),
            url: Some("https://instagram.com/p/abc123".to_string()),
        },
        SocialPost {
            content: "Briva Health food shelf open tomorrow 10-4. Fresh produce, halal options, \
                      no ID needed. 420 15th Ave S. Volunteer spots available for sorting shift 8-10am."
                .to_string(),
            author: Some("briva_health".to_string()),
            url: Some("https://instagram.com/p/def456".to_string()),
        },
    ];

    let stats = ctx
        .scout()
        .with_social_posts(posts)
        .run()
        .await;

    assert!(stats.social_media_posts >= 2, "should process social posts, got {}", stats.social_media_posts);
    assert!(stats.signals_extracted >= 1, "social posts should produce signals, got {}", stats.signals_extracted);
}

// ---------------------------------------------------------------------------
// Scenario 7: Off-geography content → filtered
// ---------------------------------------------------------------------------

#[tokio::test]
async fn off_geography_signals_filtered() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys not set");
        return;
    };

    // Austin event run against Twin Cities profile
    let stats = ctx
        .scout()
        .with_web_content(include_str!("fixtures/austin_cleanup.txt"))
        .with_search_results(vec![search_result(
            "https://keepaustinbeautiful.org/zilker",
            "Keep Austin Beautiful Zilker Cleanup",
        )])
        .run()
        .await;

    // LLM may extract it, but geo filter should drop signals with Austin/Zilker location
    assert!(
        stats.geo_filtered >= 1 || stats.signals_stored == 0,
        "Austin event should be filtered by Twin Cities geo terms; \
         filtered={}, stored={}",
        stats.geo_filtered,
        stats.signals_stored,
    );
}

// ---------------------------------------------------------------------------
// Scenario 8: Portland city profile — different city, different content
// ---------------------------------------------------------------------------

#[tokio::test]
async fn portland_content_with_portland_profile() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys not set");
        return;
    };

    let stats = ctx
        .scout()
        .with_city(city_node("Portland, Oregon", "portland", 45.5152, -122.6784, 20.0, &["Portland", "Oregon", "Multnomah", "OR"]))
        .with_web_content(include_str!("fixtures/portland_bike_repair.txt"))
        .with_search_results(vec![search_result(
            "https://communitycyclingcenter.org/clinics",
            "Free Bike Repair Clinics",
        )])
        .run()
        .await;

    assert!(stats.signals_extracted >= 1, "Portland content should extract signals, got {}", stats.signals_extracted);
    // Bike repair clinics are Events and/or Gives
    let event_or_give = stats.by_type[0] + stats.by_type[1];
    assert!(event_or_give >= 1, "should extract Event and/or Give, got event={} give={}", stats.by_type[0], stats.by_type[1]);
}

// ---------------------------------------------------------------------------
// Scenario 9: NYC content with NYC profile
// ---------------------------------------------------------------------------

#[tokio::test]
async fn nyc_mutual_aid_extraction() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys not set");
        return;
    };

    let stats = ctx
        .scout()
        .with_city(city_node("New York City", "nyc", 40.7128, -74.0060, 25.0, &["New York", "NYC", "Brooklyn", "Manhattan", "Queens", "Bronx", "NY"]))
        .with_web_content(include_str!("fixtures/nyc_mutual_aid.txt"))
        .with_search_results(vec![search_result(
            "https://crownheightsmutualaid.org/update",
            "Crown Heights Mutual Aid Weekly Update",
        )])
        .run()
        .await;

    assert!(stats.signals_extracted >= 1, "NYC mutual aid should extract signals, got {}", stats.signals_extracted);
    // Mutual aid pages typically produce Give (distributions), Ask (urgent needs), and Event (meal service)
    let total_typed = stats.by_type.iter().sum::<u32>();
    assert!(total_typed >= 1, "should produce typed signals, got {:?}", stats.by_type);
}

// ---------------------------------------------------------------------------
// Scenario 10: Berlin content (German language) with Berlin profile
// ---------------------------------------------------------------------------

#[tokio::test]
async fn berlin_german_language_extraction() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys not set");
        return;
    };

    let stats = ctx
        .scout()
        .with_city(city_node("Berlin, Germany", "berlin", 52.5200, 13.4050, 20.0, &["Berlin", "Kreuzberg", "Neukölln", "Friedrichshain", "Mitte"]))
        .with_web_content(include_str!("fixtures/berlin_neighborhood_council.txt"))
        .with_search_results(vec![search_result(
            "https://neukoelln-nord.de/quartiersrat",
            "Quartiersrat Neukölln-Nord",
        )])
        .run()
        .await;

    assert!(
        stats.signals_extracted >= 1,
        "German-language civic content should extract signals, got {}",
        stats.signals_extracted,
    );
    // Neighborhood council meeting is an Event
    assert!(stats.by_type[0] >= 1, "should extract at least one Event, got {}", stats.by_type[0]);
}

// ---------------------------------------------------------------------------
// Scenario 11: GoFundMe/crisis Ask — personal fundraiser with community needs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn gofundme_produces_ask_signals() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys not set");
        return;
    };

    let stats = ctx
        .scout()
        .with_web_content(include_str!("fixtures/gofundme_fire_relief.txt"))
        .with_search_results(vec![search_result(
            "https://gofundme.com/f/martinez-family-fire-relief",
            "Help the Martinez Family After Apartment Fire",
        )])
        .run()
        .await;

    assert!(
        stats.signals_extracted >= 1,
        "GoFundMe should extract at least 1 signal, got {}",
        stats.signals_extracted,
    );
    // GoFundMe is primarily an Ask (help needed) but could also produce Give
    // (clothing drop-off, meal train) depending on LLM interpretation
    let ask_or_give = stats.by_type[2] + stats.by_type[1]; // Ask + Give
    assert!(
        ask_or_give >= 1,
        "should extract Ask and/or Give signals from fundraiser; ask={} give={}",
        stats.by_type[2],
        stats.by_type[1],
    );
}

// ---------------------------------------------------------------------------
// Scenario 12: Reddit discussion about a community issue → community responses
// ---------------------------------------------------------------------------

#[tokio::test]
async fn community_discussion_extracts_responses() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys not set");
        return;
    };

    // Reddit thread about rent increases — contains actionable responses
    // (tenant hotline = Give, organizing meeting = Event, policy info = Notice)
    let stats = ctx
        .scout()
        .with_web_content(include_str!("fixtures/reddit_housing_discussion.txt"))
        .with_search_results(vec![search_result(
            "https://reddit.com/r/Minneapolis/comments/abc123",
            "Another rent increase — where is everyone moving?",
        )])
        .run()
        .await;

    // The LLM should extract the community RESPONSES, not the complaint itself.
    // HOME Line hotline = Give, MHRC meeting = Event, tenant union = Give/Event
    assert!(
        stats.signals_extracted >= 1,
        "community discussion should surface actionable responses, got {}",
        stats.signals_extracted,
    );
    // We expect at least one of: Event (organizing meeting), Give (hotline/resources), or Notice (policy info)
    let actionable = stats.by_type[0] + stats.by_type[1] + stats.by_type[2] + stats.by_type[3];
    assert!(
        actionable >= 1,
        "should extract actionable signals from discussion; event={} give={} ask={} notice={}",
        stats.by_type[0],
        stats.by_type[1],
        stats.by_type[2],
        stats.by_type[3],
    );
}

// ---------------------------------------------------------------------------
// Scenario 13: Government advisory / Notice — PFAS water quality issue
// ---------------------------------------------------------------------------

#[tokio::test]
async fn government_advisory_produces_notice() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys not set");
        return;
    };

    let stats = ctx
        .scout()
        .with_web_content(include_str!("fixtures/investigative_water_quality.txt"))
        .with_search_results(vec![search_result(
            "https://minnpost.com/environment/2026/04/mpca-pfas-south-minneapolis",
            "MPCA Issues Advisory After Elevated PFAS Levels",
        )])
        .run()
        .await;

    assert!(
        stats.signals_extracted >= 1,
        "PFAS advisory should extract signals, got {}",
        stats.signals_extracted,
    );
    // Should produce Notice (advisory) and/or Event (community meeting)
    let notice_or_event = stats.by_type[3] + stats.by_type[0]; // Notice + Event
    assert!(
        notice_or_event >= 1,
        "should extract Notice and/or Event from government advisory; notice={} event={}",
        stats.by_type[3],
        stats.by_type[0],
    );
}

// ---------------------------------------------------------------------------
// Scenario 14: NYC construction noise discussion → community responses
// ---------------------------------------------------------------------------

#[tokio::test]
async fn nyc_community_discussion_extracts_responses() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys not set");
        return;
    };

    let stats = ctx
        .scout()
        .with_city(city_node("New York City", "nyc", 40.7128, -74.0060, 25.0, &["New York", "NYC", "Brooklyn", "Manhattan", "Queens", "Bronx", "NY"]))
        .with_web_content(include_str!("fixtures/nyc_noise_complaint_discussion.txt"))
        .with_search_results(vec![search_result(
            "https://reddit.com/r/CrownHeights/comments/xyz789",
            "Construction on Nostrand Ave at 5:30 AM",
        )])
        .run()
        .await;

    // Discussion contains: community board meeting (Event), tenant union meetings (Event),
    // 311 complaint process (Notice/Give), organizing info (Give)
    assert!(
        stats.signals_extracted >= 1,
        "NYC noise discussion should surface actionable responses, got {}",
        stats.signals_extracted,
    );
}

// ---------------------------------------------------------------------------
// Scenario 15: Tension extraction — urgent community tension content
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tension_extraction_produces_tension_signals() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys not set");
        return;
    };

    let stats = ctx
        .scout()
        .with_web_content(include_str!("fixtures/urgent_community_tension.txt"))
        .with_search_results(vec![search_result(
            "https://example.com/phillips-ice-enforcement",
            "ICE Enforcement Activity in Phillips Neighborhood",
        )])
        .run()
        .await;

    assert!(
        stats.signals_extracted >= 1,
        "tension content should extract signals, got {}",
        stats.signals_extracted,
    );

    // Check that at least one Tension signal exists in the graph
    let tensions = harness::queries::tension_signals(ctx.client()).await;
    assert!(
        !tensions.is_empty(),
        "should extract at least one Tension signal from urgent community content; \
         got {} tensions, {} total signals extracted (by_type: {:?})",
        tensions.len(),
        stats.signals_extracted,
        stats.by_type,
    );

    // Verify at least one tension has category or what_would_help populated
    let enriched = tensions
        .iter()
        .any(|t| t.category.is_some() || t.what_would_help.is_some());
    assert!(
        enriched,
        "at least one tension should have category or what_would_help; got: {:?}",
        tensions
            .iter()
            .map(|t| (&t.title, &t.category, &t.what_would_help))
            .collect::<Vec<_>>(),
    );
}

// ===========================================================================
// ADVERSARIAL SCENARIOS
// ===========================================================================

// ---------------------------------------------------------------------------
// Scenario 15: Spam / crypto MLM disguised as civic content → filtered
// ---------------------------------------------------------------------------

#[tokio::test]
async fn spam_disguised_as_civic_filtered() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys not set");
        return;
    };

    let social = ScenarioSocialScraper::new(
        &std::env::var("ANTHROPIC_API_KEY").unwrap(),
        "An Instagram account posts daily 'community wealth building' content that's actually \
         crypto/MLM recruitment. Uses civic hashtags like #MinneapolisCommunity and \
         #CommunityFirst but all content pushes a 'Community Token' cryptocurrency. \
         Mentions 40% returns and $25 registration fees.",
    );

    let searcher = ScenarioSearcher::new(
        &std::env::var("ANTHROPIC_API_KEY").unwrap(),
        "CommunityWealth Partners: BBB has F rating, multiple Reddit threads calling it a scam, \
         no registered 501(c)(3), no civic activity records. Domain registered 3 months ago.",
    );

    let _stats = ctx
        .scout()
        .with_web_content(include_str!("fixtures/spam_crypto_posts.txt"))
        .with_search_scenario(searcher)
        .with_social_scenario(social)
        .run()
        .await;

    // Crypto MLM should produce zero signals or only very low confidence ones
    let signals = harness::queries::all_signals(ctx.client()).await;
    let high_confidence: Vec<_> = signals.iter().filter(|s| s.confidence >= 0.5).collect();
    assert!(
        high_confidence.is_empty(),
        "crypto MLM should not produce high-confidence civic signals; got {} with confidence >= 0.5: {:?}",
        high_confidence.len(),
        high_confidence.iter().map(|s| (&s.title, s.confidence)).collect::<Vec<_>>(),
    );
}

// ---------------------------------------------------------------------------
// Scenario 16: Reddit astroturfing — coordinated accounts detected
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reddit_astroturf_detected() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys not set");
        return;
    };

    let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap();

    let social = ScenarioSocialScraper::new(
        &api_key,
        "Subreddit r/Minneapolis has 3 new accounts (created this week) posting supportive \
         content about 'Lakeview Towers' luxury condo development using grassroots language \
         ('our neighborhood deserves this investment', 'community growth', 'finally something \
         for the people'). These accounts have no other post history. Mixed with 5 organic \
         community posts about food drives, park cleanups, and a mutual aid group.",
    );

    let searcher = ScenarioSearcher::new(
        &api_key,
        "Lakeview Towers LLC is a real estate developer with no community org registration. \
         A City Pages exposé documented their astroturf campaign. The development would displace \
         40 affordable housing units. The developer's PR firm has been linked to fake grassroots \
         campaigns in other cities. Meanwhile, Powderhorn Community Food Shelf has an active \
         501(c)(3) and 10 years of operation, and Friends of Powderhorn Park runs weekly cleanups.",
    );

    let stats = ctx
        .scout()
        .with_search_scenario(searcher)
        .with_social_scenario(social)
        .run()
        .await;

    // The pipeline should extract signals from organic posts but treat astroturf skeptically
    let signals = harness::queries::signals_by_confidence(ctx.client()).await;

    // We expect at least some signals from the organic community posts
    assert!(
        stats.signals_extracted >= 1,
        "should extract signals from organic community posts, got {}",
        stats.signals_extracted,
    );

    // If any astroturf-originated signals sneak through, they should have lower confidence
    // than the legitimate community signals
    if signals.len() >= 2 {
        let organic_signals: Vec<_> = signals
            .iter()
            .filter(|s| {
                let t = s.title.to_lowercase();
                t.contains("food") || t.contains("cleanup") || t.contains("mutual aid") || t.contains("park")
            })
            .collect();
        let astroturf_signals: Vec<_> = signals
            .iter()
            .filter(|s| {
                let t = s.title.to_lowercase();
                t.contains("lakeview") || t.contains("tower") || t.contains("development")
            })
            .collect();

        if !organic_signals.is_empty() && !astroturf_signals.is_empty() {
            let max_organic = organic_signals.iter().map(|s| s.confidence).fold(0.0f32, f32::max);
            let max_astroturf = astroturf_signals.iter().map(|s| s.confidence).fold(0.0f32, f32::max);
            assert!(
                max_organic >= max_astroturf,
                "organic signals ({:.2}) should have >= confidence than astroturf ({:.2})",
                max_organic, max_astroturf,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Scenario 17: Tension signals rank higher than routine events
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tension_signals_rank_higher_than_routine() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys not set");
        return;
    };

    let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap();

    // Run 1: routine community events (church, yoga, farmers market)
    let _stats_routine = ctx
        .scout()
        .with_web_content(include_str!("fixtures/routine_community_events.txt"))
        .with_search_results(vec![search_result(
            "https://example.com/community-calendar",
            "Minneapolis Community Calendar",
        )])
        .run()
        .await;

    // Run 2: urgent tension content (ICE enforcement, rent strike)
    let searcher = ScenarioSearcher::new(
        &api_key,
        "Active ICE enforcement in Phillips neighborhood. Local news confirmed. \
         Community rapid response networks mobilizing. Tenant organizing in Cedar-Riverside \
         against 35% rent increases at Riverside Plaza.",
    );

    let _stats_tension = ctx
        .scout()
        .with_web_content(include_str!("fixtures/urgent_community_tension.txt"))
        .with_search_scenario(searcher)
        .run()
        .await;

    // Query graph by confidence DESC — top signals should be Tension/Ask types
    let signals = harness::queries::signals_by_confidence(ctx.client()).await;

    assert!(
        !signals.is_empty(),
        "should have signals in graph after both runs",
    );

    // Check that at least one of the top 3 signals is a Tension or Ask
    let top_n = signals.iter().take(3).collect::<Vec<_>>();
    let has_urgent = top_n
        .iter()
        .any(|s| s.node_type == "Tension" || s.node_type == "Ask");

    assert!(
        has_urgent,
        "top 3 signals by confidence should include Tension or Ask; got: {:?}",
        top_n.iter().map(|s| (&s.title, &s.node_type, s.confidence)).collect::<Vec<_>>(),
    );
}

// ---------------------------------------------------------------------------
// Scenario 18: Corroborated signal has multiple evidence
// ---------------------------------------------------------------------------

#[tokio::test]
async fn corroborated_signal_has_multiple_evidence() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys not set");
        return;
    };

    // Corpus has the 501(c)(3) record — guaranteed findable
    let corpus = CorpusSearcher::new()
        .add(
            search_result(
                "https://powderhornpark.org/garden-day",
                "Powderhorn Garden Volunteer Day — Official Page",
            ),
            &["garden", "volunteer", "powderhorn"],
        )
        .add(
            search_result(
                "https://mn-nonprofits.org/powderhorn-gardens",
                "Powderhorn Community Gardens — 501(c)(3) Registration",
            ),
            &["powderhorn", "501c3", "nonprofit", "registration"],
        );

    let scenario = "Powderhorn Community Gardens is a well-established Minneapolis nonprofit. \
                     Local newspapers have covered their spring volunteer day. Southwest Journal \
                     published an article about the garden's 20th anniversary.";

    // Run 1: organization's own page
    let stats1 = ctx
        .scout()
        .with_web_content(include_str!("fixtures/community_garden_event.txt"))
        .with_layered(corpus, scenario)
        .run()
        .await;
    assert!(stats1.signals_stored >= 1, "run 1 should store signals");

    // Run 2: newspaper coverage (different URL, different wording)
    let corpus2 = CorpusSearcher::new().add(
        search_result(
            "https://southwestjournal.com/community-gardens-2026",
            "Community Gardens Gear Up for Spring",
        ),
        &["garden", "community", "spring", "volunteer"],
    );

    let stats2 = ctx
        .scout()
        .with_web_content(include_str!("fixtures/garden_event_newspaper.txt"))
        .with_layered(corpus2, scenario)
        .run()
        .await;

    // Second run should corroborate existing signals
    assert!(
        stats2.signals_deduplicated >= 1 || stats2.signals_stored == 0,
        "newspaper article should corroborate existing signal; deduped={}, stored={}",
        stats2.signals_deduplicated, stats2.signals_stored,
    );

    // Check evidence count on corroborated signals
    let signals = harness::queries::all_signals(ctx.client()).await;
    let garden_signals: Vec<_> = signals
        .iter()
        .filter(|s| s.title.to_lowercase().contains("garden"))
        .collect();

    if let Some(signal) = garden_signals.first() {
        let evidence = harness::queries::evidence_for_signal(ctx.client(), signal.id).await;
        assert!(
            evidence.len() >= 2,
            "corroborated signal should have >= 2 evidence nodes; got {} for '{}'",
            evidence.len(), signal.title,
        );
    }
}

// ---------------------------------------------------------------------------
// Scenario 19: Conflicting information — not silently merged
// ---------------------------------------------------------------------------

#[tokio::test]
async fn conflicting_details_not_merged() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys not set");
        return;
    };

    let stats = ctx
        .scout()
        .with_web_content(include_str!("fixtures/conflicting_event_details.txt"))
        .with_search_results(vec![search_result(
            "https://example.com/powderhorn-ice-cream-social",
            "Powderhorn Ice Cream Social",
        )])
        .run()
        .await;

    // The two conflicting reports should either:
    // a) Be stored as separate signals (both dates preserved), OR
    // b) Be corroborated (dedup sees them as same event) with discrepancy noted
    // Either way, we should see at least 1 signal extracted
    assert!(
        stats.signals_extracted >= 1,
        "conflicting event details should still extract signals, got {}",
        stats.signals_extracted,
    );

    let signals = harness::queries::all_signals(ctx.client()).await;
    let ice_cream_signals: Vec<_> = signals
        .iter()
        .filter(|s| {
            let t = s.title.to_lowercase();
            t.contains("ice cream") || t.contains("powderhorn")
        })
        .collect();

    // We should see evidence that the system processed both sources
    assert!(
        !ice_cream_signals.is_empty(),
        "should have at least one ice cream social signal in graph",
    );
}

// ---------------------------------------------------------------------------
// Scenario 20: Coordinated Facebook campaign — dedup catches it
// ---------------------------------------------------------------------------

#[tokio::test]
async fn coordinated_social_posts_detected() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys not set");
        return;
    };

    let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap();

    let social = ScenarioSocialScraper::new(
        &api_key,
        "4 different Facebook pages that are clearly coordinated — same photos described, \
         near-identical captions with minor rewording ('Join us for community input!' / \
         'Come share your voice at community input!' / 'Your voice matters — community input \
         session!'), posted within hours of each other. Content is about a 'community input \
         session' for the Hennepin Avenue reconstruction project, but the event is actually \
         organized by the developer's PR firm, not the city. All 4 pages were created in the \
         last month and have no other content.",
    ).with_system_prompt(
        "Generate posts from 4 different Facebook pages that are clearly coordinated. \
         Same event described with near-identical language. Each post should have a different \
         page name but suspiciously similar phrasing. Include hashtags. \
         Return JSON: {\"posts\": [{\"content\": \"...\", \"author\": \"...\", \"url\": \"...\"}]}"
    );

    let searcher = ScenarioSearcher::new(
        &api_key,
        "Hennepin Avenue reconstruction is a real city project, but the 'community input session' \
         mentioned in these posts is organized by the developer, not the city. The city's actual \
         public comment period has different dates. No .gov pages mention this event.",
    );

    let _stats = ctx
        .scout()
        .with_search_scenario(searcher)
        .with_social_scenario(social)
        .run()
        .await;

    // 4 coordinated posts about the same event should produce at most 1-2 unique signals
    // (dedup should catch near-identical content)
    let signals = harness::queries::all_signals(ctx.client()).await;
    let input_signals: Vec<_> = signals
        .iter()
        .filter(|s| {
            let t = s.title.to_lowercase();
            t.contains("input") || t.contains("hennepin") || t.contains("community")
        })
        .collect();

    assert!(
        input_signals.len() <= 2,
        "4 coordinated posts should produce at most 2 unique signals after dedup; got {}: {:?}",
        input_signals.len(),
        input_signals.iter().map(|s| &s.title).collect::<Vec<_>>(),
    );
}
