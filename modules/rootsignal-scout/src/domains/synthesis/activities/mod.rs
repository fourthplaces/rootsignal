//! Synthesis domain activity functions: pure logic extracted from handlers.

pub mod gathering_finder;
pub mod investigator;
pub mod response_finder;
pub mod response_mapper;
pub mod tension_linker;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use seesaw_core::Events;
use tracing::{info, warn};

use rootsignal_graph::{GraphClient, GraphStore, SimilarityBuilder};

use crate::domains::discovery::events::DiscoveryEvent;
use crate::infra::embedder::TextEmbedder;
use crate::domains::scheduling::activities::budget::{BudgetTracker, OperationCost};

/// Output from the synthesis activity: all events ready to emit.
pub struct SynthesisOutput {
    pub events: Events,
}

/// Run parallel synthesis: similarity edges, response mapping, tension linker,
/// response finder, gathering finder, investigation, and severity inference.
///
/// Pure: takes specific deps, returns events and discovered sources.
pub async fn run_synthesis(
    writer: &GraphStore,
    graph_client: &GraphClient,
    archive: Arc<rootsignal_archive::Archive>,
    embedder: &dyn TextEmbedder,
    api_key: &str,
    region: &rootsignal_common::ScoutScope,
    budget: &BudgetTracker,
    cancelled: Arc<AtomicBool>,
    run_id: String,
) -> SynthesisOutput {
    // Budget checks
    let run_response_mapping =
        budget.has_budget(OperationCost::CLAUDE_HAIKU_SYNTHESIS * 10);
    let run_tension_linker = budget.has_budget(
        OperationCost::CLAUDE_HAIKU_TENSION_LINKER
            + OperationCost::SEARCH_TENSION_LINKER,
    );
    let run_response_finder = budget.has_budget(
        OperationCost::CLAUDE_HAIKU_RESPONSE_FINDER
            + OperationCost::SEARCH_RESPONSE_FINDER,
    );
    let run_gathering_finder = budget.has_budget(
        OperationCost::CLAUDE_HAIKU_GATHERING_FINDER
            + OperationCost::SEARCH_GATHERING_FINDER,
    );
    let run_investigation = budget.has_budget(
        OperationCost::CLAUDE_HAIKU_INVESTIGATION
            + OperationCost::SEARCH_INVESTIGATION,
    );

    let region_owned = region.clone();

    info!("Starting parallel synthesis (similarity edges, response mapping, tension linker, response finder, gathering finder, investigation)...");

    let (
        _sim_result,
        rm_events,
        tl_events,
        (rf_events, rf_sources),
        (gf_events, gf_sources),
        inv_events,
    ) = tokio::join!(
        // Similarity edges
        async {
            info!("Building similarity edges...");
            let similarity = SimilarityBuilder::new(graph_client.clone());
            similarity.clear_edges().await.unwrap_or_else(|e| {
                warn!(error = %e, "Failed to clear similarity edges");
                0
            });
            match similarity.build_edges().await {
                Ok(edges) => info!(edges, "Similarity edges built"),
                Err(e) => {
                    warn!(error = %e, "Similarity edge building failed (non-fatal)")
                }
            }
        },
        // Response mapping
        async {
            let mut events = Events::new();
            if run_response_mapping {
                info!("Starting response mapping...");
                let response_mapper =
                    response_mapper::ResponseMapper::new(
                        writer,
                        api_key,
                        region_owned.center_lat,
                        region_owned.center_lng,
                        region_owned.radius_km,
                    );
                match response_mapper.map_responses(&mut events).await {
                    Ok(rm_stats) => info!("{rm_stats}"),
                    Err(e) => {
                        warn!(error = %e, "Response mapping failed (non-fatal)")
                    }
                }
            } else if budget.is_active() {
                info!("Skipping response mapping (budget exhausted)");
            }
            events
        },
        // Tension linker
        async {
            let mut events = Events::new();
            if run_tension_linker {
                info!("Starting tension linker...");
                let tension_linker =
                    tension_linker::TensionLinker::new(
                        writer,
                        archive.clone(),
                        embedder,
                        api_key,
                        region_owned.clone(),
                        cancelled.clone(),
                        run_id.clone(),
                    );
                let tl_stats = tension_linker.run(&mut events).await;
                info!("{tl_stats}");
            } else if budget.is_active() {
                info!("Skipping tension linker (budget exhausted)");
            }
            events
        },
        // Response finder
        async {
            let mut events = Events::new();
            if run_response_finder {
                info!("Starting response finder...");
                let response_finder =
                    response_finder::ResponseFinder::new(
                        writer,
                        archive.clone(),
                        embedder,
                        api_key,
                        region_owned.clone(),
                        cancelled.clone(),
                        run_id.clone(),
                    );
                let (rf_stats, rf_sources) =
                    response_finder.run(&mut events).await;
                info!("{rf_stats}");
                (events, rf_sources)
            } else {
                if budget.is_active() {
                    info!("Skipping response finder (budget exhausted)");
                }
                (events, Vec::new())
            }
        },
        // Gathering finder
        async {
            let mut events = Events::new();
            if run_gathering_finder {
                info!("Starting gathering finder...");
                let gathering_finder =
                    gathering_finder::GatheringFinder::new(
                        writer,
                        archive.clone(),
                        embedder,
                        api_key,
                        region_owned.clone(),
                        cancelled.clone(),
                        run_id.clone(),
                    );
                let (gf_stats, gf_sources) =
                    gathering_finder.run(&mut events).await;
                info!("{gf_stats}");
                (events, gf_sources)
            } else {
                if budget.is_active() {
                    info!("Skipping gathering finder (budget exhausted)");
                }
                (events, Vec::new())
            }
        },
        // Investigation
        async {
            let mut events = Events::new();
            if run_investigation {
                info!("Starting investigation phase...");
                let investigator =
                    investigator::Investigator::new(
                        writer,
                        archive.clone(),
                        api_key,
                        &region_owned,
                        cancelled.clone(),
                    );
                let inv_stats = investigator.run(&mut events).await;
                info!("{inv_stats}");
            } else if budget.is_active() {
                info!("Skipping investigation (budget exhausted)");
            }
            events
        },
    );

    // Merge all finder events
    let mut all_events = Events::new();
    all_events.extend(rm_events);
    all_events.extend(tl_events);
    all_events.extend(rf_events);
    all_events.extend(gf_events);
    all_events.extend(inv_events);

    // Register discovered sources as events
    let finder_sources: Vec<rootsignal_common::SourceNode> =
        rf_sources.into_iter().chain(gf_sources).collect();
    if !finder_sources.is_empty() {
        info!(
            count = finder_sources.len(),
            "Registering finder-discovered sources"
        );
        for source in finder_sources {
            all_events.push(DiscoveryEvent::SourceDiscovered {
                source,
                discovered_by: "synthesis".into(),
            });
        }
    }

    // Severity inference — re-evaluate Notice severity after tension linking
    let lat_delta = region_owned.radius_km / 111.0;
    let lng_delta = region_owned.radius_km
        / (111.0 * region_owned.center_lat.to_radians().cos());
    let (min_lat, max_lat) = (
        region_owned.center_lat - lat_delta,
        region_owned.center_lat + lat_delta,
    );
    let (min_lng, max_lng) = (
        region_owned.center_lng - lng_delta,
        region_owned.center_lng + lng_delta,
    );
    match rootsignal_graph::severity_inference::run_severity_inference(
        writer, min_lat, max_lat, min_lng, max_lng,
    )
    .await
    {
        Ok(updated) => {
            if updated > 0 {
                info!(updated, "Severity inference updated notices");
            }
        }
        Err(e) => warn!(error = %e, "Severity inference failed (non-fatal)"),
    }

    info!("Parallel synthesis complete");

    SynthesisOutput {
        events: all_events,
    }
}
