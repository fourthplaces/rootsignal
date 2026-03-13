//! Actor stats computation activity — reads ACTED_IN counts per actor.

use tracing::info;

use rootsignal_common::events::ActorStatScore;
use rootsignal_graph::GraphQueries;

/// Read ACTED_IN counts per actor.
pub async fn compute_actor_stats(reader: &dyn GraphQueries) -> Vec<ActorStatScore> {
    let counts = match reader.actor_signal_counts().await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to read actor signal counts");
            return Vec::new();
        }
    };

    info!(count = counts.len(), "Actor stats computed");

    counts
        .into_iter()
        .map(|(actor_id, signal_count)| ActorStatScore {
            actor_id,
            signal_count,
        })
        .collect()
}
