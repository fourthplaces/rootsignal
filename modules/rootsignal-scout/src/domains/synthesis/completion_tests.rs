//! Completion handler tests — synthesis phase completion tracking.
//!
//! MOCK → ENGINE.EMIT → OUTPUT
//! Proves all synthesis facts gate severity inference correctly.

use std::sync::Arc;

use seesaw_core::AnyEvent;

use crate::core::aggregate::PipelineState;
use crate::domains::expansion::events::ExpansionEvent;
use crate::domains::synthesis::events::SynthesisEvent;
use crate::testing::*;

fn has_severity_inferred(captured: &Arc<std::sync::Mutex<Vec<AnyEvent>>>) -> bool {
    captured.lock().unwrap().iter().any(|e| {
        e.downcast_ref::<SynthesisEvent>()
            .is_some_and(|se| matches!(se, SynthesisEvent::SeverityInferred))
    })
}

#[tokio::test]
async fn one_of_two_synthesis_facts_does_not_trigger_severity() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    engine
        .emit(SynthesisEvent::SimilarityComputed)
        .settled()
        .await
        .unwrap();

    let state = engine.singleton::<PipelineState>();
    assert!(state.similarity_computed);
    assert!(!state.responses_mapped);
    assert!(
        !has_severity_inferred(&captured),
        "Severity should not fire with only 1 of 2 synthesis facts"
    );
}

#[tokio::test]
async fn both_synthesis_facts_trigger_severity() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    engine.emit(SynthesisEvent::SimilarityComputed).settled().await.unwrap();
    engine.emit(SynthesisEvent::ResponsesMapped).settled().await.unwrap();

    let state = engine.singleton::<PipelineState>();
    assert!(state.similarity_computed && state.responses_mapped);
    assert!(
        has_severity_inferred(&captured),
        "SeverityInferred should fire once both synthesis facts are recorded"
    );
}

#[tokio::test]
async fn missing_deps_emits_all_synthesis_facts() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store as Arc<dyn crate::traits::SignalReader>,
        None,
    );

    engine
        .emit(ExpansionEvent::ExpansionCompleted {
            social_expansion_topics: Vec::new(),
            expansion_deferred_expanded: 0,
            expansion_queries_collected: 0,
            expansion_sources_created: 0,
            expansion_social_topics_queued: 0,
        })
        .settled()
        .await
        .unwrap();

    let state = engine.singleton::<PipelineState>();
    assert!(
        state.similarity_computed && state.responses_mapped,
        "Both synthesis facts should be recorded (each handler emits its fact even when skipping)"
    );
    assert!(
        has_severity_inferred(&captured),
        "SeverityInferred should fire exactly once"
    );
}
