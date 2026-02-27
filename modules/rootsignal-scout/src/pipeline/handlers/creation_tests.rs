//! Handler tests — MOCK → FUNCTION → OUTPUT.
//!
//! Set up mocks, call the real handler, assert what events came out.

use std::collections::HashMap;
use std::sync::Arc;

use rootsignal_common::events::{SystemEvent, WorldEvent};
use uuid::Uuid;

use crate::pipeline::events::{PipelineEvent, ScoutEvent};
use crate::pipeline::state::{PendingNode, PipelineDeps, PipelineState, WiringContext};
use crate::testing::*;

/// Build test PipelineDeps with a mock store.
fn test_deps(store: Arc<MockSignalStore>) -> PipelineDeps {
    PipelineDeps {
        store,
        embedder: Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM)),
        region: mpls_region(),
        run_id: "test-run".to_string(),
    }
}

// ---------------------------------------------------------------------------
// handle_create
// ---------------------------------------------------------------------------

#[tokio::test]
async fn new_signal_emits_world_system_citation_and_signal_stored() {
    let store = Arc::new(MockSignalStore::new());
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

    let events = super::creation::handle_create(node_id, "https://localorg.org/events", &mut state, &deps)
        .await
        .unwrap();

    // Should emit: World(discovery) + System(sensitivity) + World(CitationRecorded)
    //            + Pipeline(SignalStored)
    // May also emit System(ImpliedQueries) if the node has implied queries.
    assert!(events.len() >= 4, "expected at least 4 events, got {}", events.len());

    // First event: World discovery
    assert!(
        matches!(&events[0], ScoutEvent::World(_)),
        "first event should be a World event"
    );

    // At least one System event (sensitivity)
    let system_count = events.iter().filter(|e| matches!(e, ScoutEvent::System(_))).count();
    assert!(system_count >= 1, "expected at least one System event");

    // CitationRecorded present (evidence flows through events)
    let has_citation = events.iter().any(|e| matches!(
        e,
        ScoutEvent::World(WorldEvent::CitationRecorded { .. })
    ));
    assert!(has_citation, "expected CitationRecorded event");

    // Last event: SignalStored pipeline event
    let last = events.last().unwrap();
    match last {
        ScoutEvent::Pipeline(PipelineEvent::SignalStored { node_id: stored_id, .. }) => {
            assert_eq!(*stored_id, node_id);
        }
        other => panic!("expected SignalStored, got {:?}", other),
    }

    // PendingNode consumed, WiringContext stashed
    assert!(
        !state.pending_nodes.contains_key(&node_id),
        "pending node should be consumed"
    );
    assert!(
        state.wiring_contexts.contains_key(&node_id),
        "wiring context should be stashed for signal_stored"
    );
}

#[tokio::test]
async fn missing_pending_node_returns_empty_events() {
    let store = Arc::new(MockSignalStore::new());
    let deps = test_deps(store);

    let mut state = PipelineState::new(HashMap::new());
    let bogus_id = Uuid::new_v4();

    let events = super::creation::handle_create(bogus_id, "https://example.org", &mut state, &deps)
        .await
        .unwrap();

    assert!(events.is_empty(), "no pending node → no events");
}

// ---------------------------------------------------------------------------
// handle_corroborate
// ---------------------------------------------------------------------------

#[tokio::test]
async fn corroboration_emits_citation_world_and_system_events() {
    let store = Arc::new(MockSignalStore::new());
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

    assert_eq!(events.len(), 3);

    // CitationRecorded (evidence)
    match &events[0] {
        ScoutEvent::World(WorldEvent::CitationRecorded { signal_id, url, .. }) => {
            assert_eq!(*signal_id, existing_id);
            assert_eq!(url, "https://org-b.org/events");
        }
        other => panic!("expected CitationRecorded, got {:?}", other),
    }

    // ObservationCorroborated
    match &events[1] {
        ScoutEvent::World(WorldEvent::ObservationCorroborated {
            signal_id,
            new_source_url,
            ..
        }) => {
            assert_eq!(*signal_id, existing_id);
            assert_eq!(new_source_url, "https://org-b.org/events");
        }
        other => panic!("expected ObservationCorroborated, got {:?}", other),
    }

    // CorroborationScored
    match &events[2] {
        ScoutEvent::System(SystemEvent::CorroborationScored {
            signal_id,
            similarity: sim,
            new_corroboration_count,
        }) => {
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
    let store = Arc::new(MockSignalStore::new());
    let deps = test_deps(store);

    let existing_id = Uuid::new_v4();
    let node_type = rootsignal_common::types::NodeType::Gathering;

    let events = super::creation::handle_refresh(existing_id, node_type, "https://example.org", &deps)
        .await
        .unwrap();

    assert_eq!(events.len(), 2);

    // CitationRecorded
    assert!(matches!(&events[0], ScoutEvent::World(WorldEvent::CitationRecorded { .. })));

    // FreshnessConfirmed
    match &events[1] {
        ScoutEvent::System(SystemEvent::FreshnessConfirmed {
            signal_ids,
            node_type: nt,
            ..
        }) => {
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
    let store = Arc::new(MockSignalStore::new());
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
        &mut state,
        &deps,
    )
    .await
    .unwrap();

    // Should emit: SignalLinkedToSource + SignalTagged (no actor events since no author)
    assert_eq!(events.len(), 2, "expected source link + tag events");

    // Source linked via event
    assert!(
        events.iter().any(|e| matches!(
            e,
            ScoutEvent::System(SystemEvent::SignalLinkedToSource { signal_id, source_id: sid })
            if *signal_id == node_id && *sid == source_id
        )),
        "expected SignalLinkedToSource event"
    );

    // Tags wired via event
    assert!(
        events.iter().any(|e| matches!(
            e,
            ScoutEvent::System(SystemEvent::SignalTagged { signal_id, tag_slugs })
            if *signal_id == node_id && tag_slugs.len() == 2
        )),
        "expected SignalTagged event"
    );

    // Wiring context consumed
    assert!(
        !state.wiring_contexts.contains_key(&node_id),
        "wiring context should be consumed"
    );
}

#[tokio::test]
async fn signal_stored_with_author_emits_actor_linked() {
    let store = Arc::new(MockSignalStore::new());
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
        &mut state,
        &deps,
    )
    .await
    .unwrap();

    // Author present (new actor) → ActorIdentified + ActorLinkedToSignal
    assert_eq!(events.len(), 2, "expected ActorIdentified + ActorLinkedToSignal");

    // ActorIdentified emitted
    assert!(
        events.iter().any(|e| matches!(
            e,
            ScoutEvent::World(WorldEvent::ActorIdentified { name, .. })
            if name == "Northside Mutual Aid"
        )),
        "expected ActorIdentified event"
    );

    // ActorLinkedToSignal emitted
    assert!(
        events.iter().any(|e| matches!(
            e,
            ScoutEvent::World(WorldEvent::ActorLinkedToSignal { signal_id, role, .. })
            if *signal_id == node_id && role == "authored"
        )),
        "expected ActorLinkedToSignal event"
    );
}

#[tokio::test]
async fn blank_author_name_does_not_create_actor() {
    let store = Arc::new(MockSignalStore::new());
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
        &mut state,
        &deps,
    )
    .await
    .unwrap();

    assert!(events.is_empty(), "no author → no events");
}
