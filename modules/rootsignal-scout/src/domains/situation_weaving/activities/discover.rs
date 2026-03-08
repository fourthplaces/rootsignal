//! Signal discovery and candidate loading — delegates to GraphReader.

use uuid::Uuid;

use rootsignal_graph::{GraphQueries, WeaveCandidate, WeaveSignal};

/// Discover unassigned signals from a scout run.
pub async fn discover_signals(
    graph: &dyn GraphQueries,
    scout_run_id: &str,
) -> Result<Vec<WeaveSignal>, Box<dyn std::error::Error + Send + Sync>> {
    Ok(graph.discover_unassigned_signals(scout_run_id).await?)
}

/// Load all situations as candidates for signal assignment.
pub async fn load_candidates(
    graph: &dyn GraphQueries,
) -> Result<Vec<WeaveCandidate>, Box<dyn std::error::Error + Send + Sync>> {
    Ok(graph.load_weave_candidates().await?)
}

/// Find all situations that have signals from this scout run.
pub async fn find_affected_situations(
    graph: &dyn GraphQueries,
    scout_run_id: &str,
) -> Result<Vec<Uuid>, Box<dyn std::error::Error + Send + Sync>> {
    Ok(graph.find_affected_situations(scout_run_id).await?)
}
