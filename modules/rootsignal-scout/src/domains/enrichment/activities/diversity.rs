//! Diversity metric computation activity — reads graph evidence, emits events.

use tracing::info;

use rootsignal_common::events::{SignalDiversityScore, SystemEvent};
use rootsignal_common::EntityMappingOwned;
use rootsignal_graph::{compute_diversity_metrics, GraphReader};

/// Read evidence per signal label, compute diversity metrics, return events.
pub async fn compute_diversity_events(
    reader: &GraphReader,
    entity_mappings: &[EntityMappingOwned],
) -> seesaw_core::Events {
    let mut all_metrics = Vec::new();

    for label in &["Gathering", "Resource", "HelpRequest", "Announcement", "Concern", "Condition"] {
        let rows = match reader.signal_evidence_for_diversity(label).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, label, "Failed to read evidence for diversity");
                continue;
            }
        };

        for (id, self_url, evidence) in &rows {
            let m = compute_diversity_metrics(self_url, evidence, entity_mappings);
            all_metrics.push(SignalDiversityScore {
                signal_id: *id,
                label: label.to_string(),
                source_diversity: m.source_diversity,
                channel_diversity: m.channel_diversity,
                external_ratio: m.external_ratio,
            });
        }
    }

    info!(count = all_metrics.len(), "Diversity metrics computed");

    let mut events = seesaw_core::Events::new();
    if !all_metrics.is_empty() {
        events.push(SystemEvent::SignalDiversityComputed {
            metrics: all_metrics,
        });
    }
    events
}
