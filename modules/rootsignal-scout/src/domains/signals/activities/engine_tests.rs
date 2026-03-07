//! Engine integration tests — full dispatch loop via seesaw engine.
//!
//! MOCK → ENGINE.EMIT → OUTPUT
//! Proves seesaw handlers compose correctly.

use std::collections::HashMap;
use std::sync::Arc;

use crate::core::aggregate::PipelineState;
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::signals::events::{DedupOutcome, SignalEvent};
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::enrichment::events::EnrichmentEvent;
use crate::core::aggregate::ExtractedBatch;
use crate::domains::scrape::events::ScrapeEvent;
use crate::domains::scrape::activities::UrlExtraction;
use crate::domains::enrichment::activities::link_promoter::CollectedLink;
use crate::testing::*;
use chrono::Utc;
use uuid::Uuid;
use rootsignal_common::events::{Eventlike, SystemEvent, WorldEvent};
use rootsignal_common::types::NodeType;
use seesaw_core::AnyEvent;

/// Seed source plan by emitting a SourcesPrepared event.
async fn seed_scrape_plan(engine: &seesaw_core::Engine<crate::core::engine::ScoutEngineDeps>, include_social: bool) {
    engine.emit(sources_prepared_event(include_social)).settled().await.unwrap();
}

/// Emit response completion events to mark response phase done.
async fn complete_response_roles(engine: &seesaw_core::Engine<crate::core::engine::ScoutEngineDeps>) {
    engine.emit(ScrapeEvent::from(TestWebScrapeCompleted::builder().is_tension(false).build())).settled().await.unwrap();
    engine.emit(empty_social_scrape(false)).settled().await.unwrap();
    engine.emit(empty_topic_discovery()).settled().await.unwrap();
}

/// Helper: collect event variant names from captured events.
fn event_names(captured: &Arc<std::sync::Mutex<Vec<AnyEvent>>>) -> Vec<String> {
    captured
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| {
            if let Some(le) = e.downcast_ref::<LifecycleEvent>() {
                Some(le.event_type_str())
            } else if let Some(se) = e.downcast_ref::<SignalEvent>() {
                Some(se.event_type_str())
            } else if let Some(de) = e.downcast_ref::<DiscoveryEvent>() {
                Some(de.event_type_str())
            } else if let Some(ee) = e.downcast_ref::<EnrichmentEvent>() {
                Some(ee.event_type_str())
            } else if let Some(we) = e.downcast_ref::<WorldEvent>() {
                Some(we.event_type().to_string())
            } else if let Some(se) = e.downcast_ref::<SystemEvent>() {
                Some(se.event_type().to_string())
            } else {
                Some("unknown".to_string())
            }
        })
        .collect()
}

/// Extract DedupOutcome verdicts from captured events.
fn captured_verdicts(captured: &Arc<std::sync::Mutex<Vec<AnyEvent>>>) -> Vec<DedupOutcome> {
    captured
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| e.downcast_ref::<SignalEvent>())
        .filter_map(|e| match e {
            SignalEvent::DedupCompleted { verdicts, .. } => Some(verdicts.clone()),
        })
        .flatten()
        .collect()
}

// ---------------------------------------------------------------------------
// Helper: build a WebScrapeCompleted carrying an extracted batch
// ---------------------------------------------------------------------------

fn scrape_completed_with_batch(url: &str, canonical_key: &str, batch: ExtractedBatch) -> ScrapeEvent {
    let signals = batch.nodes.len() as u32;
    TestWebScrapeCompleted::builder()
        .is_tension(true)
        .urls_scraped(1)
        .signals_extracted(signals)
        .extracted_batches(vec![UrlExtraction {
            url: url.to_string(),
            canonical_key: canonical_key.to_string(),
            batch,
        }])
        .build()
        .into()
}

// ---------------------------------------------------------------------------
// New signal via WebScrapeCompleted — full dedup + creation chain
// ---------------------------------------------------------------------------

