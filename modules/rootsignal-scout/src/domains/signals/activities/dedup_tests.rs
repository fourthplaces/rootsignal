//! Dedup activity tests — MOCK → FUNCTION → OUTPUT.
//!
//! Call deduplicate_extracted_batch with a batch,
//! assert on the returned DedupBatchResult domain types.

use std::collections::HashMap;
use std::sync::Arc;

use uuid::Uuid;

use rootsignal_common::types::NodeType;

use crate::core::engine::ScoutEngineDeps;
use crate::domains::signals::events::{ActorAction, DedupBatchResult, DedupOutcome};
use crate::core::aggregate::{ExtractedBatch, PipelineState};
use crate::testing::*;

fn test_deps(store: Arc<MockSignalReader>) -> ScoutEngineDeps {
    test_scout_deps(store as Arc<dyn crate::traits::SignalReader>)
}

/// Create a Concern node with a specific URL on its meta (for fingerprint tests).
fn tension_with_url(title: &str, lat: f64, lng: f64, url: &str) -> rootsignal_common::Node {
    let mut node = tension_at(title, lat, lng);
    if let Some(meta) = node.meta_mut() {
        meta.url = url.to_string();
    }
    node
}

/// Helper: call the dedup activity with a batch.
async fn run_dedup(
    url: &str,
    batch: ExtractedBatch,
    state: &PipelineState,
    deps: &ScoutEngineDeps,
) -> DedupBatchResult {
    let ck = rootsignal_common::canonical_value(url);
    super::dedup::deduplicate_extracted_batch(url, &ck, &batch, state, deps)
        .await
        .unwrap()
}

/// Count verdicts of a specific type.
fn count_created(result: &DedupBatchResult) -> usize {
    result.verdicts.iter().filter(|v| matches!(v, DedupOutcome::Created { .. })).count()
}

fn count_corroborated(result: &DedupBatchResult) -> usize {
    result.verdicts.iter().filter(|v| matches!(v, DedupOutcome::Corroborated { .. })).count()
}

fn count_refreshed(result: &DedupBatchResult) -> usize {
    result.verdicts.iter().filter(|v| matches!(v, DedupOutcome::Refreshed { .. })).count()
}

// ---------------------------------------------------------------------------
// Layer 2: URL-based title dedup
// ---------------------------------------------------------------------------

#[tokio::test]
async fn url_title_dedup_filters_existing_titles() {
    let store = Arc::new(MockSignalReader::new());
    store.add_url_titles(
        "https://example.org/events",
        vec!["Free Legal Clinic".to_string()],
    );
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let result = run_dedup(
        "https://example.org/events",
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![
                tension_at("Free Legal Clinic", 44.93, -93.26),
                tension_at("Community Dinner", 44.95, -93.27),
            ],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
        &state,
        &deps,
    )
    .await;

    assert_eq!(count_created(&result), 1, "only 'Community Dinner' should pass");
    assert_eq!(result.created.len(), 1, "one created signal");
}

#[tokio::test]
async fn all_titles_deduped_produces_empty_result() {
    let store = Arc::new(MockSignalReader::new());
    store.add_url_titles(
        "https://example.org/events",
        vec!["Free Legal Clinic".to_string()],
    );
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let result = run_dedup(
        "https://example.org/events",
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![tension_at("Free Legal Clinic", 44.93, -93.26)],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
        &state,
        &deps,
    )
    .await;

    assert!(result.verdicts.is_empty());
    assert!(result.created.is_empty());
    assert!(result.corroborations.is_empty());
}

// ---------------------------------------------------------------------------
// Layer 2.5: Global title+type match
// ---------------------------------------------------------------------------

#[tokio::test]
async fn same_source_title_match_refreshes_without_creating() {
    let store = Arc::new(MockSignalReader::new());
    store.insert_signal(
        "Community Dinner",
        NodeType::Concern,
        "https://example.org/events",
    );
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let result = run_dedup(
        "https://example.org/events",
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![tension_at("Community Dinner", 44.95, -93.27)],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
        &state,
        &deps,
    )
    .await;

    assert!(result.created.is_empty(), "refresh should not create signals");
    assert!(result.corroborations.is_empty(), "refresh should not corroborate");
    assert_eq!(count_refreshed(&result), 1);
}

