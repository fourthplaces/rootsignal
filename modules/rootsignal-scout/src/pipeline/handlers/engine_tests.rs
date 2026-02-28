//! Engine integration tests — full dispatch loop via seesaw engine.
//!
//! MOCK → ENGINE.EMIT → OUTPUT
//! Proves seesaw handlers compose correctly.

use std::collections::HashMap;
use std::sync::Arc;

use crate::core::events::{PipelinePhase, ScoutEvent};
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::signals::events::SignalEvent;
use crate::pipeline::state::ExtractedBatch;
use crate::testing::*;

/// Helper: collect event variant names from captured events.
fn event_names(captured: &Arc<std::sync::Mutex<Vec<ScoutEvent>>>) -> Vec<String> {
    captured
        .lock()
        .unwrap()
        .iter()
        .map(|e| e.event_type_str())
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

    let pn = crate::core::aggregate::PendingNode {
        node,
        embedding: vec![0.1; TEST_EMBEDDING_DIM],
        content_hash: "abc123".to_string(),
        resource_tags: vec![],
        signal_tags: vec!["legal".to_string()],
        author_name: Some("Local Legal Aid".to_string()),
        source_id: None,
    };

    engine
        .emit(SignalEvent::NewSignalAccepted {
            node_id,
            node_type: rootsignal_common::types::NodeType::Tension,
            title: "Free Legal Clinic".to_string(),
            source_url: "https://www.instagram.com/locallegalaid".to_string(),
            pending_node: Box::new(pn),
        })
        .settled()
        .await
        .unwrap();

    let state = engine.deps().state.read().await;

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
            node_type: rootsignal_common::types::NodeType::Tension,
            source_url: "https://org-b.org/events".to_string(),
            similarity: 0.95,
        })
        .settled()
        .await
        .unwrap();

    // Reducer counted the dedup
    assert_eq!(engine.deps().state.read().await.stats.signals_deduplicated, 1);

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
            node_type: rootsignal_common::types::NodeType::Gathering,
            source_url: "https://example.org".to_string(),
            similarity: 1.0,
        })
        .settled()
        .await
        .unwrap();

    // Reducer counted the dedup
    assert_eq!(engine.deps().state.read().await.stats.signals_deduplicated, 1);

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

    let state = engine.deps().state.read().await;

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
        rootsignal_common::types::NodeType::Tension,
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

    let state = engine.deps().state.read().await;

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

    // Pre-populate collected links in engine state (simulates links found during scraping)
    {
        let mut state = engine.deps().state.write().await;
        state
            .collected_links
            .push(crate::enrichment::link_promoter::CollectedLink {
                url: "https://example.org/community".to_string(),
                discovered_on: "https://localorg.org".to_string(),
            });
        state
            .collected_links
            .push(crate::enrichment::link_promoter::CollectedLink {
                url: "https://another.org/events".to_string(),
                discovered_on: "https://localorg.org".to_string(),
            });
    }

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
    assert!(
        engine.deps().state.read().await.collected_links.is_empty(),
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
// Actor location handler — emits location events on ResponseScrape complete
// ---------------------------------------------------------------------------

#[tokio::test]
async fn actor_location_emits_events_on_response_complete() {
    use chrono::Utc;

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

    // ActorEnrichmentCompleted event emitted
    assert!(
        names.iter().any(|n| n.contains("actor_enrichment_completed")),
        "expected ActorEnrichmentCompleted, got: {names:?}"
    );
}
