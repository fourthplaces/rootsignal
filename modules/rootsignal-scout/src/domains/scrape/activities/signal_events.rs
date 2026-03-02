//! Free functions for signal event creation — no `self` dependency.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use rootsignal_common::{ActorContext, Node, NodeType, SourceNode};
use rootsignal_common::events::SystemEvent;
use seesaw_core::Events;

use crate::core::aggregate::ExtractedBatch;
use crate::core::extractor::ResourceTag;
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::signals::events::SignalEvent;
use crate::infra::util::sanitize_url;

use super::types::{batch_title_dedup, score_and_filter};

/// Collect events for extracted signals (no dispatch).
/// Returns a SignalsExtracted event wrapping an ExtractedBatch.
///
/// Pure: reads from the provided maps, does not mutate any shared state.
pub(crate) fn store_signals_events(
    url: &str,
    content: &str,
    nodes: Vec<Node>,
    resource_tags: Vec<(Uuid, Vec<ResourceTag>)>,
    signal_tags: Vec<(Uuid, Vec<String>)>,
    author_actors: &HashMap<Uuid, String>,
    url_to_canonical_key: &HashMap<String, String>,
    actor_contexts: &HashMap<String, ActorContext>,
    source_id: Option<Uuid>,
) -> Events {
    let url = sanitize_url(url);
    let raw_count = nodes.len() as u32;

    // Score quality, populate from/about locations, remove Evidence nodes
    let ck_for_fallback = url_to_canonical_key
        .get(&url)
        .cloned()
        .unwrap_or_else(|| url.clone());
    let actor_ctx = actor_contexts.get(&ck_for_fallback);
    let nodes = score_and_filter(nodes, &url, actor_ctx);

    if nodes.is_empty() {
        return Events::new();
    }

    // Layer 1: Within-batch dedup by (normalized_title, node_type)
    let nodes = batch_title_dedup(nodes);

    let canonical_key = url_to_canonical_key
        .get(&url)
        .cloned()
        .unwrap_or_else(|| url.clone());

    let batch = ExtractedBatch {
        content: content.to_string(),
        nodes,
        resource_tags: resource_tags.into_iter().collect(),
        signal_tags: signal_tags.into_iter().collect(),
        author_actors: author_actors.clone(),
        source_id,
    };

    let mut events = Events::new();
    events.push(SignalEvent::SignalsExtracted {
        url,
        canonical_key,
        count: raw_count,
        batch: Box::new(batch),
    });
    events
}

/// Collect FreshnessConfirmed events for unchanged URLs (no dispatch).
pub(crate) async fn refresh_url_signals_events(
    store: &dyn crate::traits::SignalReader,
    url: &str,
    now: DateTime<Utc>,
) -> Result<Events> {
    let all_ids = store.signal_ids_for_url(url).await?;
    if all_ids.is_empty() {
        return Ok(Events::new());
    }

    // Group by NodeType for batch FreshnessConfirmed events
    let mut by_type: HashMap<NodeType, Vec<Uuid>> = HashMap::new();
    for (id, nt) in &all_ids {
        by_type.entry(*nt).or_default().push(*id);
    }

    let mut events = Events::new();
    for (node_type, ids) in by_type {
        events.push(SystemEvent::FreshnessConfirmed {
            signal_ids: ids,
            node_type,
            confirmed_at: now,
        });
    }
    Ok(events)
}

/// Collect SourceDiscovered events for discovered sources (no dispatch).
pub(crate) fn register_sources_events(
    sources: Vec<SourceNode>,
    discovered_by: &str,
) -> Events {
    let mut events = Events::new();
    for source in sources {
        events.push(DiscoveryEvent::SourceDiscovered {
            source,
            discovered_by: discovered_by.into(),
        });
    }
    events
}