#[tokio::test]
async fn global_title_match_different_source_corroborates() {
    let store = Arc::new(MockSignalReader::new());
    let existing_id = store.insert_signal(
        "Community Dinner",
        NodeType::Concern,
        "https://other-source.org/events",
    );
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let result = run_dedup(
        "https://example.org/events",
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![tension_at("Community Dinner", 44.95, -93.27)],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
        &state,
        &deps,
    )
    .await;

    assert_eq!(result.corroborations.len(), 1);
    assert_eq!(result.corroborations[0].signal_id, existing_id);
    assert_eq!(result.corroborations[0].citation.signal_id, existing_id);
    assert_eq!(count_corroborated(&result), 1);
}

// ---------------------------------------------------------------------------
// Layer 4: No match → create signal
// ---------------------------------------------------------------------------

#[tokio::test]
async fn new_signal_creates_with_citation_and_verdict() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let node = tension_at("Free Legal Clinic", 44.93, -93.26);
    let node_id = node.id();

    let result = run_dedup(
        "https://example.org/events",
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![node],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
        &state,
        &deps,
    )
    .await;

    assert_eq!(result.created.len(), 1);
    assert_eq!(result.created[0].citation.signal_id, node_id);
    assert_eq!(count_created(&result), 1);
    assert!(result.verdicts.iter().any(|v| matches!(v, DedupOutcome::Created { node_id: id, .. } if *id == node_id)));
}

#[tokio::test]
async fn create_carries_tags_and_source_on_verdict() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let node = tension_at("Food Distribution", 44.95, -93.27);
    let meta_id = node.meta().unwrap().id;
    let node_id = node.id();

    let mut tag_map = HashMap::new();
    tag_map.insert(meta_id, vec!["food".to_string(), "mutual-aid".to_string()]);

    let mut author_actors = HashMap::new();
    author_actors.insert(meta_id, "Northside Mutual Aid".to_string());

    let source_id = Uuid::new_v4();

    let result = run_dedup(
        "https://example.org/events",
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![node],
            resource_tags: HashMap::new(),
            signal_tags: tag_map,
            author_actors,
            source_id: Some(source_id),
        },
        &state,
        &deps,
    )
    .await;

    let created: Vec<_> = result.verdicts.iter()
        .filter(|v| matches!(v, DedupOutcome::Created { node_id: id, .. } if *id == node_id))
        .collect();
    assert_eq!(created.len(), 1);

    match created[0] {
        DedupOutcome::Created { signal_tags, source_id: sid, .. } => {
            assert_eq!(signal_tags.len(), 2);
            assert_eq!(*sid, Some(source_id));
        }
        _ => panic!("expected Created verdict"),
    }
}

// ---------------------------------------------------------------------------
// Mixed batch + edge cases
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mixed_batch_produces_corroboration_and_creation() {
    let store = Arc::new(MockSignalReader::new());
    let _existing_id = store.insert_signal(
        "Existing Event",
        NodeType::Concern,
        "https://other-source.org",
    );
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let result = run_dedup(
        "https://example.org/events",
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![
                tension_at("Existing Event", 44.93, -93.26),
                tension_at("Brand New Event", 44.95, -93.27),
            ],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
        &state,
        &deps,
    )
    .await;

    assert_eq!(count_corroborated(&result), 1);
    assert_eq!(count_created(&result), 1);
    assert_eq!(result.corroborations.len(), 1);
    assert_eq!(result.created.len(), 1);
}

#[tokio::test]
async fn empty_batch_produces_empty_result() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let result = run_dedup(
        "https://example.org/events",
        ExtractedBatch {
            content: "content".to_string(),
            nodes: vec![],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
        &state,
        &deps,
    )
    .await;

    assert!(result.verdicts.is_empty());
    assert!(result.created.is_empty());
    assert!(result.corroborations.is_empty());
}

