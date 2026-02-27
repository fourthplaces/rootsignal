//! Tests for the investigation triage gate.
//!
//! The investigator generates search queries to corroborate signals. But routine
//! events (fundraisers, food shelves, concerts) don't make claims that need
//! verification. The triage instruction in QUERY_GENERATION_SYSTEM tells the LLM
//! to return empty queries for these signals, short-circuiting investigation.
//!
//! These tests call the real LLM with the query generation prompt — exactly as
//! investigator.rs does — and assert whether queries are returned.
//!
//! **Snapshots:** Each test records the raw `InvestigationQueries` JSON on first
//! run (or when `RECORD=1`). Subsequent runs replay from the snapshot.
//!
//! - Record snapshots:  `RECORD=1 cargo test -p rootsignal-scout --test investigation_triage_test`
//! - Replay snapshots:  `cargo test -p rootsignal-scout --test investigation_triage_test`

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Mirror the types and prompt from investigator.rs.
// If the prompt changes there, update it here.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
struct InvestigationQueries {
    queries: Vec<String>,
}

const QUERY_GENERATION_SYSTEM: &str = "\
You are an investigator for an intelligence system. \
Generate 1-3 targeted web search queries to verify/corroborate the signal. \
Focus on: official sources, org verification (501c3, registration), independent reporting, primary documents. \
Do NOT generate vague queries or queries returning the original source.\n\n\
TRIAGE: Not every signal warrants investigation. Return an EMPTY queries list for signals that:\n\
- Describe routine community events (fundraisers, cleanups, concerts, yoga classes, potlucks)\n\
- List resource availability (food shelf hours, clothing drives, open gym schedules)\n\
- Announce recurring activities with no claims that need verification\n\
- Are simple event promotions or entertainment listings\n\n\
Only generate queries when the signal makes a factual claim that could be true or false \
and where independent corroboration would change how much we trust the signal \
(e.g., alleged misconduct, disputed statistics, unverified closures, public health hazards).";

// ---------------------------------------------------------------------------
// Snapshot helpers
// ---------------------------------------------------------------------------

fn snapshots_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("investigation_triage")
}

fn load_snapshot(path: &Path) -> InvestigationQueries {
    let json = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read snapshot {}: {e}", path.display()));
    serde_json::from_str(&json)
        .unwrap_or_else(|e| panic!("Failed to parse snapshot {}: {e}", path.display()))
}

fn save_snapshot(path: &Path, response: &InvestigationQueries) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create snapshot dir");
    }
    let json = serde_json::to_string_pretty(response).expect("serialize snapshot");
    std::fs::write(path, json).expect("write snapshot");
}

fn load_env() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(".env");
    if let Ok(content) = std::fs::read_to_string(&path) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                if std::env::var(key.trim()).is_err() {
                    std::env::set_var(key.trim(), value.trim());
                }
            }
        }
    }
}

/// Generate investigation queries for a signal, using snapshot replay.
async fn generate_queries(name: &str, signal_type: &str, description: &str) -> InvestigationQueries {
    load_env();

    let snap_path = snapshots_dir().join(format!("{name}.json"));

    if snap_path.exists() && std::env::var("RECORD").is_err() {
        return load_snapshot(&snap_path);
    }

    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .expect("ANTHROPIC_API_KEY required to record investigation triage snapshots");

    let user_prompt = format!(
        "Signal type: {signal_type}\nTitle: {description}\nSummary: {description}\nSource URL: https://example.com\nCity: Minneapolis",
    );

    let claude = ai_client::claude::Claude::new(&api_key, "claude-haiku-4-5-20251001");
    let response: InvestigationQueries = claude
        .extract(QUERY_GENERATION_SYSTEM, &user_prompt)
        .await
        .expect("LLM query generation failed");

    save_snapshot(&snap_path, &response);
    response
}

// ===========================================================================
// SHOULD INVESTIGATE: Claims that need verification
// ===========================================================================

#[tokio::test]
async fn disputed_claim_warrants_investigation() {
    let queries = generate_queries(
        "disputed_claim",
        "Tension",
        "Local nonprofit accused of misusing federal housing funds — HUD audit pending",
    )
    .await;

    assert!(
        !queries.queries.is_empty(),
        "Disputed claim about fund misuse should generate investigation queries"
    );
}

#[tokio::test]
async fn safety_hazard_warrants_investigation() {
    let queries = generate_queries(
        "safety_hazard",
        "Tension",
        "Lead contamination found in drinking water at Riverside elementary school",
    )
    .await;

    assert!(
        !queries.queries.is_empty(),
        "Public health hazard claim should generate investigation queries"
    );
}

#[tokio::test]
async fn unverified_closure_warrants_investigation() {
    let queries = generate_queries(
        "unverified_closure",
        "Tension",
        "Hennepin County to close 3 homeless shelters by March, displacing 200 residents",
    )
    .await;

    assert!(
        !queries.queries.is_empty(),
        "Unverified closure with specific numbers should generate investigation queries"
    );
}

// ===========================================================================
// SHOULD NOT INVESTIGATE: Routine events / resource listings
// ===========================================================================

#[tokio::test]
async fn fundraiser_skipped() {
    let queries = generate_queries(
        "fundraiser",
        "Gathering",
        "Annual Spring Fundraiser Gala for Minneapolis Parks Foundation — dinner, auction, and live music at the Depot",
    )
    .await;

    assert!(
        queries.queries.is_empty(),
        "Routine fundraiser should NOT generate queries, got: {:?}",
        queries.queries,
    );
}

#[tokio::test]
async fn food_shelf_hours_skipped() {
    let queries = generate_queries(
        "food_shelf_hours",
        "Aid",
        "Open Door Pantry distributing groceries every Tuesday and Thursday 10am-2pm at Faith Lutheran Church",
    )
    .await;

    assert!(
        queries.queries.is_empty(),
        "Food shelf hours listing should NOT generate queries, got: {:?}",
        queries.queries,
    );
}

#[tokio::test]
async fn community_cleanup_skipped() {
    let queries = generate_queries(
        "community_cleanup",
        "Gathering",
        "Neighborhood spring cleanup — meet at Powderhorn Park pavilion Saturday 9am, bags and gloves provided",
    )
    .await;

    assert!(
        queries.queries.is_empty(),
        "Routine community cleanup should NOT generate queries, got: {:?}",
        queries.queries,
    );
}

#[tokio::test]
async fn concert_announcement_skipped() {
    let queries = generate_queries(
        "concert_announcement",
        "Gathering",
        "Free outdoor jazz concert series returns to Loring Park every Friday in June",
    )
    .await;

    assert!(
        queries.queries.is_empty(),
        "Concert announcement should NOT generate queries, got: {:?}",
        queries.queries,
    );
}

#[tokio::test]
async fn yoga_class_skipped() {
    let queries = generate_queries(
        "yoga_class",
        "Gathering",
        "Free community yoga in the park, Saturdays at 8am, all levels welcome",
    )
    .await;

    assert!(
        queries.queries.is_empty(),
        "Routine yoga class should NOT generate queries, got: {:?}",
        queries.queries,
    );
}
