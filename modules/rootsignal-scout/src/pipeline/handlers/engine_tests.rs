//! Engine integration tests — full dispatch loop with MemoryEventSink.
//!
//! MOCK → ENGINE.DISPATCH → OUTPUT
//! Proves Engine + ScoutReducer + handlers compose correctly.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use rootsignal_engine::{Engine, MemoryEventSink, Reducer, Router};
use rootsignal_events::StoredEvent;
use uuid::Uuid;

use crate::pipeline::events::{PipelineEvent, ScoutEvent};
use crate::pipeline::handlers;
use crate::pipeline::reducer::ScoutReducer;
use crate::pipeline::state::{ExtractedBatch, PendingNode, PipelineDeps, PipelineState};
use crate::testing::*;

// ---------------------------------------------------------------------------
// Test router — dispatches to handlers, skips projection
// ---------------------------------------------------------------------------

struct TestRouter;

#[async_trait]
impl Router<ScoutEvent, PipelineState, PipelineDeps> for TestRouter {
    async fn route(
        &self,
        event: &ScoutEvent,
        stored: &StoredEvent,
        state: &PipelineState,
        deps: &PipelineDeps,
    ) -> Result<Vec<ScoutEvent>> {
        match event {
            ScoutEvent::Pipeline(pe) => handlers::route_pipeline(pe, stored, state, deps).await,
            // Skip projection in tests
            ScoutEvent::World(_) | ScoutEvent::System(_) => Ok(vec![]),
        }
    }
}

/// Build test PipelineDeps.
fn test_deps(store: Arc<MockSignalStore>) -> PipelineDeps {
    PipelineDeps {
        store,
        embedder: Arc::new(FixedEmbedder::new(TEST_EMBEDDING_DIM)),
        region: Some(mpls_region()),
        run_id: "test-run".to_string(),
        fetcher: None,
        anthropic_api_key: None,
    }
}

// ---------------------------------------------------------------------------
// NewSignalAccepted dispatch — full chain
// ---------------------------------------------------------------------------

#[tokio::test]
async fn new_signal_accepted_dispatches_full_event_chain() {
    let store = Arc::new(MockSignalStore::new());
    let deps = test_deps(store.clone());
    let sink = Arc::new(MemoryEventSink::new());

    let engine = Engine::new(ScoutReducer, TestRouter, sink.clone(), "test-run".into());

    let node = tension_at("Free Legal Clinic", 44.9341, -93.2619);
    let node_id = node.id();

    let pn = PendingNode {
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

    let events = sink.events();

    // Tags emitted via SignalTagged event
    assert!(
        events.iter().any(|e| e.event_type == "signal_tagged"),
        "expected SignalTagged event in sink"
    );

    // Author actor emitted via ActorIdentified event
    assert!(
        events.iter().any(|e| e.event_type == "actor_identified"),
        "expected ActorIdentified event in sink"
    );

    // PendingNode consumed by reducer on SignalStored, wiring contexts stay until end of run
    assert!(!state.pending_nodes.contains_key(&node_id));
}

#[tokio::test]
async fn new_signal_accepted_persists_causal_chain() {
    let store = Arc::new(MockSignalStore::new());
    let deps = test_deps(store);
    let sink = Arc::new(MemoryEventSink::new());

    let engine = Engine::new(ScoutReducer, TestRouter, sink.clone(), "chain-run".into());

    let node = tension_at("Community Dinner", 44.95, -93.27);
    let node_id = node.id();

    let pn = PendingNode {
        node,
        embedding: vec![0.1; TEST_EMBEDDING_DIM],
        content_hash: "def456".to_string(),
        resource_tags: vec![],
        signal_tags: vec![],
        author_name: None,
        source_id: None,
    };

    let mut state = PipelineState::new(HashMap::new());

    engine
        .dispatch(
            ScoutEvent::Pipeline(PipelineEvent::NewSignalAccepted {
                node_id,
                node_type: rootsignal_common::types::NodeType::Tension,
                title: "Community Dinner".to_string(),
                source_url: "https://example.org".to_string(),
                pending_node: Box::new(pn),
            }),
            &mut state,
            &deps,
        )
        .await
        .unwrap();

    let events = sink.events();

    // Root: NewSignalAccepted
    assert_eq!(events[0].event_type, "pipeline:new_signal_accepted");
    assert!(events[0].caused_by_seq.is_none());

    // Children: World discovery + System + CitationRecorded + SignalStored
    let children: Vec<_> = events
        .iter()
        .filter(|e| e.caused_by_seq == Some(events[0].seq))
        .collect();
    assert!(
        children.len() >= 4,
        "expected at least 4 children of NewSignalAccepted, got {}",
        children.len()
    );

    assert!(children
        .iter()
        .any(|e| e.event_type == "citation_recorded"));
    assert!(children
        .iter()
        .any(|e| e.event_type == "pipeline:signal_stored"));
}

// ---------------------------------------------------------------------------
// CrossSourceMatchDetected dispatch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cross_source_match_dispatches_citation_and_scoring_events() {
    let store = Arc::new(MockSignalStore::new());
    let deps = test_deps(store);
    let sink = Arc::new(MemoryEventSink::new());

    let engine = Engine::new(ScoutReducer, TestRouter, sink.clone(), "corr-run".into());

    let existing_id = Uuid::new_v4();
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

    // Events: CrossSourceMatchDetected → CitationRecorded + ObservationCorroborated + CorroborationScored
    let events = sink.events();
    assert_eq!(events.len(), 4);
    assert_eq!(events[0].event_type, "pipeline:cross_source_match_detected");

    let child_types: Vec<&str> = events[1..].iter().map(|e| e.event_type.as_str()).collect();
    assert!(child_types.contains(&"citation_recorded"));
    assert!(child_types.contains(&"observation_corroborated"));
    assert!(child_types.contains(&"corroboration_scored"));
}