// ---------------------------------------------------------------------------
// Actor race condition: same author in batch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn same_author_in_batch_creates_one_actor() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let node1 = tension_at("Event A", 44.93, -93.26);
    let node2 = tension_at("Event B", 44.95, -93.27);
    let meta_id1 = node1.meta().unwrap().id;
    let meta_id2 = node2.meta().unwrap().id;

    let mut author_actors = HashMap::new();
    author_actors.insert(meta_id1, "Same Org".to_string());
    author_actors.insert(meta_id2, "Same Org".to_string());

    let result = run_dedup(
        "https://www.instagram.com/same_org",
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![node1, node2],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors,
            source_id: None,
        },
        &state,
        &deps,
    )
    .await;

    let identified_count = result.actor_actions.iter()
        .filter(|a| matches!(a, ActorAction::Identified { .. }))
        .count();
    assert_eq!(identified_count, 1, "expected exactly 1 ActorIdentified, got {identified_count}");

    let linked_count = result.actor_actions.iter()
        .filter(|a| matches!(a, ActorAction::LinkedToSignal { .. }))
        .count();
    assert_eq!(linked_count, 2, "expected 2 LinkedToSignal, got {linked_count}");

    // Both verdicts have the same actor_id
    let created_actors: Vec<_> = result.verdicts.iter()
        .filter_map(|v| match v {
            DedupOutcome::Created { actor, .. } => actor.as_ref().map(|a| a.actor_id),
            _ => None,
        })
        .collect();
    assert_eq!(created_actors.len(), 2);
    assert_eq!(created_actors[0], created_actors[1], "both signals should have the same actor_id");
}

// ---------------------------------------------------------------------------
// Layer 2.75: Fingerprint dedup (url, node_type)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fingerprint_same_source_refreshes() {
    let store = Arc::new(MockSignalReader::new());
    let post_url = "https://www.instagram.com/p/ABC123";
    let source_url = "https://www.instagram.com/local_org";
    let source_ck = rootsignal_common::canonical_value(source_url);
    store.insert_signal_from_source("Community Dinner", NodeType::Concern, post_url, &source_ck);
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let result = run_dedup(
        source_url,
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![tension_with_url("Different Title", 44.95, -93.27, post_url)],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
        &state,
        &deps,
    )
    .await;

    assert!(result.created.is_empty());
    assert!(result.corroborations.is_empty());
    assert_eq!(count_refreshed(&result), 1);
}

#[tokio::test]
async fn fingerprint_different_source_corroborates() {
    let store = Arc::new(MockSignalReader::new());
    let post_url = "https://www.instagram.com/p/ABC123";
    let original_source_ck = rootsignal_common::canonical_value("https://www.instagram.com/original_org");
    let existing_id = store.insert_signal_from_source("Community Dinner", NodeType::Concern, post_url, &original_source_ck);
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let result = run_dedup(
        "https://www.facebook.com/other_org",
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![tension_with_url("Different Title", 44.95, -93.27, post_url)],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
        &state,
        &deps,
    )
    .await;

    assert_eq!(result.corroborations.len(), 1);
    assert_eq!(result.corroborations[0].signal_id, existing_id);
    assert_eq!(result.corroborations[0].citation.signal_id, existing_id);
    assert_eq!(count_corroborated(&result), 1);
}

#[tokio::test]
async fn no_fingerprint_match_falls_through_to_create() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let result = run_dedup(
        "https://www.instagram.com/local_org",
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![tension_with_url("New Event", 44.95, -93.27, "https://www.instagram.com/p/NEWPOST")],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
        &state,
        &deps,
    )
    .await;

    assert_eq!(count_created(&result), 1);
    assert_eq!(result.created.len(), 1);
}

#[tokio::test]
async fn empty_url_signals_skip_fingerprint_layer() {
    let store = Arc::new(MockSignalReader::new());
    store.insert_signal("Community Dinner", NodeType::Concern, "https://www.instagram.com/p/ABC123");
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let result = run_dedup(
        "https://www.instagram.com/local_org",
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![tension_at("Brand New Signal", 44.95, -93.27)],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
        &state,
        &deps,
    )
    .await;

    assert_eq!(count_created(&result), 1, "empty URL should skip fingerprint and create");
}

