//! Extraction snapshot tests.
//!
//! Fixture content → LLM (recorded once) → ExtractionResponse snapshot →
//! `Extractor::convert_signals()` → assert domain fields.
//!
//! **Snapshots:** Each test calls the LLM once and saves the raw `ExtractionResponse`
//! JSON. Subsequent runs replay the snapshot through `convert_signals()` — testing
//! both the LLM output quality AND the conversion pipeline on every run.
//!
//! - Record snapshots:  `RECORD=1 cargo test -p rootsignal-scout --test extraction_test`
//! - Replay snapshots:  `cargo test -p rootsignal-scout --test extraction_test`
//!
//! Re-record when the extraction prompt changes.

use std::path::{Path, PathBuf};

use chrono::Datelike;
use rootsignal_scout::pipeline::extractor::{
    build_system_prompt, ExtractionResponse, ExtractionResult, Extractor,
};

// ---------------------------------------------------------------------------
// Snapshot helpers
// ---------------------------------------------------------------------------

fn snapshots_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("extraction")
}

fn load_snapshot(path: &Path) -> ExtractionResponse {
    let json = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read snapshot {}: {e}", path.display()));
    serde_json::from_str(&json)
        .unwrap_or_else(|e| panic!("Failed to parse snapshot {}: {e}", path.display()))
}

fn save_snapshot(path: &Path, response: &ExtractionResponse) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create snapshot dir");
    }
    let json = serde_json::to_string_pretty(response).expect("serialize snapshot");
    std::fs::write(path, json).expect("write snapshot");
}

/// Load a saved snapshot or record a new one by calling the LLM.
///
/// - **Replay:** loads the `ExtractionResponse` JSON from disk, then runs it
///   through `Extractor::convert_signals()` — the real conversion pipeline.
/// - **Record:** calls the LLM to get the raw `ExtractionResponse`, saves it,
///   then converts via `convert_signals()`.
///
/// Returns `(ExtractionResponse, ExtractionResult)` so tests can assert on
/// both the raw LLM output and the converted domain nodes.
async fn load_or_record(
    name: &str,
    content: &str,
    url: &str,
) -> (ExtractionResponse, ExtractionResult) {
    let snap_path = snapshots_dir().join(format!("{name}.json"));

    if snap_path.exists() && std::env::var("RECORD").is_err() {
        // Replay: load snapshot and convert through the real pipeline
        let response = load_snapshot(&snap_path);
        let result = Extractor::convert_signals(response.clone(), url);
        return (response, result);
    }

    // Record mode: call LLM directly to capture the raw ExtractionResponse
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .expect("ANTHROPIC_API_KEY required to record extraction snapshots");

    let claude = ai_client::claude::Claude::new(&api_key, "claude-haiku-4-5-20251001");
    let system_prompt = build_system_prompt("Minneapolis", 44.9778, -93.2650, &[]);

    // Truncate content to match what extract_impl does
    let content = if content.len() > 30_000 {
        let mut end = 30_000;
        while !content.is_char_boundary(end) {
            end -= 1;
        }
        &content[..end]
    } else {
        content
    };

    let user_prompt =
        format!("Extract all signals from this web page.\n\nSource URL: {url}\n\n---\n\n{content}");

    let response: ExtractionResponse = claude
        .extract(&system_prompt, &user_prompt)
        .await
        .expect("LLM extraction failed");

    save_snapshot(&snap_path, &response);
    let result = Extractor::convert_signals(response.clone(), url);

    (response, result)
}

// ---------------------------------------------------------------------------
// Fixture loaders
// ---------------------------------------------------------------------------

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read fixture {}: {e}", path.display()))
}

// ===========================================================================
// Test: community_garden_event
// ===========================================================================

