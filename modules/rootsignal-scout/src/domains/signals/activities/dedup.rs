//! Dedup activity — 4-layer deduplication for extracted signals.
//!
//! Returns domain types (`DedupBatchResult`). The handler maps these to events.
//!
//! - Create: node + citation + actor actions + DedupOutcome::Created
//! - Corroborate: citation + corroboration data + DedupOutcome::Corroborated
//! - Refresh: DedupOutcome::Refreshed (no events — freshness is the default)
//!
//! Layer 1 (batch title dedup) is applied by the caller before stashing.
//! This activity runs layers 2–4.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use rootsignal_common::types::NodeType;
use tracing::{info, warn};
use uuid::Uuid;

use crate::domains::signals::events::{
    ActorAction, Corroboration, CreatedSignal, DedupBatchResult, DedupOutcome,
    NewCitation, ResolvedActor,
};
use crate::domains::signals::activities::dedup_utils::{dedup_verdict, is_owned_source, normalize_title, DedupVerdict};
use crate::core::engine::ScoutEngineDeps;
use crate::core::aggregate::{ExtractedBatch, PipelineState};

/// Run 4-layer dedup on an extracted batch, returning domain types.
pub async fn deduplicate_extracted_batch(
    url: &str,
    canonical_key: &str,
    batch: &ExtractedBatch,
    state: &PipelineState,
    deps: &ScoutEngineDeps,
) -> Result<DedupBatchResult> {
    let empty_result = || DedupBatchResult {
        created: Vec::new(),
        corroborations: Vec::new(),
        actor_actions: Vec::new(),
        verdicts: Vec::new(),
    };

    if batch.nodes.is_empty() {
        return Ok(empty_result());
    }

    let content_hash_str = format!("{:x}", rootsignal_common::content_hash(&batch.content));
    let mut created = Vec::new();
    let mut corroborations = Vec::new();
    let mut actor_actions = Vec::new();
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
        return Ok(empty_result());
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
                let (corr, verdict) = build_corroboration(
                    existing_id, node.node_type(), url, similarity,
                    &content_hash_str, deps, &mut corroboration_counts,
                ).await?;
                corroborations.push(corr);
                verdicts.push(verdict);
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
        return Ok(DedupBatchResult { created, corroborations, actor_actions, verdicts });
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
                    let (corr, verdict) = build_corroboration(
                        existing_id, node.node_type(), url, similarity,
                        &content_hash_str, deps, &mut corroboration_counts,
                    ).await?;
                    corroborations.push(corr);
                    verdicts.push(verdict);
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
        return Ok(DedupBatchResult { created, corroborations, actor_actions, verdicts });
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
            return Ok(DedupBatchResult { created, corroborations, actor_actions, verdicts });
        }
    };

    // --- Layer 3: Vector dedup (cache + graph) ---
    let (lat_delta, lng_delta) = match state.run_scope.region() {
        Some(r) => {
            let lat_d = r.radius_km / 111.0;
            let lng_d = r.radius_km / (111.0 * r.center_lat.to_radians().cos());
            (lat_d, lng_d)
        }
        None => (90.0, 180.0),
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
                let (corr, verdict) = build_corroboration(
                    existing_id, node_type, url, similarity,
                    &content_hash_str, deps, &mut corroboration_counts,
                ).await?;
                corroborations.push(corr);
                verdicts.push(verdict);
            }
            DedupVerdict::Create => {
                let node_id = node.id();
                let meta_id = node.meta().map(|m| m.id);

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

                let citation = NewCitation {
                    citation_id: Uuid::new_v4(),
                    signal_id: node_id,
                    url: url.to_string(),
                    content_hash: content_hash_str.clone(),
                    snippet: node.meta().map(|m| m.summary.clone()),
                    channel_type: Some(rootsignal_common::channel_type(url)),
                };

                let resolved_actor = resolve_actor_inline(
                    node_id, url, &author_name, batch.source_id,
                    &mut seen_actors, &mut actor_actions, deps,
                ).await?;

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

                created.push(CreatedSignal { node, citation });
            }
        }
    }

    Ok(DedupBatchResult { created, corroborations, actor_actions, verdicts })
}

/// Build corroboration data + verdict for a cross-source match.
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
) -> Result<(Corroboration, DedupOutcome)> {
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

    let corroboration = Corroboration {
        signal_id: existing_id,
        node_type,
        url: source_url.to_string(),
        similarity,
        new_corroboration_count: new_count,
        citation: NewCitation {
            citation_id: Uuid::new_v4(),
            signal_id: existing_id,
            url: source_url.to_string(),
            content_hash: content_hash.to_string(),
            snippet: None,
            channel_type: Some(rootsignal_common::channel_type(source_url)),
        },
    };

    let verdict = DedupOutcome::Corroborated {
        existing_id,
        node_type,
        similarity,
        url: source_url.to_string(),
        new_corroboration_count: new_count,
    };

    Ok((corroboration, verdict))
}

/// Build verdict for a same-source re-encounter (no events needed).
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

/// Resolve author → Actor on owned sources.
///
/// Appends `ActorAction`s to the accumulator instead of constructing events.
/// Uses `seen_actors` to avoid duplicate actor creation within the same batch.
async fn resolve_actor_inline(
    signal_id: Uuid,
    source_url: &str,
    author_name: &Option<String>,
    source_id: Option<Uuid>,
    seen_actors: &mut HashMap<String, Uuid>,
    actor_actions: &mut Vec<ActorAction>,
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

    if let Some(&cached_id) = seen_actors.get(&canonical_key) {
        actor_actions.push(ActorAction::LinkedToSignal {
            actor_id: cached_id,
            signal_id,
        });
        return Ok(Some(ResolvedActor {
            actor_id: cached_id,
            is_new: false,
            name: author_name.clone(),
            canonical_key,
            source_id,
        }));
    }

    match deps.store.find_actor_by_canonical_key(&canonical_key).await {
        Ok(Some(actor_id)) => {
            seen_actors.insert(canonical_key.clone(), actor_id);
            actor_actions.push(ActorAction::LinkedToSignal {
                actor_id,
                signal_id,
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

            actor_actions.push(ActorAction::Identified {
                actor_id: new_id,
                name: author_name.to_string(),
                canonical_key: canonical_key.clone(),
            });
            if let Some(sid) = source_id {
                actor_actions.push(ActorAction::LinkedToSource {
                    actor_id: new_id,
                    source_id: sid,
                });
            }
            actor_actions.push(ActorAction::LinkedToSignal {
                actor_id: new_id,
                signal_id,
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
