//! Completion handler tests — enrichment phase completion tracking.
//!
//! MOCK → ENGINE.EMIT → OUTPUT
//! Proves all 4 enrichment facts gate expansion correctly.

use std::sync::Arc;

use crate::core::aggregate::PipelineState;
use crate::domains::enrichment::events::EnrichmentEvent;
use crate::domains::expansion::events::ExpansionEvent;
use crate::domains::scrape::events::ScrapeEvent;
use crate::testing::*;
use seesaw_core::AnyEvent;

fn has_expansion_completed(captured: &Arc<std::sync::Mutex<Vec<AnyEvent>>>) -> bool {
    captured.lock().unwrap().iter().any(|e| {
        e.downcast_ref::<ExpansionEvent>()
            .is_some_and(|ee| matches!(ee, ExpansionEvent::ExpansionCompleted { .. }))
    })
}

/// Emit response scrape completion events to trigger enrichment handlers.
async fn emit_response_scrape_done(engine: &seesaw_core::Engine<crate::core::engine::ScoutEngineDeps>) {
    engine.emit(ScrapeEvent::from(TestWebScrapeCompleted::builder().is_tension(false).build())).settled().await.unwrap();
    engine.emit(empty_social_scrape(false)).settled().await.unwrap();
    engine.emit(empty_topic_discovery()).settled().await.unwrap();
}

#[tokio::test]
async fn three_of_four_enrichment_facts_does_not_trigger_expansion() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    let three_events = [
        EnrichmentEvent::DiversityScored,
        EnrichmentEvent::ActorStatsComputed,
        EnrichmentEvent::ActorsLocated,
    ];

    for event in three_events {
        engine.emit(event).settled().await.unwrap();
    }

    let state = engine.singleton::<PipelineState>();
    assert!(!state.actors_extracted);
    assert!(state.diversity_scored);
    assert!(state.actor_stats_computed);
    assert!(state.actors_located);
    assert!(
        !has_expansion_completed(&captured),
        "Expansion should not fire with only 3 of 4 enrichment facts"
    );
}

#[tokio::test]
async fn fourth_enrichment_fact_triggers_expansion() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    let all_events = [
        EnrichmentEvent::ActorsExtracted,
        EnrichmentEvent::DiversityScored,
        EnrichmentEvent::ActorStatsComputed,
        EnrichmentEvent::ActorsLocated,
    ];

    for event in all_events {
        engine.emit(event).settled().await.unwrap();
    }

    let state = engine.singleton::<PipelineState>();
    assert!(state.all_enrichment_complete());
    assert!(
        has_expansion_completed(&captured),
        "ExpansionCompleted should fire once all 4 enrichment facts are recorded"
    );
}

#[tokio::test]
async fn missing_deps_skips_enrichment_with_immediate_completion() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store as Arc<dyn crate::traits::SignalReader>,
        None,
    );

    use crate::testing::sources_prepared_event;
    engine.emit(sources_prepared_event(false)).settled().await.unwrap();

    emit_response_scrape_done(&engine).await;

    assert!(
        has_expansion_completed(&captured),
        "All handlers should emit their fact when deps are missing, triggering expansion"
    );

    let state = engine.singleton::<PipelineState>();
    assert!(
        state.all_enrichment_complete(),
        "All 4 enrichment facts should be recorded (with skip) when deps are missing"
    );
}

#[tokio::test]
async fn response_scrape_skipped_triggers_enrichment_and_expansion() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    use crate::testing::sources_prepared_event;
    engine.emit(sources_prepared_event(false)).settled().await.unwrap();

    engine
        .emit(ScrapeEvent::ResponseScrapeSkipped {
            reason: "missing region or graph".into(),
        })
        .settled()
        .await
        .unwrap();

    let state = engine.singleton::<PipelineState>();
    assert!(
        state.all_enrichment_complete(),
        "All enrichment facts should be recorded (with skip) when response scrape is skipped"
    );
    assert!(
        has_expansion_completed(&captured),
        "ExpansionCompleted should fire — skipped response scrape still unlocks expansion"
    );
}