#[tokio::test]
async fn community_garden_post_yields_gathering_signal() {
    let content = fixture("community_garden_event.txt");
    let (response, result) = load_or_record(
        "community_garden_event",
        &content,
        "https://powderhornpark.org/events/spring-2026",
    )
    .await;

    // Should extract at least one signal
    assert!(
        !result.nodes.is_empty(),
        "Should extract at least one signal from community garden fixture"
    );

    // Find the primary Gathering signal
    let gathering = result
        .nodes
        .iter()
        .find(|n| matches!(n, rootsignal_common::Node::Gathering(_)))
        .expect("Should extract a Gathering signal");

    let meta = gathering.meta().unwrap();

    // starts_at should be present (April 12 2026)
    if let rootsignal_common::Node::Gathering(g) = gathering {
        assert!(g.starts_at.is_some(), "Gathering should have starts_at");
        if let Some(starts) = g.starts_at {
            assert_eq!(starts.date_naive().month(), 4, "Should be April");
            assert_eq!(starts.date_naive().day(), 12, "Should be the 12th");
            assert_eq!(starts.date_naive().year(), 2026, "Should be 2026");
        }
    }

    // Location should be near Powderhorn (44.9486, -93.2636)
    assert!(
        meta.about_location.is_some(),
        "Should have location coordinates"
    );
    if let Some(loc) = &meta.about_location {
        let dist = rootsignal_common::haversine_km(loc.lat, loc.lng, 44.9486, -93.2636);
        assert!(
            dist < 2.0,
            "Location should be near Powderhorn, got ({}, {}), distance {dist}km",
            loc.lat,
            loc.lng
        );
    }

    // Location name should mention Powderhorn
    assert!(
        meta.about_location_name
            .as_deref()
            .map(|n| n.to_lowercase().contains("powderhorn"))
            .unwrap_or(false),
        "location_name should contain 'Powderhorn', got {:?}",
        meta.about_location_name
    );

    // Organizer should mention Powderhorn Park Neighborhood Association
    if let rootsignal_common::Node::Gathering(g) = gathering {
        assert!(
            g.organizer
                .as_deref()
                .map(|o| o.to_lowercase().contains("powderhorn"))
                .unwrap_or(false),
            "organizer should mention Powderhorn, got {:?}",
            g.organizer
        );
    }

    // action_url should contain eventbrite
    let has_eventbrite = response.signals.iter().any(|s| {
        s.action_url
            .as_deref()
            .map(|u| u.to_lowercase().contains("eventbrite"))
            .unwrap_or(false)
    });
    assert!(has_eventbrite, "Should have an eventbrite action_url");

    // mentioned_actors should include Cafe Racer or Briva Health
    let all_actors: Vec<String> = response
        .signals
        .iter()
        .flat_map(|s| s.mentioned_actors.iter().flatten())
        .map(|a| a.to_lowercase())
        .collect();
    let has_cafe_racer = all_actors.iter().any(|a| a.contains("cafe racer"));
    let has_briva = all_actors.iter().any(|a| a.contains("briva"));
    assert!(
        has_cafe_racer || has_briva,
        "mentioned_actors should include 'Cafe Racer' or 'Briva Health', got {:?}",
        all_actors
    );
}

// ===========================================================================
// Test: food_shelf_give
// ===========================================================================

#[tokio::test]
async fn food_shelf_post_yields_aid_signal() {
    let content = fixture("food_shelf_give.txt");
    let (response, result) = load_or_record(
        "food_shelf_give",
        &content,
        "https://brivahealth.org/food-shelf",
    )
    .await;

    assert!(
        !result.nodes.is_empty(),
        "Should extract at least one signal from food shelf fixture"
    );

    // Find an Aid signal
    let aid = result
        .nodes
        .iter()
        .find(|n| matches!(n, rootsignal_common::Node::Aid(_)))
        .expect("Should extract an Aid signal");

    let meta = aid.meta().unwrap();

    // availability should mention days/hours
    if let rootsignal_common::Node::Aid(a) = aid {
        assert!(
            a.availability.is_some(),
            "Aid should have availability schedule"
        );
        let avail = a.availability.as_deref().unwrap().to_lowercase();
        assert!(
            avail.contains("tuesday") || avail.contains("tue") || avail.contains("fri"),
            "availability should mention days, got: {}",
            avail
        );
    }

    // Location near 420 15th Ave S (44.9696, -93.2466)
    assert!(
        meta.about_location.is_some(),
        "Should have location coordinates"
    );
    if let Some(loc) = &meta.about_location {
        let dist = rootsignal_common::haversine_km(loc.lat, loc.lng, 44.9696, -93.2466);
        assert!(
            dist < 2.0,
            "Location should be near 420 15th Ave S, got ({}, {}), distance {dist}km",
            loc.lat,
            loc.lng
        );
    }

    // action_url should contain brivahealth
    let has_briva_url = response.signals.iter().any(|s| {
        s.action_url
            .as_deref()
            .map(|u| u.to_lowercase().contains("brivahealth"))
            .unwrap_or(false)
    });
    assert!(has_briva_url, "Should have a brivahealth action_url");

    // Should have resource tags with "food" and role "offers"
    let has_food_resource = response.signals.iter().any(|s| {
        s.resources
            .iter()
            .any(|r| r.slug.contains("food") && r.role == "offers")
    });
    assert!(
        has_food_resource,
        "Should have a ResourceTag with slug containing 'food' and role 'offers'"
    );
}

