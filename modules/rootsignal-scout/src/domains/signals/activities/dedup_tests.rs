//! Dedup handler tests — MOCK → FUNCTION → OUTPUT.
//!
//! Stash an ExtractedBatch in state, call deduplicate_extracted_batch,
//! assert which verdict events came out.

use std::collections::HashMap;
use std::sync::Arc;

use uuid::Uuid;

use crate::core::engine::ScoutEngineDeps;
use crate::domains::signals::events::SignalEvent;
use crate::core::aggregate::{ExtractedBatch, PipelineState};
use crate::testing::*;
use rootsignal_common::types::NodeType;

fn test_deps(store: Arc<MockSignalReader>) -> ScoutEngineDeps {
    test_scout_deps(store as Arc<dyn crate::traits::SignalReader>)
}

/// Helper: call the dedup handler with a batch.
async fn run_dedup(
    url: &str,
    batch: ExtractedBatch,
    state: &mut PipelineState,
    deps: &ScoutEngineDeps,
) -> Vec<SignalEvent> {
    super::dedup::deduplicate_extracted_batch(url, &batch, &*state, deps)
        .await
        .unwrap()
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
    let mut state = PipelineState::new(HashMap::new());

    let events = run_dedup(
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
        &mut state,
        &deps,
    )
    .await;

    // "Free Legal Clinic" filtered out, "Community Dinner" passes as new
    let creates: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            SignalEvent::NewSignalAccepted { title, .. } => Some(title.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(creates, vec!["Community Dinner"]);
}

#[tokio::test]
async fn all_titles_deduped_returns_no_events() {
    let store = Arc::new(MockSignalReader::new());
    store.add_url_titles(
        "https://example.org/events",
        vec!["Free Legal Clinic".to_string()],
    );
    let deps = test_deps(store);
    let mut state = PipelineState::new(HashMap::new());

    let events = run_dedup(
        "https://example.org/events",
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![tension_at("Free Legal Clinic", 44.93, -93.26)],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
        &mut state,
        &deps,
    )
    .await;

    // Only DedupCompleted (all titles filtered)
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0], SignalEvent::DedupCompleted { .. }));
}

// ---------------------------------------------------------------------------
// Layer 2.5: Global title+type match
// ---------------------------------------------------------------------------

#[tokio::test]
async fn global_title_match_same_source_emits_reencountered() {
    let store = Arc::new(MockSignalReader::new());
    let existing_id = store.insert_signal(
        "Community Dinner",
        NodeType::Tension,
        "https://example.org/events",
    );
    let deps = test_deps(store);
    let mut state = PipelineState::new(HashMap::new());

    let events = run_dedup(
        "https://example.org/events",
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![tension_at("Community Dinner", 44.95, -93.27)],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
        &mut state,
        &deps,
    )
    .await;

    // SameSourceReencountered + DedupCompleted
    assert_eq!(events.len(), 2);
    match &events[0] {
        SignalEvent::SameSourceReencountered {
            existing_id: id,
            similarity,
            ..
        } => {
            assert_eq!(*id, existing_id);
            assert!((similarity - 1.0).abs() < f64::EPSILON);
        }
        other => panic!("expected SameSourceReencountered, got {:?}", other),
    }
}

