//! Handlers that react to dedup verdict facts.
//!
//! NewSignalAccepted → construct World + System events for the new signal.
//! CrossSourceMatchDetected → construct corroboration events.
//! SameSourceReencountered → construct freshness confirmation event.
//! SignalReaderd → wire edges (source, actor, resources, tags) via events.
//!
//! All graph writes flow through events → engine → EventStore → GraphProjector.
//! Handlers only emit events — no direct store writes.

use anyhow::Result;
use chrono::Utc;
use rootsignal_common::events::{SystemEvent, WorldEvent};
use rootsignal_common::types::NodeType;
use uuid::Uuid;

use crate::pipeline::events::{PipelineEvent, ScoutEvent};
use crate::pipeline::state::{PipelineDeps, PipelineState};
use crate::store::event_sourced::{node_system_events, node_to_world_event};

/// NewSignalAccepted: a new signal passed all dedup layers.
/// Emits World + System + Citation events, then triggers edge wiring via SignalReaderd.
///
/// Reads PendingNode from state (stashed by reducer). Pure — no state mutations.
pub async fn handle_create(
    node_id: Uuid,
    scrape_url: &str,
    state: &PipelineState,
    _deps: &PipelineDeps,
) -> Result<Vec<ScoutEvent>> {
    let pending = match state.pending_nodes.get(&node_id) {
        Some(p) => p,
        None => {
            tracing::warn!(%node_id, "NewSignalAccepted: no pending node found");
            return Ok(vec![]);
        }
    };

    let stored_id = pending.node.id();

    // embed_cache.add already done by dedup handler
    // wiring_contexts already stashed by reducer

    let mut events = Vec::new();

    // 1. World fact — the discovery (engine persists → projector creates node in Neo4j)
    events.push(ScoutEvent::World(node_to_world_event(&pending.node)));

    // 2. System classifications
    for sys in node_system_events(&pending.node) {
        events.push(ScoutEvent::System(sys));
    }

    // 3. Citation evidence (engine persists → projector creates evidence in Neo4j)
    events.push(ScoutEvent::World(WorldEvent::CitationRecorded {
        citation_id: Uuid::new_v4(),
        signal_id: stored_id,
        url: scrape_url.to_string(),
        content_hash: pending.content_hash.clone(),
        snippet: pending.node.meta().map(|m| m.summary.clone()),
        relevance: None,
        channel_type: Some(rootsignal_common::channel_type(scrape_url)),
        evidence_confidence: None,
    }));

    // 4. Trigger edge wiring
    let canonical_key = state
        .url_to_canonical_key
        .get(scrape_url)
        .cloned()
        .unwrap_or_else(|| scrape_url.to_string());
    events.push(ScoutEvent::Pipeline(PipelineEvent::SignalReaderd {
        node_id: stored_id,
        node_type: pending.node.node_type(),
        source_url: scrape_url.to_string(),
        canonical_key,
    }));

    Ok(events)
}

/// CrossSourceMatchDetected: cross-source match found.
/// Emits citation, corroboration, and scoring events.
pub async fn handle_corroborate(
    existing_id: Uuid,
    node_type: NodeType,
    source_url: &str,
    similarity: f64,
    deps: &PipelineDeps,
) -> Result<Vec<ScoutEvent>> {
    let current_count = deps
        .store
        .read_corroboration_count(existing_id, node_type)
        .await
        .unwrap_or(0);

    Ok(vec![
        ScoutEvent::World(WorldEvent::CitationRecorded {
            citation_id: Uuid::new_v4(),
            signal_id: existing_id,
            url: source_url.to_string(),
            content_hash: String::new(),
            snippet: None,
            relevance: None,
            channel_type: Some(rootsignal_common::channel_type(source_url)),
            evidence_confidence: None,
        }),
        ScoutEvent::World(WorldEvent::ObservationCorroborated {
            signal_id: existing_id,
            node_type,
            new_source_url: source_url.to_string(),
            summary: None,
        }),
        ScoutEvent::System(SystemEvent::CorroborationScored {
            signal_id: existing_id,
            similarity,
            new_corroboration_count: current_count + 1,
        }),
    ])
}

/// SameSourceReencountered: same-source re-encounter.
/// Emits citation and freshness confirmation events.
pub async fn handle_refresh(
    existing_id: Uuid,
    node_type: NodeType,
    source_url: &str,
    _deps: &PipelineDeps,
) -> Result<Vec<ScoutEvent>> {
    let now = Utc::now();

    Ok(vec![
        ScoutEvent::World(WorldEvent::CitationRecorded {
            citation_id: Uuid::new_v4(),
            signal_id: existing_id,
            url: source_url.to_string(),
            content_hash: String::new(),
            snippet: None,
            relevance: None,
            channel_type: Some(rootsignal_common::channel_type(source_url)),
            evidence_confidence: None,
        }),
        ScoutEvent::System(SystemEvent::FreshnessConfirmed {
            signal_ids: vec![existing_id],
            node_type,
            confirmed_at: now,
        }),
    ])
}

