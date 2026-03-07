//! Free functions for signal event creation — no `self` dependency.

use rootsignal_common::SourceNode;
use seesaw_core::Events;

use crate::domains::discovery::events::DiscoveryEvent;

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
