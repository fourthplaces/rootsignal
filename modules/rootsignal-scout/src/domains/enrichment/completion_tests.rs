//! Completion handler tests — enrichment phase completion tracking.
//!
//! MOCK → ENGINE.EMIT → OUTPUT
//! Proves the superset check gates MetricsCompleted correctly.

use std::sync::Arc;

use crate::core::aggregate::PipelineState;
use crate::domains::enrichment::events::{EnrichmentEvent, EnrichmentRole};
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::scrape::events::{ScrapeEvent, ScrapeRole};
use crate::testing::*;
use seesaw_core::AnyEvent;

fn has_metrics_completed(captured: &Arc<std::sync::Mutex<Vec<AnyEvent>>>) -> bool {
    captured.lock().unwrap().iter().any(|e| {
        e.downcast_ref::<LifecycleEvent>()
            .is_some_and(|le| matches!(le, LifecycleEvent::MetricsCompleted))
    })
}

/// Emit all 3 response ScrapeRoleCompleted events to trigger enrichment handlers.
async fn emit_response_scrape_done(engine: &seesaw_core::Engine<crate::core::engine::ScoutEngineDeps>) {
    for role in [ScrapeRole::ResponseWeb, ScrapeRole::ResponseSocial, ScrapeRole::TopicDiscovery] {
        engine
            .emit(ScrapeEvent::from(TestScrapeRoleCompleted::builder().role(role).build()))
            .settled()
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn three_of_four_enrichment_roles_does_not_trigger_metrics() {
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
        !has_metrics_completed(&captured),
        "MetricsCompleted should not fire with only 3 of 4 roles"
    );
}

#[tokio::test]
async fn fourth_enrichment_role_triggers_metrics() {
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
        has_metrics_completed(&captured),
        "MetricsCompleted should fire once all 4 roles complete"
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

    // Emit response scrape completion — triggers enrichment via state-gated filters
    emit_response_scrape_done(&engine).await;

    assert!(
        has_metrics_completed(&captured),
        "All role handlers should emit role completed when deps are missing, triggering MetricsCompleted"
    );

    let state = engine.singleton::<PipelineState>();
    assert_eq!(
        state.completed_enrichment_roles.len(),
        4,
        "All 4 roles should complete (with skip) when deps are missing"
    );
}
