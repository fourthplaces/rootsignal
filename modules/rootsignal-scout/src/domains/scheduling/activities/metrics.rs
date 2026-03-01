//! Source metrics and weight update stage.
//!
//! After scraping completes, this stage records per-source scrape metrics,
//! recomputes weights based on signal production history, updates cadences,
//! and deactivates dead sources/queries.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use tracing::{info, warn};

use rootsignal_common::events::{SourceChange, SystemEvent};
use rootsignal_common::{is_web_query, SourceNode};
use rootsignal_graph::GraphReader;

use seesaw_core::Events;

pub(crate) struct Metrics<'a> {
    graph: &'a GraphReader,
    _region_slug: &'a str,
}

impl<'a> Metrics<'a> {
    pub fn new(
        graph: &'a GraphReader,
        region_slug: &'a str,
    ) -> Self {
        Self {
            graph,
            _region_slug: region_slug,
        }
    }

    /// Compute source metrics, weights, cadences, and find dead sources.
    ///
    /// Emits SourceScraped, SourceChanged (Weight/Cadence), and SourceDeactivated events.
    /// Graph reads (count_source_tensions, find_dead_*, get_source_stats) remain.
    pub async fn compute_source_metrics(
        &self,
        all_sources: &[SourceNode],
        source_signal_counts: &HashMap<String, u32>,
        query_api_errors: &HashSet<String>,
        now: DateTime<Utc>,
    ) -> Events {
        let mut events = Events::new();

        // Record per-source scrape metrics. Skip queries where the search API errored.
        for (canonical_key, signals_produced) in source_signal_counts {
            if query_api_errors.contains(canonical_key) {
                continue;
            }
            events.push(SystemEvent::SourceScraped {
                canonical_key: canonical_key.clone(),
                signals_produced: *signals_produced,
                scraped_at: now,
            });
        }

        // Compute source weights and emit change events.
        for source in all_sources {
            let tension_count = self
                .graph
                .count_source_tensions(&source.canonical_key)
                .await
                .unwrap_or(0);
            let fresh_signals = source_signal_counts
                .get(&source.canonical_key)
                .copied()
                .unwrap_or(0);
            let total_signals = source.signals_produced + fresh_signals;
            let scrape_count =
                if fresh_signals > 0 || source_signal_counts.contains_key(&source.canonical_key) {
                    (source.scrape_count + 1).max(1)
                } else {
                    source.scrape_count.max(1)
                };
            let base_weight = super::scheduler::compute_weight(
                total_signals,
                source.signals_corroborated,
                scrape_count,
                tension_count,
                if fresh_signals > 0 {
                    Some(now)
                } else {
                    source.last_produced_signal
                },
                now,
            );
            let new_weight = (base_weight * source.quality_penalty).clamp(0.1, 1.0);
            let empty_runs =
                if source_signal_counts.contains_key(&source.canonical_key) && fresh_signals == 0 {
                    source.consecutive_empty_runs + 1
                } else {
                    source.consecutive_empty_runs
                };
            let cadence = if is_web_query(&source.canonical_value) {
                super::scheduler::cadence_hours_with_backoff(
                    new_weight,
                    empty_runs,
                    &source.discovery_method,
                )
            } else {
                super::scheduler::cadence_hours_for_weight(new_weight)
            };

            if (new_weight - source.weight).abs() > f64::EPSILON {
                events.push(SystemEvent::SourceChanged {
                    source_id: source.id,
                    canonical_key: source.canonical_key.clone(),
                    change: SourceChange::Weight {
                        old: source.weight,
                        new: new_weight,
                    },
                });
            }
            if source.cadence_hours != Some(cadence) {
                events.push(SystemEvent::SourceChanged {
                    source_id: source.id,
                    canonical_key: source.canonical_key.clone(),
                    change: SourceChange::Cadence {
                        old: source.cadence_hours,
                        new: Some(cadence),
                    },
                });
            }
        }

        // Find and deactivate dead sources (10+ consecutive empty runs, non-curated/human only)
        match self.graph.find_dead_sources(10).await {
            Ok(ids) if !ids.is_empty() => {
                info!(deactivated = ids.len(), "Found dead sources to deactivate");
                events.push(SystemEvent::SourceDeactivated {
                    source_ids: ids,
                    reason: "consecutive_empty_runs >= 10".into(),
                });
            }
            Ok(_) => {}
            Err(e) => warn!(error = %e, "Failed to find dead sources"),
        }

        // Find and deactivate dead web queries (stricter: 5+ empty, 3+ scrapes, 0 signals)
        match self.graph.find_dead_web_queries().await {
            Ok(ids) if !ids.is_empty() => {
                info!(deactivated = ids.len(), "Found dead web queries to deactivate");
                events.push(SystemEvent::SourceDeactivated {
                    source_ids: ids,
                    reason: "unproductive_web_query".into(),
                });
            }
            Ok(_) => {}
            Err(e) => warn!(error = %e, "Failed to find dead web queries"),
        }

        // Source stats
        match self.graph.get_source_stats().await {
            Ok(ss) => {
                info!(
                    total = ss.total,
                    active = ss.active,
                    curated = ss.curated,
                    discovered = ss.discovered,
                    "Source registry stats"
                );
            }
            Err(e) => warn!(error = %e, "Failed to get source stats"),
        }

        events
    }
}