// ===========================================================================
// Test: urgent_community_tension
// ===========================================================================

#[tokio::test]
async fn urgent_community_issue_yields_tension_signal() {
    let content = fixture("urgent_community_tension.txt");
    let (response, result) = load_or_record(
        "urgent_community_tension",
        &content,
        "https://community-alerts.example.com/phillips-update",
    )
    .await;

    // Should extract multiple signals (tension + gathering + aid)
    assert!(
        result.nodes.len() >= 2,
        "Should extract at least 2 signals, got {}",
        result.nodes.len()
    );

    // At least one Tension with high severity and enforcement/immigration category
    let tensions: Vec<_> = result
        .nodes
        .iter()
        .filter(|n| matches!(n, rootsignal_common::Node::Tension(_)))
        .collect();
    assert!(
        !tensions.is_empty(),
        "Should extract at least one Tension signal"
    );

    let has_ice_tension = tensions.iter().any(|n| {
        if let rootsignal_common::Node::Tension(t) = n {
            let severity_ok = matches!(
                t.severity,
                rootsignal_common::Severity::High | rootsignal_common::Severity::Critical
            );
            let category_ok = t
                .category
                .as_deref()
                .map(|c| {
                    let cl = c.to_lowercase();
                    cl.contains("enforcement")
                        || cl.contains("immigration")
                        || cl.contains("civil_rights")
                })
                .unwrap_or(false);
            severity_ok && category_ok
        } else {
            false
        }
    });
    assert!(
        has_ice_tension,
        "Should have a high-severity Tension with enforcement/immigration category"
    );

    // Should have at least one Gathering (emergency community meeting)
    let gatherings: Vec<_> = result
        .nodes
        .iter()
        .filter(|n| matches!(n, rootsignal_common::Node::Gathering(_)))
        .collect();
    // The LLM may or may not extract the emergency meeting as a separate Gathering;
    // it might embed it in the Tension summary. Check for either.
    let has_meeting_signal = !gatherings.is_empty()
        || response.signals.iter().any(|s| {
            s.title.to_lowercase().contains("meeting")
                || s.summary.to_lowercase().contains("community meeting")
        });
    assert!(
        has_meeting_signal,
        "Should extract an emergency community meeting (Gathering or mentioned in summary)"
    );

    // Should have at least one Aid (legal support, safe spaces)
    let aids: Vec<_> = result
        .nodes
        .iter()
        .filter(|n| matches!(n, rootsignal_common::Node::Aid(_)))
        .collect();
    let has_aid_signal = !aids.is_empty()
        || response.signals.iter().any(|s| {
            let lower = s.summary.to_lowercase();
            lower.contains("legal") || lower.contains("safe space") || lower.contains("shelter")
        });
    assert!(
        has_aid_signal,
        "Should extract legal support or safe spaces as Aid signals"
    );

    // mentioned_actors across all signals should include MIRC or Minneapolis Immigrant Rights Coalition
    let all_actors: Vec<String> = response
        .signals
        .iter()
        .flat_map(|s| s.mentioned_actors.iter().flatten())
        .map(|a| a.to_lowercase())
        .collect();
    let has_mirc = all_actors
        .iter()
        .any(|a| a.contains("mirc") || a.contains("immigrant rights coalition"));
    assert!(
        has_mirc,
        "mentioned_actors should include MIRC or Minneapolis Immigrant Rights Coalition, got {:?}",
        all_actors
    );
}

// ===========================================================================
// Adversarial: satirical content should not produce real signals
// ===========================================================================

