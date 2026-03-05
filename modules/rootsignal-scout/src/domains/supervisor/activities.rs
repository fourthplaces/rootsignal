//! Supervisor activities — extracted from workflows/supervisor.rs.

use tracing::{info, warn};

use rootsignal_common::events::SystemEvent;
use rootsignal_common::telemetry_events::TelemetryEvent;
use rootsignal_graph::GraphReader;

use crate::core::engine::ScoutEngineDeps;

/// Run supervisor: issue detection, merge duplicates, cause heat.
/// Returns events (e.g. DuplicateTensionMerged) for the caller to dispatch.
pub async fn supervise(deps: &ScoutEngineDeps, events: &mut seesaw_core::Events) {
    let (graph_client, region, pg_pool, api_key) = match (
        deps.graph_client.as_ref(),
        deps.run_scope.region(),
        deps.pg_pool.as_ref(),
        deps.anthropic_api_key.as_deref(),
    ) {
        (Some(g), Some(r), Some(p), Some(k)) => (g, r, p, k),
        _ => {
            events.push(TelemetryEvent::SystemLog {
                message: "Skipped supervisor: missing graph_client, region, pg_pool, or api_key".into(),
                context: Some(serde_json::json!({
                    "handler": "supervisor:supervise",
                    "reason": "missing_deps",
                    "missing": {
                        "graph_client": deps.graph_client.is_none(),
                        "region": deps.run_scope.region().is_none(),
                        "pg_pool": deps.pg_pool.is_none(),
                        "api_key": deps.anthropic_api_key.is_none(),
                    },
                })),
            });
            return;
        }
    };

    let graph = GraphReader::new(graph_client.clone());
    let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();

    // 1. Run supervisor checks — collects events from auto_fix, echo, source_penalty
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
        Ok((stats, supervisor_events)) => {
            info!(%stats, "Supervisor run complete");
            for evt in supervisor_events {
                events.push(evt);
            }
        }
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
                    events.push(SystemEvent::DuplicateConcernMerged {
                        survivor_id,
                        duplicate_id,
                    });
                }
                info!(merged, "Duplicate tensions merged");
            }
        }
        Err(e) => warn!(error = %e, "Failed to find duplicate tension pairs"),
    }

    // 3. Compute cause heat — push CauseHeatComputed for non-empty scores
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
        Ok(scores) => {
            let hot: Vec<_> = scores.into_iter().filter(|s| s.cause_heat > 0.0).collect();
            if !hot.is_empty() {
                info!(count = hot.len(), "Cause heat computed");
                events.push(SystemEvent::CauseHeatComputed { scores: hot });
            }
        }
        Err(e) => warn!(error = %e, "Failed to compute cause heat"),
    }

    // Beacon detection removed — will be rebuilt as Region-based discovery.
}
