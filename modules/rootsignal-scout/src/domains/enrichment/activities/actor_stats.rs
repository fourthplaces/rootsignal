//! Actor stats computation activity — reads ACTED_IN counts, emits events.

use tracing::info;

use rootsignal_common::events::{ActorStatScore, SystemEvent};
use rootsignal_graph::GraphReader;

/// Read ACTED_IN counts per actor, return events.
pub async fn compute_actor_stats_events(reader: &GraphReader) -> seesaw_core::Events {
    let counts = match reader.actor_signal_counts().await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to read actor signal counts");
            return seesaw_core::Events::new();
        }
    };

    info!(count = counts.len(), "Actor stats computed");

    let mut events = seesaw_core::Events::new();
    if !counts.is_empty() {
        let stats: Vec<ActorStatScore> = counts
            .into_iter()
            .map(|(actor_id, signal_count)| ActorStatScore {
                actor_id,
                signal_count,
            })
            .collect();
        events.push(SystemEvent::ActorStatsComputed { stats });
    }
    events
}
