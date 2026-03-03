//! Completion handler tests — enrichment phase completion tracking.
//!
//! MOCK → ENGINE.EMIT → OUTPUT
//! Proves the superset check gates PhaseCompleted correctly.

use std::sync::Arc;

use crate::core::aggregate::PipelineState;
use crate::core::events::PipelinePhase;
use crate::domains::enrichment::events::{EnrichmentEvent, EnrichmentRole};
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::testing::*;
use seesaw_core::AnyEvent;

fn has_phase_completed_actor_enrichment(captured: &Arc<std::sync::Mutex<Vec<AnyEvent>>>) -> bool {
    captured.lock().unwrap().iter().any(|e| {
        e.downcast_ref::<LifecycleEvent>().is_some_and(|le| {
            matches!(
                le,
                LifecycleEvent::PhaseCompleted { phase }
                    if matches!(phase, PipelinePhase::ActorEnrichment)
            )
        })
    })
}

#[tokio::test]
async fn three_of_four_enrichment_roles_does_not_emit_phase_completed() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured) = test_engine_with_capture_for_store(
        store as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    let three_roles = [
        EnrichmentRole::Diversity,
        EnrichmentRole::ActorStats,
        EnrichmentRole::ActorLocation,
    ];

    for role in three_roles {
        engine
            .emit(EnrichmentEvent::EnrichmentRoleCompleted { role })
            .settled()
            .await
            .unwrap();
    }

    let state = engine.singleton::<PipelineState>();
    assert_eq!(state.completed_enrichment_roles.len(), 3);
    assert!(
        !has_phase_completed_actor_enrichment(&captured),
        "PhaseCompleted(ActorEnrichment) should not fire with only 3 of 4 roles"
    );
}

#[tokio::test]
async fn fourth_enrichment_role_emits_phase_completed() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured) = test_engine_with_capture_for_store(
        store as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    let all_roles = [
        EnrichmentRole::ActorExtraction,
        EnrichmentRole::Diversity,
        EnrichmentRole::ActorStats,
        EnrichmentRole::ActorLocation,
    ];

    for role in all_roles {
        engine
            .emit(EnrichmentEvent::EnrichmentRoleCompleted { role })
            .settled()
            .await
            .unwrap();
    }

    let state = engine.singleton::<PipelineState>();
    assert_eq!(state.completed_enrichment_roles.len(), 4);
    assert!(
        has_phase_completed_actor_enrichment(&captured),
        "PhaseCompleted(ActorEnrichment) should fire once all 4 roles complete"
    );
}

#[tokio::test]
async fn missing_deps_skips_enrichment_with_immediate_role_completed() {
    let store = Arc::new(MockSignalReader::new());
    // No region, no graph_client — role handlers should emit role completed immediately
    let (engine, captured) = test_engine_with_capture_for_store(
        store as Arc<dyn crate::traits::SignalReader>,
        None,
    );

    engine
        .emit(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::ResponseScrape,
        })
        .settled()
        .await
        .unwrap();

    assert!(
        has_phase_completed_actor_enrichment(&captured),
        "All role handlers should emit role completed when deps are missing, triggering PhaseCompleted(ActorEnrichment)"
    );

    let state = engine.singleton::<PipelineState>();
    assert_eq!(
        state.completed_enrichment_roles.len(),
        4,
        "All 4 roles should complete (with skip) when deps are missing"
    );
}