/// SignalReaderd: wire edges (source, actor, resources, tags) via events.
/// Reads WiringContext from state (stashed by reducer). Pure — no state mutations.
pub async fn handle_signal_stored(
    node_id: Uuid,
    _node_type: NodeType,
    source_url: &str,
    canonical_key: &str,
    state: &PipelineState,
    deps: &PipelineDeps,
) -> Result<Vec<ScoutEvent>> {
    let ctx = match state.wiring_contexts.get(&node_id) {
        Some(c) => c,
        None => return Ok(vec![]),
    };

    let mut events = Vec::new();

    // PRODUCED_BY edge (signal → source) via event
    if let Some(sid) = ctx.source_id {
        events.push(ScoutEvent::System(SystemEvent::SignalLinkedToSource {
            signal_id: node_id,
            source_id: sid,
        }));
    }

    // Resource edges — pure event emission, no store calls.
    // ResourceIdentified creates the node (MERGE on slug), ResourceEdgeCreated wires the edge.
    for tag in ctx.resource_tags.iter().filter(|t| t.confidence >= 0.3) {
        let slug = rootsignal_common::slugify(&tag.slug);
        let description = tag.context.as_deref().unwrap_or("").to_string();

        events.push(ScoutEvent::World(WorldEvent::ResourceIdentified {
            resource_id: Uuid::new_v4(),
            name: tag.slug.clone(),
            slug: slug.clone(),
            description: description.clone(),
        }));

        let confidence = tag.confidence.clamp(0.0, 1.0) as f32;
        match tag.role.as_str() {
            "requires" | "prefers" | "offers" => {
                events.push(ScoutEvent::World(WorldEvent::ResourceEdgeCreated {
                    signal_id: node_id,
                    resource_slug: slug,
                    role: tag.role.clone(),
                    confidence,
                    quantity: if tag.role == "requires" {
                        tag.context.clone()
                    } else {
                        None
                    },
                    notes: None,
                    capacity: if tag.role == "offers" {
                        tag.context.clone()
                    } else {
                        None
                    },
                }));
            }
            other => {
                tracing::warn!(role = other, slug = slug.as_str(), "Unknown resource role");
            }
        }
    }

    // Signal tags via event
    if !ctx.signal_tags.is_empty() {
        events.push(ScoutEvent::System(SystemEvent::SignalTagged {
            signal_id: node_id,
            tag_slugs: ctx.signal_tags.clone(),
        }));
    }

    // Actor wiring — only for owned (social) sources
    let strategy = rootsignal_common::scraping_strategy(source_url);
    if crate::pipeline::scrape_phase::is_owned_source(&strategy) {
        if let Some(ref author_name) = ctx.author_name {
            let discovery_depth = state
                .actor_contexts
                .get(canonical_key)
                .map(|ac| ac.discovery_depth + 1)
                .unwrap_or(0);
            let actor_events = handle_author_actor(
                node_id, author_name, source_url, ctx.source_id, discovery_depth, deps,
            )
            .await?;
            events.extend(actor_events);
        }
    }

    Ok(events)
}

/// Resolve author → Actor node on owned sources.
/// Emits ActorIdentified + ActorLinkedToSource + ActorLinkedToSignal events.
async fn handle_author_actor(
    signal_id: Uuid,
    author_name: &str,
    source_url: &str,
    source_id: Option<Uuid>,
    discovery_depth: u32,
    deps: &PipelineDeps,
) -> Result<Vec<ScoutEvent>> {
    let canonical_key = rootsignal_common::canonical_value(source_url);

    // Read-only: check if actor already exists
    let actor_id = match deps.store.find_actor_by_canonical_key(&canonical_key).await {
        Ok(Some(id)) => id,
        Ok(None) => {
            // New actor — emit ActorIdentified event (projector creates it in Neo4j)
            let new_id = Uuid::new_v4();
            let mut events = vec![
                ScoutEvent::World(WorldEvent::ActorIdentified {
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
                }),
            ];
            if let Some(sid) = source_id {
                events.push(ScoutEvent::System(SystemEvent::ActorLinkedToSource {
                    actor_id: new_id,
                    source_id: sid,
                }));
            }
            events.push(ScoutEvent::World(WorldEvent::ActorLinkedToSignal {
                actor_id: new_id,
                signal_id,
                role: "authored".to_string(),
            }));
            return Ok(events);
        }
        Err(e) => {
            tracing::warn!(error = %e, actor = author_name, "Actor lookup failed");
            return Ok(vec![]);
        }
    };

    // Existing actor — just link to signal
    Ok(vec![ScoutEvent::World(WorldEvent::ActorLinkedToSignal {
        actor_id,
        signal_id,
        role: "authored".to_string(),
    })])
}
