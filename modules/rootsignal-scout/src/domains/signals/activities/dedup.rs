//! Dedup handler — 4-layer deduplication for extracted signals.
//!
//! Triggered by scrape completion events (carry extracted batches in-memory).
//! Runs dedup layers and emits final facts directly:
//!
//! - Create: WorldEvent + SystemEvents + CitationPublished + DedupOutcome::Created
//! - Corroborate: CitationPublished + DedupOutcome::Corroborated
//! - Refresh: DedupOutcome::Refreshed (no events — freshness is the default)
//!
//! Layer 1 (batch title dedup) is applied by the caller before stashing.
//! This handler runs layers 2–4.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use rootsignal_common::events::{SystemEvent, WorldEvent};
use rootsignal_common::types::NodeType;
use seesaw_core::Events;
use tracing::{info, warn};
use uuid::Uuid;

use crate::domains::signals::events::{DedupOutcome, ResolvedActor, SignalEvent};
use crate::domains::signals::activities::dedup_utils::{dedup_verdict, is_owned_source, normalize_title, DedupVerdict};
use crate::core::engine::ScoutEngineDeps;
use crate::core::aggregate::{ExtractedBatch, PipelineState};
use crate::store::event_sourced::{node_system_events, node_to_world_event};

/// Handle extracted batch: run 4-layer dedup, emit final facts.
///
/// The batch is carried directly on the event payload — no stash/cleanup needed.
pub async fn deduplicate_extracted_batch(
    url: &str,
    canonical_key: &str,
    batch: &ExtractedBatch,
    state: &PipelineState,
    deps: &ScoutEngineDeps,
) -> Result<Events> {

    if batch.nodes.is_empty() {
        let mut events = Events::new();
        events.push(SignalEvent::DedupCompleted {
            url: url.to_string(),
            canonical_key: canonical_key.to_string(),
            verdicts: vec![],
        });
        return Ok(events);
    }

    let content_hash_str = format!("{:x}", rootsignal_common::content_hash(&batch.content));
    let mut events = Events::new();
    let mut verdicts: Vec<DedupOutcome> = Vec::new();

    // Track seen actors within the batch to avoid duplicate creation
    let mut seen_actors: HashMap<String, Uuid> = HashMap::new();

    // Track corroboration counts per target to merge multiple corroborations
    let mut corroboration_counts: HashMap<Uuid, u32> = HashMap::new();

    // --- Layer 2: URL-based title dedup ---
    let existing_titles: HashSet<String> = deps
        .store
        .existing_titles_for_url(url)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|t| normalize_title(&t))
        .collect();

    let before_url_dedup = batch.nodes.len();
    let nodes: Vec<_> = batch
        .nodes
        .iter()
        .filter(|n| !existing_titles.contains(&normalize_title(n.title())))
        .cloned()
        .collect();
    let url_deduped = before_url_dedup - nodes.len();
    if url_deduped > 0 {
        info!(url, skipped = url_deduped, "URL-based title dedup");
    }

    if nodes.is_empty() {
        events.push(SignalEvent::DedupCompleted {
            url: url.to_string(),
            canonical_key: canonical_key.to_string(),
            verdicts: vec![],
        });
        return Ok(events);
    }

    let ck = state
        .url_to_canonical_key
        .get(url)
        .cloned()
        .unwrap_or_else(|| canonical_key.to_string());

    // --- Layer 2.5: Global exact-title+type match ---
    let title_type_pairs: Vec<(String, NodeType)> = nodes
        .iter()
        .map(|n| (normalize_title(n.title()), n.node_type()))
        .collect();

    let global_matches = deps
        .store
        .find_by_titles_and_types(&title_type_pairs)
        .await
        .unwrap_or_default();

    let mut remaining_nodes = Vec::new();
    for node in nodes {
        let key = (normalize_title(node.title()), node.node_type());
        let global_hit = global_matches.get(&key).map(|(id, existing_ck)| (*id, existing_ck.as_str()));

        match dedup_verdict(&ck, node.node_type(), global_hit, None, None) {
            DedupVerdict::Corroborate {
                existing_id,
                similarity,
                ..
            } => {
                info!(
                    existing_id = %existing_id,
                    title = node.title(),
                    new_source = url,
                    "Global title+type match from different source"
                );
                let (count, verdict) = build_corroboration(
                    existing_id, node.node_type(), url, similarity,
                    &content_hash_str, deps, &mut corroboration_counts,
                ).await?;
                events.extend(verdict.0);
                verdicts.push(verdict.1);
            }
            DedupVerdict::Refresh {
                existing_id,
                similarity: _,
                ..
            } => {
                info!(
                    existing_id = %existing_id,
                    title = node.title(),
                    source = url,
                    "Same-source title match"
                );
                verdicts.push(build_freshness(existing_id, node.node_type(), url));
            }
            DedupVerdict::Create => {
                remaining_nodes.push(node);
            }
        }
    }
    let nodes = remaining_nodes;

    if nodes.is_empty() {
        events.push(SignalEvent::DedupCompleted {
            url: url.to_string(),
            canonical_key: canonical_key.to_string(),
            verdicts,
        });
        return Ok(events);
    }

    // --- Layer 2.75: Fingerprint dedup (url, node_type) ---
    let fingerprints: Vec<(String, NodeType)> = nodes
        .iter()
        .filter(|n| !n.meta().map(|m| m.url.is_empty()).unwrap_or(true))
        .map(|n| (n.meta().unwrap().url.clone(), n.node_type()))
        .collect();

    let fingerprint_matches = if !fingerprints.is_empty() {
        deps.store
            .find_by_fingerprints(&fingerprints)
            .await
            .unwrap_or_default()
    } else {
        HashMap::new()
    };

    let mut remaining_nodes = Vec::new();
    for node in nodes {
        let fp_key = node.meta().map(|m| (m.url.clone(), node.node_type()));
        let fp_hit = fp_key
            .as_ref()
            .and_then(|k| fingerprint_matches.get(k))
            .map(|(id, ck)| (*id, ck.as_str()));

        if fp_hit.is_some() {
            match dedup_verdict(&ck, node.node_type(), fp_hit, None, None) {
                DedupVerdict::Corroborate {
                    existing_id,
                    similarity,
                    ..
                } => {
                    info!(
                        existing_id = %existing_id,
                        title = node.title(),
                        new_source = url,
                        "Fingerprint match from different source"
                    );
                    let (_count, verdict) = build_corroboration(
                        existing_id, node.node_type(), url, similarity,
                        &content_hash_str, deps, &mut corroboration_counts,
                    ).await?;
                    events.extend(verdict.0);
                    verdicts.push(verdict.1);
                }
                DedupVerdict::Refresh {
                    existing_id,
                    ..
                } => {
                    info!(
                        existing_id = %existing_id,
                        title = node.title(),
                        source = url,
                        "Fingerprint match same source"
                    );
                    verdicts.push(build_freshness(existing_id, node.node_type(), url));
                }
                DedupVerdict::Create => unreachable!("fingerprint hit always resolves to Refresh or Corroborate"),
            }
        } else {
            remaining_nodes.push(node);
        }
    }
    let nodes = remaining_nodes;

    if nodes.is_empty() {
        events.push(SignalEvent::DedupCompleted {
            url: url.to_string(),
            canonical_key: canonical_key.to_string(),
            verdicts,
        });
        return Ok(events);
    }

    // --- Batch embed remaining signals ---
    let content_snippet = if batch.content.len() > 500 {
        let mut end = 500;
        while !batch.content.is_char_boundary(end) {
            end -= 1;
        }
        &batch.content[..end]
    } else {
        &batch.content
    };
    let embed_texts: Vec<String> = nodes
        .iter()
        .map(|n| format!("{} {}", n.title(), content_snippet))
        .collect();

    let embeddings = match deps.embedder.embed_batch(embed_texts).await {
        Ok(e) => e,
        Err(e) => {
            warn!(url, error = %e, "Batch embedding failed, skipping all signals");
            return Ok(events);
        }
    };

    // --- Layer 3: Vector dedup (cache + graph) ---
    let (lat_delta, lng_delta) = match state.run_scope.region() {
        Some(r) => {
            let lat_d = r.radius_km / 111.0;
            let lng_d = r.radius_km / (111.0 * r.center_lat.to_radians().cos());
            (lat_d, lng_d)
        }
        None => (90.0, 180.0), // global fallback
    };
    let (center_lat, center_lng) = state
        .run_scope
        .region()
        .map(|r| (r.center_lat, r.center_lng))
        .unwrap_or((0.0, 0.0));

    for (node, embedding) in nodes.into_iter().zip(embeddings.into_iter()) {
        let node_type = node.node_type();
        if node_type == NodeType::Citation {
            continue;
        }

        // 3a: Check in-memory cache first
        let cache_hit = deps.embed_cache.find_match(&embedding, 0.85);

        // 3b: Check graph index (region-scoped)
        let graph_hit = match deps
            .store
            .find_duplicate(
                &embedding,
                node_type,
                0.85,
                center_lat - lat_delta,
                center_lat + lat_delta,
                center_lng - lng_delta,
                center_lng + lng_delta,
            )
            .await
        {
            Ok(Some(dup)) => {
                Some((dup.id, dup.node_type, dup.canonical_key, dup.similarity))
            }
            Ok(None) => None,
            Err(e) => {
                warn!(error = %e, "Dedup check failed, proceeding with creation");
                None
            }
        };

        let cache_match = cache_hit
            .as_ref()
            .map(|(id, ty, u, s)| (*id, *ty, u.as_str(), *s));
        let graph_match = graph_hit
            .as_ref()
            .map(|(id, ty, u, s)| (*id, *ty, u.as_str(), *s));

        match dedup_verdict(&ck, node_type, None, cache_match, graph_match) {
            DedupVerdict::Refresh {
                existing_id,
                similarity,
                ..
            } => {
                let source_layer = if cache_hit.is_some() {
                    "cache"
                } else {
                    "graph"
                };
                info!(
                    existing_id = %existing_id,
                    similarity,
                    title = node.title(),
                    source = source_layer,
                    "Same-source duplicate"
                );
                // Update embed cache if verdict came from graph
                if cache_hit.is_none() {
                    if let Some((_, _, ref hit_ck, _)) = graph_hit {
                        deps.embed_cache.add(
                            embedding,
                            existing_id,
                            node_type,
                            hit_ck.clone(),
                        );
                    }
                }
                verdicts.push(build_freshness(existing_id, node_type, url));
            }
            DedupVerdict::Corroborate {
                existing_id,
                similarity,
                ..
            } => {
                let source_layer = if cache_match.map(|c| c.0) == Some(existing_id) {
                    "cache"
                } else {
                    "graph"
                };
                info!(
                    existing_id = %existing_id,
                    similarity,
                    title = node.title(),
                    source = source_layer,
                    "Cross-source duplicate"
                );
                // Update embed cache if verdict came from graph
                if cache_hit.is_none() {
                    if let Some((_, _, ref hit_ck, _)) = graph_hit {
                        deps.embed_cache.add(
                            embedding,
                            existing_id,
                            node_type,
                            hit_ck.clone(),
                        );
                    }
                }
                let (count, verdict) = build_corroboration(
                    existing_id, node_type, url, similarity,
                    &content_hash_str, deps, &mut corroboration_counts,
                ).await?;
                events.extend(verdict.0);
                verdicts.push(verdict.1);
            }
            DedupVerdict::Create => {
                let node_id = node.id();
                let meta_id = node.meta().map(|m| m.id);

                // Add to embed cache
                deps
                    .embed_cache
                    .add(embedding.clone(), node_id, node_type, ck.clone());

                let author_name = meta_id
                    .and_then(|mid| batch.author_actors.get(&mid))
                    .cloned();
                let node_resource_tags = meta_id
                    .and_then(|mid| batch.resource_tags.get(&mid))
                    .cloned()
                    .unwrap_or_default();
                let node_signal_tags = meta_id
                    .and_then(|mid| batch.signal_tags.get(&mid))
                    .cloned()
                    .unwrap_or_default();

                // 1. World fact — the discovery
                events.push(node_to_world_event(&node));

                // 2. System classifications
                for sys in node_system_events(&node) {
                    events.push(sys);
                }

                // 3. Citation evidence
                events.push(WorldEvent::CitationPublished {
                    citation_id: Uuid::new_v4(),
                    signal_id: node_id,
                    url: url.to_string(),
                    content_hash: content_hash_str.clone(),
                    snippet: node.meta().map(|m| m.summary.clone()),
                    relevance: None,
                    channel_type: Some(rootsignal_common::channel_type(url)),
                    evidence_confidence: None,
                });

                // 4. Resolve actor inline (batch-scoped dedup via seen_actors)
                let resolved_actor = resolve_actor_inline(
                    node_id, url, &author_name, batch.source_id,
                    &mut seen_actors, &mut events, deps,
                ).await?;

                // 5. Build verdict
                verdicts.push(DedupOutcome::Created {
                    node_id,
                    node_type,
                    content_hash: content_hash_str.clone(),
                    url: url.to_string(),
                    canonical_key: ck.clone(),
                    resource_tags: node_resource_tags,
                    signal_tags: node_signal_tags,
                    source_id: batch.source_id,
                    actor: resolved_actor,
                });
            }
        }
    }

    events.push(SignalEvent::DedupCompleted {
        url: url.to_string(),
        canonical_key: canonical_key.to_string(),
        verdicts,
    });

    Ok(events)
}

