//! Dedup handler — 4-layer deduplication for extracted signals.
//!
//! Triggered by `SignalsExtracted` (fact: "extraction produced N nodes for this URL").
//! Pulls the extracted batch from state, runs dedup layers, emits per-node facts:
//!
//! - `NewSignalAccepted` — passed all dedup layers, this is a new signal
//! - `CrossSourceMatchDetected` — found from a different source
//! - `SameSourceReencountered` — re-encountered from the same source
//!
//! Layer 1 (batch title dedup) is applied by the caller before stashing.
//! This handler runs layers 2–4.

use std::collections::HashSet;

use anyhow::Result;
use rootsignal_common::types::NodeType;
use tracing::{info, warn};

use crate::infra::util::sanitize_url;
use crate::pipeline::events::{PipelineEvent, ScoutEvent};
use crate::pipeline::scrape_phase::{dedup_verdict, normalize_title, DedupVerdict};
use crate::pipeline::state::{PendingNode, PipelineDeps, PipelineState};

/// Handle `SignalsExtracted`: pull the extracted batch from state and run dedup.
pub async fn handle_signals_extracted(
    url: &str,
    state: &PipelineState,
    deps: &PipelineDeps,
) -> Result<Vec<ScoutEvent>> {
    let batch = match state.extracted_batches.get(url) {
        Some(b) => b,
        None => {
            tracing::warn!(url, "SignalsExtracted: no extracted batch found in state");
            return Ok(vec![]);
        }
    };

    if batch.nodes.is_empty() {
        return Ok(vec![ScoutEvent::Pipeline(PipelineEvent::DedupCompleted {
            url: url.to_string(),
        })]);
    }

    let content_hash_str = format!("{:x}", rootsignal_common::content_hash(&batch.content));
    let mut events = Vec::new();

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
        events.push(ScoutEvent::Pipeline(PipelineEvent::DedupCompleted {
            url: url.to_string(),
        }));
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
                events.push(ScoutEvent::Pipeline(PipelineEvent::CrossSourceMatchDetected {
                    existing_id,
                    node_type: node.node_type(),
                    source_url: url.to_string(),
                    similarity,
                }));
            }
            DedupVerdict::Refresh {
                existing_id,
                similarity,
                ..
            } => {
                info!(
                    existing_id = %existing_id,
                    title = node.title(),
                    source = url,
                    "Same-source title match"
                );
                events.push(ScoutEvent::Pipeline(PipelineEvent::SameSourceReencountered {
                    existing_id,
                    node_type: node.node_type(),
                    source_url: url.to_string(),
                    similarity,
                }));
            }
            DedupVerdict::Create => {
                remaining_nodes.push(node);
            }
        }
    }
    let nodes = remaining_nodes;

    if nodes.is_empty() {
        events.push(ScoutEvent::Pipeline(PipelineEvent::DedupCompleted {
            url: url.to_string(),
        }));
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
    let (lat_delta, lng_delta) = match &deps.region {
        Some(r) => {
            let lat_d = r.radius_km / 111.0;
            let lng_d = r.radius_km / (111.0 * r.center_lat.to_radians().cos());
            (lat_d, lng_d)
        }
        None => (90.0, 180.0), // global fallback
    };
    let (center_lat, center_lng) = deps
        .region
        .as_ref()
        .map(|r| (r.center_lat, r.center_lng))
        .unwrap_or((0.0, 0.0));

    for (node, embedding) in nodes.into_iter().zip(embeddings.into_iter()) {
        let node_type = node.node_type();
        if node_type == NodeType::Citation {
            continue;
        }

        // 3a: Check in-memory cache first
        let cache_hit = state.embed_cache.find_match(&embedding, 0.85);

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
                let source_layer = if cache_hit.is_some() { "cache" } else { "graph" };
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
                        state.embed_cache.add(
                            embedding,
                            existing_id,
                            node_type,
                            sanitized_url.clone(),
                        );
                    }
                }
                events.push(ScoutEvent::Pipeline(PipelineEvent::SameSourceReencountered {
                    existing_id,
                    node_type,
                    source_url: url.to_string(),
                    similarity,
                }));
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
                        state.embed_cache.add(
                            embedding,
                            existing_id,
                            node_type,
                            sanitized_url.clone(),
                        );
                    }
                }
                events.push(ScoutEvent::Pipeline(PipelineEvent::CrossSourceMatchDetected {
                    existing_id,
                    node_type,
                    source_url: url.to_string(),
                    similarity,
                }));
            }
            DedupVerdict::Create => {
                let node_id = node.id();
                let meta_id = node.meta().map(|m| m.id);

                // Add to embed cache (interior mutability — exception for deterministic cache)
                state.embed_cache.add(
                    embedding.clone(),
                    node_id,
                    node_type,
                    url.to_string(),
                );

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

                let title = node.title().to_string();
                let pn = PendingNode {
                    node,
                    embedding,
                    content_hash: content_hash_str.clone(),
                    resource_tags: node_resource_tags,
                    signal_tags: node_signal_tags,
                    author_name,
                    source_id: batch.source_id,
                };

                events.push(ScoutEvent::Pipeline(PipelineEvent::NewSignalAccepted {
                    node_id,
                    node_type,
                    title,
                    source_url: url.to_string(),
                    pending_node: Box::new(pn),
                }));
            }
        }
    }

    events.push(ScoutEvent::Pipeline(PipelineEvent::DedupCompleted {
        url: url.to_string(),
    }));

    Ok(events)
}
