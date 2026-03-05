//! Free functions for signal event creation — no `self` dependency.

use std::collections::HashMap;

use anyhow::Result;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use rootsignal_common::{NodeType, SourceNode};
use rootsignal_common::events::SystemEvent;
use seesaw_core::Events;

use crate::domains::discovery::events::DiscoveryEvent;

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

/// Collect a batch SourcesDiscovered event for discovered sources (no dispatch).
pub(crate) fn register_sources_events(
    sources: Vec<SourceNode>,
    discovered_by: &str,
) -> Events {
    if sources.is_empty() {
        return Events::new();
    }
    let mut events = Events::new();
    events.push(DiscoveryEvent::SourcesDiscovered {
        sources,
        discovered_by: discovered_by.into(),
    });
    events
}
