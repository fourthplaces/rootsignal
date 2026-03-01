//! Tests for the first-hand filter prompt.
//!
//! The first-hand filter is prepended to content from web search and social
//! search results (in scrape_phase.rs) to distinguish actionable community
//! signal from abstract political commentary. The LLM sets `is_firsthand`
//! on each extracted signal.
//!
//! These tests call the real LLM with the filter prepended — exactly as
//! scrape_phase does — and assert that `is_firsthand` is classified correctly.
//!
//! **Snapshots:** Each test records the raw `ExtractionResponse` JSON on first
//! run (or when `RECORD=1`). Subsequent runs replay from the snapshot.
//!
//! - Record snapshots:  `RECORD=1 cargo test -p rootsignal-scout --test firsthand_filter_test`
//! - Replay snapshots:  `cargo test -p rootsignal-scout --test firsthand_filter_test`

use std::path::{Path, PathBuf};

use rootsignal_scout::core::extractor::{build_system_prompt, ExtractionResponse};

// ---------------------------------------------------------------------------
// The first-hand filter prompt — copied from scrape_phase.rs web search path.
// If the prompt changes there, update it here (or extract to a shared constant).
// ---------------------------------------------------------------------------

const FIRSTHAND_FILTER: &str = "\
FIRST-HAND FILTER (applies to this content):\n\
This content comes from web search results, which may contain \
abstract political commentary. Apply filtering based on LOCAL SPECIFICITY:\n\n\
Mark is_firsthand: true when the content describes:\n\
- Concrete events, services, or gatherings at specific times and places\n\
- Local journalism reporting impacts on specific communities, schools, or neighborhoods\n\
- Community organizing with actionable details (rallies, petitions, forums, resource centers)\n\
- People or organizations describing what is happening in their area\n\
- Event listings from organizers creating community activity\n\n\
Mark is_firsthand: false when the content is:\n\
- Abstract political opinion with no local specificity (\"ICE is doing great work\", \"open borders are wrong\")\n\
- National punditry that mentions a city only in passing\n\
- Social media hot takes expressing a viewpoint without describing concrete local reality\n\n\
The question is NOT \"is the author personally affected?\" — a journalist reporting \
on school closures with specific dates, locations, and community responses is firsthand. \
The question IS \"does this describe concrete reality in a specific place, or is it \
abstract political commentary?\"\n\n\
Only extract signals where is_firsthand is true. Reject the rest.\n\n";

// ---------------------------------------------------------------------------
// Snapshot helpers
// ---------------------------------------------------------------------------

fn snapshots_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("firsthand_filter")
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

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read fixture {}: {e}", path.display()))
}

/// Prepend the first-hand filter to content (mimicking scrape_phase.rs),
/// send through the extractor, and return the raw ExtractionResponse.
async fn extract_with_firsthand_filter(
    name: &str,
    content: &str,
    source_url: &str,
) -> ExtractionResponse {
    let snap_path = snapshots_dir().join(format!("{name}.json"));

    if snap_path.exists() && std::env::var("RECORD").is_err() {
        return load_snapshot(&snap_path);
    }

    // Record mode: call LLM with the first-hand filter prepended
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .expect("ANTHROPIC_API_KEY required to record firsthand filter snapshots");

    let system_prompt = build_system_prompt("Minneapolis", 44.9778, -93.2650, &[]);
    let filtered_content = format!("{FIRSTHAND_FILTER}{content}");
    let user_prompt = format!(
        "Extract all signals from this web page.\n\nSource URL: {source_url}\n\n---\n\n{filtered_content}"
    );

    let claude = ai_client::claude::Claude::new(&api_key, "claude-haiku-4-5-20251001");
    let response: ExtractionResponse = claude
        .extract(&system_prompt, &user_prompt)
        .await
        .expect("LLM extraction failed");

    save_snapshot(&snap_path, &response);
    response
}

// ===========================================================================
// SHOULD KEEP: Content that describes concrete local community activity
// ===========================================================================

/// An Eventbrite community discussion event should NOT be classified as
/// political commentary, even though it discusses political themes.
/// The organizer is creating real community activity at a specific time/place.
#[tokio::test]
async fn eventbrite_community_discussion_is_firsthand() {
    let content = fixture("eventbrite_civility_discussion.txt");
    let response = extract_with_firsthand_filter(
        "eventbrite_civility_discussion",
        &content,
        "https://www.eventbrite.com/e/civility-peace-tickets-1982450989275",
    )
    .await;

    assert!(
        !response.signals.is_empty(),
        "Should extract at least one signal from community discussion event"
    );

    // No signal should be marked is_firsthand: false
    let dropped: Vec<_> = response
        .signals
        .iter()
        .filter(|s| s.is_firsthand == Some(false))
        .map(|s| s.title.as_str())
        .collect();
    assert!(
        dropped.is_empty(),
        "Community event signals should NOT be marked is_firsthand: false. Dropped: {dropped:?}"
    );
}

