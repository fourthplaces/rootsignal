//! Supervisor activities — extracted from workflows/supervisor.rs.

use tracing::{info, warn};

use rootsignal_common::events::SystemEvent;
use rootsignal_graph::{GraphReader, GraphStore};

use crate::core::engine::ScoutEngineDeps;

/// Run supervisor: issue detection, merge duplicates, cause heat, beacon detection.
/// Returns events (e.g. DuplicateTensionMerged) for the caller to dispatch.
pub async fn supervise(deps: &ScoutEngineDeps, events: &mut seesaw_core::Events) {
    let (graph_client, region, pg_pool, api_key) = match (
        deps.graph_client.as_ref(),
        deps.region.as_ref(),
        deps.pg_pool.as_ref(),
        deps.anthropic_api_key.as_deref(),
    ) {
        (Some(g), Some(r), Some(p), Some(k)) => (g, r, p, k),
        _ => return,
    };

    let graph = GraphReader::new(graph_client.clone());
    let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();

    // 1. Run supervisor checks
    let notifier: Box<dyn rootsignal_scout_supervisor::notify::backend::NotifyBackend> =
        Box::new(rootsignal_scout_supervisor::notify::noop::NoopBackend);

    let supervisor = rootsignal_scout_supervisor::supervisor::Supervisor::new(
        graph_client.clone(),
        pg_pool.clone(),
        region.clone(),
        api_key.to_string(),
        notifier,
    );

    match supervisor.run().await {
        Ok(stats) => info!(%stats, "Supervisor run complete"),
        Err(e) => warn!(error = %e, "Supervisor run failed"),
    }

    // 2. Merge duplicate tensions (before heat computation)
    match graph
        .find_duplicate_tension_pairs(0.85, min_lat, max_lat, min_lng, max_lng)
        .await
    {
        Ok(pairs) => {
            if !pairs.is_empty() {
                let merged = pairs.len();
                for (survivor_id, duplicate_id) in pairs {
                    events.push(SystemEvent::DuplicateTensionMerged {
                        survivor_id,
                        duplicate_id,
                    });
                }
                info!(merged, "Duplicate tensions merged");
            }
        }
        Err(e) => warn!(error = %e, "Failed to find duplicate tension pairs"),
    }

    // 3. Compute cause heat
    match rootsignal_graph::cause_heat::compute_cause_heat(
        graph_client,
        0.7,
        min_lat,
        max_lat,
        min_lng,
        max_lng,
    )
    .await
    {
        Ok(_) => info!("Cause heat computed"),
        Err(e) => warn!(error = %e, "Failed to compute cause heat"),
    }

    // 4. Detect beacons (geographic signal clusters → new ScoutTasks)
    let graph_rw = GraphStore::new(graph_client.clone());
    match rootsignal_graph::beacon::detect_beacons(graph_client, &graph_rw).await {
        Ok(tasks) if !tasks.is_empty() => info!(count = tasks.len(), "Beacon tasks created"),
        Ok(_) => {}
        Err(e) => warn!(error = %e, "Beacon detection failed"),
    }
}
