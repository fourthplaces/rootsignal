//! Source metrics and weight update stage.
//!
//! After scraping completes, this stage records per-source scrape metrics,
//! recomputes weights based on signal production history, updates cadences,
//! and deactivates dead sources/queries.

use chrono::{DateTime, Utc};
use tracing::{info, warn};

use rootsignal_common::{SourceNode, SourceType};
use rootsignal_graph::GraphWriter;

use crate::scrape_phase::RunContext;

pub(crate) struct Metrics<'a> {
    writer: &'a GraphWriter,
    city_slug: &'a str,
}

impl<'a> Metrics<'a> {
    pub fn new(writer: &'a GraphWriter, city_slug: &'a str) -> Self {
        Self { writer, city_slug }
    }

    /// Update source metrics, weights, cadences, and deactivate dead sources.
    ///
    /// Takes an immutable reference to `RunContext` — reads signal counts and
    /// query errors but does not mutate them. Uses `all_sources` (the snapshot
    /// from the start of the run, NOT `fresh_sources`).
    pub async fn update(
        &self,
        all_sources: &[SourceNode],
        ctx: &RunContext,
        now: DateTime<Utc>,
    ) {
        // Record per-source scrape metrics. Skip queries where the search API errored.
        for (canonical_key, signals_produced) in &ctx.source_signal_counts {
            if ctx.query_api_errors.contains(canonical_key) {
                continue;
            }
            if let Err(e) = self
                .writer
                .record_source_scrape(canonical_key, *signals_produced, now)
                .await
            {
                warn!(canonical_key, error = %e, "Failed to record source scrape metrics");
            }
        }

        // Update source weights based on scrape results.
        for source in all_sources {
            let tension_count = self
                .writer
                .count_source_tensions(&source.canonical_key)
                .await
                .unwrap_or(0);
            let fresh_signals = ctx
                .source_signal_counts
                .get(&source.canonical_key)
                .copied()
                .unwrap_or(0);
            let total_signals = source.signals_produced + fresh_signals;
            let scrape_count = if fresh_signals > 0
                || ctx
                    .source_signal_counts
                    .contains_key(&source.canonical_key)
            {
                (source.scrape_count + 1).max(1)
            } else {
                source.scrape_count.max(1)
            };
            let base_weight = crate::scheduler::compute_weight(
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
            let empty_runs = if ctx
                .source_signal_counts
                .contains_key(&source.canonical_key)
                && fresh_signals == 0
            {
                source.consecutive_empty_runs + 1
            } else {
                source.consecutive_empty_runs
            };
            let cadence = if source.source_type == SourceType::WebQuery {
                crate::scheduler::cadence_hours_with_backoff(new_weight, empty_runs)
            } else {
                crate::scheduler::cadence_hours_for_weight(new_weight)
            };
            if let Err(e) = self
                .writer
                .update_source_weight(&source.canonical_key, new_weight, cadence)
                .await
            {
                warn!(canonical_key = source.canonical_key.as_str(), error = %e, "Failed to update source weight");
            }
        }

        // Deactivate dead sources (10+ consecutive empty runs, non-curated/human only)
        match self
            .writer
            .deactivate_dead_sources(self.city_slug, 10)
            .await
        {
            Ok(n) if n > 0 => info!(deactivated = n, "Deactivated dead sources"),
            Ok(_) => {}
            Err(e) => warn!(error = %e, "Failed to deactivate dead sources"),
        }

        // Deactivate dead web queries (stricter: 5+ empty, 3+ scrapes, 0 signals)
        match self
            .writer
            .deactivate_dead_web_queries(self.city_slug)
            .await
        {
            Ok(n) if n > 0 => info!(deactivated = n, "Deactivated dead web queries"),
            Ok(_) => {}
            Err(e) => warn!(error = %e, "Failed to deactivate dead web queries"),
        }

        // Source stats
        match self.writer.get_source_stats(self.city_slug).await {
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
    }
}
