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
use crate::domains::signals::events::SignalEvent;
use crate::core::aggregate::{ExtractedBatch, PipelineState};
use crate::testing::*;

fn test_deps(store: Arc<MockSignalReader>) -> ScoutEngineDeps {
    test_scout_deps(store as Arc<dyn crate::traits::SignalReader>)
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
    let events = super::dedup::deduplicate_extracted_batch(url, "test-key", &batch, state, deps)
        .await
        .unwrap();
    extract_events(events)
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

    // "Free Legal Clinic" filtered out, "Community Dinner" passes as new → SignalCreated
    let created: Vec<_> = signal
        .iter()
        .filter(|e| matches!(e, SignalEvent::SignalCreated { .. }))
        .collect();
    assert_eq!(created.len(), 1, "expected 1 SignalCreated, got {}", created.len());

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

    // Only DedupCompleted (all titles filtered)
    assert_eq!(signal.len(), 1);
    assert!(matches!(&signal[0], SignalEvent::DedupCompleted { .. }));
    assert!(world.is_empty());
    assert!(system.is_empty());
}

// ---------------------------------------------------------------------------
// Layer 2.5: Global title+type match
// ---------------------------------------------------------------------------

#[tokio::test]
async fn global_title_match_same_source_emits_freshness() {
    let store = Arc::new(MockSignalReader::new());
    let existing_id = store.insert_signal(
        "Community Dinner",
        NodeType::Concern,
        "https://example.org/events",
    );
    let deps = test_deps(store);
    let state = PipelineState::new(HashMap::new());

    let (_world, system, signal) = run_dedup(
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

    // FreshnessConfirmed emitted (same-source match)
    assert!(
        system.iter().any(|e| matches!(e, SystemEvent::FreshnessConfirmed { signal_ids, .. } if signal_ids.contains(&existing_id))),
        "expected FreshnessConfirmed for existing signal"
    );

    // DedupCompleted
    assert!(signal.iter().any(|e| matches!(e, SignalEvent::DedupCompleted { .. })));
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

    // DedupCompleted
    assert!(signal.iter().any(|e| matches!(e, SignalEvent::DedupCompleted { .. })));
}

// ---------------------------------------------------------------------------
// Layer 4: No match → create signal
// ---------------------------------------------------------------------------

#[tokio::test]
async fn new_signal_emits_world_facts_and_signal_created() {
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

    // SignalCreated + DedupCompleted
    assert!(signal.iter().any(|e| matches!(e, SignalEvent::SignalCreated { node_id: id, .. } if *id == node_id)));
    assert!(signal.iter().any(|e| matches!(e, SignalEvent::DedupCompleted { .. })));
}

#[tokio::test]
async fn create_carries_tags_and_author_from_extraction_id() {
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

    // SignalCreated emitted (tags and author wiring happens in wire_edges handler)
    assert!(
        signal.iter().any(|e| matches!(e, SignalEvent::SignalCreated { node_id: id, .. } if *id == node_id)),
        "expected SignalCreated for the new signal"
    );
    assert!(signal.iter().any(|e| matches!(e, SignalEvent::DedupCompleted { .. })));
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

    let (world, system, signal) = run_dedup(
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

    // Corroboration for "Existing Event" (cross-source match)
    assert!(
        system.iter().any(|e| matches!(e, SystemEvent::ObservationCorroborated { .. })),
        "expected ObservationCorroborated for 'Existing Event'"
    );

    // Creation for "Brand New Event"
    assert!(
        signal.iter().any(|e| matches!(e, SignalEvent::SignalCreated { .. })),
        "expected SignalCreated for 'Brand New Event'"
    );

    // DedupCompleted
    assert!(signal.iter().any(|e| matches!(e, SignalEvent::DedupCompleted { .. })));
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
    assert!(matches!(&signal[0], SignalEvent::DedupCompleted { .. }));
}
