//! Situation weaving activities — extracted from workflows/situation_weaver.rs.

use std::sync::Arc;

use tracing::{info, warn};

use rootsignal_common::events::SystemEvent;
use rootsignal_graph::{GraphReader, GraphStore};

use crate::core::engine::ScoutEngineDeps;
use crate::domains::scheduling::activities::budget::OperationCost;

/// Run situation weaving: assign signals to situations, boost hot sources, trigger curiosity.
/// Returns events emitted during the weaving process (e.g. CuriosityTriggered).
pub async fn weave_situations(deps: &ScoutEngineDeps) -> seesaw_core::Events {
    let mut events = seesaw_core::Events::new();

    let (graph_client, api_key, region, budget) = match (
        deps.graph_client.as_ref(),
        deps.anthropic_api_key.as_deref(),
        deps.region.as_ref(),
        deps.budget.as_ref(),
    ) {
        (Some(g), Some(k), Some(r), Some(b)) => (g, k, r, b),
        _ => return events,
    };

    let graph = GraphReader::new(graph_client.clone());
    let graph_rw = GraphStore::new(graph_client.clone());
    let run_id = deps.run_id.clone();

    // 1. Situation weaving (assigns signals to living situations)
    info!("Starting situation weaving...");
    let situation_weaver = rootsignal_graph::SituationWeaver::new(
        graph_client.clone(),
        api_key,
        Arc::clone(&deps.embedder),
        region.clone(),
    );
    let has_situation_budget = budget.has_budget(OperationCost::CLAUDE_HAIKU_STORY_WEAVE);
    match situation_weaver.run(&run_id, has_situation_budget).await {
        Ok(sit_stats) => info!("{sit_stats}"),
        Err(e) => warn!(error = %e, "Situation weaving failed (non-fatal)"),
    }

    // 2. Situation-driven source boost
    match graph.get_situation_landscape(20).await {
        Ok(situations) => {
            let hot: Vec<_> = situations
                .iter()
                .filter(|s| {
                    s.temperature >= 0.6
                        && s.sensitivity != "SENSITIVE"
                        && s.sensitivity != "RESTRICTED"
                })
                .collect();
            if !hot.is_empty() {
                info!(count = hot.len(), "Hot situations boosting source cadence");
                for sit in &hot {
                    if let Err(e) = graph_rw
                        .boost_sources_for_situation_headline(&sit.headline, 1.2)
                        .await
                    {
                        warn!(error = %e, headline = sit.headline.as_str(), "Failed to boost sources for hot situation");
                    }
                }
            }

            let fuzzy: Vec<_> = situations
                .iter()
                .filter(|s| s.clarity == "Fuzzy" && s.temperature >= 0.3)
                .collect();
            if !fuzzy.is_empty() {
                info!(
                    count = fuzzy.len(),
                    "Fuzzy situations identified for investigation: {}",
                    fuzzy
                        .iter()
                        .map(|s| s.headline.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
        Err(e) => warn!(error = %e, "Failed to fetch situation landscape for feedback"),
    }

    // 3. Situation-triggered curiosity re-investigation (read + emit events)
    match graph.find_curiosity_candidates().await {
        Ok(candidates) if !candidates.is_empty() => {
            info!(count = candidates.len(), "Situations triggered curiosity re-investigation");
            for (situation_id, signal_ids) in candidates {
                events.push(SystemEvent::CuriosityTriggered {
                    situation_id,
                    signal_ids,
                });
            }
        }
        Ok(_) => {}
        Err(e) => warn!(error = %e, "Failed to find curiosity candidates"),
    }

    events
}