#[tokio::test]
async fn satirical_content_produces_no_real_signals() {
    let content = fixture("satirical_community_alert.txt");
    let (response, result) = load_or_record(
        "satirical_community_alert",
        &content,
        "https://phillipspatch.example.com/satire/potholes",
    )
    .await;

    // The extractor might produce signals, but they should be low-quality or
    // clearly satirical. Ideally it extracts nothing actionable.
    //
    // Key checks:
    // 1. No Gathering should have a real starts_at (the "Pothole Appreciation Walk"
    //    is explicitly "NOT a real event")
    // 2. No Need should have an action_url (the GoFundMe is a joke for $47)
    // 3. If signals are extracted, summaries should reflect the satirical tone
    //    or the signal count should be very low.

    // Lenient check: the extractor might still extract signals (LLMs don't
    // always detect satire), but there shouldn't be high-confidence ones.
    let gatherings: Vec<_> = result
        .nodes
        .iter()
        .filter(|n| matches!(n, rootsignal_common::Node::Gathering(_)))
        .collect();

    for g in &gatherings {
        if let rootsignal_common::Node::Gathering(g) = g {
            // The "Pothole Appreciation Walk" should not have a real date
            let title_lower = g.meta.title.to_lowercase();
            if title_lower.contains("pothole") && title_lower.contains("walk") {
                assert!(
                    g.starts_at.is_none(),
                    "Satirical 'Pothole Appreciation Walk' should NOT have a real starts_at"
                );
            }
        }
    }

    // The fake GoFundMe ($47 from 3 donors) should not be extracted as a real Need
    let serious_needs: Vec<_> = result
        .nodes
        .iter()
        .filter(|n| {
            if let rootsignal_common::Node::Need(need) = n {
                // If action_url is present, it's being treated as real
                need.action_url.is_some()
            } else {
                false
            }
        })
        .collect();

    // This is a soft assertion — we document whether the LLM falls for satire
    if !serious_needs.is_empty() {
        eprintln!(
            "WARNING: Extractor produced {} Need signals from satirical content. \
             LLM may not be detecting satire.",
            serious_needs.len()
        );
    }

    // Hard check: the fake phone number "612-555-HOLE" should not appear as
    // a real action URL
    let has_fake_phone = response.signals.iter().any(|s| {
        s.action_url
            .as_deref()
            .map(|u| u.contains("555-HOLE"))
            .unwrap_or(false)
    });
    assert!(
        !has_fake_phone,
        "Fake phone number '612-555-HOLE' should not be an action_url"
    );
}

// ===========================================================================
// Adversarial: vague/partial dates
// ===========================================================================

#[tokio::test]
async fn vague_dates_handled_gracefully() {
    let content = fixture("vague_dates_event.txt");
    let (response, result) = load_or_record(
        "vague_dates_event",
        &content,
        "https://phillipsneighborhood.org/bulletin/feb-2026",
    )
    .await;

    // Should extract several signals despite vague timing
    assert!(
        !result.nodes.is_empty(),
        "Should extract signals even with vague dates"
    );

    // The "Community Garden Plot Lottery" deadline "was last Tuesday" — this is
    // a past event. It should either:
    // - Not be extracted (ideal)
    // - Be extracted with published_at in the past
    let lottery_signals: Vec<_> = response
        .signals
        .iter()
        .filter(|s| {
            s.title.to_lowercase().contains("lottery")
                || s.title.to_lowercase().contains("garden plot")
        })
        .collect();

    // If extracted, starts_at should NOT be in the future (deadline was "last Tuesday")
    for s in &lottery_signals {
        if let Some(ref dt_str) = s.starts_at {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(dt_str) {
                assert!(
                    dt < chrono::Utc::now(),
                    "Garden plot lottery deadline should be in the past, got {}",
                    dt_str
                );
            }
        }
    }

    // "Sometime in mid-June" — Block Party should either:
    // - Have no starts_at (can't determine exact date)
    // - Have an approximate June date
    let block_party_signals: Vec<_> = response
        .signals
        .iter()
        .filter(|s| s.title.to_lowercase().contains("block party"))
        .collect();
    for s in &block_party_signals {
        if let Some(ref dt_str) = s.starts_at {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(dt_str) {
                assert_eq!(
                    dt.month(),
                    6,
                    "Block party 'mid-June' should be in June if date is guessed, got month {}",
                    dt.month()
                );
            }
        }
        // It's also fine if starts_at is None — the date is genuinely vague
    }

    // "Every other Wednesday starting first week of April" — should have some
    // recurrence indication
    let food_dist_signals: Vec<_> = response
        .signals
        .iter()
        .filter(|s| {
            s.title.to_lowercase().contains("food")
                || s.summary.to_lowercase().contains("food distribution")
        })
        .collect();
    let has_recurring = food_dist_signals
        .iter()
        .any(|s| s.is_recurring == Some(true));
    // Soft check — recurring might be hard to detect from "every other Wednesday"
    if !has_recurring && !food_dist_signals.is_empty() {
        eprintln!("NOTE: Food distribution 'every other Wednesday' not marked as recurring");
    }
}

// ===========================================================================
// Adversarial: multi-location service
// ===========================================================================

