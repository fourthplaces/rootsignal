pub mod actor_extractor;
pub mod actor_location;
pub mod actor_stats;
pub mod diversity;
pub mod domain_filter;
pub mod link_promoter;
pub mod quality;
pub mod signal_reviewer;
pub mod universe_check;

use std::collections::{HashMap, HashSet};

use chrono::Utc;

use rootsignal_common::SourceNode;
use rootsignal_graph::GraphReader;

use crate::domains::scheduling::activities::metrics::Metrics;

/// Compute source weight and cadence metrics, returning events.
pub async fn compute_source_metrics(
    graph: &GraphReader,
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
