//! Dedup activity — fingerprint-based deduplication for extracted signals.
//!
//! A signal's identity is `(url, node_type)`. If that pair already exists
//! in the graph, the signal is a re-encounter (Refreshed). Otherwise it's new
//! (Created).
//!
//! Step 0 (batch dedup) is applied by the caller before stashing.
//! This activity runs the fingerprint check against Neo4j.

use std::collections::HashMap;

use anyhow::Result;
use rootsignal_common::types::NodeType;
use tracing::{info, warn};
use uuid::Uuid;

use crate::domains::signals::events::{
    ActorAction, CreatedSignal, DedupBatchResult, DedupOutcome,
    NewCitation, ResolvedActor,
};
use crate::domains::signals::activities::dedup_utils::is_owned_source;
use crate::core::engine::ScoutEngineDeps;
use crate::core::aggregate::{ExtractedBatch, PipelineState};

/// Per-signal URL when available, falling back to the batch-level URL.
fn signal_url<'a>(node: &'a rootsignal_common::Node, batch_url: &'a str) -> &'a str {
    node.meta()
        .map(|m| m.url.as_str())
        .filter(|u| !u.is_empty())
        .unwrap_or(batch_url)
}

/// Run fingerprint dedup on an extracted batch, returning domain types.
pub async fn deduplicate_extracted_batch(
    url: &str,
    canonical_key: &str,
    batch: &ExtractedBatch,
    state: &PipelineState,
    deps: &ScoutEngineDeps,
) -> Result<DedupBatchResult> {
    let empty_result = || DedupBatchResult {
        created: Vec::new(),
        actor_actions: Vec::new(),
        verdicts: Vec::new(),
    };

    if batch.nodes.is_empty() {
        return Ok(empty_result());
    }

    let content_hash_str = format!("{:x}", rootsignal_common::content_hash(&batch.content));
    let mut created = Vec::new();
    let mut actor_actions = Vec::new();
    let mut verdicts: Vec<DedupOutcome> = Vec::new();

    let mut seen_actors: HashMap<String, Uuid> = HashMap::new();

    let ck = state
        .url_to_canonical_key
        .get(url)
        .cloned()
        .unwrap_or_else(|| canonical_key.to_string());

    // --- Fingerprint dedup: (url, node_type) ---
    let fingerprints: Vec<(String, NodeType)> = batch.nodes
        .iter()
        .map(|n| (signal_url(n, url).to_string(), n.node_type()))
        .collect();

    let fingerprint_matches = deps
        .store
        .find_by_fingerprints(&fingerprints)
        .await
        .unwrap_or_default();

    for node in &batch.nodes {
        let node_type = node.node_type();
        if node_type == NodeType::Citation {
            continue;
        }

        let sig_url = signal_url(node, url);
        let fp_key = (sig_url.to_string(), node_type);

        if let Some((existing_id, _existing_ck)) = fingerprint_matches.get(&fp_key) {
            info!(
                existing_id = %existing_id,
                title = node.title(),
                url = sig_url,
                "Fingerprint match — refreshing"
            );
            verdicts.push(DedupOutcome::Refreshed {
                existing_id: *existing_id,
                node_type,
                url: sig_url.to_string(),
            });
            continue;
        }

        // New signal — create
        let node_id = node.id();
        let meta_id = node.meta().map(|m| m.id);

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
            url: sig_url.to_string(),
            content_hash: content_hash_str.clone(),
            snippet: node.meta().map(|m| m.summary.clone()),
            channel_type: Some(rootsignal_common::channel_type(sig_url)),
        };

        let resolved_actor = resolve_actor_inline(
            node_id, url, &author_name, batch.source_id,
            &mut seen_actors, &mut actor_actions, deps,
        ).await?;

        verdicts.push(DedupOutcome::Created {
            node_id,
            node_type,
            content_hash: content_hash_str.clone(),
            url: sig_url.to_string(),
            canonical_key: ck.clone(),
            resource_tags: node_resource_tags,
            signal_tags: node_signal_tags,
            source_id: batch.source_id,
            actor: resolved_actor,
        });

        created.push(CreatedSignal { node: node.clone(), citation });
    }

    Ok(DedupBatchResult { created, actor_actions, verdicts })
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
