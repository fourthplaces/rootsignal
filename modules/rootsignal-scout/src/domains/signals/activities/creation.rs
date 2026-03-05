//! Signal creation helpers called by the dedup handler.
//!
//! create_signal_events — construct World + System events for a new signal.
//! create_corroboration_events — construct corroboration events for cross-source match.
//! create_freshness_events — construct freshness confirmation for same-source re-encounter.
//! wire_signal_edges — wire edges (source, actor, resources, tags) via events.
//!
//! All graph writes flow through events → engine → EventStore → GraphProjector.

use anyhow::Result;
use chrono::Utc;
use rootsignal_common::events::{SystemEvent, WorldEvent};
use rootsignal_common::types::NodeType;
use seesaw_core::{events, Events};
use uuid::Uuid;

use crate::core::engine::ScoutEngineDeps;
use crate::core::extractor::ResourceRole;
use crate::domains::signals::activities::dedup_utils::is_owned_source;
use crate::domains::signals::events::SignalEvent;
use crate::core::aggregate::{PendingNode, PipelineState, WiringContext};
use crate::store::event_sourced::{node_system_events, node_to_world_event};

/// New signal passed all dedup layers.
/// Emits World + System + Citation events, then triggers edge wiring via SignalCreated.
///
/// Takes PendingNode directly (dedup has it in hand). Pure — no state mutations.
pub async fn create_signal_events(
    pending: &PendingNode,
    canonical_key: &str,
    scrape_url: &str,
    state: &PipelineState,
    _deps: &ScoutEngineDeps,
) -> Result<Events> {
    let stored_id = pending.node.id();

    let mut events = events![];

    // 1. World fact — the discovery (engine persists → projector creates node in Neo4j)
    events = events.add(node_to_world_event(&pending.node));

    // 2. System classifications
    for sys in node_system_events(&pending.node) {
        events = events.add(sys);
    }

    // 3. Citation evidence (engine persists → projector creates evidence in Neo4j)
    events = events.add(WorldEvent::CitationPublished {
        citation_id: Uuid::new_v4(),
        signal_id: stored_id,
        url: scrape_url.to_string(),
        content_hash: pending.content_hash.clone(),
        snippet: pending.node.meta().map(|m| m.summary.clone()),
        relevance: None,
        channel_type: Some(rootsignal_common::channel_type(scrape_url)),
        evidence_confidence: None,
    });

    // 4. Trigger edge wiring — carry WiringContext for the wire_edges handler
    let ck = state
        .url_to_canonical_key
        .get(scrape_url)
        .cloned()
        .unwrap_or_else(|| canonical_key.to_string());
    events = events.add(SignalEvent::SignalCreated {
        node_id: stored_id,
        node_type: pending.node.node_type(),
        source_url: scrape_url.to_string(),
        canonical_key: ck,
        wiring: Some(WiringContext {
            resource_tags: pending.resource_tags.clone(),
            signal_tags: pending.signal_tags.clone(),
            author_name: pending.author_name.clone(),
            source_id: pending.source_id,
        }),
    });

    Ok(events)
}

/// Cross-source match found: emits citation, corroboration, and scoring events.
pub async fn create_corroboration_events(
    existing_id: Uuid,
    node_type: NodeType,
    source_url: &str,
    similarity: f64,
    deps: &ScoutEngineDeps,
) -> Result<Events> {
    let current_count = deps
        .store
        .read_corroboration_count(existing_id, node_type)
        .await
        .unwrap_or(0);

    Ok(events![
        WorldEvent::CitationPublished {
            citation_id: Uuid::new_v4(),
            signal_id: existing_id,
            url: source_url.to_string(),
            content_hash: String::new(),
            snippet: None,
            relevance: None,
            channel_type: Some(rootsignal_common::channel_type(source_url)),
            evidence_confidence: None,
        },
        SystemEvent::ObservationCorroborated {
            signal_id: existing_id,
            node_type,
            new_source_url: source_url.to_string(),
            summary: None,
        },
        SystemEvent::CorroborationScored {
            signal_id: existing_id,
            similarity,
            new_corroboration_count: current_count + 1,
        }
    ])
}

/// Same-source re-encounter: emits citation and freshness confirmation events.
pub async fn create_freshness_events(
    existing_id: Uuid,
    node_type: NodeType,
    source_url: &str,
    _deps: &ScoutEngineDeps,
) -> Result<Events> {
    let now = Utc::now();

    Ok(events![
        WorldEvent::CitationPublished {
            citation_id: Uuid::new_v4(),
            signal_id: existing_id,
            url: source_url.to_string(),
            content_hash: String::new(),
            snippet: None,
            relevance: None,
            channel_type: Some(rootsignal_common::channel_type(source_url)),
            evidence_confidence: None,
        },
        SystemEvent::FreshnessConfirmed {
            signal_ids: vec![existing_id],
            node_type,
            confirmed_at: now,
        }
    ])
}

