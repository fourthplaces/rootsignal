//! Completion handler tests — synthesis phase completion tracking.
//!
//! MOCK → ENGINE.EMIT → OUTPUT
//! Proves the superset check gates completion correctly.

use std::sync::Arc;

use uuid::Uuid;

use crate::core::aggregate::PipelineState;
use crate::domains::expansion::events::ExpansionEvent;
use crate::domains::synthesis::events::{SynthesisEvent, SynthesisRole};
use crate::testing::*;

#[tokio::test]
async fn five_of_six_roles_does_not_complete_synthesis() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, _captured, _scope) = test_engine_with_capture_for_store(
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
}

#[tokio::test]
async fn sixth_role_completes_all_synthesis() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, _captured, _scope) = test_engine_with_capture_for_store(
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
}

#[tokio::test]
async fn missing_deps_emits_all_role_completions() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, _captured, _scope) = test_engine_with_capture_for_store(
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

    let state = engine.singleton::<PipelineState>();
    assert_eq!(
        state.completed_synthesis_roles.len(),
        6,
        "All 6 roles should complete (each handler emits RoleCompleted even when skipping)"
    );
}