/// Local journalism reporting concrete impacts on specific communities
/// (schools, families, neighborhoods) should pass the filter. The MinnPost
/// article describes ICE impacts on Fridley schools, specific superintendent
/// quotes, a family resource center — all concrete and local.
#[tokio::test]
async fn local_journalism_with_community_impact_is_firsthand() {
    let content = fixture("minnpost_ice_impact.txt");
    let response = extract_with_firsthand_filter(
        "minnpost_ice_impact",
        &content,
        "https://www.minnpost.com/glean/2026/02/have-things-changed-since-the-end-of-operation-metro-surge/",
    )
    .await;

    assert!(
        !response.signals.is_empty(),
        "Should extract signals from local journalism about community impacts"
    );

    // The article describes a family resource center with specific services,
    // school attendance drops, and sanctuary policy — these are concrete local impacts.
    let dropped: Vec<_> = response
        .signals
        .iter()
        .filter(|s| s.is_firsthand == Some(false))
        .map(|s| s.title.as_str())
        .collect();
    assert!(
        dropped.is_empty(),
        "Local journalism with concrete community impacts should NOT be marked \
         is_firsthand: false. Dropped: {dropped:?}"
    );
}

/// A neighborhood safety walk organized by a neighborhood association in
/// response to specific local incidents should pass. The organizer is directly
/// involved in the community.
#[tokio::test]
async fn neighborhood_safety_meeting_is_firsthand() {
    let content = fixture("neighborhood_safety_meeting.txt");
    let response = extract_with_firsthand_filter(
        "neighborhood_safety_meeting",
        &content,
        "https://whittieralliance.org/safety-walk-2026",
    )
    .await;

    assert!(
        !response.signals.is_empty(),
        "Should extract signals from neighborhood safety meeting"
    );

    let dropped: Vec<_> = response
        .signals
        .iter()
        .filter(|s| s.is_firsthand == Some(false))
        .map(|s| s.title.as_str())
        .collect();
    assert!(
        dropped.is_empty(),
        "Neighborhood safety meeting signals should NOT be marked is_firsthand: false. \
         Dropped: {dropped:?}"
    );
}

/// A mosque open house responding to Islamophobic incidents — the community
/// is directly affected and organizing a concrete response.
#[tokio::test]
async fn mosque_open_house_responding_to_incidents_is_firsthand() {
    let content = fixture("mosque_open_house.txt");
    let response = extract_with_firsthand_filter(
        "mosque_open_house",
        &content,
        "https://masjidannur.org/events/open-house-march-2026",
    )
    .await;

    assert!(
        !response.signals.is_empty(),
        "Should extract signals from mosque open house event"
    );

    let dropped: Vec<_> = response
        .signals
        .iter()
        .filter(|s| s.is_firsthand == Some(false))
        .map(|s| s.title.as_str())
        .collect();
    assert!(
        dropped.is_empty(),
        "Mosque open house signals should NOT be marked is_firsthand: false. \
         Dropped: {dropped:?}"
    );
}

/// Local journalism about school closures affecting specific neighborhoods —
/// parents rallying, specific schools named, petition with 2,000 signatures,
/// community forum organized. Concrete local impacts with specific people.
#[tokio::test]
async fn local_journalism_school_closures_is_firsthand() {
    let content = fixture("local_journalism_school_closures.txt");
    let response = extract_with_firsthand_filter(
        "local_journalism_school_closures",
        &content,
        "https://southwestjournal.com/parents-rally-school-closures-2026/",
    )
    .await;

    assert!(
        !response.signals.is_empty(),
        "Should extract signals from school closure coverage"
    );

    let dropped: Vec<_> = response
        .signals
        .iter()
        .filter(|s| s.is_firsthand == Some(false))
        .map(|s| s.title.as_str())
        .collect();
    assert!(
        dropped.is_empty(),
        "School closure coverage with community organizing should NOT be marked \
         is_firsthand: false. Dropped: {dropped:?}"
    );
}

// ===========================================================================
// SHOULD DROP: Abstract political commentary with no local specificity
// ===========================================================================

/// Pure political hot takes on social media — no specific community, no
/// concrete impact, no actionable information. All signals should be
/// is_firsthand: false (or no signals extracted at all).
#[tokio::test]
async fn political_hot_takes_are_not_firsthand() {
    let content = fixture("political_hot_takes.txt");
    let response = extract_with_firsthand_filter(
        "political_hot_takes",
        &content,
        "https://twitter.com/search?q=minneapolis+immigration",
    )
    .await;

    // Either no signals extracted, or all marked is_firsthand: false
    let kept: Vec<_> = response
        .signals
        .iter()
        .filter(|s| s.is_firsthand != Some(false))
        .map(|s| s.title.as_str())
        .collect();
    assert!(
        kept.is_empty(),
        "Political hot takes should all be marked is_firsthand: false (or not extracted). \
         Incorrectly kept: {kept:?}"
    );
}

/// National punditry with no local grounding — an AEI fellow opining about
/// immigration policy from 30,000 feet. Mentions Minneapolis once in passing
/// but provides no local information.
#[tokio::test]
async fn national_punditry_is_not_firsthand() {
    let content = fixture("national_punditry_immigration.txt");
    let response = extract_with_firsthand_filter(
        "national_punditry_immigration",
        &content,
        "https://www.nationalreview.com/2026/02/the-immigration-debate-were-not-having/",
    )
    .await;

    // Either no signals extracted, or all marked is_firsthand: false
    let kept: Vec<_> = response
        .signals
        .iter()
        .filter(|s| s.is_firsthand != Some(false))
        .map(|s| s.title.as_str())
        .collect();
    assert!(
        kept.is_empty(),
        "National punditry should all be marked is_firsthand: false (or not extracted). \
         Incorrectly kept: {kept:?}"
    );
}
