//! Handler tests — MOCK → FUNCTION → OUTPUT.
//!
//! Set up mocks, call the real handler, assert what events came out.

use std::collections::HashMap;
use std::sync::Arc;

use rootsignal_common::events::{SystemEvent, WorldEvent};
use seesaw_core::Events;
use uuid::Uuid;

use crate::core::engine::ScoutEngineDeps;
use crate::domains::signals::events::SignalEvent;
use crate::pipeline::state::{PendingNode, PipelineState, WiringContext};
use crate::testing::*;

/// Build test ScoutEngineDeps with a mock store.
fn test_deps(store: Arc<MockSignalReader>) -> ScoutEngineDeps {
    test_scout_deps(store as Arc<dyn crate::traits::SignalReader>)
}

/// Extract typed events from a heterogeneous Events collection.
fn extract_events(events: Events) -> (Vec<WorldEvent>, Vec<SystemEvent>, Vec<SignalEvent>) {
    let mut world = Vec::new();
    let mut system = Vec::new();
    let mut signal = Vec::new();
    for output in events.into_outputs() {
        if let Some(e) = output.value.downcast_ref::<WorldEvent>() {
            world.push(e.clone());
        } else if let Some(e) = output.value.downcast_ref::<SystemEvent>() {
            system.push(e.clone());
        } else if let Some(e) = output.value.downcast_ref::<SignalEvent>() {
            signal.push(e.clone());
        }
    }
    (world, system, signal)
}

// ---------------------------------------------------------------------------
// handle_create
// ---------------------------------------------------------------------------

#[tokio::test]
async fn new_signal_emits_world_system_citation_and_signal_stored() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);

    let node = tension_at("Free Legal Clinic", 44.9341, -93.2619);
    let node_id = node.id();

    let mut state = PipelineState::new(HashMap::new());
    state.pending_nodes.insert(
        node_id,
        PendingNode {
            node,
            embedding: vec![0.1; TEST_EMBEDDING_DIM],
            content_hash: "abc123".to_string(),
            resource_tags: vec![],
            signal_tags: vec!["legal".to_string()],
            author_name: Some("Local Legal Aid".to_string()),
            source_id: None,
        },
    );

    let events =
        super::creation::handle_create(node_id, "https://localorg.org/events", &state, &deps)
            .await
            .unwrap();

    let (world, system, signal) = extract_events(events);

    // World: discovery + CitationPublished (at minimum)
    assert!(world.len() >= 2, "expected at least 2 World events, got {}", world.len());

    // CitationPublished present
    assert!(
        world.iter().any(|e| matches!(e, WorldEvent::CitationPublished { .. })),
        "expected CitationPublished event"
    );

    // At least one System event (sensitivity classification)
    assert!(!system.is_empty(), "expected at least one System event");

    // SignalCreated triggers edge wiring
    assert_eq!(signal.len(), 1, "expected one SignalEvent (SignalCreated)");
    match &signal[0] {
        SignalEvent::SignalCreated { node_id: stored_id, .. } => {
            assert_eq!(*stored_id, node_id);
        }
        other => panic!("expected SignalCreated, got {:?}", other),
    }

    // PendingNode still in state (handler reads, reducer cleans up on SignalCreated)
    assert!(
        state.pending_nodes.contains_key(&node_id),
        "pending node should still be in state (handler reads, reducer cleans up)"
    );
}

#[tokio::test]
async fn missing_pending_node_returns_empty_events() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);

    let state = PipelineState::new(HashMap::new());
    let bogus_id = Uuid::new_v4();

    let events = super::creation::handle_create(bogus_id, "https://example.org", &state, &deps)
        .await
        .unwrap();

    let (world, system, signal) = extract_events(events);
    assert!(world.is_empty() && system.is_empty() && signal.is_empty(), "no pending node → no events");
}

// ---------------------------------------------------------------------------
// handle_corroborate
// ---------------------------------------------------------------------------