#[tokio::test]
async fn mixed_batch_fingerprint_hit_and_miss() {
    let store = Arc::new(MockSignalReader::new());
    let known_url = "https://www.instagram.com/p/KNOWN";
    let source_url = "https://www.instagram.com/local_org";
    let source_ck = rootsignal_common::canonical_value(source_url);
    store.insert_signal_from_source("Old Event", NodeType::Concern, known_url, &source_ck);
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let result = run_dedup(
        "https://www.instagram.com/local_org",
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![
                tension_with_url("Re-encountered Event", 44.93, -93.26, known_url),
                tension_with_url("Brand New Event", 44.95, -93.27, "https://www.instagram.com/p/NEW"),
            ],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
        &state,
        &deps,
    )
    .await;

    assert_eq!(count_refreshed(&result), 1, "expected 1 Refreshed from fingerprint hit");
    assert_eq!(count_created(&result), 1, "expected 1 Created for new URL");
    assert!(result.corroborations.is_empty(), "same-source fingerprint should not corroborate");
}

#[tokio::test]
async fn fingerprint_same_url_different_type_does_not_match() {
    let store = Arc::new(MockSignalReader::new());
    let post_url = "https://www.instagram.com/p/ABC123";
    let source_ck = rootsignal_common::canonical_value("https://www.instagram.com/local_org");
    store.insert_signal_from_source("Community Dinner", NodeType::Resource, post_url, &source_ck);
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let result = run_dedup(
        "https://www.instagram.com/local_org",
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![tension_with_url("Community Dinner", 44.95, -93.27, post_url)],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
        &state,
        &deps,
    )
    .await;

    assert_eq!(count_created(&result), 1, "same URL but different type should create, not match");
}

#[tokio::test]
async fn fingerprint_takes_priority_over_vector_dedup() {
    let store = Arc::new(MockSignalReader::new());
    let post_url = "https://www.instagram.com/p/ABC123";
    let source_ck = rootsignal_common::canonical_value("https://www.instagram.com/local_org");
    store.insert_signal_from_source("Community Dinner", NodeType::Concern, post_url, &source_ck);
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let result = run_dedup(
        "https://www.instagram.com/local_org",
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![tension_with_url("Community Dinner", 44.95, -93.27, post_url)],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
        &state,
        &deps,
    )
    .await;

    assert!(result.created.is_empty(), "fingerprint refresh should not create");
    assert!(result.corroborations.is_empty(), "fingerprint refresh should not corroborate");
    assert_eq!(count_refreshed(&result), 1);
}

#[tokio::test]
async fn fingerprint_with_title_dedup_interaction() {
    let store = Arc::new(MockSignalReader::new());
    let post_url = "https://www.instagram.com/p/ABC123";
    let source_url = "https://www.instagram.com/local_org";
    let source_ck = rootsignal_common::canonical_value(source_url);

    store.add_url_titles(source_url, vec!["Community Dinner".to_string()]);
    store.insert_signal_from_source("Community Dinner", NodeType::Concern, post_url, &source_ck);
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let result = run_dedup(
        source_url,
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![tension_with_url("Community Dinner", 44.95, -93.27, post_url)],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
        &state,
        &deps,
    )
    .await;

    // Title dedup (Layer 2) catches it first — no verdicts at all
    assert!(result.verdicts.is_empty(), "title dedup should catch before fingerprint layer");
}

#[tokio::test]
async fn fingerprint_batch_with_duplicate_urls_deduplicates() {
    let store = Arc::new(MockSignalReader::new());
    let post_url = "https://www.instagram.com/p/ABC123";
    let source_url = "https://www.instagram.com/local_org";
    let source_ck = rootsignal_common::canonical_value(source_url);
    store.insert_signal_from_source("Existing Event", NodeType::Concern, post_url, &source_ck);
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let result = run_dedup(
        source_url,
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![
                tension_with_url("Signal A", 44.93, -93.26, post_url),
                tension_with_url("Signal B", 44.95, -93.27, post_url),
            ],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
        &state,
        &deps,
    )
    .await;

    assert_eq!(count_refreshed(&result), 2, "both signals with same URL should refresh");
}
