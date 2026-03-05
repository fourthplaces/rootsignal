//! Engine integration tests — full dispatch loop via seesaw engine.
//!
//! MOCK → ENGINE.EMIT → OUTPUT
//! Proves seesaw handlers compose correctly.

use std::collections::HashMap;
use std::sync::Arc;

use crate::core::aggregate::PipelineState;
use crate::core::events::PipelinePhase;
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::signals::events::SignalEvent;
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::enrichment::events::EnrichmentEvent;
use crate::core::aggregate::{ExtractedBatch, PendingNode};
use crate::domains::enrichment::activities::link_promoter::CollectedLink;
use crate::testing::*;
use chrono::Utc;
use uuid::Uuid;
use rootsignal_common::events::{Eventlike, SystemEvent, WorldEvent};
use rootsignal_common::types::NodeType;
use seesaw_core::AnyEvent;

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

// ---------------------------------------------------------------------------
// NewSignalAccepted dispatch — full chain
// ---------------------------------------------------------------------------

#[tokio::test]
async fn new_signal_accepted_dispatches_full_event_chain() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    let node = tension_at("Free Legal Clinic", 44.9341, -93.2619);
    let node_id = node.id();

    let pn = PendingNode {
        node,
        content_hash: "abc123".to_string(),
        resource_tags: vec![],
        signal_tags: vec!["legal".to_string()],
        author_name: Some("Local Legal Aid".to_string()),
        source_id: None,
    };

    engine
        .emit(SignalEvent::NewSignalAccepted {
            node_id,
            node_type: NodeType::Concern,
            title: "Free Legal Clinic".to_string(),
            source_url: "https://www.instagram.com/locallegalaid".to_string(),
            pending_node: Box::new(pn),
        })
        .settled()
        .await
        .unwrap();

    let state = engine.singleton::<PipelineState>();

    // Reducer counted the create
    assert_eq!(state.stats.signals_stored, 1);

    let names = event_names(&captured);

    // Tags emitted via SignalTagged event
    assert!(
        names.iter().any(|n| n == "signal_tagged"),
        "expected SignalTagged event, got: {names:?}"
    );

    // Author actor emitted via ActorIdentified event
    assert!(
        names.iter().any(|n| n == "actor_identified"),
        "expected ActorIdentified event, got: {names:?}"
    );

    // PendingNode consumed by reducer on SignalCreated
    assert!(!state.pending_nodes.contains_key(&node_id));
}

// ---------------------------------------------------------------------------
// CrossSourceMatchDetected dispatch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cross_source_match_dispatches_citation_and_scoring_events() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    let existing_id = uuid::Uuid::new_v4();

    engine
        .emit(SignalEvent::CrossSourceMatchDetected {
            existing_id,
            node_type: NodeType::Concern,
            source_url: "https://org-b.org/events".to_string(),
            similarity: 0.95,
        })
        .settled()
        .await
        .unwrap();

    // Reducer counted the dedup
    let state = engine.singleton::<PipelineState>();
    assert_eq!(state.stats.signals_deduplicated, 1);

    let names = event_names(&captured);
    // CrossSourceMatchDetected → CitationPublished + ObservationCorroborated + CorroborationScored
    assert_eq!(names.len(), 4, "expected 4 events, got: {names:?}");
    assert!(names.contains(&"citation_published".to_string()));
    assert!(names.contains(&"observation_corroborated".to_string()));
    assert!(names.contains(&"corroboration_scored".to_string()));
}

// ---------------------------------------------------------------------------
// SameSourceReencountered dispatch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn same_source_reencountered_dispatches_citation_and_freshness() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    let existing_id = uuid::Uuid::new_v4();

    engine
        .emit(SignalEvent::SameSourceReencountered {
            existing_id,
            node_type: NodeType::Gathering,
            source_url: "https://example.org".to_string(),
            similarity: 1.0,
        })
        .settled()
        .await
        .unwrap();

    // Reducer counted the dedup
    let state = engine.singleton::<PipelineState>();
    assert_eq!(state.stats.signals_deduplicated, 1);

    let names = event_names(&captured);
    // SameSourceReencountered → CitationPublished + FreshnessConfirmed
    assert_eq!(names.len(), 3, "expected 3 events, got: {names:?}");
    assert!(names.contains(&"citation_published".to_string()));
    assert!(names.contains(&"freshness_confirmed".to_string()));
}