#[tokio::test]
async fn corroboration_emits_citation_world_and_system_events() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);

    let existing_id = Uuid::new_v4();
    let node_type = rootsignal_common::types::NodeType::Tension;
    let similarity = 0.92;

    let events = super::creation::handle_corroborate(
        existing_id,
        node_type,
        "https://org-b.org/events",
        similarity,
        &deps,
    )
    .await
    .unwrap();

    let (world, system, _signal) = extract_events(events);

    // CitationPublished (WorldEvent)
    assert_eq!(world.len(), 1);
    match &world[0] {
        WorldEvent::CitationPublished { signal_id, url, .. } => {
            assert_eq!(*signal_id, existing_id);
            assert_eq!(url, "https://org-b.org/events");
        }
        other => panic!("expected CitationPublished, got {:?}", other),
    }

    // ObservationCorroborated + CorroborationScored (SystemEvents)
    assert_eq!(system.len(), 2);
    match &system[0] {
        SystemEvent::ObservationCorroborated { signal_id, new_source_url, .. } => {
            assert_eq!(*signal_id, existing_id);
            assert_eq!(new_source_url, "https://org-b.org/events");
        }
        other => panic!("expected ObservationCorroborated, got {:?}", other),
    }
    match &system[1] {
        SystemEvent::CorroborationScored { signal_id, similarity: sim, new_corroboration_count } => {
            assert_eq!(*signal_id, existing_id);
            assert!((sim - 0.92).abs() < f64::EPSILON);
            assert_eq!(*new_corroboration_count, 1);
        }
        other => panic!("expected CorroborationScored, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// handle_refresh
// ---------------------------------------------------------------------------

#[tokio::test]
async fn refresh_emits_citation_and_freshness_confirmed() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);

    let existing_id = Uuid::new_v4();
    let node_type = rootsignal_common::types::NodeType::Gathering;

    let events =
        super::creation::handle_refresh(existing_id, node_type, "https://example.org", &deps)
            .await
            .unwrap();

    let (world, system, _signal) = extract_events(events);

    // CitationPublished
    assert_eq!(world.len(), 1);
    assert!(matches!(&world[0], WorldEvent::CitationPublished { .. }));

    // FreshnessConfirmed
    assert_eq!(system.len(), 1);
    match &system[0] {
        SystemEvent::FreshnessConfirmed { signal_ids, node_type: nt, .. } => {
            assert_eq!(signal_ids, &[existing_id]);
            assert_eq!(*nt, node_type);
        }
        other => panic!("expected FreshnessConfirmed, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// handle_signal_stored
// ---------------------------------------------------------------------------

#[tokio::test]
async fn signal_stored_wires_tags_and_source_link() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store.clone());

    let node = tension_at("Community Dinner", 44.95, -93.27);
    let node_id = node.id();
    let source_id = Uuid::new_v4();

    let mut state = PipelineState::new(HashMap::new());
    state.wiring_contexts.insert(
        node_id,
        WiringContext {
            resource_tags: vec![],
            signal_tags: vec!["food".to_string(), "community".to_string()],
            author_name: None,
            source_id: Some(source_id),
        },
    );

    let events = super::creation::handle_signal_stored(
        node_id,
        rootsignal_common::types::NodeType::Tension,
        "https://localorg.org/events",
        "localorg.org",
        &state,
        &deps,
    )
    .await
    .unwrap();

    let (world, system, _signal) = extract_events(events);

    // Should emit: SignalLinkedToSource (world) + SignalTagged (system)
    assert_eq!(system.len(), 1, "expected 1 system event (SignalTagged)");

    // Source linked via world event
    assert!(
        world.iter().any(|e| matches!(
            e,
            WorldEvent::SignalLinkedToSource { signal_id, source_id: sid }
            if *signal_id == node_id && *sid == source_id
        )),
        "expected SignalLinkedToSource event"
    );

    // Tags wired via system event
    assert!(
        system.iter().any(|e| matches!(
            e,
            SystemEvent::SignalTagged { signal_id, tag_slugs }
            if *signal_id == node_id && tag_slugs.len() == 2
        )),
        "expected SignalTagged event"
    );

    // Wiring context stays in state (handler reads, cleaned up at end of run)
    assert!(
        state.wiring_contexts.contains_key(&node_id),
        "wiring context should still be in state (handler reads, not consumes)"
    );
}

#[tokio::test]
async fn signal_stored_with_author_emits_actor_linked() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store.clone());

    let node = tension_at("Food Distribution", 44.95, -93.27);
    let node_id = node.id();

    let mut state = PipelineState::new(HashMap::new());
    state.wiring_contexts.insert(
        node_id,
        WiringContext {
            resource_tags: vec![],
            signal_tags: vec![],
            author_name: Some("Northside Mutual Aid".to_string()),
            source_id: None,
        },
    );

    let events = super::creation::handle_signal_stored(
        node_id,
        rootsignal_common::types::NodeType::Tension,
        "https://instagram.com/northsidemutualaid",
        "instagram.com/northsidemutualaid",
        &state,
        &deps,
    )
    .await
    .unwrap();

    let (_world, system, _signal) = extract_events(events);

    // Author present (new actor) → ActorIdentified + ActorLinkedToSignal (SystemEvents)
    assert_eq!(system.len(), 2, "expected ActorIdentified + ActorLinkedToSignal");

    // ActorIdentified emitted
    assert!(
        system.iter().any(|e| matches!(
            e,
            SystemEvent::ActorIdentified { name, .. }
            if name == "Northside Mutual Aid"
        )),
        "expected ActorIdentified event"
    );

    // ActorLinkedToSignal emitted
    assert!(
        system.iter().any(|e| matches!(
            e,
            SystemEvent::ActorLinkedToSignal { signal_id, role, .. }
            if *signal_id == node_id && role == "authored"
        )),
        "expected ActorLinkedToSignal event"
    );
}

#[tokio::test]
async fn blank_author_name_does_not_create_actor() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store.clone());

    let node_id = Uuid::new_v4();

    let mut state = PipelineState::new(HashMap::new());
    state.wiring_contexts.insert(
        node_id,
        WiringContext {
            resource_tags: vec![],
            signal_tags: vec![],
            author_name: None,
            source_id: None,
        },
    );

    let events = super::creation::handle_signal_stored(
        node_id,
        rootsignal_common::types::NodeType::Tension,
        "https://example.org",
        "example.org",
        &state,
        &deps,
    )
    .await
    .unwrap();

    let (world, system, signal) = extract_events(events);
    assert!(
        world.is_empty() && system.is_empty() && signal.is_empty(),
        "no author → no events"
    );
}
