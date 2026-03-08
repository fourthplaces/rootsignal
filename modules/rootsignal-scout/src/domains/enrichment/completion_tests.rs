//! Completion handler tests — enrichment phase completion tracking.
//!
//! MOCK → ENGINE.EMIT → OUTPUT
//! Proves all 4 enrichment facts + review counters gate expansion correctly.
//!
//! Two trampolines collapse fan-in:
//!   review gate ([SystemEvent, SignalEvent]) → EnrichmentReady (1 event)
//!   expansion gate (EnrichmentEvent)         → ExpansionReady  (1 event)

use std::sync::Arc;

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
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

/// Emit response scrape completion events to satisfy `response_scrape_done()`.
/// Each scrape completion triggers dedup → NoNewSignals, which wakes enrichment handlers.
async fn emit_response_scrape_done(engine: &seesaw_core::Engine<ScoutEngineDeps>) {
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
async fn missing_deps_completes_enrichment_via_dedup_no_new_signals() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store as Arc<dyn crate::traits::SignalReader>,
        None,
    );

    use crate::testing::sources_prepared_event;
    engine.emit(sources_prepared_event(false)).settled().await.unwrap();

    // Each scrape completion triggers dedup → NoNewSignals → enrichment handlers.
    // No deps (region/graph/AI) → handlers skip work but emit their facts.
    // review_complete() is 0==0=true, response_scrape_done() becomes true.
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
async fn response_scrape_skipped_completes_enrichment_via_dedup() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    use crate::testing::sources_prepared_event;
    engine.emit(sources_prepared_event(false)).settled().await.unwrap();

    // ResponseScrapeSkipped passes is_completion() → dedup fires → NoNewSignals.
    // Reducer sets response_scrape_done()=true. NoNewSignals wakes enrichment handlers.
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

#[tokio::test]
async fn pending_reviews_block_enrichment() {
    let url = "https://example.com/events";
    let store = Arc::new(MockSignalReader::new());
    let fetcher = Arc::new(crate::testing::MockFetcher::new().on_page(
        url,
        crate::testing::archived_page(url, "Community dinner at Powderhorn Park"),
    ));
    let extractor = Arc::new(crate::testing::MockExtractor::new().on_url(
        url,
        crate::core::extractor::ExtractionResult {
            nodes: vec![crate::testing::tension_at("Community Dinner", 44.95, -93.27)],
            ..Default::default()
        },
    ));
    let (engine, captured, _scope) = crate::testing::test_engine_with_scrape_capture(
        store as Arc<dyn crate::traits::SignalReader>,
        fetcher,
        extractor,
        Some(mpls_region()),
    );

    // SourcesPrepared with actual web_urls → start_web_scrape fetches the page,
    // extractor finds "Community Dinner" → dedup creates a WorldEvent →
    // signals_awaiting_review=1. No reviews completed → review_complete()=false.
    engine
        .emit(crate::testing::sources_prepared_with_web_urls(url))
        .settled()
        .await
        .unwrap();

    let state = engine.singleton::<PipelineState>();
    assert!(
        state.signals_awaiting_review > 0,
        "dedup should have created at least one signal (awaiting={})",
        state.signals_awaiting_review,
    );
    assert_eq!(state.signals_review_completed, 0);
    assert!(!state.review_complete());

    assert!(
        !has_expansion_completed(&captured),
        "Expansion should NOT fire because review is incomplete ({} awaiting, 0 completed)",
        state.signals_awaiting_review,
    );
}

#[tokio::test]
async fn zero_signals_skips_review_gate() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store as Arc<dyn crate::traits::SignalReader>,
        None,
    );

    use crate::testing::sources_prepared_event;
    engine.emit(sources_prepared_event(false)).settled().await.unwrap();

    // No signals created → signals_awaiting_review stays 0.
    // Response scrape completes → dedup → NoNewSignals → enrichment fires
    // because review_complete() is 0==0=true.
    emit_response_scrape_done(&engine).await;

    let state = engine.singleton::<PipelineState>();
    assert_eq!(state.signals_awaiting_review, 0);
    assert_eq!(state.signals_review_completed, 0);
    assert!(state.review_complete(), "review_complete() should be true when 0==0");
    assert!(
        state.all_enrichment_complete(),
        "All enrichment facts should be set (handlers skipped due to missing deps)"
    );
    assert!(
        has_expansion_completed(&captured),
        "Expansion should fire — zero signals means review gate passes immediately"
    );
}