/// SignalCreated: wire edges (source, actor, resources, tags) via events.
/// Reads WiringContext from state (stashed by reducer). Pure — no state mutations.
pub async fn wire_signal_edges(
    node_id: Uuid,
    _node_type: NodeType,
    source_url: &str,
    canonical_key: &str,
    state: &PipelineState,
    deps: &ScoutEngineDeps,
) -> Result<Events> {
    let ctx = match state.wiring_contexts.get(&node_id) {
        Some(c) => c,
        None => return Ok(events![]),
    };

    let mut events = events![];

    // PRODUCED_BY edge (signal → source) via event
    if let Some(sid) = ctx.source_id {
        events = events.add(WorldEvent::SignalLinkedToSource {
            signal_id: node_id,
            source_id: sid,
        });
    }

    // Resource edges — pure event emission, no store calls.
    // ResourceIdentified creates the node (MERGE on slug), ResourceLinked wires the edge.
    for tag in ctx.resource_tags.iter().filter(|t| t.confidence >= 0.3) {
        let slug = rootsignal_common::slugify(&tag.slug);
        let description = tag.context.as_deref().unwrap_or("").to_string();

        events = events.add(WorldEvent::ResourceIdentified {
            resource_id: Uuid::new_v4(),
            name: tag.slug.clone(),
            slug: slug.clone(),
            description: description.clone(),
        });

        let confidence = tag.confidence.clamp(0.0, 1.0) as f32;
        let (quantity, capacity) = match tag.role {
            ResourceRole::Requires => (tag.context.clone(), None),
            ResourceRole::Prefers => (None, None),
            ResourceRole::Offers => (None, tag.context.clone()),
        };
        events = events.add(WorldEvent::ResourceLinked {
            signal_id: node_id,
            resource_slug: slug,
            role: tag.role.to_string(),
            confidence,
            quantity,
            notes: None,
            capacity,
        });
    }

    // Signal tags via event
    if !ctx.signal_tags.is_empty() {
        events = events.add(SystemEvent::SignalTagged {
            signal_id: node_id,
            tag_slugs: ctx.signal_tags.clone(),
        });
    }

    // Actor wiring — only for owned (social) sources
    let strategy = rootsignal_common::scraping_strategy(source_url);
    if is_owned_source(&strategy) {
        if let Some(ref author_name) = ctx.author_name {
            let discovery_depth = state
                .actor_contexts
                .get(canonical_key)
                .map(|ac| ac.discovery_depth + 1)
                .unwrap_or(0);
            events = resolve_author_actor(
                events,
                node_id,
                author_name,
                source_url,
                ctx.source_id,
                discovery_depth,
                deps,
            )
            .await?;
        }
    }

    Ok(events)
}

/// Resolve author → Actor node on owned sources.
/// Adds ActorIdentified + ActorLinkedToSource + ActorLinkedToSignal events to the collection.
async fn resolve_author_actor(
    mut events: Events,
    signal_id: Uuid,
    author_name: &str,
    source_url: &str,
    source_id: Option<Uuid>,
    _discovery_depth: u32,
    deps: &ScoutEngineDeps,
) -> Result<Events> {
    let canonical_key = rootsignal_common::canonical_value(source_url);

    // Read-only: check if actor already exists
    let actor_id = match deps.store.find_actor_by_canonical_key(&canonical_key).await {
        Ok(Some(id)) => id,
        Ok(None) => {
            // New actor — emit ActorIdentified event (projector creates it in Neo4j)
            let new_id = Uuid::new_v4();
            events = events.add(SystemEvent::ActorIdentified {
                actor_id: new_id,
                name: author_name.to_string(),
                actor_type: rootsignal_common::ActorType::Organization,
                canonical_key,
                domains: vec![],
                social_urls: vec![],
                description: String::new(),
                bio: None,
                location_lat: None,
                location_lng: None,
                location_name: None,
            });
            if let Some(sid) = source_id {
                events = events.add(WorldEvent::ActorLinkedToSource {
                    actor_id: new_id,
                    source_id: sid,
                });
            }
            events = events.add(SystemEvent::ActorLinkedToSignal {
                actor_id: new_id,
                signal_id,
                role: "authored".to_string(),
            });
            return Ok(events);
        }
        Err(e) => {
            tracing::warn!(error = %e, actor = author_name, "Actor lookup failed");
            return Ok(events);
        }
    };

    // Existing actor — just link to signal
    events = events.add(SystemEvent::ActorLinkedToSignal {
        actor_id,
        signal_id,
        role: "authored".to_string(),
    });
    Ok(events)
}