#[tokio::test]
async fn multi_location_service_extracts_multiple_signals() {
    let content = fixture("multi_location_service.txt");
    let (_response, result) = load_or_record(
        "multi_location_service",
        &content,
        "https://mplsmutualaid.org/fridges",
    )
    .await;

    // The fixture describes 4 fridge locations. The extractor should ideally
    // create separate Aid signals for each location.
    let aid_signals: Vec<_> = result
        .nodes
        .iter()
        .filter(|n| matches!(n, rootsignal_common::Node::Aid(_)))
        .collect();

    // At minimum, should extract more than one Aid signal
    assert!(
        aid_signals.len() >= 2,
        "Multi-location service should produce multiple Aid signals (one per fridge), got {}",
        aid_signals.len()
    );

    // Each Aid signal should have a distinct location
    let locations: Vec<Option<&rootsignal_common::GeoPoint>> = aid_signals
        .iter()
        .map(|n| n.meta().unwrap().about_location.as_ref())
        .collect();

    let with_coords: Vec<_> = locations.iter().filter(|l| l.is_some()).collect();
    assert!(
        with_coords.len() >= 2,
        "At least 2 fridge signals should have distinct coordinates, got {}",
        with_coords.len()
    );

    // Location names should cover different neighborhoods
    let location_names: Vec<String> = aid_signals
        .iter()
        .filter_map(|n| n.meta().unwrap().about_location_name.clone())
        .map(|n| n.to_lowercase())
        .collect();

    let neighborhoods = ["powderhorn", "cedar", "north", "longfellow"];
    let matched_neighborhoods: Vec<_> = neighborhoods
        .iter()
        .filter(|nh| location_names.iter().any(|ln| ln.contains(**nh)))
        .collect();
    assert!(
        matched_neighborhoods.len() >= 2,
        "Should reference at least 2 distinct neighborhoods, got {:?} from {:?}",
        matched_neighborhoods,
        location_names
    );

    // All should be ongoing (24/7 operation)
    for aid in &aid_signals {
        if let rootsignal_common::Node::Aid(a) = aid {
            assert!(
                a.is_ongoing,
                "Community fridges are 24/7, should be marked ongoing: {}",
                a.meta.title
            );
        }
    }
}

// ===========================================================================
// Adversarial: phone-only resource list (no URLs)
// ===========================================================================

#[tokio::test]
async fn phone_only_resource_extracts_aid_signals() {
    let content = fixture("phone_only_resource.txt");
    let (response, result) = load_or_record(
        "phone_only_resource",
        &content,
        "https://phillipsnetwork.org/resources",
    )
    .await;

    // Should extract multiple Aid signals (crisis, food, shelter, legal, health)
    assert!(
        result.nodes.len() >= 3,
        "Phone-only resource list should produce multiple signals, got {}",
        result.nodes.len()
    );

    // Most signals will lack action_url since the source only has phone numbers.
    // Check that phone numbers are preserved somewhere (summary, availability, etc.)
    let all_text: String = response
        .signals
        .iter()
        .map(|s| format!("{} {} {:?}", s.title, s.summary, s.availability))
        .collect::<Vec<_>>()
        .join(" ");
    let all_lower = all_text.to_lowercase();

    // At least some phone numbers should appear in summaries or availability
    let has_phone =
        all_lower.contains("612") || all_lower.contains("763") || all_lower.contains("651");
    assert!(
        has_phone,
        "Phone numbers should be preserved in signal text"
    );

    // Signals without action_url should still have useful information
    let no_url_signals: Vec<_> = response
        .signals
        .iter()
        .filter(|s| s.action_url.is_none() || s.action_url.as_deref() == Some(""))
        .collect();
    // It's OK if some signals use the source URL as action_url fallback
    // But the key info (phone numbers) shouldn't be lost
    eprintln!(
        "Phone-only fixture: {} signals total, {} without action_url",
        response.signals.len(),
        no_url_signals.len()
    );
}

// ===========================================================================
// Adversarial: Spanish-language community alert
// ===========================================================================