// ---------------------------------------------------------------------------
// SameSourceReencountered dispatch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn same_source_reencountered_dispatches_citation_and_freshness() {
    let store = Arc::new(MockSignalStore::new());
    let deps = test_deps(store);
    let sink = Arc::new(MemoryEventSink::new());

    let engine = Engine::new(ScoutReducer, TestRouter, sink.clone(), "refresh-run".into());

    let existing_id = Uuid::new_v4();
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

    // Events: SameSourceReencountered → CitationRecorded + FreshnessConfirmed
    let events = sink.events();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].event_type, "pipeline:same_source_reencountered");

    let child_types: Vec<&str> = events[1..].iter().map(|e| e.event_type.as_str()).collect();
    assert!(child_types.contains(&"citation_recorded"));
    assert!(child_types.contains(&"freshness_confirmed"));
}

// ---------------------------------------------------------------------------
// SignalsExtracted → full dispatch chain through dedup + creation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn signals_extracted_dispatches_dedup_and_creation_chain() {
    let store = Arc::new(MockSignalStore::new());
    let deps = test_deps(store.clone());
    let sink = Arc::new(MemoryEventSink::new());

    let engine = Engine::new(ScoutReducer, TestRouter, sink.clone(), "extract-run".into());

    let node = tension_at("Free Legal Clinic", 44.9341, -93.2619);
    let node_id = node.id();

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

    // Pending nodes cleaned up by reducer on SignalStored
    assert!(state.pending_nodes.is_empty());

    // Wiring contexts stay until end of run (handler reads, not consumes)
    assert!(!state.wiring_contexts.is_empty());

    // Causal chain in event store
    let events = sink.events();

    // Root: SignalsExtracted
    assert_eq!(events[0].event_type, "pipeline:signals_extracted");
    assert!(events[0].caused_by_seq.is_none());

    // Child: NewSignalAccepted
    let new_signal = events
        .iter()
        .find(|e| e.event_type == "pipeline:new_signal_accepted")
        .expect("expected NewSignalAccepted event");
    assert_eq!(new_signal.caused_by_seq, Some(events[0].seq));

    // Grandchildren: World discovery + System + CitationRecorded + SignalStored
    let grandchildren: Vec<_> = events
        .iter()
        .filter(|e| e.caused_by_seq == Some(new_signal.seq))
        .collect();
    assert!(
        grandchildren.len() >= 4,
        "expected at least 4 grandchildren of NewSignalAccepted, got {}",
        grandchildren.len()
    );
}

#[tokio::test]
async fn signals_extracted_with_existing_title_emits_reencounter() {
    let store = Arc::new(MockSignalStore::new());
    // Pre-populate: "Community Dinner" exists at same URL
    let existing_id = store.insert_signal(
        "Community Dinner",
        rootsignal_common::types::NodeType::Tension,
        "https://example.org/events",
    );
    let deps = test_deps(store);
    let sink = Arc::new(MemoryEventSink::new());

    let engine = Engine::new(ScoutReducer, TestRouter, sink.clone(), "reenc-run".into());

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

    // Causal chain: SignalsExtracted → SameSourceReencountered → CitationRecorded + FreshnessConfirmed
    let events = sink.events();
    let reencounter = events
        .iter()
        .find(|e| e.event_type == "pipeline:same_source_reencountered")
        .expect("expected SameSourceReencountered event");
    assert_eq!(reencounter.caused_by_seq, Some(events[0].seq));

    assert!(events
        .iter()
        .any(|e| e.event_type == "freshness_confirmed"));
}

// ---------------------------------------------------------------------------
// SourceDiscovered dispatch — reducer increments stat
// ---------------------------------------------------------------------------

#[tokio::test]
async fn source_discovered_increments_stat() {
    let store = Arc::new(MockSignalStore::new());
    let sink = Arc::new(MemoryEventSink::new());
    let deps = test_deps(store);

    let engine = Engine::new(
        ScoutReducer,
        TestRouter,
        sink.clone() as Arc<dyn rootsignal_engine::EventPersister>,
        "test-run".to_string(),
    );

    let source = rootsignal_common::SourceNode::new(
        "example.org".into(),
        "example.org".into(),
        Some("https://example.org".into()),
        rootsignal_common::DiscoveryMethod::LinkedFrom,
        0.25,
        rootsignal_common::SourceRole::Mixed,
        None,
    );

    let mut ctx = PipelineState::new(HashMap::new());

    engine
        .dispatch(
            ScoutEvent::Pipeline(PipelineEvent::SourceDiscovered {
                source,
                discovered_by: "link_promoter".into(),
            }),
            &mut ctx,
            &deps,
        )
        .await
        .expect("dispatch should succeed");

    assert_eq!(ctx.stats.sources_discovered, 1);

    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "pipeline:source_discovered");
}
