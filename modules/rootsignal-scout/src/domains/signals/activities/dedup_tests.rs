//! Dedup handler tests — MOCK → FUNCTION → OUTPUT.
//!
//! Call deduplicate_extracted_batch with a batch,
//! assert which fact events came out (World, System, Signal).

use std::collections::HashMap;
use std::sync::Arc;

use uuid::Uuid;

use rootsignal_common::events::{SystemEvent, WorldEvent};
use rootsignal_common::types::NodeType;
use seesaw_core::Events;

use crate::core::engine::ScoutEngineDeps;
use crate::domains::signals::events::{DedupOutcome, SignalEvent};
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

/// Extract typed events from a heterogeneous Events collection.
fn extract_events(events: Events) -> (Vec<WorldEvent>, Vec<SystemEvent>, Vec<SignalEvent>) {
    let mut world = Vec::new();
    let mut system = Vec::new();
    let mut signal = Vec::new();
    for output in events.into_outputs() {
        if output.type_id == std::any::TypeId::of::<WorldEvent>() {
            if let Ok(e) = serde_json::from_value::<WorldEvent>(output.payload) {
                world.push(e);
            }
        } else if output.type_id == std::any::TypeId::of::<SystemEvent>() {
            if let Ok(e) = serde_json::from_value::<SystemEvent>(output.payload) {
                system.push(e);
            }
        } else if output.type_id == std::any::TypeId::of::<SignalEvent>() {
            if let Ok(e) = serde_json::from_value::<SignalEvent>(output.payload) {
                signal.push(e);
            }
        }
    }
    (world, system, signal)
}

/// Helper: call the dedup handler with a batch.
async fn run_dedup(
    url: &str,
    batch: ExtractedBatch,
    state: &PipelineState,
    deps: &ScoutEngineDeps,
) -> (Vec<WorldEvent>, Vec<SystemEvent>, Vec<SignalEvent>) {
    let ck = rootsignal_common::canonical_value(url);
    let events = super::dedup::deduplicate_extracted_batch(url, &ck, &batch, state, deps)
        .await
        .unwrap();
    extract_events(events)
}

