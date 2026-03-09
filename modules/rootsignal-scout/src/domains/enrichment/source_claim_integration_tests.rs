//! Integration tests for source claiming during enrichment.
//!
//! MOCK → ENGINE.EMIT → OUTPUT
//! Proves that run_enrichment calls claim_profile_sources and emits
//! the correct world events and discovery events.

use std::sync::Arc;

use rootsignal_common::canonical_value;
use rootsignal_common::events::WorldEvent;

use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::scrape::events::ScrapeEvent;
use crate::testing::*;
use seesaw_core::AnyEvent;

/// Emit response scrape completion events to satisfy `response_scrape_done()`.
async fn emit_response_scrape_done(engine: &seesaw_core::Engine<crate::core::engine::ScoutEngineDeps>) {
    engine.emit(ScrapeEvent::from(TestWebScrapeCompleted::builder().is_tension(false).build())).settled().await.unwrap();
    engine.emit(empty_social_scrape(false)).settled().await.unwrap();
    engine.emit(empty_topic_discovery()).settled().await.unwrap();
}

fn has_actor_linked_to_source(captured: &Arc<std::sync::Mutex<Vec<AnyEvent>>>) -> bool {
    captured.lock().unwrap().iter().any(|e| {
        e.downcast_ref::<WorldEvent>()
            .is_some_and(|we| matches!(we, WorldEvent::ActorLinkedToSource { .. }))
    })
}

fn has_sources_discovered(captured: &Arc<std::sync::Mutex<Vec<AnyEvent>>>) -> bool {
    captured.lock().unwrap().iter().any(|e| {
        e.downcast_ref::<DiscoveryEvent>()
            .is_some_and(|de| matches!(de, DiscoveryEvent::SourcesDiscovered { .. }))
    })
}

#[tokio::test]
async fn external_url_matching_known_source_emits_link_event() {
    let fb_ck = canonical_value("https://www.facebook.com/sanctuarysupply");
    let fb_source = make_source("https://www.facebook.com/sanctuarysupply", &fb_ck);
    let ig_source = make_source("https://www.instagram.com/sanctuarysupply", "instagram.com/sanctuarysupply");

    let actor = {
        let mut a = actor_with_external_url(
            "Sanctuary Supply",
            "instagram.com/sanctuarysupply",
            "https://www.facebook.com/sanctuarysupply",
        );
        a
    };

    let store = Arc::new(
        MockSignalReader::new()
            .add_source(fb_source.clone())
            .add_source(ig_source.clone())
            .with_actor(actor.clone(), vec![ig_source]),
    );

    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store as Arc<dyn crate::traits::SignalReader>,
        None,
    );

    engine.emit(sources_prepared_event(false)).settled().await.unwrap();
    emit_response_scrape_done(&engine).await;

    assert!(
        has_actor_linked_to_source(&captured),
        "enrichment should emit ActorLinkedToSource when external_url matches a known source"
    );
    assert!(
        !has_sources_discovered(&captured),
        "should NOT emit SourcesDiscovered — source already exists"
    );
}

#[tokio::test]
async fn external_url_unknown_site_emits_link_and_discovery() {
    let ig_source = make_source("https://www.instagram.com/sanctuarysupply", "instagram.com/sanctuarysupply");

    let actor = actor_with_external_url(
        "Sanctuary Supply",
        "instagram.com/sanctuarysupply",
        "https://www.sanctuarysupply.org",
    );

    let store = Arc::new(
        MockSignalReader::new()
            .add_source(ig_source.clone())
            .with_actor(actor.clone(), vec![ig_source]),
    );

    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store as Arc<dyn crate::traits::SignalReader>,
        None,
    );

    engine.emit(sources_prepared_event(false)).settled().await.unwrap();
    emit_response_scrape_done(&engine).await;

    assert!(
        has_actor_linked_to_source(&captured),
        "enrichment should emit ActorLinkedToSource for the new source"
    );
    assert!(
        has_sources_discovered(&captured),
        "enrichment should emit SourcesDiscovered for the unknown external URL"
    );
}

#[tokio::test]
async fn actor_without_external_url_emits_no_claim_events() {
    let ig_source = make_source("https://www.instagram.com/sanctuarysupply", "instagram.com/sanctuarysupply");

    let actor = actor_without_external_url(
        "Sanctuary Supply",
        "instagram.com/sanctuarysupply",
    );

    let store = Arc::new(
        MockSignalReader::new()
            .add_source(ig_source.clone())
            .with_actor(actor, vec![ig_source]),
    );

    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store as Arc<dyn crate::traits::SignalReader>,
        None,
    );

    engine.emit(sources_prepared_event(false)).settled().await.unwrap();
    emit_response_scrape_done(&engine).await;

    assert!(
        !has_actor_linked_to_source(&captured),
        "should not emit any link events when actor has no external_url"
    );
}
