//! Engine integration tests — full dispatch loop via seesaw CompatEngine.
//!
//! MOCK → ENGINE.DISPATCH → OUTPUT
//! Proves CompatEngine + seesaw handlers compose correctly.

use std::collections::HashMap;
use std::sync::Arc;

use crate::core::events::{PipelineEvent, ScoutEvent};
use crate::pipeline::state::{ExtractedBatch, PipelineState};
use crate::testing::*;

/// Build test PipelineDeps with a store and Minneapolis region.
fn test_deps(store: Arc<MockSignalReader>) -> crate::pipeline::state::PipelineDeps {
    crate::pipeline::state::PipelineDeps {
        store,
        embedder: Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM)),
        region: Some(mpls_region()),
        run_id: "test-run".to_string(),
        fetcher: None,
        anthropic_api_key: None,
    }
}

/// Helper: collect event variant names from captured events.
fn event_names(captured: &Arc<std::sync::Mutex<Vec<ScoutEvent>>>) -> Vec<String> {
    captured
        .lock()
        .unwrap()
        .iter()
        .map(|e| e.event_type_str())
        .collect()
}

// ---------------------------------------------------------------------------
// NewSignalAccepted dispatch — full chain
// ---------------------------------------------------------------------------

#[tokio::test]
async fn new_signal_accepted_dispatches_full_event_chain() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);
    let (engine, captured) = test_engine_with_capture();

    let node = tension_at("Free Legal Clinic", 44.9341, -93.2619);
    let node_id = node.id();

    let pn = crate::core::aggregate::PendingNode {
        node,
        embedding: vec![0.1; TEST_EMBEDDING_DIM],
        content_hash: "abc123".to_string(),
        resource_tags: vec![],
        signal_tags: vec!["legal".to_string()],
        author_name: Some("Local Legal Aid".to_string()),
        source_id: None,
    };

    let mut state = PipelineState::new(HashMap::new());

    engine
        .dispatch(
            ScoutEvent::Pipeline(PipelineEvent::NewSignalAccepted {
                node_id,
                node_type: rootsignal_common::types::NodeType::Tension,
                title: "Free Legal Clinic".to_string(),
                source_url: "https://www.instagram.com/locallegalaid".to_string(),
                pending_node: Box::new(pn),
            }),
            &mut state,
            &deps,
        )
        .await
        .unwrap();

    // Reducer counted the create
    assert_eq!(state.stats.signals_stored, 1);

    let names = event_names(&captured);

    // Tags emitted via SignalTagged event
    assert!(
        names.iter().any(|n| n == "signal_tagged"),
        "expected SignalTagged event, got: {names:?}"
    );

    // Author actor emitted via ActorIdentified event
    assert!(
        names.iter().any(|n| n == "actor_identified"),
        "expected ActorIdentified event, got: {names:?}"
    );

    // PendingNode consumed by reducer on SignalReaderd
    assert!(!state.pending_nodes.contains_key(&node_id));
}

// ---------------------------------------------------------------------------
// CrossSourceMatchDetected dispatch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cross_source_match_dispatches_citation_and_scoring_events() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);
    let (engine, captured) = test_engine_with_capture();

    let existing_id = uuid::Uuid::new_v4();
    let mut state = PipelineState::new(HashMap::new());

    engine
        .dispatch(
            ScoutEvent::Pipeline(PipelineEvent::CrossSourceMatchDetected {
                existing_id,
                node_type: rootsignal_common::types::NodeType::Tension,
                source_url: "https://org-b.org/events".to_string(),
                similarity: 0.95,
            }),
            &mut state,
            &deps,
        )
        .await
        .unwrap();

    // Reducer counted the dedup
    assert_eq!(state.stats.signals_deduplicated, 1);

    let names = event_names(&captured);
    // CrossSourceMatchDetected → CitationRecorded + ObservationCorroborated + CorroborationScored
    assert_eq!(names.len(), 4, "expected 4 events, got: {names:?}");
    assert!(names.contains(&"citation_recorded".to_string()));
    assert!(names.contains(&"observation_corroborated".to_string()));
    assert!(names.contains(&"corroboration_scored".to_string()));
}