#[tokio::test]
async fn new_signal_dispatches_full_event_chain() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    let node = tension_at("Free Legal Clinic", 44.9341, -93.2619);
    let meta_id = node.meta().unwrap().id;

    let mut signal_tags = HashMap::new();
    signal_tags.insert(meta_id, vec!["legal".to_string()]);

    let mut author_actors = HashMap::new();
    author_actors.insert(meta_id, "Local Legal Aid".to_string());

    let batch = ExtractedBatch {
        content: "page content about legal clinic".to_string(),
        nodes: vec![node],
        resource_tags: HashMap::new(),
        signal_tags,
        author_actors,
        source_id: None,
    };

    engine
        .emit(scrape_completed_with_batch(
            "https://www.instagram.com/locallegalaid",
            "instagram.com/locallegalaid",
            batch,
        ))
        .settled()
        .await
        .unwrap();

    let state = engine.singleton::<PipelineState>();

    // Reducer counted the create
    assert_eq!(state.stats.signals_stored, 1);

    let names = event_names(&captured);

    // Actor events emitted directly by dedup handler
    assert!(
        names.iter().any(|n| n == "actor_identified"),
        "expected ActorIdentified event, got: {names:?}"
    );

    assert!(
        names.iter().any(|n| n == "actor_linked_to_signal"),
        "expected ActorLinkedToSignal event, got: {names:?}"
    );

    // Created verdict carries signal_tags
    let vs = captured_verdicts(&captured);
    let created = vs.iter().find(|v| matches!(v, DedupOutcome::Created { .. }));
    assert!(created.is_some(), "expected Created verdict");
    match created.unwrap() {
        DedupOutcome::Created { signal_tags, actor, .. } => {
            assert_eq!(signal_tags, &["legal".to_string()]);
            assert!(actor.is_some(), "expected resolved actor on verdict");
        }
        _ => unreachable!(),
    }
}

// ---------------------------------------------------------------------------
// Cross-source match via dedup — corroboration events
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cross_source_match_dispatches_citation_and_scoring_events() {
    let store = Arc::new(MockSignalReader::new());
    // Pre-populate: "Community Dinner" exists at a DIFFERENT source
    store.insert_signal(
        "Community Dinner",
        NodeType::Concern,
        "https://other-source.org/events",
    );
    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    let batch = ExtractedBatch {
        content: "page content".to_string(),
        nodes: vec![tension_at("Community Dinner", 44.95, -93.27)],
        resource_tags: HashMap::new(),
        signal_tags: HashMap::new(),
        author_actors: HashMap::new(),
        source_id: None,
    };

    engine
        .emit(scrape_completed_with_batch(
            "https://example.org/events",
            "example.org",
            batch,
        ))
        .settled()
        .await
        .unwrap();

    // Reducer counted the dedup
    let state = engine.singleton::<PipelineState>();
    assert_eq!(state.stats.signals_deduplicated, 1);

    let names = event_names(&captured);
    // Dedup emits CitationPublished + ObservationCorroborated + CorroborationScored directly
    assert!(names.contains(&"citation_published".to_string()), "expected CitationPublished, got: {names:?}");
    assert!(names.contains(&"observation_corroborated".to_string()), "expected ObservationCorroborated, got: {names:?}");
    assert!(names.contains(&"corroboration_scored".to_string()), "expected CorroborationScored, got: {names:?}");

    // Corroborated verdict
    let vs = captured_verdicts(&captured);
    assert!(vs.iter().any(|v| matches!(v, DedupOutcome::Corroborated { .. })));
}

// ---------------------------------------------------------------------------
// Same-source reencounter via dedup — no freshness events
// ---------------------------------------------------------------------------

