//! Completion handler tests — synthesis phase completion tracking.
//!
//! MOCK → ENGINE.EMIT → OUTPUT
//! Proves the superset check gates finalize correctly.

use std::sync::Arc;

use uuid::Uuid;

use crate::core::aggregate::PipelineState;
use crate::domains::expansion::events::ExpansionEvent;
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::synthesis::events::{SynthesisEvent, SynthesisRole};
use crate::testing::*;
use seesaw_core::AnyEvent;

fn has_run_completed(captured: &Arc<std::sync::Mutex<Vec<AnyEvent>>>) -> bool {
    captured.lock().unwrap().iter().any(|e| {
        e.downcast_ref::<LifecycleEvent>()
            .is_some_and(|le| matches!(le, LifecycleEvent::RunCompleted { .. }))
    })
}

#[tokio::test]
async fn five_of_six_synthesis_roles_does_not_trigger_finalize() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured) = test_engine_with_capture_for_store(
        store as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    let run_id = Uuid::new_v4();
    let five_roles = [
        SynthesisRole::Similarity,
        SynthesisRole::ResponseMapping,
        SynthesisRole::ConcernLinker,
        SynthesisRole::ResponseFinder,
        SynthesisRole::GatheringFinder,
    ];

    for role in five_roles {
        engine
            .emit(SynthesisEvent::SynthesisRoleCompleted { run_id, role })
            .settled()
            .await
            .unwrap();
    }

    let state = engine.singleton::<PipelineState>();
    assert_eq!(state.completed_synthesis_roles.len(), 5);
    assert!(
        !has_run_completed(&captured),
        "RunCompleted should not fire with only 5 of 6 roles"
    );
}

#[tokio::test]
async fn sixth_synthesis_role_triggers_finalize() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured) = test_engine_with_capture_for_store(
        store as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    let run_id = Uuid::new_v4();
    let all_roles = [
        SynthesisRole::Similarity,
        SynthesisRole::ResponseMapping,
        SynthesisRole::ConcernLinker,
        SynthesisRole::ResponseFinder,
        SynthesisRole::GatheringFinder,
        SynthesisRole::Investigation,
    ];

    for role in all_roles {
        engine
            .emit(SynthesisEvent::SynthesisRoleCompleted { run_id, role })
            .settled()
            .await
            .unwrap();
    }

    let state = engine.singleton::<PipelineState>();
    assert_eq!(state.completed_synthesis_roles.len(), 6);
    assert!(
        has_run_completed(&captured),
        "RunCompleted should fire once all 6 roles complete"
    );
}

#[tokio::test]
async fn missing_deps_skips_synthesis_with_immediate_finalize() {
    let store = Arc::new(MockSignalReader::new());
    // No region, no graph_client, no budget — each handler guards its own deps
    // and emits SynthesisRoleCompleted (fact: "completed with nothing to do")
    let (engine, captured) = test_engine_with_capture_for_store(
        store as Arc<dyn crate::traits::SignalReader>,
        None,
    );

    // ExpansionCompleted triggers all 6 synthesis role handlers
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

    assert!(
        has_run_completed(&captured),
        "RunCompleted should fire when all handlers skip due to missing deps"
    );

    let state = engine.singleton::<PipelineState>();
    assert_eq!(
        state.completed_synthesis_roles.len(),
        6,
        "All 6 roles should complete (each handler emits RoleCompleted even when skipping)"
    );
}