// ---------------------------------------------------------------------------
// SameSourceReencountered dispatch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn same_source_reencountered_dispatches_citation_and_freshness() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);
    let (engine, captured) = test_engine_with_capture();

    let existing_id = uuid::Uuid::new_v4();
    let mut state = PipelineState::new(HashMap::new());

    engine
        .dispatch(
            ScoutEvent::Pipeline(PipelineEvent::SameSourceReencountered {
                existing_id,
                node_type: rootsignal_common::types::NodeType::Gathering,
                source_url: "https://example.org".to_string(),
                similarity: 1.0,
            }),
            &mut state,
            &deps,
        )
        .await
        .unwrap();

    // Reducer counted the dedup
    assert_eq!(state.stats.signals_deduplicated, 1);

    let names = event_names(&captured);
    // SameSourceReencountered → CitationRecorded + FreshnessConfirmed
    assert_eq!(names.len(), 3, "expected 3 events, got: {names:?}");
    assert!(names.contains(&"citation_recorded".to_string()));
    assert!(names.contains(&"freshness_confirmed".to_string()));
}

// ---------------------------------------------------------------------------
// SignalsExtracted → full dispatch chain through dedup + creation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn signals_extracted_dispatches_dedup_and_creation_chain() {
    let store = Arc::new(MockSignalReader::new());
    let deps = test_deps(store);
    let (engine, captured) = test_engine_with_capture();

    let node = tension_at("Free Legal Clinic", 44.9341, -93.2619);

    let mut state = PipelineState::new(HashMap::new());

    // Stash extracted batch in state (this is what the scrape phase does)
    state.extracted_batches.insert(
        "https://localorg.org/events".to_string(),
        ExtractedBatch {
            content: "page content about legal clinic".to_string(),
            nodes: vec![node],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
    );

    // Dispatch SignalsExtracted — the engine does the rest
    engine
        .dispatch(
            ScoutEvent::Pipeline(PipelineEvent::SignalsExtracted {
                url: "https://localorg.org/events".to_string(),
                canonical_key: "localorg.org".to_string(),
                count: 1,
            }),
            &mut state,
            &deps,
        )
        .await
        .unwrap();

    // Reducer counted extraction + creation
    assert_eq!(state.stats.signals_extracted, 1);
    assert_eq!(state.stats.signals_stored, 1);

    // Extracted batch cleaned up by reducer on DedupCompleted
    assert!(state.extracted_batches.is_empty());

    // Pending nodes cleaned up by reducer on SignalReaderd
    assert!(state.pending_nodes.is_empty());

    // Wiring contexts stay until end of run
    assert!(!state.wiring_contexts.is_empty());

    let names = event_names(&captured);

    // Root: SignalsExtracted
    assert_eq!(names[0], "pipeline:signals_extracted");

    // NewSignalAccepted somewhere in chain
    assert!(
        names.contains(&"pipeline:new_signal_accepted".to_string()),
        "expected NewSignalAccepted, got: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// SignalsExtracted with existing title → dedup reencounter path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn signals_extracted_with_existing_title_emits_reencounter() {
    let store = Arc::new(MockSignalReader::new());
    // Pre-populate: "Community Dinner" exists at same URL
    store.insert_signal(
        "Community Dinner",
        rootsignal_common::types::NodeType::Tension,
        "https://example.org/events",
    );
    let deps = test_deps(store);
    let (engine, captured) = test_engine_with_capture();

    let mut state = PipelineState::new(HashMap::new());

    state.extracted_batches.insert(
        "https://example.org/events".to_string(),
        ExtractedBatch {
            content: "page content".to_string(),
            nodes: vec![tension_at("Community Dinner", 44.95, -93.27)],
            resource_tags: HashMap::new(),
            signal_tags: HashMap::new(),
            author_actors: HashMap::new(),
            source_id: None,
        },
    );

    engine
        .dispatch(
            ScoutEvent::Pipeline(PipelineEvent::SignalsExtracted {
                url: "https://example.org/events".to_string(),
                canonical_key: "example.org".to_string(),
                count: 1,
            }),
            &mut state,
            &deps,
        )
        .await
        .unwrap();

    // Reducer counted extraction + dedup
    assert_eq!(state.stats.signals_extracted, 1);
    assert_eq!(state.stats.signals_deduplicated, 1);
    assert_eq!(state.stats.signals_stored, 0);

    let names = event_names(&captured);

    // SameSourceReencountered in chain
    assert!(
        names.contains(&"pipeline:same_source_reencountered".to_string()),
        "expected SameSourceReencountered, got: {names:?}"
    );

    // FreshnessConfirmed emitted
    assert!(
        names.contains(&"freshness_confirmed".to_string()),
        "expected FreshnessConfirmed, got: {names:?}"
    );
}