#[tokio::test]
async fn same_source_reencounter_emits_no_freshness_events() {
    let store = Arc::new(MockSignalReader::new());
    store.insert_signal(
        "Community Dinner",
        NodeType::Concern,
        "https://example.org/events",
    );
    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    let batch = ExtractedBatch {
        content: "page content".to_string(),
        nodes: vec![tension_at("Community Dinner", 44.95, -93.27)],
        resource_tags: HashMap::new(),
        signal_tags: HashMap::new(),
        author_actors: HashMap::new(),
        source_id: None,
    };

    engine
        .emit(scrape_completed_with_batch(
            "https://example.org/events",
            "example.org",
            batch,
        ))
        .settled()
        .await
        .unwrap();

    let state = engine.singleton::<PipelineState>();
    assert_eq!(state.stats.signals_deduplicated, 1);

    let names = event_names(&captured);
    assert!(!names.contains(&"citation_published".to_string()), "refresh should not emit CitationPublished, got: {names:?}");
    assert!(!names.contains(&"freshness_confirmed".to_string()), "refresh should not emit FreshnessConfirmed, got: {names:?}");
}

// ---------------------------------------------------------------------------
// WebScrapeCompleted with batch — dedup + creation chain
// ---------------------------------------------------------------------------

#[tokio::test]
async fn scrape_completed_dispatches_dedup_and_creation_chain() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    let node = tension_at("Free Legal Clinic", 44.9341, -93.2619);

    let batch = ExtractedBatch {
        content: "page content about legal clinic".to_string(),
        nodes: vec![node],
        resource_tags: HashMap::new(),
        signal_tags: HashMap::new(),
        author_actors: HashMap::new(),
        source_id: None,
    };

    engine
        .emit(scrape_completed_with_batch(
            "https://localorg.org/events",
            "localorg.org",
            batch,
        ))
        .settled()
        .await
        .unwrap();

    let state = engine.singleton::<PipelineState>();

    // Reducer counted creation
    assert_eq!(state.stats.signals_stored, 1);

    let names = event_names(&captured);

    // DedupCompleted emitted with Created verdict
    assert!(
        names.iter().any(|n| n == "signal:dedup_completed"),
        "expected DedupCompleted, got: {names:?}"
    );
    let vs = captured_verdicts(&captured);
    assert!(
        vs.iter().any(|v| matches!(v, DedupOutcome::Created { .. })),
        "expected Created verdict"
    );
}

// ---------------------------------------------------------------------------
// WebScrapeCompleted with existing title — dedup reencounter path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn same_source_title_match_counts_dedup_without_events() {
    let store = Arc::new(MockSignalReader::new());
    store.insert_signal(
        "Community Dinner",
        NodeType::Concern,
        "https://example.org/events",
    );
    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    let batch = ExtractedBatch {
        content: "page content".to_string(),
        nodes: vec![tension_at("Community Dinner", 44.95, -93.27)],
        resource_tags: HashMap::new(),
        signal_tags: HashMap::new(),
        author_actors: HashMap::new(),
        source_id: None,
    };

    engine
        .emit(scrape_completed_with_batch(
            "https://example.org/events",
            "example.org",
            batch,
        ))
        .settled()
        .await
        .unwrap();

    let state = engine.singleton::<PipelineState>();
    assert_eq!(state.stats.signals_deduplicated, 1);
    assert_eq!(state.stats.signals_stored, 0);

    let names = event_names(&captured);
    assert!(!names.contains(&"freshness_confirmed".to_string()), "refresh should not emit FreshnessConfirmed, got: {names:?}");
}

// ---------------------------------------------------------------------------
// Link promotion handler — tension roles complete promotes collected links
// ---------------------------------------------------------------------------

#[tokio::test]
async fn link_promotion_promotes_links_on_phase_completed() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );
    seed_scrape_plan(&engine, false).await;
    captured.lock().unwrap().clear();

    // WebScrapeCompleted(TensionWeb) with links → promotion fires immediately
    engine
        .emit(ScrapeEvent::from(TestWebScrapeCompleted::builder()
            .is_tension(true)
            .collected_links(vec![
                CollectedLink {
                    url: "https://instagram.com/mutual_aid_mpls".to_string(),
                    discovered_on: "https://localorg.org".to_string(),
                },
                CollectedLink {
                    url: "https://twitter.com/mpls_community".to_string(),
                    discovered_on: "https://localorg.org".to_string(),
                },
            ])
            .build()))
        .settled()
        .await
        .unwrap();

    let names = event_names(&captured);

    // SourcesRegistered batch emitted for promoted links (via domain_filter chokepoint)
    assert!(
        names.iter().any(|n| n.contains("sources_registered")),
        "expected SourcesRegistered, got: {names:?}"
    );

    // Reducer cleared collected_links after link promotion
    let state = engine.singleton::<PipelineState>();
    assert!(
        state.collected_links.is_empty(),
        "collected_links should be cleared after link promotion"
    );
}

