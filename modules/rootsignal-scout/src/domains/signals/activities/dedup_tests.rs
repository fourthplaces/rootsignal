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

fn count_refreshed(result: &DedupBatchResult) -> usize {
    result.verdicts.iter().filter(|v| matches!(v, DedupOutcome::Refreshed { .. })).count()
}

// ---------------------------------------------------------------------------
// No match → create signal
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
// Edge cases
// ---------------------------------------------------------------------------

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
}

#[tokio::test]
async fn citation_nodes_in_batch_are_skipped() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let citation = rootsignal_common::Node::Citation(rootsignal_common::CitationNode {
        id: Uuid::new_v4(),
        source_url: "https://example.com".to_string(),
        retrieved_at: chrono::Utc::now(),
        content_hash: "abc".to_string(),
        snippet: None,
        relevance: None,
        confidence: None,
        channel_type: None,
    });

    let result = run_dedup(
        "https://example.org/events",
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![
                citation,
                tension_at("Real Signal", 44.95, -93.27),
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

    assert_eq!(count_created(&result), 1);
    assert_eq!(result.created[0].node.title(), "Real Signal");
}

#[tokio::test]
async fn signal_without_url_falls_back_to_batch_url_for_matching() {
    let batch_url = "https://example.org/events";
    let store = Arc::new(MockSignalReader::new());
    store.insert_signal("Old Signal", NodeType::Concern, batch_url);
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let result = run_dedup(
        batch_url,
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![tension_at("Different Title", 44.95, -93.27)],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
        &state,
        &deps,
    )
    .await;

    assert_eq!(count_refreshed(&result), 1, "node with no URL should match via batch URL");
    assert!(result.created.is_empty());
}

#[tokio::test]
async fn all_signals_matching_produces_no_creates() {
    let store = Arc::new(MockSignalReader::new());
    let source_url = "https://www.instagram.com/local_org";
    let source_ck = rootsignal_common::canonical_value(source_url);
    store.insert_signal_from_source("A", NodeType::Concern, "https://www.instagram.com/p/AAA", &source_ck);
    store.insert_signal_from_source("B", NodeType::Concern, "https://www.instagram.com/p/BBB", &source_ck);
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let result = run_dedup(
        source_url,
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![
                tension_with_url("A updated", 44.93, -93.26, "https://www.instagram.com/p/AAA"),
                tension_with_url("B updated", 44.95, -93.27, "https://www.instagram.com/p/BBB"),
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

    assert_eq!(count_refreshed(&result), 2);
    assert!(result.created.is_empty(), "no new signals should be created");
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

/// Two signals from different post URLs under the same Instagram profile
/// should resolve to one actor, not one per post.
#[tokio::test]
async fn same_author_across_post_urls_creates_one_actor() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let profile_url = "https://www.instagram.com/local_org";

    let node1 = tension_with_url("Event A", 44.93, -93.26, "https://www.instagram.com/p/POST111");
    let node2 = tension_with_url("Event B", 44.95, -93.27, "https://www.instagram.com/p/POST222");
    let meta_id1 = node1.meta().unwrap().id;
    let meta_id2 = node2.meta().unwrap().id;

    let mut author_actors = HashMap::new();
    author_actors.insert(meta_id1, "Local Org".to_string());
    author_actors.insert(meta_id2, "Local Org".to_string());

    let result = run_dedup(
        profile_url,
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
    assert_eq!(identified_count, 1, "expected 1 ActorIdentified, got {identified_count}");

    let linked_count = result.actor_actions.iter()
        .filter(|a| matches!(a, ActorAction::LinkedToSignal { .. }))
        .count();
    assert_eq!(linked_count, 2, "expected 2 LinkedToSignal, got {linked_count}");

    // Both signals should reference the same actor
    let actor_ids: Vec<_> = result.actor_actions.iter()
        .filter_map(|a| match a {
            ActorAction::Identified { actor_id, .. } => Some(*actor_id),
            _ => None,
        })
        .collect();
    let linked_ids: Vec<_> = result.actor_actions.iter()
        .filter_map(|a| match a {
            ActorAction::LinkedToSignal { actor_id, .. } => Some(*actor_id),
            _ => None,
        })
        .collect();
    assert_eq!(linked_ids.len(), 2);
    assert_eq!(linked_ids[0], actor_ids[0], "first signal should link to the identified actor");
    assert_eq!(linked_ids[1], actor_ids[0], "second signal should link to the identified actor");
}

// ---------------------------------------------------------------------------
// Fingerprint dedup (url, node_type)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reencountered_signal_refreshes() {
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
    assert_eq!(count_refreshed(&result), 1);
}

#[tokio::test]
async fn same_url_from_different_source_still_refreshes() {
    let store = Arc::new(MockSignalReader::new());
    let post_url = "https://www.instagram.com/p/ABC123";
    let original_source_ck = rootsignal_common::canonical_value("https://www.instagram.com/original_org");
    store.insert_signal_from_source("Community Dinner", NodeType::Concern, post_url, &original_source_ck);
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

    assert!(result.created.is_empty());
    assert_eq!(count_refreshed(&result), 1, "same (url, type) from any source is a refresh");
}

#[tokio::test]
async fn no_fingerprint_match_creates_signal() {
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
async fn signal_without_url_uses_batch_url_for_fingerprint() {
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

    assert_eq!(count_created(&result), 1, "no fingerprint match on batch URL should create");
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
}

#[tokio::test]
async fn same_url_different_type_creates_new_signal() {
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
async fn duplicate_urls_in_batch_both_refresh() {
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

// ---------------------------------------------------------------------------
// Actor-source linking: owned sources create actors linked to sources
// ---------------------------------------------------------------------------

#[tokio::test]
async fn owned_source_actor_links_to_source() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let node = tension_at("Volunteer Call", 44.95, -93.27);
    let meta_id = node.meta().unwrap().id;

    let mut author_actors = HashMap::new();
    author_actors.insert(meta_id, "Sanctuary Supply Depot".to_string());

    let source_id = Uuid::new_v4();

    let result = run_dedup(
        "https://www.instagram.com/sanctuarysupplydepot",
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![node],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors,
            source_id: Some(source_id),
        },
        &state,
        &deps,
    )
    .await;

    let linked_to_source: Vec<_> = result.actor_actions.iter()
        .filter(|a| matches!(a, ActorAction::LinkedToSource { .. }))
        .collect();
    assert_eq!(linked_to_source.len(), 1, "actor should be linked to its source");

    match &linked_to_source[0] {
        ActorAction::LinkedToSource { source_id: sid, .. } => {
            assert_eq!(*sid, source_id, "should link to the batch source_id");
        }
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn web_page_source_creates_no_actor() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let node = tension_at("Community Meeting", 44.95, -93.27);
    let meta_id = node.meta().unwrap().id;

    let mut author_actors = HashMap::new();
    author_actors.insert(meta_id, "City of Minneapolis".to_string());

    let result = run_dedup(
        "https://www.minneapolismn.gov/events",
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![node],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors,
            source_id: None,
        },
        &state,
        &deps,
    )
    .await;

    assert!(
        result.actor_actions.is_empty(),
        "web page sources should not create actors (got {} actions)",
        result.actor_actions.len(),
    );
}

#[tokio::test]
async fn owned_source_without_source_id_creates_actor_but_no_source_link() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let node = tension_at("Event A", 44.95, -93.27);
    let meta_id = node.meta().unwrap().id;

    let mut author_actors = HashMap::new();
    author_actors.insert(meta_id, "Some Org".to_string());

    let result = run_dedup(
        "https://www.instagram.com/some_org",
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![node],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors,
            source_id: None, // no source_id
        },
        &state,
        &deps,
    )
    .await;

    let identified = result.actor_actions.iter()
        .filter(|a| matches!(a, ActorAction::Identified { .. }))
        .count();
    let linked_to_signal = result.actor_actions.iter()
        .filter(|a| matches!(a, ActorAction::LinkedToSignal { .. }))
        .count();
    let linked_to_source = result.actor_actions.iter()
        .filter(|a| matches!(a, ActorAction::LinkedToSource { .. }))
        .count();

    assert_eq!(identified, 1, "actor should still be created");
    assert_eq!(linked_to_signal, 1, "actor should still link to signal");
    assert_eq!(linked_to_source, 0, "no source_id means no LinkedToSource");
}

#[tokio::test]
async fn actor_canonical_key_derives_from_source_url_not_name() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let node = tension_at("Weekly Meetup", 44.95, -93.27);
    let meta_id = node.meta().unwrap().id;

    let mut author_actors = HashMap::new();
    author_actors.insert(meta_id, "Friends of the Falls".to_string());

    let source_url = "https://www.instagram.com/friendsofthefalls";

    let result = run_dedup(
        source_url,
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![node],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors,
            source_id: None,
        },
        &state,
        &deps,
    )
    .await;

    let identified: Vec<_> = result.actor_actions.iter()
        .filter_map(|a| match a {
            ActorAction::Identified { canonical_key, .. } => Some(canonical_key.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(identified.len(), 1);
    let expected_ck = rootsignal_common::canonical_value(source_url);
    assert_eq!(
        identified[0], expected_ck,
        "canonical_key should derive from source URL, not from author name"
    );
    // Name-derived key would be "friends-of-the-falls" (lowercased, spaces→dashes).
    // Source-derived key is "instagram.com/friendsofthefalls".
    assert_ne!(
        identified[0], "friends-of-the-falls",
        "canonical_key must not be name-derived"
    );
}

// ---------------------------------------------------------------------------
// Actor idempotency: second scrape reuses existing actor
// ---------------------------------------------------------------------------

#[tokio::test]
async fn second_scrape_reuses_existing_actor_by_canonical_key() {
    let source_url = "https://www.instagram.com/sanctuarysupplydepot";
    let existing_actor_id = Uuid::new_v4();
    let expected_ck = rootsignal_common::canonical_value(source_url);

    let store = Arc::new(MockSignalReader::new());
    store.add_actor_by_canonical_key(&expected_ck, existing_actor_id);
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let node = tension_at("Volunteer Call", 44.95, -93.27);
    let meta_id = node.meta().unwrap().id;

    let mut author_actors = HashMap::new();
    author_actors.insert(meta_id, "Sanctuary Supply Depot".to_string());

    let result = run_dedup(
        source_url,
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![node],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors,
            source_id: Some(Uuid::new_v4()),
        },
        &state,
        &deps,
    )
    .await;

    let identified_count = result.actor_actions.iter()
        .filter(|a| matches!(a, ActorAction::Identified { .. }))
        .count();
    assert_eq!(identified_count, 0, "should not create a new actor when one already exists");

    let linked: Vec<_> = result.actor_actions.iter()
        .filter_map(|a| match a {
            ActorAction::LinkedToSignal { actor_id, .. } => Some(*actor_id),
            _ => None,
        })
        .collect();
    assert_eq!(linked.len(), 1, "should link signal to existing actor");
    assert_eq!(linked[0], existing_actor_id, "should reuse the existing actor_id");
}

// ---------------------------------------------------------------------------
// Per-signal URL: citations and verdicts use node URL, not batch URL
// ---------------------------------------------------------------------------

#[tokio::test]
async fn created_citation_uses_per_signal_url_not_batch_url() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let account_url = "https://instagram.com/sanctuarysupplydepot";
    let post_url = "https://instagram.com/p/POST123";
    let node = tension_with_url("Volunteer Call", 44.95, -93.27, post_url);
    let node_id = node.id();

    let result = run_dedup(
        account_url,
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
    assert_eq!(
        result.created[0].citation.url, post_url,
        "citation should use the per-signal post URL, not the batch account URL"
    );

    let created_verdict = result.verdicts.iter().find(|v| matches!(v, DedupOutcome::Created { node_id: id, .. } if *id == node_id));
    match created_verdict {
        Some(DedupOutcome::Created { url, .. }) => {
            assert_eq!(url, post_url, "verdict should use the per-signal post URL, not the batch account URL");
        }
        _ => panic!("expected a Created verdict"),
    }
}
