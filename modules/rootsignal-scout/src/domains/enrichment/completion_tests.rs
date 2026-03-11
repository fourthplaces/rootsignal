//! Completion handler tests — enrichment phase completion tracking.
//!
//! MOCK → ENGINE.EMIT → OUTPUT
//! Proves review counters + response scrape gate enrichment correctly.
//!
//! One trampoline collapses fan-in:
//!   review gate ([SystemEvent, SignalEvent]) → EnrichmentReady (1 event)
//!
//! `run_enrichment` fires once on EnrichmentReady, runs all enrichment steps,
//! then emits ExpansionReady in a single handler output.

use std::sync::Arc;

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
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
/// Each scrape completion triggers dedup → NoNewSignals, which wakes the review gate.
async fn emit_response_scrape_done(engine: &seesaw_core::Engine<ScoutEngineDeps>) {
    engine.emit(ScrapeEvent::from(TestWebScrapeCompleted::builder().is_tension(false).build())).settled().await.unwrap();
    engine.emit(empty_social_scrape(false)).settled().await.unwrap();
    engine.emit(empty_topic_discovery()).settled().await.unwrap();
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
    // No deps (region/graph/AI) → handlers skip work but still emit ExpansionReady.
    // review_complete() is 0==0=true, response_scrape_done() becomes true.
    emit_response_scrape_done(&engine).await;

    assert!(
        has_expansion_completed(&captured),
        "Enrichment should complete and trigger expansion even when deps are missing"
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

    assert!(
        has_expansion_completed(&captured),
        "ExpansionCompleted should fire — skipped response scrape still unlocks expansion"
    );
}

#[test]
fn enrichment_gate_requires_both_review_complete_and_response_done() {
    let mut state = PipelineState::default();
    state.source_plan = Some(crate::core::aggregate::SourcePlan {
        all_sources: vec![],
        selected_sources: vec![],
        tension_phase_keys: std::collections::HashSet::new(),
        response_phase_keys: std::collections::HashSet::new(),
        selected_keys: std::collections::HashSet::new(),
        consumed_pin_ids: Vec::new(),
    });

    // review done but response not done → gate blocked
    state.signals_awaiting_review = 1;
    state.signals_review_completed = 1;
    state.response_web_done = false;
    assert!(state.review_complete());
    assert!(!state.response_scrape_done());

    // response done but review not done → gate blocked
    state.signals_review_completed = 0;
    state.response_web_done = true;
    state.topic_discovery_done = true;
    assert!(!state.review_complete());
    assert!(state.response_scrape_done());

    // both done → gate open
    state.signals_review_completed = 1;
    assert!(state.review_complete());
    assert!(state.response_scrape_done());
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
        has_expansion_completed(&captured),
        "Expansion should fire — zero signals means review gate passes immediately"
    );
}

// ---------------------------------------------------------------------------
// Bug: AI unavailable → review never completes → pipeline stuck forever
// ---------------------------------------------------------------------------

#[tokio::test]
async fn no_ai_auto_accepts_signals_so_enrichment_proceeds() {
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
    // No AI configured — deps.ai is None
    let (engine, captured, _scope) = crate::testing::test_engine_with_scrape_capture(
        store as Arc<dyn crate::traits::SignalReader>,
        fetcher,
        extractor,
        Some(mpls_region()),
    );

    engine
        .emit(crate::testing::sources_prepared_with_web_urls(url))
        .settled()
        .await
        .unwrap();

    // Signal was created — awaiting_review incremented
    let state = engine.singleton::<PipelineState>();
    assert!(
        state.signals_awaiting_review > 0,
        "dedup should have created at least one signal"
    );

    // Without AI, review should auto-accept so the gate can open
    assert!(
        state.review_complete(),
        "Review must auto-complete when AI is unavailable ({} awaiting, {} completed)",
        state.signals_awaiting_review,
        state.signals_review_completed,
    );

    // Complete response scrape so enrichment gate can fire
    emit_response_scrape_done(&engine).await;

    assert!(
        has_expansion_completed(&captured),
        "Enrichment must proceed when AI is unavailable — auto-accept signals"
    );
}

// ---------------------------------------------------------------------------
// Signal review must fire even without a region
// ---------------------------------------------------------------------------

#[tokio::test]
async fn regionless_run_still_reviews_signals() {
    let url = "https://example.com/events";

    // Mock AI returns "pass" for any signal it reviews
    let ai_response = serde_json::json!({
        "verdicts": [],
        "run_analysis": null
    });
    let ai = Arc::new(MockAgent::with_response(ai_response));

    let harness = ScoutRunTest::new()
        // No .region() — this is a source-targeted run without geographic context
        .source(url, archived_page(url, "Community dinner at Powderhorn Park"))
        .extraction(url, crate::core::extractor::ExtractionResult {
            nodes: vec![tension_at("Community Dinner", 44.95, -93.27)],
            ..Default::default()
        })
        .ai(ai)
        .build();

    harness.run().await;

    let state = harness.state();
    assert!(
        state.signals_awaiting_review > 0,
        "dedup should have created at least one signal (awaiting={})",
        state.signals_awaiting_review,
    );
    assert!(
        state.review_complete(),
        "review should complete even without a region ({} awaiting, {} completed)",
        state.signals_awaiting_review,
        state.signals_review_completed,
    );
}
