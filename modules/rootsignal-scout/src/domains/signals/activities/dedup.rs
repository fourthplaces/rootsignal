//! Dedup handler — 4-layer deduplication for extracted signals.
//!
//! Triggered by `ScrapeRoleCompleted` (carries extracted batches in-memory).
//! Runs dedup layers and emits final facts directly:
//!
//! - Create: WorldEvent + SystemEvents + CitationPublished + SignalCreated
//! - Corroborate: CitationPublished + ObservationCorroborated + CorroborationScored
//! - Refresh: CitationPublished + FreshnessConfirmed
//!
//! Layer 1 (batch title dedup) is applied by the caller before stashing.
//! This handler runs layers 2–4.

use std::collections::HashSet;

use anyhow::Result;
use rootsignal_common::types::NodeType;
use seesaw_core::Events;
use tracing::{info, warn};

use crate::domains::signals::events::SignalEvent;
use crate::infra::util::sanitize_url;
use crate::domains::signals::activities::dedup_utils::{dedup_verdict, normalize_title, DedupVerdict};
use crate::core::engine::ScoutEngineDeps;
use crate::core::aggregate::{ExtractedBatch, PendingNode, PipelineState};

use super::creation;

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
            signals_created: 0,
            signals_deduplicated: 0,
        });
        return Ok(events);
    }

    let content_hash_str = format!("{:x}", rootsignal_common::content_hash(&batch.content));
    let mut events = Events::new();
    let mut created_count: u32 = 0;
    let mut deduped_count: u32 = 0;

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
        deduped_count += url_deduped as u32;
        events.push(SignalEvent::DedupCompleted {
            url: url.to_string(),
            canonical_key: canonical_key.to_string(),
            signals_created: 0,
            signals_deduplicated: deduped_count,
        });
        return Ok(events);
    }

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
        let global_hit = global_matches.get(&key).map(|(id, u)| (*id, u.as_str()));

        match dedup_verdict(url, node.node_type(), global_hit, None, None) {
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
                deduped_count += 1;
                let corr_events = creation::create_corroboration_events(
                    existing_id, node.node_type(), url, similarity, deps,
                ).await?;
                events.extend(corr_events);
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
                deduped_count += 1;
                let fresh_events = creation::create_freshness_events(
                    existing_id, node.node_type(), url, deps,
                ).await?;
                events.extend(fresh_events);
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
            signals_created: 0,
            signals_deduplicated: deduped_count,
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
    let (lat_delta, lng_delta) = match deps.run_scope.region() {
        Some(r) => {
            let lat_d = r.radius_km / 111.0;
            let lng_d = r.radius_km / (111.0 * r.center_lat.to_radians().cos());
            (lat_d, lng_d)
        }
        None => (90.0, 180.0), // global fallback
    };
    let (center_lat, center_lng) = deps
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
                let sanitized = sanitize_url(&dup.source_url);
                Some((dup.id, dup.node_type, sanitized, dup.similarity))
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

        match dedup_verdict(url, node_type, None, cache_match, graph_match) {
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
                    if let Some((_, _, ref sanitized_url, _)) = graph_hit {
                        deps.embed_cache.add(
                            embedding,
                            existing_id,
                            node_type,
                            sanitized_url.clone(),
                        );
                    }
                }
                deduped_count += 1;
                let fresh_events = creation::create_freshness_events(
                    existing_id, node_type, url, deps,
                ).await?;
                events.extend(fresh_events);
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
                    if let Some((_, _, ref sanitized_url, _)) = graph_hit {
                        deps.embed_cache.add(
                            embedding,
                            existing_id,
                            node_type,
                            sanitized_url.clone(),
                        );
                    }
                }
                deduped_count += 1;
                let corr_events = creation::create_corroboration_events(
                    existing_id, node_type, url, similarity, deps,
                ).await?;
                events.extend(corr_events);
            }
            DedupVerdict::Create => {
                let node_id = node.id();
                let meta_id = node.meta().map(|m| m.id);

                // Add to embed cache (interior mutability — exception for deterministic cache)
                deps
                    .embed_cache
                    .add(embedding.clone(), node_id, node_type, url.to_string());

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

                let pn = PendingNode {
                    node,
                    content_hash: content_hash_str.clone(),
                    resource_tags: node_resource_tags,
                    signal_tags: node_signal_tags,
                    author_name,
                    source_id: batch.source_id,
                };

                // Emit final facts directly
                created_count += 1;
                let create_events = creation::create_signal_events(
                    &pn, canonical_key, url, state, deps,
                ).await?;
                events.extend(create_events);
            }
        }
    }

    events.push(SignalEvent::DedupCompleted {
        url: url.to_string(),
        canonical_key: canonical_key.to_string(),
        signals_created: created_count,
        signals_deduplicated: deduped_count,
    });

    Ok(events)
}