// ---------------------------------------------------------------------------
// Link promotion handler — skips when no links collected
// ---------------------------------------------------------------------------

#[tokio::test]
async fn link_promotion_skips_when_no_links() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    seed_scrape_plan(&engine, false).await;
    captured.lock().unwrap().clear();

    // Emit TensionWeb completion with no links — filter short-circuits on is_empty
    engine
        .emit(ScrapeEvent::from(TestWebScrapeCompleted::builder()
            .is_tension(true)
            .build()))
        .settled()
        .await
        .unwrap();

    let names = event_names(&captured);

    assert!(
        !names.iter().any(|n| n.contains("sources_registered")),
        "should not emit SourcesRegistered with no links, got: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// Page triage: social handles always promoted regardless of signal count
// ---------------------------------------------------------------------------

#[tokio::test]
async fn social_handles_always_promoted_from_zero_signal_pages() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );
    seed_scrape_plan(&engine, false).await;
    captured.lock().unwrap().clear();

    engine
        .emit(ScrapeEvent::from(TestWebScrapeCompleted::builder()
            .is_tension(true)
            .urls_scraped(1)
            .collected_links(vec![
                CollectedLink {
                    url: "https://instagram.com/mpls_mutual_aid".to_string(),
                    discovered_on: "https://hub-page.org".to_string(),
                },
                CollectedLink {
                    url: "https://x.com/mpls_help".to_string(),
                    discovered_on: "https://hub-page.org".to_string(),
                },
            ])
            .build()))
        .settled()
        .await
        .unwrap();

    let names = event_names(&captured);
    assert!(
        names.iter().any(|n| n.contains("sources_registered")),
        "social handles should be promoted even from zero-signal pages, got: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// Page triage: productive page content links promoted without AI triage
// ---------------------------------------------------------------------------

#[tokio::test]
async fn productive_page_content_links_promoted_without_ai_triage() {
    let store = Arc::new(MockSignalReader::new());
    // No AI — content links from productive pages should still be promoted
    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );
    seed_scrape_plan(&engine, false).await;
    captured.lock().unwrap().clear();

    let mut signal_counts = HashMap::new();
    signal_counts.insert("https://hub-page.org/resources".to_string(), 3u32);

    engine
        .emit(ScrapeEvent::from(TestWebScrapeCompleted::builder()
            .is_tension(true)
            .urls_scraped(1)
            .signals_extracted(3)
            .source_signal_counts(signal_counts)
            .collected_links(vec![
                CollectedLink {
                    url: "https://partner-org.org/programs".to_string(),
                    discovered_on: "https://hub-page.org/resources".to_string(),
                },
                CollectedLink {
                    url: "https://foodshelf.org/volunteer".to_string(),
                    discovered_on: "https://hub-page.org/resources".to_string(),
                },
            ])
            .build()))
        .settled()
        .await
        .unwrap();

    let names = event_names(&captured);
    assert!(
        names.iter().any(|n| n.contains("sources_registered")),
        "content links from productive page should be promoted, got: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// Page triage: zero-signal page content links NOT promoted without AI
// ---------------------------------------------------------------------------

#[tokio::test]
async fn zero_signal_page_content_links_not_promoted_without_ai() {
    let store = Arc::new(MockSignalReader::new());
    // No AI configured — zero-signal pages should be fail-closed
    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );
    seed_scrape_plan(&engine, false).await;
    captured.lock().unwrap().clear();

    engine
        .emit(ScrapeEvent::from(TestWebScrapeCompleted::builder()
            .is_tension(true)
            .urls_scraped(1)
            .collected_links(vec![
                CollectedLink {
                    url: "https://partner-org.org/programs".to_string(),
                    discovered_on: "https://empty-page.org".to_string(),
                },
            ])
            .build()))
        .settled()
        .await
        .unwrap();

    let names = event_names(&captured);
    let source_count = names.iter().filter(|n| n.contains("sources_registered")).count();
    assert_eq!(
        source_count, 0,
        "content links from zero-signal page should NOT be promoted without AI, got: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// Page triage: LLM approves zero-signal page → content links promoted
// ---------------------------------------------------------------------------

#[tokio::test]
async fn zero_signal_page_triaged_and_promoted() {
    use crate::testing::MockAgent;
    let store = Arc::new(MockSignalReader::new());

    // Mock AI returns "relevant: true" for the zero-signal page
    let ai = Arc::new(MockAgent::with_response(serde_json::json!({
        "pages": [
            { "url": "https://directory-page.org/links", "relevant": true, "reason": "resource directory" }
        ]
    })));

    let (engine, captured, _scope) = test_engine_with_ai(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        ai,
        Some(mpls_region()),
    );
    seed_scrape_plan(&engine, false).await;
    captured.lock().unwrap().clear();

    engine
        .emit(ScrapeEvent::from(TestWebScrapeCompleted::builder()
            .is_tension(true)
            .urls_scraped(1)
            .collected_links(vec![
                CollectedLink {
                    url: "https://partner-org.org/programs".to_string(),
                    discovered_on: "https://directory-page.org/links".to_string(),
                },
                CollectedLink {
                    url: "https://foodshelf.org/volunteer".to_string(),
                    discovered_on: "https://directory-page.org/links".to_string(),
                },
            ])
            .page_previews(HashMap::from([(
                "https://directory-page.org/links".to_string(),
                "Community Resources Directory: links to local orgs".to_string(),
            )]))
            .build()))
        .settled()
        .await
        .unwrap();

    let names = event_names(&captured);

    assert!(
        names.iter().any(|n| n.contains("sources_registered")),
        "content links from triage-passed page should be promoted, got: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// Page triage: LLM rejects zero-signal page → content links NOT promoted
// ---------------------------------------------------------------------------

#[tokio::test]
async fn zero_signal_page_triaged_and_rejected() {
    use crate::testing::MockAgent;
    let store = Arc::new(MockSignalReader::new());

    // Mock AI returns "relevant: false"
    let ai = Arc::new(MockAgent::with_response(serde_json::json!({
        "pages": [
            { "url": "https://news-article.com/story", "relevant": false, "reason": "news article, links to other articles" }
        ]
    })));

    let (engine, captured, _scope) = test_engine_with_ai(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        ai,
        Some(mpls_region()),
    );
    seed_scrape_plan(&engine, false).await;
    captured.lock().unwrap().clear();

    engine
        .emit(ScrapeEvent::from(TestWebScrapeCompleted::builder()
            .is_tension(true)
            .urls_scraped(1)
            .collected_links(vec![
                CollectedLink {
                    url: "https://other-news.com/article".to_string(),
                    discovered_on: "https://news-article.com/story".to_string(),
                },
            ])
            .page_previews(HashMap::from([(
                "https://news-article.com/story".to_string(),
                "Breaking: Local politics debate continues...".to_string(),
            )]))
            .build()))
        .settled()
        .await
        .unwrap();

    let names = event_names(&captured);

    let source_count = names.iter().filter(|n| n.contains("sources_registered")).count();
    assert_eq!(
        source_count, 0,
        "content links from rejected page should NOT be promoted, got: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// Page triage: AI error fails closed (no content links promoted from triage)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ai_error_fails_closed_no_content_links_promoted_from_triage() {
    use crate::testing::MockAgent;
    let store = Arc::new(MockSignalReader::new());

    // Mock AI configured to fail
    let ai = Arc::new(MockAgent::failing());

    let (engine, captured, _scope) = test_engine_with_ai(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        ai,
        Some(mpls_region()),
    );
    seed_scrape_plan(&engine, false).await;
    captured.lock().unwrap().clear();

    engine
        .emit(ScrapeEvent::from(TestWebScrapeCompleted::builder()
            .is_tension(true)
            .urls_scraped(1)
            .collected_links(vec![
                CollectedLink {
                    url: "https://partner-org.org/programs".to_string(),
                    discovered_on: "https://zero-signal-page.org".to_string(),
                },
            ])
            .page_previews(HashMap::from([(
                "https://zero-signal-page.org".to_string(),
                "Some page content".to_string(),
            )]))
            .build()))
        .settled()
        .await
        .unwrap();

    let names = event_names(&captured);

    // No content links promoted
    let source_count = names.iter().filter(|n| n.contains("sources_registered")).count();
    assert_eq!(
        source_count, 0,
        "AI error should fail closed — no content links promoted, got: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// Page triage: content links capped at max per source
// ---------------------------------------------------------------------------

#[tokio::test]
async fn content_links_capped_per_source() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );
    seed_scrape_plan(&engine, false).await;
    captured.lock().unwrap().clear();

    // Create 15 content links from a productive page — should be capped at 10
    let collected_links: Vec<CollectedLink> = (0..15)
        .map(|i| CollectedLink {
            url: format!("https://site-{i}.org/page"),
            discovered_on: "https://productive.org/resources".to_string(),
        })
        .collect();

    engine
        .emit(ScrapeEvent::from(TestWebScrapeCompleted::builder()
            .is_tension(true)
            .urls_scraped(1)
            .signals_extracted(5)
            .source_signal_counts(HashMap::from([("https://productive.org/resources".to_string(), 5u32)]))
            .collected_links(collected_links)
            .build()))
        .settled()
        .await
        .unwrap();

    let names = event_names(&captured);
    let source_count = names.iter().filter(|n| n.contains("sources_registered")).count();
    assert!(
        source_count <= 10,
        "content links should be capped at 10 per source, got {source_count}: {names:?}"
    );
    assert!(
        source_count > 0,
        "some content links should be promoted from productive page, got: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// Page previews cleared after link promotion
// ---------------------------------------------------------------------------

#[tokio::test]
async fn page_previews_cleared_after_link_promotion() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, _captured, _scope) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );
    seed_scrape_plan(&engine, false).await;

    // WebScrapeCompleted with links + page_previews → promotion fires → previews cleared
    engine
        .emit(ScrapeEvent::from(TestWebScrapeCompleted::builder()
            .is_tension(true)
            .urls_scraped(1)
            .collected_links(vec![
                CollectedLink {
                    url: "https://instagram.com/test_handle".to_string(),
                    discovered_on: "https://page.org".to_string(),
                },
            ])
            .page_previews(HashMap::from([(
                "https://page.org".to_string(),
                "some preview".to_string(),
            )]))
            .build()))
        .settled()
        .await
        .unwrap();

    let state = engine.singleton::<PipelineState>();
    assert!(
        state.page_previews.is_empty(),
        "page_previews should be cleared after link promotion"
    );
}

// ---------------------------------------------------------------------------
// Actor location handler — emits location events on ResponseScrape complete
// ---------------------------------------------------------------------------

#[tokio::test]
async fn actor_location_emits_events_on_response_complete() {

    let store = Arc::new(MockSignalReader::new());

    // Create an actor with no location
    let actor = rootsignal_common::ActorNode {
        id: uuid::Uuid::new_v4(),
        name: "Phillips Org".to_string(),
        actor_type: rootsignal_common::ActorType::Organization,
        canonical_key: "phillips-org-entity".to_string(),
        domains: vec![],
        social_urls: vec![],
        description: String::new(),
        signal_count: 0,
        first_seen: Utc::now(),
        last_active: Utc::now(),
        typical_roles: vec![],
        bio: None,
        location_lat: None,
        location_lng: None,
        location_name: None,
        discovery_depth: 0,
    };
    store.upsert_actor(&actor).await.unwrap();

    // Seed 2 signals at Phillips — enough for triangulation to produce a location
    let node1 = {
        let mut n = tension_at("Community Event A", 44.9489, -93.2601);
        if let Some(meta) = n.meta_mut() {
            meta.about_location_name = Some("Phillips".to_string());
        }
        n
    };
    let sig1 = store
        .create_node(&node1, &[0.1], "test", "run-1")
        .await
        .unwrap();
    store
        .link_actor_to_signal(actor.id, sig1, "authored")
        .await
        .unwrap();

    let node2 = {
        let mut n = tension_at("Community Event B", 44.9489, -93.2601);
        if let Some(meta) = n.meta_mut() {
            meta.about_location_name = Some("Phillips".to_string());
        }
        n
    };
    let sig2 = store
        .create_node(&node2, &[0.2], "test", "run-1")
        .await
        .unwrap();
    store
        .link_actor_to_signal(actor.id, sig2, "authored")
        .await
        .unwrap();

    // Build engine with this store so the handler can read actors + signals
    let (engine, captured, _scope) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );
    seed_scrape_plan(&engine, false).await;

    complete_response_roles(&engine).await;

    let names = event_names(&captured);

    // ActorLocationIdentified event emitted
    assert!(
        names.contains(&"actor_location_identified".to_string()),
        "expected ActorLocationIdentified, got: {names:?}"
    );

    // ActorsLocated enrichment fact emitted
    assert!(
        names.iter().any(|n| n.contains("actors_located")),
        "expected ActorsLocated, got: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// Full pipeline: SERP query → linktree pages → scrape → extract social links
// ---------------------------------------------------------------------------

#[tokio::test]
async fn serp_query_resolves_and_extracts_social_links_from_linktree_pages() {
    use crate::domains::scrape::events::ScrapeEvent;

    let store = Arc::new(MockSignalReader::new());
    let query = "site:linktr.ee mutual aid Minneapolis";
    let result_urls = [
        "https://linktr.ee/mpls_mutual_aid",
        "https://linktr.ee/northside_aid",
        "https://linktr.ee/south_mpls_community",
    ];

    // MOCK: fetcher — SERP returns linktree URLs, each page has outbound social links
    let fetcher = MockFetcher::new()
        .on_search(query, search_results(query, &result_urls))
        .on_page(
            "https://linktr.ee/mpls_mutual_aid",
            rootsignal_common::ArchivedPage {
                id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                fetched_at: Utc::now(),
                content_hash: "hash-0".into(),
                raw_html: String::new(),
                markdown: "# Minneapolis Mutual Aid\n\n\
                           Food, housing, and legal help for our community.\n\n\
                           Sign up for a food shelf slot or request housing assistance."
                    .into(),
                title: Some("MPLS Mutual Aid".into()),
                links: vec![
                    "https://www.instagram.com/mpls_mutual_aid".into(),
                    "https://twitter.com/mplsmutualaid".into(),
                    "https://www.givemn.org/mpls-mutual-aid".into(),
                ],
                published_at: None,
            },
        )
        .on_page(
            "https://linktr.ee/northside_aid",
            rootsignal_common::ArchivedPage {
                id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                fetched_at: Utc::now(),
                content_hash: "hash-1".into(),
                raw_html: String::new(),
                markdown: "# Northside Aid Network\n\n\
                           Supporting North Minneapolis families with groceries and supplies."
                    .into(),
                title: Some("Northside Aid".into()),
                links: vec![
                    "https://www.instagram.com/northside_aid".into(),
                    "https://linktr.ee/northside_aid/about".into(),
                ],
                published_at: None,
            },
        )
        .on_page(
            "https://linktr.ee/south_mpls_community",
            rootsignal_common::ArchivedPage {
                id: Uuid::new_v4(),
                source_id: Uuid::new_v4(),
                fetched_at: Utc::now(),
                content_hash: "hash-2".into(),
                raw_html: String::new(),
                markdown: "# South MPLS Community Resources\n\n\
                           Mutual aid collective serving South Minneapolis neighborhoods."
                    .into(),
                title: Some("South MPLS Community".into()),
                links: vec![
                    "https://www.instagram.com/southmpls_community".into(),
                ],
                published_at: None,
            },
        );

    // MOCK: AI agent — returns a realistic extraction response.
    // The real Extractor constructs the prompt and parses this response.
    // Same response for all pages (dedup handles duplicates).
    let ai = Arc::new(MockAgent::with_response(serde_json::json!({
        "signals": [{
            "signal_type": "resource",
            "title": "Community mutual aid program",
            "summary": "Local mutual aid network providing food and housing assistance",
            "sensitivity": "general",
            "latitude": 44.9778,
            "longitude": -93.2650,
            "location_name": "Minneapolis",
            "is_ongoing": true
        }]
    })));

    // ENGINE: source-targeted run — real Extractor, real link promotion, real dispatch
    let (engine, captured, scope) = test_engine_for_source_run(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        vec![web_query_source(query)],
        Arc::new(fetcher),
        ai,
    );

    // INPUT: one event kicks off the entire causal chain
    engine
        .emit(LifecycleEvent::ScoutRunRequested {
            run_id: Uuid::new_v4(),
            scope,
        })
        .settled()
        .await
        .unwrap();

    // OUTPUT
    let names = event_names(&captured);
    let events = captured.lock().unwrap();

    // 1. SERP resolved all 3 linktree URLs (carried on SourcesPrepared)
    let resolved_urls = events
        .iter()
        .filter_map(|e| e.downcast_ref::<LifecycleEvent>())
        .find_map(|e| match e {
            LifecycleEvent::SourcesPrepared { web_urls, .. } => Some(web_urls.clone()),
            _ => None,
        })
        .expect("should emit SourcesPrepared with web_urls");
    assert_eq!(
        resolved_urls.len(),
        3,
        "SERP should resolve all 3 linktree URLs"
    );

    // 2. All 3 pages scraped
    let urls_scraped = events
        .iter()
        .filter_map(|e| e.downcast_ref::<ScrapeEvent>())
        .find_map(|e| match e {
            ScrapeEvent::WebScrapeCompleted {
                is_tension: true,
                urls_scraped,
                ..
            } => Some(*urls_scraped),
            _ => None,
        })
        .expect("should emit WebScrapeCompleted(tension)");
    assert_eq!(urls_scraped, 3, "should scrape all 3 linktree pages");

    // 3. Social handles extracted from page links and promoted as sources
    //    Sources flow through SourcesDiscovered → domain_filter → SourcesRegistered
    let promoted_urls: Vec<String> = events
        .iter()
        .filter_map(|e| e.downcast_ref::<SystemEvent>())
        .filter_map(|e| match e {
            SystemEvent::SourcesRegistered { sources } => {
                Some(sources.iter().filter_map(|s| s.url.clone()).collect::<Vec<_>>())
            }
            _ => None,
        })
        .flatten()
        .collect();

    assert!(
        promoted_urls
            .iter()
            .any(|u| u.contains("instagram.com/mpls_mutual_aid")),
        "instagram.com/mpls_mutual_aid should be promoted from linktree page, got: {promoted_urls:?}"
    );
    assert!(
        promoted_urls
            .iter()
            .any(|u| u.contains("instagram.com/northside_aid")),
        "instagram.com/northside_aid should be promoted from linktree page, got: {promoted_urls:?}"
    );
    assert!(
        promoted_urls
            .iter()
            .any(|u| u.contains("instagram.com/southmpls_community")),
        "instagram.com/southmpls_community should be promoted from linktree page, got: {promoted_urls:?}"
    );
    assert!(
        promoted_urls.iter().any(|u| u.contains("x.com/mplsmutualaid")),
        "x.com/mplsmutualaid should be promoted from linktree page, got: {promoted_urls:?}"
    );

    // 4. Pipeline state: signals extracted and stored
    drop(events);
    let state = engine.singleton::<PipelineState>();
    assert!(
        state.stats.signals_stored > 0,
        "should store at least one signal from scraped pages (stored: {})",
        state.stats.signals_stored,
    );

    // 6. Full chain settled — signals stored confirms pipeline ran to completion
    assert!(
        state.stats.signals_extracted > 0,
        "pipeline should extract signals (extracted: {})",
        state.stats.signals_extracted,
    );
}