/// Extract verdicts from signal events.
fn verdicts(signal: &[SignalEvent]) -> Vec<&DedupOutcome> {
    signal
        .iter()
        .filter_map(|e| match e {
            SignalEvent::DedupCompleted { verdicts, .. } => Some(verdicts.iter()),
            SignalEvent::NoNewSignals => None,
        })
        .flatten()
        .collect()
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

    let (world, _system, signal) = run_dedup(
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

    // "Free Legal Clinic" filtered out, "Community Dinner" passes as new → Created verdict
    let created: Vec<_> = verdicts(&signal)
        .into_iter()
        .filter(|v| matches!(v, DedupOutcome::Created { .. }))
        .collect();
    assert_eq!(created.len(), 1, "expected 1 Created verdict, got {}", created.len());

    // World event for the new signal
    assert!(
        world.iter().any(|e| matches!(e, WorldEvent::ConcernRaised { .. })),
        "expected ConcernRaised for 'Community Dinner'"
    );
}

#[tokio::test]
async fn all_titles_deduped_emits_only_dedup_completed() {
    let store = Arc::new(MockSignalReader::new());
    store.add_url_titles(
        "https://example.org/events",
        vec!["Free Legal Clinic".to_string()],
    );
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let (world, system, signal) = run_dedup(
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

    // Only DedupCompleted with empty verdicts (all titles filtered)
    assert_eq!(signal.len(), 1);
    assert!(matches!(&signal[0], SignalEvent::DedupCompleted { verdicts, .. } if verdicts.is_empty()));
    assert!(world.is_empty());
    assert!(system.is_empty());
}

// ---------------------------------------------------------------------------
// Layer 2.5: Global title+type match
// ---------------------------------------------------------------------------

#[tokio::test]
async fn same_source_title_match_emits_no_events() {
    let store = Arc::new(MockSignalReader::new());
    store.insert_signal(
        "Community Dinner",
        NodeType::Concern,
        "https://example.org/events",
    );
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let (world, system, signal) = run_dedup(
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

    assert!(world.is_empty(), "refresh should emit no world events");
    assert!(system.is_empty(), "refresh should emit no system events");

    let refreshed: Vec<_> = verdicts(&signal)
        .into_iter()
        .filter(|v| matches!(v, DedupOutcome::Refreshed { .. }))
        .collect();
    assert_eq!(refreshed.len(), 1, "expected 1 Refreshed verdict");
}

#[tokio::test]
async fn global_title_match_different_source_emits_corroboration() {
    let store = Arc::new(MockSignalReader::new());
    let existing_id = store.insert_signal(
        "Community Dinner",
        NodeType::Concern,
        "https://other-source.org/events",
    );
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let (world, system, signal) = run_dedup(
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

    // CitationPublished (WorldEvent)
    assert!(
        world.iter().any(|e| matches!(e, WorldEvent::CitationPublished { signal_id, .. } if *signal_id == existing_id)),
        "expected CitationPublished for existing signal"
    );

    // ObservationCorroborated (SystemEvent)
    assert!(
        system.iter().any(|e| matches!(e, SystemEvent::ObservationCorroborated { signal_id, .. } if *signal_id == existing_id)),
        "expected ObservationCorroborated"
    );

    // Corroborated verdict
    let corroborated: Vec<_> = verdicts(&signal)
        .into_iter()
        .filter(|v| matches!(v, DedupOutcome::Corroborated { .. }))
        .collect();
    assert_eq!(corroborated.len(), 1, "expected 1 Corroborated verdict");
}

// ---------------------------------------------------------------------------
// Layer 4: No match → create signal
// ---------------------------------------------------------------------------

#[tokio::test]
async fn new_signal_emits_world_facts_and_created_verdict() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let node = tension_at("Free Legal Clinic", 44.93, -93.26);
    let node_id = node.id();

    let (world, system, signal) = run_dedup(
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

    // World: at least discovery + CitationPublished
    assert!(world.len() >= 2, "expected at least 2 World events, got {}", world.len());
    assert!(world.iter().any(|e| matches!(e, WorldEvent::CitationPublished { .. })));

    // System: at least sensitivity classification
    assert!(!system.is_empty(), "expected at least one System event");

    // Created verdict with correct node_id
    let created: Vec<_> = verdicts(&signal)
        .into_iter()
        .filter(|v| matches!(v, DedupOutcome::Created { node_id: id, .. } if *id == node_id))
        .collect();
    assert_eq!(created.len(), 1, "expected 1 Created verdict for node_id");

    // DedupCompleted present
    assert!(signal.iter().any(|e| matches!(e, SignalEvent::DedupCompleted { .. })));
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

    let (_world, _system, signal) = run_dedup(
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

    // Created verdict carries tags and source_id
    let created: Vec<_> = verdicts(&signal)
        .into_iter()
        .filter(|v| matches!(v, DedupOutcome::Created { node_id: id, .. } if *id == node_id))
        .collect();
    assert_eq!(created.len(), 1, "expected Created verdict");

    match created[0] {
        DedupOutcome::Created { signal_tags, source_id: sid, .. } => {
            assert_eq!(signal_tags.len(), 2, "expected 2 tags");
            assert_eq!(*sid, Some(source_id));
        }
        _ => panic!("expected Created verdict"),
    }
}

// ---------------------------------------------------------------------------
// Mixed batch + edge cases
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mixed_batch_emits_correct_verdicts() {
    let store = Arc::new(MockSignalReader::new());
    let _existing_id = store.insert_signal(
        "Existing Event",
        NodeType::Concern,
        "https://other-source.org",
    );
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let (_world, system, signal) = run_dedup(
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

    // Corroboration for "Existing Event"
    assert!(
        system.iter().any(|e| matches!(e, SystemEvent::ObservationCorroborated { .. })),
        "expected ObservationCorroborated for 'Existing Event'"
    );

    let vs = verdicts(&signal);
    assert!(
        vs.iter().any(|v| matches!(v, DedupOutcome::Corroborated { .. })),
        "expected Corroborated verdict"
    );
    assert!(
        vs.iter().any(|v| matches!(v, DedupOutcome::Created { .. })),
        "expected Created verdict for 'Brand New Event'"
    );
}

#[tokio::test]
async fn empty_batch_emits_only_dedup_completed() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let (_world, _system, signal) = run_dedup(
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

    assert_eq!(signal.len(), 1);
    assert!(matches!(&signal[0], SignalEvent::DedupCompleted { verdicts, .. } if verdicts.is_empty()));
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

    let (_world, system, signal) = run_dedup(
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

    // Only one ActorIdentified — second signal reuses cached actor
    let actor_identified_count = system
        .iter()
        .filter(|e| matches!(e, SystemEvent::ActorIdentified { .. }))
        .count();
    assert_eq!(actor_identified_count, 1, "expected exactly 1 ActorIdentified, got {actor_identified_count}");

    // But both signals linked to the actor
    let actor_linked_count = system
        .iter()
        .filter(|e| matches!(e, SystemEvent::ActorLinkedToSignal { .. }))
        .count();
    assert_eq!(actor_linked_count, 2, "expected 2 ActorLinkedToSignal, got {actor_linked_count}");

    // Both verdicts have the same actor_id
    let created: Vec<_> = verdicts(&signal)
        .into_iter()
        .filter_map(|v| match v {
            DedupOutcome::Created { actor, .. } => actor.as_ref().map(|a| a.actor_id),
            _ => None,
        })
        .collect();
    assert_eq!(created.len(), 2);
    assert_eq!(created[0], created[1], "both signals should have the same actor_id");
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

    let (world, system, signal) = run_dedup(
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

    assert!(world.is_empty(), "same-source fingerprint should emit no world events");
    assert!(system.is_empty(), "same-source fingerprint should emit no system events");

    let refreshed: Vec<_> = verdicts(&signal)
        .into_iter()
        .filter(|v| matches!(v, DedupOutcome::Refreshed { .. }))
        .collect();
    assert_eq!(refreshed.len(), 1, "expected 1 Refreshed verdict from fingerprint");
}

#[tokio::test]
async fn fingerprint_different_source_corroborates() {
    let store = Arc::new(MockSignalReader::new());
    let post_url = "https://www.instagram.com/p/ABC123";
    let original_source_ck = rootsignal_common::canonical_value("https://www.instagram.com/original_org");
    let existing_id = store.insert_signal_from_source("Community Dinner", NodeType::Concern, post_url, &original_source_ck);
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    // Scrape from a different source that found the same post URL
    let (world, system, signal) = run_dedup(
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

    assert!(
        world.iter().any(|e| matches!(e, WorldEvent::CitationPublished { signal_id, .. } if *signal_id == existing_id)),
        "expected CitationPublished for existing signal"
    );
    assert!(
        system.iter().any(|e| matches!(e, SystemEvent::ObservationCorroborated { signal_id, .. } if *signal_id == existing_id)),
        "expected ObservationCorroborated"
    );

    let corroborated: Vec<_> = verdicts(&signal)
        .into_iter()
        .filter(|v| matches!(v, DedupOutcome::Corroborated { .. }))
        .collect();
    assert_eq!(corroborated.len(), 1, "expected 1 Corroborated verdict from fingerprint");
}

#[tokio::test]
async fn no_fingerprint_match_falls_through_to_create() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let (world, _system, signal) = run_dedup(
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

    let created: Vec<_> = verdicts(&signal)
        .into_iter()
        .filter(|v| matches!(v, DedupOutcome::Created { .. }))
        .collect();
    assert_eq!(created.len(), 1, "expected 1 Created verdict");
    assert!(
        world.iter().any(|e| matches!(e, WorldEvent::ConcernRaised { .. })),
        "expected ConcernRaised for new signal"
    );
}

#[tokio::test]
async fn empty_url_signals_skip_fingerprint_layer() {
    let store = Arc::new(MockSignalReader::new());
    // Insert a signal — but the batch node has empty URL so fingerprint won't match
    store.insert_signal("Community Dinner", NodeType::Concern, "https://www.instagram.com/p/ABC123");
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let (_world, _system, signal) = run_dedup(
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

    // Empty URL means fingerprint layer is skipped; signal proceeds to create
    let created: Vec<_> = verdicts(&signal)
        .into_iter()
        .filter(|v| matches!(v, DedupOutcome::Created { .. }))
        .collect();
    assert_eq!(created.len(), 1, "empty URL should skip fingerprint and create");
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

    let (_world, system, signal) = run_dedup(
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

    let vs = verdicts(&signal);
    let refreshed = vs.iter().filter(|v| matches!(v, DedupOutcome::Refreshed { .. })).count();
    let created = vs.iter().filter(|v| matches!(v, DedupOutcome::Created { .. })).count();
    assert_eq!(refreshed, 1, "expected 1 Refreshed from fingerprint hit");
    assert_eq!(created, 1, "expected 1 Created for new URL");
    assert!(system.is_empty() || !system.iter().any(|e| matches!(e, SystemEvent::ObservationCorroborated { .. })),
        "same-source fingerprint should not corroborate");
}

#[tokio::test]
async fn fingerprint_same_url_different_type_does_not_match() {
    let store = Arc::new(MockSignalReader::new());
    let post_url = "https://www.instagram.com/p/ABC123";
    let source_ck = rootsignal_common::canonical_value("https://www.instagram.com/local_org");
    store.insert_signal_from_source("Community Dinner", NodeType::Resource, post_url, &source_ck);
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    // Same URL but different node type — fingerprint should not match
    let (_world, _system, signal) = run_dedup(
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

    let created: Vec<_> = verdicts(&signal)
        .into_iter()
        .filter(|v| matches!(v, DedupOutcome::Created { .. }))
        .collect();
    assert_eq!(created.len(), 1, "same URL but different type should create, not match");
}

#[tokio::test]
async fn fingerprint_takes_priority_over_vector_dedup() {
    let store = Arc::new(MockSignalReader::new());
    let post_url = "https://www.instagram.com/p/ABC123";
    let source_ck = rootsignal_common::canonical_value("https://www.instagram.com/local_org");
    let existing_id = store.insert_signal_from_source("Community Dinner", NodeType::Concern, post_url, &source_ck);
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    // Same URL, same source — fingerprint catches it before embeddings are computed
    let (world, system, signal) = run_dedup(
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

    // Should be Refreshed (not Created — vector dedup never runs)
    assert!(world.is_empty(), "fingerprint refresh should emit no world events");
    assert!(system.is_empty(), "fingerprint refresh should emit no system events");

    let refreshed: Vec<_> = verdicts(&signal)
        .into_iter()
        .filter(|v| matches!(v, DedupOutcome::Refreshed { existing_id: id, .. } if *id == existing_id))
        .collect();
    assert_eq!(refreshed.len(), 1, "fingerprint should catch before vector dedup");
}

#[tokio::test]
async fn fingerprint_with_title_dedup_interaction() {
    let store = Arc::new(MockSignalReader::new());
    let post_url = "https://www.instagram.com/p/ABC123";
    let source_url = "https://www.instagram.com/local_org";
    let source_ck = rootsignal_common::canonical_value(source_url);

    // Signal exists in both URL-title index AND fingerprint store
    store.add_url_titles(source_url, vec!["Community Dinner".to_string()]);
    store.insert_signal_from_source("Community Dinner", NodeType::Concern, post_url, &source_ck);
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let (_world, _system, signal) = run_dedup(
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
    assert_eq!(signal.len(), 1);
    assert!(matches!(&signal[0], SignalEvent::DedupCompleted { verdicts, .. } if verdicts.is_empty()),
        "title dedup should catch before fingerprint layer");
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

    // Two different signals in batch that share the same post URL
    let (_world, _system, signal) = run_dedup(
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

    // Both should be caught by fingerprint as same-source refreshes
    let refreshed = verdicts(&signal)
        .into_iter()
        .filter(|v| matches!(v, DedupOutcome::Refreshed { .. }))
        .count();
    assert_eq!(refreshed, 2, "both signals with same URL should refresh");
}