#[tokio::test]
async fn spanish_content_yields_signals_in_english() {
    let content = fixture("spanish_community_alert.txt");
    let (response, result) = load_or_record(
        "spanish_community_alert",
        &content,
        "https://alianzacomunitaria.org/aviso/marzo-2026",
    )
    .await;

    // Should extract signals despite being in Spanish
    assert!(
        !result.nodes.is_empty(),
        "Should extract signals from Spanish-language content"
    );

    // Should extract the food distribution as an Aid or Gathering
    let has_food = response.signals.iter().any(|s| {
        let lower = format!("{} {}", s.title, s.summary).to_lowercase();
        lower.contains("food") || lower.contains("alimento") || lower.contains("distribu")
    });
    assert!(has_food, "Should extract the food distribution event");

    // Should extract the legal clinic
    let has_legal = response.signals.iter().any(|s| {
        let lower = format!("{} {}", s.title, s.summary).to_lowercase();
        lower.contains("legal") || lower.contains("tenant") || lower.contains("inquilino")
    });
    assert!(
        has_legal,
        "Should extract the legal clinic / tenant rights event"
    );

    // Location should be in Phillips/Minneapolis area
    let has_local_location = result.nodes.iter().any(|n| {
        let meta = n.meta().unwrap();
        meta.about_location_name
            .as_deref()
            .map(|l| {
                let lower = l.to_lowercase();
                lower.contains("minneapolis")
                    || lower.contains("phillips")
                    || lower.contains("lake st")
                    || lower.contains("11th ave")
            })
            .unwrap_or(false)
            || meta
                .about_location
                .map(|loc| {
                    rootsignal_common::haversine_km(loc.lat, loc.lng, 44.9486, -93.2476) < 5.0
                })
                .unwrap_or(false)
    });
    assert!(
        has_local_location,
        "Spanish content should still produce Minneapolis-area locations"
    );

    // Should detect MIRC as a mentioned actor
    let all_actors: Vec<String> = response
        .signals
        .iter()
        .flat_map(|s| s.mentioned_actors.iter().flatten())
        .map(|a| a.to_lowercase())
        .collect();
    let has_mirc = all_actors.iter().any(|a| a.contains("mirc"));
    let has_volunteer_lawyers = all_actors
        .iter()
        .any(|a| a.contains("volunteer lawyers") || a.contains("lawyers network"));
    assert!(
        has_mirc || has_volunteer_lawyers,
        "Should detect MIRC or Volunteer Lawyers Network as actors, got {:?}",
        all_actors
    );
}

// ===========================================================================
// Adversarial: stale/closed program (content from 2019)
// ===========================================================================

#[tokio::test]
async fn closed_program_excluded_or_marked_inactive() {
    let content = fixture("stale_closed_program.txt");
    let (response, result) = load_or_record(
        "stale_closed_program",
        &content,
        "https://mplscares.org/coat-drive-2019",
    )
    .await;

    // This is a completed 2019 coat drive — ideally the extractor recognizes
    // it as stale/completed and either:
    // 1. Extracts no signals (ideal)
    // 2. Extracts signals but with published_at in 2019 (so staleness filter catches them)
    // 3. Extracts signals (worst case — documenting current behavior)

    if !result.nodes.is_empty() {
        // If signals were extracted, check published_at
        let has_old_published_at = response.signals.iter().any(|s| {
            s.published_at
                .as_deref()
                .map(|d| d.contains("2019") || d.contains("2020"))
                .unwrap_or(false)
        });

        if has_old_published_at {
            eprintln!(
                "Good: Extractor set published_at to 2019/2020 for stale fixture ({} signals)",
                result.nodes.len()
            );
        } else {
            eprintln!(
                "WARNING: Extractor produced {} signals from 2019 content without old published_at. \
                 Staleness filtering gap detected.",
                result.nodes.len()
            );
        }
    } else {
        eprintln!("Excellent: Extractor correctly produced no signals from closed 2019 program");
    }

    // Drop-off locations are explicitly "NOW CLOSED" — should not have Aid signals
    // with is_ongoing=true
    let ongoing_aids: Vec<_> = result
        .nodes
        .iter()
        .filter(|n| {
            if let rootsignal_common::Node::Aid(a) = n {
                a.is_ongoing
            } else {
                false
            }
        })
        .collect();
    assert!(
        ongoing_aids.is_empty(),
        "Closed 2019 program should NOT produce ongoing Aid signals, got {}",
        ongoing_aids.len()
    );

    // No starts_at should be in 2019 if the extractor misinterprets the dates
    // as upcoming events
    for node in &result.nodes {
        if let rootsignal_common::Node::Gathering(g) = node {
            if let Some(starts) = g.starts_at {
                assert!(
                    starts.date_naive().year() >= 2025,
                    "Should not create future-looking Gathering from 2019 content, got year {}",
                    starts.date_naive().year()
                );
            }
        }
    }
}