// ---------------------------------------------------------------------------
// SignalsExtracted → full dispatch chain through dedup + creation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn signals_extracted_dispatches_dedup_and_creation_chain() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured) = test_engine_with_capture_for_store(
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

    // Dispatch SignalEvent::SignalsExtracted — the engine does the rest
    engine
        .emit(SignalEvent::SignalsExtracted {
            url: "https://localorg.org/events".to_string(),
            canonical_key: "localorg.org".to_string(),
            count: 1,
            batch: Box::new(batch),
        })
        .settled()
        .await
        .unwrap();

    let state = engine.singleton::<PipelineState>();

    // Reducer counted extraction + creation
    assert_eq!(state.stats.signals_extracted, 1);
    assert_eq!(state.stats.signals_stored, 1);

    // Pending nodes cleaned up by reducer on SignalCreated
    assert!(state.pending_nodes.is_empty());

    // Wiring contexts stay until end of run
    assert!(!state.wiring_contexts.is_empty());

    let names = event_names(&captured);

    // Root: SignalsExtracted
    assert_eq!(names[0], "signal:signals_extracted");

    // NewSignalAccepted somewhere in chain
    assert!(
        names.contains(&"signal:new_signal_accepted".to_string()),
        "expected NewSignalAccepted, got: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// SignalsExtracted with existing title → dedup reencounter path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn signals_extracted_with_existing_title_emits_reencounter() {
    let store = Arc::new(MockSignalReader::new());
    // Pre-populate: "Community Dinner" exists at same URL
    store.insert_signal(
        "Community Dinner",
        NodeType::Concern,
        "https://example.org/events",
    );
    let (engine, captured) = test_engine_with_capture_for_store(
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
        .emit(SignalEvent::SignalsExtracted {
            url: "https://example.org/events".to_string(),
            canonical_key: "example.org".to_string(),
            count: 1,
            batch: Box::new(batch),
        })
        .settled()
        .await
        .unwrap();

    let state = engine.singleton::<PipelineState>();

    // Reducer counted extraction + dedup
    assert_eq!(state.stats.signals_extracted, 1);
    assert_eq!(state.stats.signals_deduplicated, 1);
    assert_eq!(state.stats.signals_stored, 0);

    let names = event_names(&captured);

    // SameSourceReencountered in chain
    assert!(
        names.contains(&"signal:same_source_reencountered".to_string()),
        "expected SameSourceReencountered, got: {names:?}"
    );

    // FreshnessConfirmed emitted
    assert!(
        names.contains(&"freshness_confirmed".to_string()),
        "expected FreshnessConfirmed, got: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// Link promotion handler — PhaseCompleted(TensionScrape) promotes collected links
// ---------------------------------------------------------------------------

#[tokio::test]
async fn link_promotion_promotes_links_on_phase_completed() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    // Seed collected links via ScrapeRoleCompleted (simulates links found during scraping)
    use crate::domains::scrape::events::{ScrapeEvent, ScrapeRole};

    engine
        .emit(ScrapeEvent::ScrapeRoleCompleted {
            run_id: Uuid::new_v4(),
            role: ScrapeRole::TensionWeb,
            urls_scraped: 0,
            urls_unchanged: 0,
            urls_failed: 0,
            signals_extracted: 0,
            source_signal_counts: HashMap::new(),
            collected_links: vec![
                CollectedLink {
                    url: "https://instagram.com/mutual_aid_mpls".to_string(),
                    discovered_on: "https://localorg.org".to_string(),
                },
                CollectedLink {
                    url: "https://twitter.com/mpls_community".to_string(),
                    discovered_on: "https://localorg.org".to_string(),
                },
            ],
            expansion_queries: vec![],
            stats_delta: Default::default(),
            page_previews: Default::default(),
        })
        .settled()
        .await
        .unwrap();

    // Clear captured events from seeding so we only see link promotion events
    captured.lock().unwrap().clear();

    // Dispatch LifecycleEvent::PhaseCompleted(TensionScrape) — link_promotion_handler fires
    engine
        .emit(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::TensionScrape,
        })
        .settled()
        .await
        .unwrap();

    let names = event_names(&captured);

    // SourceDiscovered events emitted for promoted links
    let source_discovered_count = names
        .iter()
        .filter(|n| n.contains("source_discovered"))
        .count();
    assert!(
        source_discovered_count >= 1,
        "expected at least 1 SourceDiscovered, got: {names:?}"
    );

    // LinksPromoted event emitted
    assert!(
        names.iter().any(|n| n.contains("links_promoted")),
        "expected LinksPromoted, got: {names:?}"
    );

    // Reducer cleared collected_links on LinksPromoted
    let state = engine.singleton::<PipelineState>();
    assert!(
        state.collected_links.is_empty(),
        "collected_links should be cleared after LinksPromoted"
    );
}

// ---------------------------------------------------------------------------
// Link promotion handler — skips when no links collected
// ---------------------------------------------------------------------------

#[tokio::test]
async fn link_promotion_skips_when_no_links() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    // No collected links

    engine
        .emit(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::TensionScrape,
        })
        .settled()
        .await
        .unwrap();

    let names = event_names(&captured);

    // Only the root PhaseCompleted event — no handler output
    assert!(
        !names.iter().any(|n| n.contains("source_discovered")),
        "should not emit SourceDiscovered with no links, got: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.contains("links_promoted")),
        "should not emit LinksPromoted with no links, got: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// Page triage: social handles always promoted regardless of signal count
// ---------------------------------------------------------------------------

#[tokio::test]
async fn social_handles_always_promoted_from_zero_signal_pages() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, captured) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    use crate::domains::scrape::events::{ScrapeEvent, ScrapeRole};

    // Seed links from a page with zero signals — no source_signal_counts entry
    engine
        .emit(ScrapeEvent::ScrapeRoleCompleted {
            run_id: Uuid::new_v4(),
            role: ScrapeRole::TensionWeb,
            urls_scraped: 1,
            urls_unchanged: 0,
            urls_failed: 0,
            signals_extracted: 0,
            source_signal_counts: HashMap::new(), // zero signals
            collected_links: vec![
                CollectedLink {
                    url: "https://instagram.com/mpls_mutual_aid".to_string(),
                    discovered_on: "https://hub-page.org".to_string(),
                },
                CollectedLink {
                    url: "https://x.com/mpls_help".to_string(),
                    discovered_on: "https://hub-page.org".to_string(),
                },
            ],
            expansion_queries: vec![],
            stats_delta: Default::default(),
            page_previews: Default::default(),
        })
        .settled()
        .await
        .unwrap();

    captured.lock().unwrap().clear();

    engine
        .emit(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::TensionScrape,
        })
        .settled()
        .await
        .unwrap();

    let names = event_names(&captured);
    let source_count = names.iter().filter(|n| n.contains("source_discovered")).count();
    assert!(
        source_count >= 2,
        "social handles should be promoted even from zero-signal pages, got: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.contains("links_promoted")),
        "LinksPromoted should be emitted, got: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// Page triage: productive page content links promoted without triage
// ---------------------------------------------------------------------------

#[tokio::test]
async fn productive_page_content_links_promoted_without_triage() {
    let store = Arc::new(MockSignalReader::new());
    // No AI — content links from productive pages should still be promoted
    let (engine, captured) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    use crate::domains::scrape::events::{ScrapeEvent, ScrapeRole};

    // Seed links from a productive page (signal_count > 0).
    // Use discovered_on URL as canonical key so url_to_canonical_key lookup succeeds.
    let mut signal_counts = HashMap::new();
    signal_counts.insert("https://hub-page.org/resources".to_string(), 3u32);

    engine
        .emit(ScrapeEvent::ScrapeRoleCompleted {
            run_id: Uuid::new_v4(),
            role: ScrapeRole::TensionWeb,
            urls_scraped: 1,
            urls_unchanged: 0,
            urls_failed: 0,
            signals_extracted: 3,
            source_signal_counts: signal_counts,
            collected_links: vec![
                CollectedLink {
                    url: "https://partner-org.org/programs".to_string(),
                    discovered_on: "https://hub-page.org/resources".to_string(),
                },
                CollectedLink {
                    url: "https://foodshelf.org/volunteer".to_string(),
                    discovered_on: "https://hub-page.org/resources".to_string(),
                },
            ],
            expansion_queries: vec![],
            stats_delta: Default::default(),
            page_previews: Default::default(),
        })
        .settled()
        .await
        .unwrap();

    captured.lock().unwrap().clear();

    engine
        .emit(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::TensionScrape,
        })
        .settled()
        .await
        .unwrap();

    let names = event_names(&captured);
    let source_count = names.iter().filter(|n| n.contains("source_discovered")).count();
    assert!(
        source_count >= 2,
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
    let (engine, captured) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    use crate::domains::scrape::events::{ScrapeEvent, ScrapeRole};

    // Seed only non-social links from a zero-signal page
    engine
        .emit(ScrapeEvent::ScrapeRoleCompleted {
            run_id: Uuid::new_v4(),
            role: ScrapeRole::TensionWeb,
            urls_scraped: 1,
            urls_unchanged: 0,
            urls_failed: 0,
            signals_extracted: 0,
            source_signal_counts: HashMap::new(),
            collected_links: vec![
                CollectedLink {
                    url: "https://partner-org.org/programs".to_string(),
                    discovered_on: "https://empty-page.org".to_string(),
                },
            ],
            expansion_queries: vec![],
            stats_delta: Default::default(),
            page_previews: Default::default(),
        })
        .settled()
        .await
        .unwrap();

    captured.lock().unwrap().clear();

    engine
        .emit(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::TensionScrape,
        })
        .settled()
        .await
        .unwrap();

    let names = event_names(&captured);
    let source_count = names.iter().filter(|n| n.contains("source_discovered")).count();
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

    let (engine, captured) = test_engine_with_ai(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        ai,
        Some(mpls_region()),
    );

    use crate::domains::scrape::events::{ScrapeEvent, ScrapeRole};

    engine
        .emit(ScrapeEvent::ScrapeRoleCompleted {
            run_id: Uuid::new_v4(),
            role: ScrapeRole::TensionWeb,
            urls_scraped: 1,
            urls_unchanged: 0,
            urls_failed: 0,
            signals_extracted: 0,
            source_signal_counts: HashMap::new(),
            collected_links: vec![
                CollectedLink {
                    url: "https://partner-org.org/programs".to_string(),
                    discovered_on: "https://directory-page.org/links".to_string(),
                },
                CollectedLink {
                    url: "https://foodshelf.org/volunteer".to_string(),
                    discovered_on: "https://directory-page.org/links".to_string(),
                },
            ],
            expansion_queries: vec![],
            stats_delta: Default::default(),
            page_previews: {
                let mut m = HashMap::new();
                m.insert(
                    "https://directory-page.org/links".to_string(),
                    "Community Resources Directory: links to local orgs".to_string(),
                );
                m
            },
        })
        .settled()
        .await
        .unwrap();

    captured.lock().unwrap().clear();

    engine
        .emit(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::TensionScrape,
        })
        .settled()
        .await
        .unwrap();

    let names = event_names(&captured);

    // PageTriaged event emitted
    assert!(
        names.iter().any(|n| n.contains("page_triaged")),
        "expected PageTriaged event, got: {names:?}"
    );

    // Content links promoted from triage-passed page
    let source_count = names.iter().filter(|n| n.contains("source_discovered")).count();
    assert!(
        source_count >= 2,
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

    let (engine, captured) = test_engine_with_ai(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        ai,
        Some(mpls_region()),
    );

    use crate::domains::scrape::events::{ScrapeEvent, ScrapeRole};

    engine
        .emit(ScrapeEvent::ScrapeRoleCompleted {
            run_id: Uuid::new_v4(),
            role: ScrapeRole::TensionWeb,
            urls_scraped: 1,
            urls_unchanged: 0,
            urls_failed: 0,
            signals_extracted: 0,
            source_signal_counts: HashMap::new(),
            collected_links: vec![
                CollectedLink {
                    url: "https://other-news.com/article".to_string(),
                    discovered_on: "https://news-article.com/story".to_string(),
                },
            ],
            expansion_queries: vec![],
            stats_delta: Default::default(),
            page_previews: {
                let mut m = HashMap::new();
                m.insert(
                    "https://news-article.com/story".to_string(),
                    "Breaking: Local politics debate continues...".to_string(),
                );
                m
            },
        })
        .settled()
        .await
        .unwrap();

    captured.lock().unwrap().clear();

    engine
        .emit(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::TensionScrape,
        })
        .settled()
        .await
        .unwrap();

    let names = event_names(&captured);

    // PageTriaged emitted
    assert!(
        names.iter().any(|n| n.contains("page_triaged")),
        "expected PageTriaged event, got: {names:?}"
    );

    // No content links promoted
    let source_count = names.iter().filter(|n| n.contains("source_discovered")).count();
    assert_eq!(
        source_count, 0,
        "content links from rejected page should NOT be promoted, got: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// Page triage: AI error fails closed (no content links promoted)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ai_error_fails_closed_no_content_links_promoted() {
    use crate::testing::MockAgent;
    let store = Arc::new(MockSignalReader::new());

    // Mock AI configured to fail
    let ai = Arc::new(MockAgent::failing());

    let (engine, captured) = test_engine_with_ai(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        ai,
        Some(mpls_region()),
    );

    use crate::domains::scrape::events::{ScrapeEvent, ScrapeRole};

    engine
        .emit(ScrapeEvent::ScrapeRoleCompleted {
            run_id: Uuid::new_v4(),
            role: ScrapeRole::TensionWeb,
            urls_scraped: 1,
            urls_unchanged: 0,
            urls_failed: 0,
            signals_extracted: 0,
            source_signal_counts: HashMap::new(),
            collected_links: vec![
                CollectedLink {
                    url: "https://partner-org.org/programs".to_string(),
                    discovered_on: "https://zero-signal-page.org".to_string(),
                },
            ],
            expansion_queries: vec![],
            stats_delta: Default::default(),
            page_previews: {
                let mut m = HashMap::new();
                m.insert(
                    "https://zero-signal-page.org".to_string(),
                    "Some page content".to_string(),
                );
                m
            },
        })
        .settled()
        .await
        .unwrap();

    captured.lock().unwrap().clear();

    engine
        .emit(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::TensionScrape,
        })
        .settled()
        .await
        .unwrap();

    let names = event_names(&captured);

    // PageTriaged emitted (with relevant=false due to error)
    assert!(
        names.iter().any(|n| n.contains("page_triaged")),
        "expected PageTriaged event even on error, got: {names:?}"
    );

    // No content links promoted
    let source_count = names.iter().filter(|n| n.contains("source_discovered")).count();
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
    let (engine, captured) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    use crate::domains::scrape::events::{ScrapeEvent, ScrapeRole};

    // Create 15 content links from a productive page — should be capped at 10
    let mut signal_counts = HashMap::new();
    signal_counts.insert("https://productive.org/resources".to_string(), 5u32);

    let collected_links: Vec<CollectedLink> = (0..15)
        .map(|i| CollectedLink {
            url: format!("https://site-{i}.org/page"),
            discovered_on: "https://productive.org/resources".to_string(),
        })
        .collect();

    engine
        .emit(ScrapeEvent::ScrapeRoleCompleted {
            run_id: Uuid::new_v4(),
            role: ScrapeRole::TensionWeb,
            urls_scraped: 1,
            urls_unchanged: 0,
            urls_failed: 0,
            signals_extracted: 5,
            source_signal_counts: signal_counts,
            collected_links,
            expansion_queries: vec![],
            stats_delta: Default::default(),
            page_previews: Default::default(),
        })
        .settled()
        .await
        .unwrap();

    captured.lock().unwrap().clear();

    engine
        .emit(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::TensionScrape,
        })
        .settled()
        .await
        .unwrap();

    let names = event_names(&captured);
    let source_count = names.iter().filter(|n| n.contains("source_discovered")).count();
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
// Page previews cleared after LinksPromoted
// ---------------------------------------------------------------------------