#[tokio::test]
async fn global_title_match_different_source_emits_cross_source_match() {
    let store = Arc::new(MockSignalReader::new());
    let existing_id = store.insert_signal(
        "Community Dinner",
        NodeType::Tension,
        "https://other-source.org/events",
    );
    let deps = test_deps(store);
    let mut state = PipelineState::new(HashMap::new());

    let events = run_dedup(
        "https://example.org/events",
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![tension_at("Community Dinner", 44.95, -93.27)],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
        &mut state,
        &deps,
    )
    .await;

    // CrossSourceMatchDetected + DedupCompleted
    assert_eq!(events.len(), 2);
    match &events[0] {
        SignalEvent::CrossSourceMatchDetected {
            existing_id: id,
            similarity,
            ..
        } => {
            assert_eq!(*id, existing_id);
            assert!((similarity - 1.0).abs() < f64::EPSILON);
        }
        other => panic!("expected CrossSourceMatchDetected, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Layer 4: No match → NewSignalAccepted
// ---------------------------------------------------------------------------

#[tokio::test]
async fn new_signal_emits_accepted_and_stashes_pending_node() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);
    let mut state = PipelineState::new(HashMap::new());

    let node = tension_at("Free Legal Clinic", 44.93, -93.26);
    let node_id = node.id();

    let events = run_dedup(
        "https://example.org/events",
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![node],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
        &mut state,
        &deps,
    )
    .await;

    // DedupCompleted is appended at the end
    assert_eq!(events.len(), 2);
    match &events[0] {
        SignalEvent::NewSignalAccepted {
            node_id: id,
            title,
            source_url,
            pending_node,
            ..
        } => {
            assert_eq!(*id, node_id);
            assert_eq!(title, "Free Legal Clinic");
            assert_eq!(source_url, "https://example.org/events");
            // PendingNode carried in the event for the reducer to stash
            assert!(!pending_node.embedding.is_empty());
        }
        other => panic!("expected NewSignalAccepted, got {:?}", other),
    }
    assert!(matches!(&events[1], SignalEvent::DedupCompleted { .. }));
}

#[tokio::test]
async fn create_stashes_tags_and_author_from_extraction_id() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);
    let mut state = PipelineState::new(HashMap::new());

    let node = tension_at("Food Distribution", 44.95, -93.27);
    let meta_id = node.meta().unwrap().id;
    let node_id = node.id();

    let mut tag_map = HashMap::new();
    tag_map.insert(meta_id, vec!["food".to_string(), "mutual-aid".to_string()]);

    let mut author_actors = HashMap::new();
    author_actors.insert(meta_id, "Northside Mutual Aid".to_string());

    let source_id = Uuid::new_v4();

    let events = run_dedup(
        "https://example.org/events",
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![node],
            resource_tags: HashMap::new(),
            signal_tags: tag_map,
            author_actors,
            source_id: Some(source_id),
        },
        &mut state,
        &deps,
    )
    .await;

    // NewSignalAccepted + DedupCompleted
    assert_eq!(events.len(), 2);

    match &events[0] {
        SignalEvent::NewSignalAccepted { pending_node, .. } => {
            assert_eq!(
                pending_node.signal_tags,
                vec!["food".to_string(), "mutual-aid".to_string()]
            );
            assert_eq!(
                pending_node.author_name.as_deref(),
                Some("Northside Mutual Aid")
            );
            assert_eq!(pending_node.source_id, Some(source_id));
        }
        other => panic!("expected NewSignalAccepted, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Mixed batch + edge cases
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mixed_batch_emits_correct_verdicts() {
    let store = Arc::new(MockSignalReader::new());
    let existing_id = store.insert_signal(
        "Existing Event",
        NodeType::Tension,
        "https://other-source.org",
    );
    let deps = test_deps(store);
    let mut state = PipelineState::new(HashMap::new());

    let events = run_dedup(
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
        &mut state,
        &deps,
    )
    .await;

    // CrossSourceMatchDetected + NewSignalAccepted + DedupCompleted
    assert_eq!(events.len(), 3);

    match &events[0] {
        SignalEvent::CrossSourceMatchDetected {
            existing_id: id, ..
        } => assert_eq!(*id, existing_id),
        other => panic!("expected CrossSourceMatchDetected, got {:?}", other),
    }

    match &events[1] {
        SignalEvent::NewSignalAccepted { title, .. } => {
            assert_eq!(title, "Brand New Event")
        }
        other => panic!("expected NewSignalAccepted, got {:?}", other),
    }

    assert!(matches!(&events[2], SignalEvent::DedupCompleted { .. }));
}

#[tokio::test]
async fn empty_batch_emits_only_dedup_completed() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);
    let mut state = PipelineState::new(HashMap::new());

    let events = run_dedup(
        "https://example.org/events",
        ExtractedBatch {
            content: "content".to_string(),
            nodes: vec![],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
        &mut state,
        &deps,
    )
    .await;

    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0], SignalEvent::DedupCompleted { .. }));
}
