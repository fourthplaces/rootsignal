pub mod actor_location;
pub mod actor_stats;
pub mod diversity;
pub mod domain_filter;
pub mod link_promoter;
pub mod profile_enrichment;
pub mod quality;
pub mod signal_reviewer;
pub mod source_claimer;
pub mod universe_check;

#[cfg(test)]
mod source_claimer_tests;

use std::collections::{HashMap, HashSet};

use chrono::Utc;

use rootsignal_common::SourceNode;
use rootsignal_graph::GraphQueries;

use crate::domains::scheduling::activities::metrics::Metrics;

/// Compute source weight and cadence metrics, returning events.
pub async fn compute_source_metrics(
    graph: &dyn GraphQueries,
    region_name: &str,
    all_sources: &[SourceNode],
    source_signal_counts: &HashMap<String, u32>,
    query_api_errors: &HashSet<String>,
) -> seesaw_core::Events {
    let metrics = Metrics::new(graph, region_name);
    metrics
        .compute_source_metrics(all_sources, source_signal_counts, query_api_errors, Utc::now())
        .await
}