/// Build corroboration events + verdict for a cross-source match.
///
/// Tracks cumulative counts per target within the batch via `corroboration_counts`.
async fn build_corroboration(
    existing_id: Uuid,
    node_type: NodeType,
    source_url: &str,
    similarity: f64,
    content_hash: &str,
    deps: &ScoutEngineDeps,
    corroboration_counts: &mut HashMap<Uuid, u32>,
) -> Result<(u32, (Events, DedupOutcome))> {
    // Read base count once per unique target, then increment for batch duplicates
    let base_count = if let std::collections::hash_map::Entry::Vacant(e) = corroboration_counts.entry(existing_id) {
        let count = deps
            .store
            .read_corroboration_count(existing_id, node_type)
            .await
            .unwrap_or(0);
        e.insert(count);
        count
    } else {
        *corroboration_counts.get(&existing_id).unwrap()
    };

    let new_count = base_count + 1;
    *corroboration_counts.get_mut(&existing_id).unwrap() = new_count;

    let mut events = Events::new();

    events.push(WorldEvent::CitationPublished {
        citation_id: Uuid::new_v4(),
        signal_id: existing_id,
        url: source_url.to_string(),
        content_hash: content_hash.to_string(),
        snippet: None,
        relevance: None,
        channel_type: Some(rootsignal_common::channel_type(source_url)),
        evidence_confidence: None,
    });

    events.push(SystemEvent::ObservationCorroborated {
        signal_id: existing_id,
        node_type,
        new_url: source_url.to_string(),
        summary: None,
    });

    events.push(SystemEvent::CorroborationScored {
        signal_id: existing_id,
        similarity,
        new_corroboration_count: new_count,
    });

    let verdict = DedupOutcome::Corroborated {
        existing_id,
        node_type,
        similarity,
        url: source_url.to_string(),
        new_corroboration_count: new_count,
    };

    Ok((new_count, (events, verdict)))
}

