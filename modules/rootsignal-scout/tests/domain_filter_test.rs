//! Live LLM tests for the domain filter prompt.
//!
//! The domain filter evaluates whether scraped domains are likely to contain
//! current, firsthand community signals for a target region. These tests call
//! the real LLM with the actual prompt and assert correct accept/reject verdicts.
//!
//! **Snapshots:** Each test records the raw LLM response on first run (or when
//! `RECORD=1`). Subsequent runs replay from the snapshot.
//!
//! - Record snapshots:  `RECORD=1 cargo test -p rootsignal-scout --test domain_filter_test`
//! - Replay snapshots:  `cargo test -p rootsignal-scout --test domain_filter_test`

use std::path::{Path, PathBuf};

use rootsignal_scout::domains::enrichment::activities::domain_filter;
use rootsignal_scout::testing::MockSignalReader;

// ---------------------------------------------------------------------------
// Snapshot helpers
// ---------------------------------------------------------------------------

fn snapshots_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("domain_filter")
}

/// Result of a domain filter run — just the accepted URLs.
fn load_snapshot(path: &Path) -> Vec<String> {
    let json = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read snapshot {}: {e}", path.display()));
    serde_json::from_str(&json)
        .unwrap_or_else(|e| panic!("Failed to parse snapshot {}: {e}", path.display()))
}

fn save_snapshot(path: &Path, accepted: &[String]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create snapshot dir");
    }
    let json = serde_json::to_string_pretty(accepted).expect("serialize snapshot");
    std::fs::write(path, json).expect("write snapshot");
}

/// Run the real domain filter with the given URLs and region, using snapshots.
async fn filter_with_snapshot(name: &str, urls: &[&str], region: &str) -> Vec<String> {
    let snap_path = snapshots_dir().join(format!("{name}.json"));

    if snap_path.exists() && std::env::var("RECORD").is_err() {
        return load_snapshot(&snap_path);
    }

    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .expect("ANTHROPIC_API_KEY required to record domain filter snapshots");

    let claude = ai_client::claude::Claude::new(&api_key, "claude-haiku-4-5-20251001");
    let store = MockSignalReader::new();

    let url_strings: Vec<String> = urls.iter().map(|s| s.to_string()).collect();
    let accepted = domain_filter::filter_domains_batch(&url_strings, region, &claude, &store).await;

    save_snapshot(&snap_path, &accepted);
    accepted
}

// ===========================================================================
// Federal/national government domains should be REJECTED for local runs
// ===========================================================================

/// Library of Congress blog has no local community signals for Minneapolis.
/// This was the actual bug: blogs.loc.gov was accepted during a Minneapolis run,
/// polluting the source list with Laos War oral history content.
#[tokio::test]
async fn federal_gov_blog_rejected_for_local_run() {
    let urls = &[
        "https://blogs.loc.gov/international-collections/2026/02/unveiling-secret-war-laos/",
    ];
    let accepted = filter_with_snapshot("federal_gov_blog_loc", urls, "Minneapolis").await;

    assert!(
        accepted.is_empty(),
        "blogs.loc.gov should be REJECTED for a Minneapolis run — \
         federal government blog with no local community signals. Accepted: {accepted:?}"
    );
}

/// Smithsonian blogs are national institutional content, not local signals.
#[tokio::test]
async fn national_institution_rejected_for_local_run() {
    let urls = &["https://www.si.edu/newsdesk/releases/new-exhibition-opens"];
    let accepted = filter_with_snapshot("national_institution_si", urls, "Minneapolis").await;

    assert!(
        accepted.is_empty(),
        "si.edu should be REJECTED for a Minneapolis run — \
         national institution with no local desk. Accepted: {accepted:?}"
    );
}

// ===========================================================================
// Local sources should be ACCEPTED
// ===========================================================================

/// Local government site should always be accepted.
#[tokio::test]
async fn local_government_accepted() {
    let urls = &["https://www.minneapolismn.gov/government/city-council/meetings/"];
    let accepted = filter_with_snapshot("local_gov_minneapolis", urls, "Minneapolis").await;

    assert!(
        !accepted.is_empty(),
        "minneapolismn.gov should be ACCEPTED — local government for the target region"
    );
}

/// Local journalism should be accepted.
#[tokio::test]
async fn local_journalism_accepted() {
    let urls = &["https://www.minnpost.com/metro/2026/03/local-story/"];
    let accepted = filter_with_snapshot("local_journalism_minnpost", urls, "Minneapolis").await;

    assert!(
        !accepted.is_empty(),
        "minnpost.com should be ACCEPTED — local journalism for Minneapolis"
    );
}

/// Community nonprofit should be accepted.
#[tokio::test]
async fn local_nonprofit_accepted() {
    let urls = &["https://www.pillsburyunited.org/programs"];
    let accepted = filter_with_snapshot("local_nonprofit_pillsbury", urls, "Minneapolis").await;

    assert!(
        !accepted.is_empty(),
        "pillsburyunited.org should be ACCEPTED — local community nonprofit"
    );
}

// ===========================================================================
// Mixed batch — mirrors real-world link promotion batches
// ===========================================================================

/// A realistic batch with local and irrelevant domains mixed together.
/// The filter should accept local sources and reject federal/national ones.
#[tokio::test]
async fn mixed_batch_filters_correctly() {
    let urls = &[
        "https://www.minneapolismn.gov/government/city-council/",
        "https://blogs.loc.gov/international-collections/2026/02/some-article/",
        "https://www.minnpost.com/metro/2026/03/local-story/",
        "https://www.si.edu/exhibitions/current",
        "https://www.pillsburyunited.org/events",
    ];
    let accepted = filter_with_snapshot("mixed_batch", urls, "Minneapolis").await;

    // Local sources should be accepted
    let has_mpls_gov = accepted.iter().any(|u| u.contains("minneapolismn.gov"));
    let has_minnpost = accepted.iter().any(|u| u.contains("minnpost.com"));
    let has_pillsbury = accepted.iter().any(|u| u.contains("pillsburyunited.org"));

    assert!(has_mpls_gov, "minneapolismn.gov should be accepted in mixed batch");
    assert!(has_minnpost, "minnpost.com should be accepted in mixed batch");
    assert!(has_pillsbury, "pillsburyunited.org should be accepted in mixed batch");

    // Federal/national sources should be rejected
    let has_loc = accepted.iter().any(|u| u.contains("blogs.loc.gov"));
    let has_si = accepted.iter().any(|u| u.contains("si.edu"));

    assert!(!has_loc, "blogs.loc.gov should be REJECTED in mixed batch. Accepted: {accepted:?}");
    assert!(!has_si, "si.edu should be REJECTED in mixed batch. Accepted: {accepted:?}");
}