#[tokio::test]
async fn page_previews_cleared_after_links_promoted() {
    let store = Arc::new(MockSignalReader::new());
    let (engine, _captured) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    use crate::domains::scrape::events::{ScrapeEvent, ScrapeRole};

    engine
        .emit(ScrapeEvent::ScrapeRoleCompleted {
            run_id: Uuid::new_v4(),
            role: ScrapeRole::TensionWeb,
            urls_scraped: 1,
            urls_unchanged: 0,
            urls_failed: 0,
            signals_extracted: 0,
            source_signal_counts: HashMap::new(),
            collected_links: vec![
                CollectedLink {
                    url: "https://instagram.com/test_handle".to_string(),
                    discovered_on: "https://page.org".to_string(),
                },
            ],
            expansion_queries: vec![],
            stats_delta: Default::default(),
            page_previews: {
                let mut m = HashMap::new();
                m.insert("https://page.org".to_string(), "some preview".to_string());
                m
            },
        })
        .settled()
        .await
        .unwrap();

    let state = engine.singleton::<PipelineState>();
    assert!(
        !state.page_previews.is_empty(),
        "page_previews should be populated after ScrapeRoleCompleted"
    );

    engine
        .emit(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::TensionScrape,
        })
        .settled()
        .await
        .unwrap();

    let state = engine.singleton::<PipelineState>();
    assert!(
        state.page_previews.is_empty(),
        "page_previews should be cleared after LinksPromoted"
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
    let (engine, captured) = test_engine_with_capture_for_store(
        store.clone() as Arc<dyn crate::traits::SignalReader>,
        Some(mpls_region()),
    );

    // Dispatch LifecycleEvent::PhaseCompleted(ResponseScrape) — actor_location_handler fires
    engine
        .emit(LifecycleEvent::PhaseCompleted {
            phase: PipelinePhase::ResponseScrape,
        })
        .settled()
        .await
        .unwrap();

    let names = event_names(&captured);

    // ActorLocationIdentified event emitted
    assert!(
        names.contains(&"actor_location_identified".to_string()),
        "expected ActorLocationIdentified, got: {names:?}"
    );

    // EnrichmentRoleCompleted event emitted (actor_location role)
    assert!(
        names.iter().any(|n| n.contains("enrichment_role_completed")),
        "expected EnrichmentRoleCompleted, got: {names:?}"
    );
}