/// Build verdict for a same-source re-encounter.
///
/// No events emitted — "still there" is the default assumption.
/// The projection bumps `last_confirmed_active` from the verdict directly.
fn build_freshness(
    existing_id: Uuid,
    node_type: NodeType,
    source_url: &str,
) -> DedupOutcome {
    DedupOutcome::Refreshed {
        existing_id,
        node_type,
        url: source_url.to_string(),
    }
}

/// Resolve author → Actor node on owned sources.
///
/// Uses `seen_actors` to avoid duplicate actor creation for same-author
/// signals within the same batch.
async fn resolve_actor_inline(
    signal_id: Uuid,
    source_url: &str,
    author_name: &Option<String>,
    source_id: Option<Uuid>,
    seen_actors: &mut HashMap<String, Uuid>,
    events: &mut Events,
    deps: &ScoutEngineDeps,
) -> Result<Option<ResolvedActor>> {
    let author_name = match author_name {
        Some(name) => name,
        None => return Ok(None),
    };

    let strategy = rootsignal_common::scraping_strategy(source_url);
    if !is_owned_source(&strategy) {
        return Ok(None);
    }

    let canonical_key = rootsignal_common::canonical_value(source_url);

    // Check batch-local cache first
    if let Some(&cached_id) = seen_actors.get(&canonical_key) {
        events.push(SystemEvent::ActorLinkedToSignal {
            actor_id: cached_id,
            signal_id,
            role: "authored".to_string(),
        });
        return Ok(Some(ResolvedActor {
            actor_id: cached_id,
            is_new: false,
            name: author_name.clone(),
            canonical_key,
            source_id,
        }));
    }

    // Store lookup
    match deps.store.find_actor_by_canonical_key(&canonical_key).await {
        Ok(Some(actor_id)) => {
            seen_actors.insert(canonical_key.clone(), actor_id);
            events.push(SystemEvent::ActorLinkedToSignal {
                actor_id,
                signal_id,
                role: "authored".to_string(),
            });
            Ok(Some(ResolvedActor {
                actor_id,
                is_new: false,
                name: author_name.clone(),
                canonical_key,
                source_id,
            }))
        }
        Ok(None) => {
            let new_id = Uuid::new_v4();
            seen_actors.insert(canonical_key.clone(), new_id);

            events.push(SystemEvent::ActorIdentified {
                actor_id: new_id,
                name: author_name.to_string(),
                actor_type: rootsignal_common::ActorType::Organization,
                canonical_key: canonical_key.clone(),
                domains: vec![],
                social_urls: vec![],
                description: String::new(),
                bio: None,
                location_lat: None,
                location_lng: None,
                location_name: None,
            });
            if let Some(sid) = source_id {
                events.push(WorldEvent::ActorLinkedToSource {
                    actor_id: new_id,
                    source_id: sid,
                });
            }
            events.push(SystemEvent::ActorLinkedToSignal {
                actor_id: new_id,
                signal_id,
                role: "authored".to_string(),
            });
            Ok(Some(ResolvedActor {
                actor_id: new_id,
                is_new: true,
                name: author_name.clone(),
                canonical_key,
                source_id,
            }))
        }
        Err(e) => {
            warn!(error = %e, actor = author_name, "Actor lookup failed");
            Ok(None)
        }
    }
}
