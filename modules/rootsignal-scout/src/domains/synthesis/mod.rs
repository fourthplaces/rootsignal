// Synthesis domain: similarity edges, parallel finders, severity inference.

pub mod activities;
pub mod events;
pub mod util;

#[cfg(test)]
mod completion_tests;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use seesaw_core::{events, handle, handlers, Context, Events};
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::events::SystemEvent;
use rootsignal_graph::GraphReader;

use rootsignal_common::telemetry_events::TelemetryEvent;

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::core::events::PipelinePhase;
use crate::core::pipeline_events::PipelineEvent;
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::scheduling::activities::budget::OperationCost;
use crate::domains::synthesis::events::{
    all_synthesis_roles, SynthesisEvent, SynthesisRole,
};

fn is_signal_expansion_completed(e: &LifecycleEvent) -> bool {
    matches!(
        e,
        LifecycleEvent::PhaseCompleted { phase }
            if matches!(phase, PipelinePhase::SignalExpansion)
    )
}

fn is_synthesis_triggered(e: &SynthesisEvent) -> bool {
    matches!(e, SynthesisEvent::SynthesisTriggered { .. })
}

fn is_synthesis_role_completed(e: &SynthesisEvent) -> bool {
    matches!(e, SynthesisEvent::SynthesisRoleCompleted { .. })
}

fn is_synthesis_completed(e: &LifecycleEvent) -> bool {
    matches!(
        e,
        LifecycleEvent::PhaseCompleted { phase }
            if matches!(phase, PipelinePhase::Synthesis)
    )
}

// Per-target filter functions
fn is_concern_linker_target_requested(e: &SynthesisEvent) -> bool {
    matches!(e, SynthesisEvent::ConcernLinkerTargetRequested { .. })
}
fn is_concern_linker_target_completed(e: &SynthesisEvent) -> bool {
    matches!(e, SynthesisEvent::ConcernLinkerTargetCompleted { .. })
}
fn is_response_finder_target_requested(e: &SynthesisEvent) -> bool {
    matches!(e, SynthesisEvent::ResponseFinderTargetRequested { .. })
}
fn is_response_finder_target_completed(e: &SynthesisEvent) -> bool {
    matches!(e, SynthesisEvent::ResponseFinderTargetCompleted { .. })
}
fn is_gathering_finder_target_requested(e: &SynthesisEvent) -> bool {
    matches!(e, SynthesisEvent::GatheringFinderTargetRequested { .. })
}
fn is_gathering_finder_target_completed(e: &SynthesisEvent) -> bool {
    matches!(e, SynthesisEvent::GatheringFinderTargetCompleted { .. })
}
fn is_investigation_target_requested(e: &SynthesisEvent) -> bool {
    matches!(e, SynthesisEvent::InvestigationTargetRequested { .. })
}
fn is_investigation_target_completed(e: &SynthesisEvent) -> bool {
    matches!(e, SynthesisEvent::InvestigationTargetCompleted { .. })
}
fn is_response_mapping_target_requested(e: &SynthesisEvent) -> bool {
    matches!(e, SynthesisEvent::ResponseMappingTargetRequested { .. })
}
fn is_response_mapping_target_completed(e: &SynthesisEvent) -> bool {
    matches!(e, SynthesisEvent::ResponseMappingTargetCompleted { .. })
}

#[handlers]
pub mod handlers {
    use super::*;

    // ---------------------------------------------------------------
    // Trigger: PhaseCompleted(SignalExpansion) → SynthesisTriggered
    // ---------------------------------------------------------------

    #[handle(on = LifecycleEvent, id = "synthesis:trigger", filter = is_signal_expansion_completed)]
    async fn trigger(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();

        // Guard: if deps are missing, skip synthesis entirely
        if deps.region.is_none()
            || deps.graph_client.is_none()
            || deps.budget.is_none()
            || deps.archive.is_none()
        {
            let mut skip = events![LifecycleEvent::PhaseCompleted {
                phase: PipelinePhase::Synthesis,
            }];
            skip.push(TelemetryEvent::SystemLog {
                message: "Skipped entire synthesis phase: missing deps".into(),
                context: Some(serde_json::json!({
                    "handler": "synthesis:trigger",
                    "reason": "missing_deps",
                    "missing": {
                        "region": deps.region.is_none(),
                        "graph_client": deps.graph_client.is_none(),
                        "budget": deps.budget.is_none(),
                        "archive": deps.archive.is_none(),
                    },
                })),
            });
            return Ok(skip);
        }

        let run_id = Uuid::parse_str(&deps.run_id).unwrap_or_else(|_| Uuid::new_v4());
        info!("Synthesis triggered, dispatching to role handlers");
        Ok(events![SynthesisEvent::SynthesisTriggered { run_id }])
    }

    // ---------------------------------------------------------------
    // Similarity: unchanged (single graph-wide operation, not atomized)
    // ---------------------------------------------------------------

    #[handle(on = SynthesisEvent, id = "synthesis:similarity", filter = is_synthesis_triggered)]
    async fn similarity(
        event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let run_id = event.run_id();
        let deps = ctx.deps();
        let graph_client = deps.graph_client.as_ref().expect("guarded by trigger");

        info!("Building similarity edges...");
        match rootsignal_graph::similarity::compute_edges(graph_client).await {
            Ok(edges) => {
                info!(edges = edges.len(), "Similarity edges computed");
                let mut out = Events::new();
                out.push(SystemEvent::SimilarityEdgesRebuilt { edges });
                out.push(SynthesisEvent::SynthesisRoleCompleted {
                    run_id,
                    role: SynthesisRole::Similarity,
                });
                Ok(out)
            }
            Err(e) => {
                warn!(error = %e, "Similarity edge building failed (non-fatal)");
                Ok(events![SynthesisEvent::SynthesisRoleCompleted {
                    run_id,
                    role: SynthesisRole::Similarity,
                }])
            }
        }
    }

    // ===============================================================
    // ConcernLinker: fan-out → per-target → completion check
    // ===============================================================

    #[handle(on = SynthesisEvent, id = "synthesis:fan_out_concern_linker", filter = is_synthesis_triggered)]
    async fn fan_out_concern_linker(
        event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let run_id = event.run_id();
        let deps = ctx.deps();
        let region = deps.region.as_ref().expect("guarded by trigger");
        let graph_client = deps.graph_client.as_ref().expect("guarded by trigger");
        let budget = deps.budget.as_ref().expect("guarded by trigger");
        let graph = GraphReader::new(graph_client.clone());

        if deps.archive.is_none() {
            let mut skip = events![SynthesisEvent::SynthesisRoleCompleted {
                run_id,
                role: SynthesisRole::ConcernLinker,
            }];
            skip.push(TelemetryEvent::SystemLog {
                message: "Skipped concern linker: missing archive".into(),
                context: Some(serde_json::json!({
                    "handler": "synthesis:fan_out_concern_linker",
                    "reason": "missing_deps",
                })),
            });
            return Ok(skip);
        }

        let mut out = Events::new();
        if !budget.has_budget(
            OperationCost::CLAUDE_HAIKU_TENSION_LINKER + OperationCost::SEARCH_TENSION_LINKER,
        ) {
            out.push(TelemetryEvent::SystemLog {
                message: "Skipped concern linker: insufficient budget".into(),
                context: Some(serde_json::json!({
                    "handler": "synthesis:fan_out_concern_linker",
                    "reason": "budget_exhausted",
                })),
            });
            out.push(SynthesisEvent::SynthesisRoleCompleted {
                run_id,
                role: SynthesisRole::ConcernLinker,
            });
            return Ok(out);
        }

        // Pre-pass: promote exhausted retries to abandoned (via event)
        out.push(SystemEvent::ExhaustedRetriesPromoted {
            promoted_at: chrono::Utc::now(),
        });

        let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();
        let targets = match graph
            .find_tension_linker_targets(10, min_lat, max_lat, min_lng, max_lng)
            .await
        {
            Ok(t) => t,
            Err(e) => {
                warn!(error = %e, "Failed to find concern linker targets");
                out.push(SynthesisEvent::SynthesisRoleCompleted {
                    run_id,
                    role: SynthesisRole::ConcernLinker,
                });
                return Ok(out);
            }
        };

        if targets.is_empty() {
            info!("No concern linker targets found");
            out.push(SynthesisEvent::SynthesisRoleCompleted {
                run_id,
                role: SynthesisRole::ConcernLinker,
            });
            return Ok(out);
        }

        let count = targets.len() as u32;
        info!(count, "Dispatching concern linker targets");

        out.push(SynthesisEvent::SynthesisTargetsDispatched {
            run_id,
            role: SynthesisRole::ConcernLinker,
            count,
        });

        for target in &targets {
            out.push(SynthesisEvent::ConcernLinkerTargetRequested {
                run_id,
                signal_id: target.signal_id,
                signal_title: target.title.clone(),
                signal_type: target.label.clone(),
                source_url: target.source_url.clone(),
            });
        }

        Ok(out)
    }

    #[handle(on = SynthesisEvent, id = "synthesis:process_concern_linker_target", filter = is_concern_linker_target_requested)]
    async fn process_concern_linker_target(
        event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (run_id, signal_id) = match &event {
            SynthesisEvent::ConcernLinkerTargetRequested { run_id, signal_id, .. } => (*run_id, *signal_id),
            _ => unreachable!(),
        };

        let deps = ctx.deps();
        let cancelled = deps.cancelled.clone().unwrap_or_else(|| Arc::new(AtomicBool::new(false)));

        // Check cancellation
        if cancelled.load(Ordering::Relaxed) {
            return Ok(events![SynthesisEvent::ConcernLinkerTargetCompleted {
                run_id,
                signal_id,
                outcome: "cancelled".to_string(),
                tensions_discovered: 0,
                edges_created: 0,
            }]);
        }

        let region = deps.region.as_ref().expect("guarded by trigger");
        let graph_client = deps.graph_client.as_ref().expect("guarded by trigger");
        let ai = deps.ai.as_ref().expect("guarded by trigger");
        let archive = deps.archive.as_ref().expect("guarded by fan-out").clone();
        let graph = GraphReader::new(graph_client.clone());

        let tl = activities::concern_linker::ConcernLinker::new(
            &graph,
            archive,
            &*deps.embedder,
            ai.as_ref(),
            region.clone(),
            cancelled,
            deps.run_id.clone(),
        );

        // Re-fetch targets from graph to find this specific one
        let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();
        let targets = graph
            .find_tension_linker_targets(10, min_lat, max_lat, min_lng, max_lng)
            .await
            .unwrap_or_default();

        let target = targets.iter().find(|t| t.signal_id == signal_id);
        let Some(target) = target else {
            warn!(%signal_id, "Concern linker target not found in graph, skipping");
            return Ok(events![SynthesisEvent::ConcernLinkerTargetCompleted {
                run_id,
                signal_id,
                outcome: "not_found".to_string(),
                tensions_discovered: 0,
                edges_created: 0,
            }]);
        };

        // Load landscapes (shared context)
        let tension_landscape = match graph
            .get_tension_landscape(min_lat, max_lat, min_lng, max_lng)
            .await
        {
            Ok(tensions) => {
                if tensions.is_empty() {
                    "No tensions known yet.".to_string()
                } else {
                    tensions
                        .iter()
                        .enumerate()
                        .map(|(i, (title, summary))| format!("{}. {} — {}", i + 1, title, summary))
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to load tension landscape");
                "Unable to load existing tensions.".to_string()
            }
        };

        let situation_landscape = match graph.get_situation_landscape(15).await {
            Ok(situations) => {
                if situations.is_empty() {
                    String::new()
                } else {
                    situations
                        .iter()
                        .enumerate()
                        .map(|(i, s)| {
                            format!(
                                "{}. {} [{}] (temp={:.2}, clarity={}, {} signals)",
                                i + 1, s.headline, s.arc, s.temperature, s.clarity, s.signal_count,
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to load situation landscape");
                String::new()
            }
        };

        let (target_events, target_stats) = tl
            .process_single_target(target, &tension_landscape, &situation_landscape)
            .await;

        let mut out = target_events;
        out.push(SynthesisEvent::ConcernLinkerTargetCompleted {
            run_id,
            signal_id,
            outcome: target_stats.outcome,
            tensions_discovered: target_stats.tensions_discovered,
            edges_created: target_stats.edges_created,
        });

        Ok(out)
    }

    #[handle(on = SynthesisEvent, id = "synthesis:check_concern_linker_complete", filter = is_concern_linker_target_completed)]
    async fn check_concern_linker_complete(
        _event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (_, state) = ctx.singleton::<PipelineState>();
        let total = state.synthesis_role_totals.get(&SynthesisRole::ConcernLinker).copied().unwrap_or(0);
        let completed = state.synthesis_role_completed.get(&SynthesisRole::ConcernLinker).copied().unwrap_or(0);

        if completed >= total && total > 0 {
            let run_id = Uuid::parse_str(&ctx.deps().run_id).unwrap_or_else(|_| Uuid::new_v4());
            info!(total, completed, "All concern linker targets complete");
            Ok(events![SynthesisEvent::SynthesisRoleCompleted {
                run_id,
                role: SynthesisRole::ConcernLinker,
            }])
        } else {
            Ok(events![PipelineEvent::HandlerSkipped {
                handler_id: "synthesis:check_concern_linker_complete".into(),
                reason: format!("waiting for ConcernLinker: {completed}/{total} targets complete"),
            }])
        }
    }

    // ===============================================================
    // ResponseFinder: fan-out → per-target → completion check
    // ===============================================================

    #[handle(on = SynthesisEvent, id = "synthesis:fan_out_response_finder", filter = is_synthesis_triggered)]
    async fn fan_out_response_finder(
        event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let run_id = event.run_id();
        let deps = ctx.deps();
        let region = deps.region.as_ref().expect("guarded by trigger");
        let graph_client = deps.graph_client.as_ref().expect("guarded by trigger");
        let budget = deps.budget.as_ref().expect("guarded by trigger");
        let graph = GraphReader::new(graph_client.clone());

        if deps.archive.is_none() {
            let mut skip = events![SynthesisEvent::SynthesisRoleCompleted {
                run_id,
                role: SynthesisRole::ResponseFinder,
            }];
            skip.push(TelemetryEvent::SystemLog {
                message: "Skipped response finder: missing archive".into(),
                context: Some(serde_json::json!({
                    "handler": "synthesis:fan_out_response_finder",
                    "reason": "missing_deps",
                })),
            });
            return Ok(skip);
        }

        let mut out = Events::new();
        if !budget.has_budget(
            OperationCost::CLAUDE_HAIKU_RESPONSE_FINDER + OperationCost::SEARCH_RESPONSE_FINDER,
        ) {
            out.push(TelemetryEvent::SystemLog {
                message: "Skipped response finder: insufficient budget".into(),
                context: Some(serde_json::json!({
                    "handler": "synthesis:fan_out_response_finder",
                    "reason": "budget_exhausted",
                })),
            });
            out.push(SynthesisEvent::SynthesisRoleCompleted {
                run_id,
                role: SynthesisRole::ResponseFinder,
            });
            return Ok(out);
        }

        let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();
        let targets = match graph
            .find_response_finder_targets(5, min_lat, max_lat, min_lng, max_lng)
            .await
        {
            Ok(t) => t,
            Err(e) => {
                warn!(error = %e, "Failed to find response finder targets");
                out.push(SynthesisEvent::SynthesisRoleCompleted {
                    run_id,
                    role: SynthesisRole::ResponseFinder,
                });
                return Ok(out);
            }
        };

        if targets.is_empty() {
            info!("No response finder targets found");
            out.push(SynthesisEvent::SynthesisRoleCompleted {
                run_id,
                role: SynthesisRole::ResponseFinder,
            });
            return Ok(out);
        }

        let count = targets.len() as u32;
        info!(count, "Dispatching response finder targets");

        out.push(SynthesisEvent::SynthesisTargetsDispatched {
            run_id,
            role: SynthesisRole::ResponseFinder,
            count,
        });

        for target in &targets {
            out.push(SynthesisEvent::ResponseFinderTargetRequested {
                run_id,
                concern_id: target.concern_id,
                concern_title: target.title.clone(),
            });
        }

        Ok(out)
    }

    #[handle(on = SynthesisEvent, id = "synthesis:process_response_finder_target", filter = is_response_finder_target_requested)]
    async fn process_response_finder_target(
        event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (run_id, concern_id) = match &event {
            SynthesisEvent::ResponseFinderTargetRequested { run_id, concern_id, .. } => (*run_id, *concern_id),
            _ => unreachable!(),
        };

        let deps = ctx.deps();
        let cancelled = deps.cancelled.clone().unwrap_or_else(|| Arc::new(AtomicBool::new(false)));

        if cancelled.load(Ordering::Relaxed) {
            return Ok(events![SynthesisEvent::ResponseFinderTargetCompleted {
                run_id,
                concern_id,
                responses_discovered: 0,
                edges_created: 0,
            }]);
        }

        let region = deps.region.as_ref().expect("guarded by trigger");
        let graph_client = deps.graph_client.as_ref().expect("guarded by trigger");
        let ai = deps.ai.as_ref().expect("guarded by trigger");
        let archive = deps.archive.as_ref().expect("guarded by fan-out").clone();
        let graph = GraphReader::new(graph_client.clone());

        let rf = activities::response_finder::ResponseFinder::new(
            &graph,
            archive,
            &*deps.embedder,
            ai.as_ref(),
            region.clone(),
            cancelled,
            deps.run_id.clone(),
        );

        // Re-fetch targets from graph to find this specific one
        let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();
        let targets = graph
            .find_response_finder_targets(5, min_lat, max_lat, min_lng, max_lng)
            .await
            .unwrap_or_default();

        let target = targets.iter().find(|t| t.concern_id == concern_id);
        let Some(target) = target else {
            warn!(%concern_id, "Response finder target not found in graph, skipping");
            return Ok(events![SynthesisEvent::ResponseFinderTargetCompleted {
                run_id,
                concern_id,
                responses_discovered: 0,
                edges_created: 0,
            }]);
        };

        // Load situation context
        let situation_context = match graph.get_situation_landscape(15).await {
            Ok(situations) => {
                situations
                    .iter()
                    .filter(|s| s.temperature >= 0.2)
                    .map(|s| {
                        let gap_note = if s.dispatch_count == 0 {
                            " [NO RESPONSES YET]"
                        } else if s.dispatch_count < s.signal_count / 3 {
                            " [RESPONSE GAP]"
                        } else {
                            ""
                        };
                        format!(
                            "- {} [{}] (temp={:.2}, {} signals, {} dispatches){gap_note}",
                            s.headline, s.arc, s.temperature, s.signal_count, s.dispatch_count,
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            Err(e) => {
                warn!(error = %e, "Failed to load situation landscape");
                String::new()
            }
        };

        let (target_events, target_sources, target_stats) = rf
            .process_single_target(target, &situation_context)
            .await;

        let mut out = target_events;
        for source in target_sources {
            out.push(DiscoveryEvent::SourceDiscovered {
                source,
                discovered_by: "synthesis".into(),
            });
        }
        out.push(SynthesisEvent::ResponseFinderTargetCompleted {
            run_id,
            concern_id,
            responses_discovered: target_stats.inner.responses_discovered,
            edges_created: target_stats.inner.edges_created,
        });

        Ok(out)
    }

    #[handle(on = SynthesisEvent, id = "synthesis:check_response_finder_complete", filter = is_response_finder_target_completed)]
    async fn check_response_finder_complete(
        _event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (_, state) = ctx.singleton::<PipelineState>();
        let total = state.synthesis_role_totals.get(&SynthesisRole::ResponseFinder).copied().unwrap_or(0);
        let completed = state.synthesis_role_completed.get(&SynthesisRole::ResponseFinder).copied().unwrap_or(0);

        if completed >= total && total > 0 {
            let run_id = Uuid::parse_str(&ctx.deps().run_id).unwrap_or_else(|_| Uuid::new_v4());
            info!(total, completed, "All response finder targets complete");
            Ok(events![SynthesisEvent::SynthesisRoleCompleted {
                run_id,
                role: SynthesisRole::ResponseFinder,
            }])
        } else {
            Ok(events![PipelineEvent::HandlerSkipped {
                handler_id: "synthesis:check_response_finder_complete".into(),
                reason: format!("waiting for ResponseFinder: {completed}/{total} targets complete"),
            }])
        }
    }

    // ===============================================================
    // GatheringFinder: fan-out → per-target → completion check
    // ===============================================================

    #[handle(on = SynthesisEvent, id = "synthesis:fan_out_gathering_finder", filter = is_synthesis_triggered)]
    async fn fan_out_gathering_finder(
        event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let run_id = event.run_id();
        let deps = ctx.deps();
        let region = deps.region.as_ref().expect("guarded by trigger");
        let graph_client = deps.graph_client.as_ref().expect("guarded by trigger");
        let budget = deps.budget.as_ref().expect("guarded by trigger");
        let graph = GraphReader::new(graph_client.clone());

        if deps.archive.is_none() {
            let mut skip = events![SynthesisEvent::SynthesisRoleCompleted {
                run_id,
                role: SynthesisRole::GatheringFinder,
            }];
            skip.push(TelemetryEvent::SystemLog {
                message: "Skipped gathering finder: missing archive".into(),
                context: Some(serde_json::json!({
                    "handler": "synthesis:fan_out_gathering_finder",
                    "reason": "missing_deps",
                })),
            });
            return Ok(skip);
        }

        let mut out = Events::new();
        if !budget.has_budget(
            OperationCost::CLAUDE_HAIKU_GATHERING_FINDER + OperationCost::SEARCH_GATHERING_FINDER,
        ) {
            out.push(TelemetryEvent::SystemLog {
                message: "Skipped gathering finder: insufficient budget".into(),
                context: Some(serde_json::json!({
                    "handler": "synthesis:fan_out_gathering_finder",
                    "reason": "budget_exhausted",
                })),
            });
            out.push(SynthesisEvent::SynthesisRoleCompleted {
                run_id,
                role: SynthesisRole::GatheringFinder,
            });
            return Ok(out);
        }

        let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();
        let targets = match graph
            .find_gathering_finder_targets(5, min_lat, max_lat, min_lng, max_lng)
            .await
        {
            Ok(t) => t,
            Err(e) => {
                warn!(error = %e, "Failed to find gathering finder targets");
                out.push(SynthesisEvent::SynthesisRoleCompleted {
                    run_id,
                    role: SynthesisRole::GatheringFinder,
                });
                return Ok(out);
            }
        };

        if targets.is_empty() {
            info!("No gathering finder targets found");
            out.push(SynthesisEvent::SynthesisRoleCompleted {
                run_id,
                role: SynthesisRole::GatheringFinder,
            });
            return Ok(out);
        }

        let count = targets.len() as u32;
        info!(count, "Dispatching gathering finder targets");

        out.push(SynthesisEvent::SynthesisTargetsDispatched {
            run_id,
            role: SynthesisRole::GatheringFinder,
            count,
        });

        for target in &targets {
            out.push(SynthesisEvent::GatheringFinderTargetRequested {
                run_id,
                concern_id: target.concern_id,
                concern_title: target.title.clone(),
            });
        }

        Ok(out)
    }

    #[handle(on = SynthesisEvent, id = "synthesis:process_gathering_finder_target", filter = is_gathering_finder_target_requested)]
    async fn process_gathering_finder_target(
        event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (run_id, concern_id) = match &event {
            SynthesisEvent::GatheringFinderTargetRequested { run_id, concern_id, .. } => (*run_id, *concern_id),
            _ => unreachable!(),
        };

        let deps = ctx.deps();
        let cancelled = deps.cancelled.clone().unwrap_or_else(|| Arc::new(AtomicBool::new(false)));

        if cancelled.load(Ordering::Relaxed) {
            return Ok(events![SynthesisEvent::GatheringFinderTargetCompleted {
                run_id,
                concern_id,
                gatherings_discovered: 0,
                no_gravity: false,
                edges_created: 0,
            }]);
        }

        let region = deps.region.as_ref().expect("guarded by trigger");
        let graph_client = deps.graph_client.as_ref().expect("guarded by trigger");
        let ai = deps.ai.as_ref().expect("guarded by trigger");
        let archive = deps.archive.as_ref().expect("guarded by fan-out").clone();
        let graph = GraphReader::new(graph_client.clone());

        let gf_deps = activities::gathering_finder::GatheringFinderDeps::new(
            &graph,
            archive,
            &*deps.embedder,
            ai.as_ref(),
            region.clone(),
            cancelled,
            deps.run_id.clone(),
        );

        // Re-fetch targets from graph to find this specific one
        let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();
        let targets = graph
            .find_gathering_finder_targets(5, min_lat, max_lat, min_lng, max_lng)
            .await
            .unwrap_or_default();

        let target = targets.iter().find(|t| t.concern_id == concern_id);
        let Some(target) = target else {
            warn!(%concern_id, "Gathering finder target not found in graph, skipping");
            return Ok(events![SynthesisEvent::GatheringFinderTargetCompleted {
                run_id,
                concern_id,
                gatherings_discovered: 0,
                no_gravity: false,
                edges_created: 0,
            }]);
        };

        let (target_events, target_sources, target_stats) =
            activities::gathering_finder::investigate_single_target(&gf_deps, target).await;

        let mut out = target_events;
        for source in target_sources {
            out.push(DiscoveryEvent::SourceDiscovered {
                source,
                discovered_by: "synthesis".into(),
            });
        }
        out.push(SynthesisEvent::GatheringFinderTargetCompleted {
            run_id,
            concern_id,
            gatherings_discovered: target_stats.gatherings_discovered,
            no_gravity: target_stats.no_gravity,
            edges_created: target_stats.edges_created,
        });

        Ok(out)
    }

    #[handle(on = SynthesisEvent, id = "synthesis:check_gathering_finder_complete", filter = is_gathering_finder_target_completed)]
    async fn check_gathering_finder_complete(
        _event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (_, state) = ctx.singleton::<PipelineState>();
        let total = state.synthesis_role_totals.get(&SynthesisRole::GatheringFinder).copied().unwrap_or(0);
        let completed = state.synthesis_role_completed.get(&SynthesisRole::GatheringFinder).copied().unwrap_or(0);

        if completed >= total && total > 0 {
            let run_id = Uuid::parse_str(&ctx.deps().run_id).unwrap_or_else(|_| Uuid::new_v4());
            info!(total, completed, "All gathering finder targets complete");
            Ok(events![SynthesisEvent::SynthesisRoleCompleted {
                run_id,
                role: SynthesisRole::GatheringFinder,
            }])
        } else {
            Ok(events![PipelineEvent::HandlerSkipped {
                handler_id: "synthesis:check_gathering_finder_complete".into(),
                reason: format!("waiting for GatheringFinder: {completed}/{total} targets complete"),
            }])
        }
    }

    // ===============================================================
    // Investigation: fan-out → per-target → completion check
    // ===============================================================

    #[handle(on = SynthesisEvent, id = "synthesis:fan_out_investigation", filter = is_synthesis_triggered)]
    async fn fan_out_investigation(
        event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let run_id = event.run_id();
        let deps = ctx.deps();
        let region = deps.region.as_ref().expect("guarded by trigger");
        let graph_client = deps.graph_client.as_ref().expect("guarded by trigger");
        let budget = deps.budget.as_ref().expect("guarded by trigger");
        let graph = GraphReader::new(graph_client.clone());

        if deps.archive.is_none() {
            let mut skip = events![SynthesisEvent::SynthesisRoleCompleted {
                run_id,
                role: SynthesisRole::Investigation,
            }];
            skip.push(TelemetryEvent::SystemLog {
                message: "Skipped investigation: missing archive".into(),
                context: Some(serde_json::json!({
                    "handler": "synthesis:fan_out_investigation",
                    "reason": "missing_deps",
                })),
            });
            return Ok(skip);
        }

        let mut out = Events::new();
        if !budget.has_budget(
            OperationCost::CLAUDE_HAIKU_INVESTIGATION + OperationCost::SEARCH_INVESTIGATION,
        ) {
            out.push(TelemetryEvent::SystemLog {
                message: "Skipped investigation: insufficient budget".into(),
                context: Some(serde_json::json!({
                    "handler": "synthesis:fan_out_investigation",
                    "reason": "budget_exhausted",
                })),
            });
            out.push(SynthesisEvent::SynthesisRoleCompleted {
                run_id,
                role: SynthesisRole::Investigation,
            });
            return Ok(out);
        }

        let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();
        let targets = match graph
            .find_investigation_targets(min_lat, max_lat, min_lng, max_lng)
            .await
        {
            Ok(t) => t,
            Err(e) => {
                warn!(error = %e, "Failed to find investigation targets");
                out.push(SynthesisEvent::SynthesisRoleCompleted {
                    run_id,
                    role: SynthesisRole::Investigation,
                });
                return Ok(out);
            }
        };

        let targets: Vec<_> = targets.into_iter().take(8).collect();

        if targets.is_empty() {
            info!("No investigation targets found");
            out.push(SynthesisEvent::SynthesisRoleCompleted {
                run_id,
                role: SynthesisRole::Investigation,
            });
            return Ok(out);
        }

        let count = targets.len() as u32;
        info!(count, "Dispatching investigation targets");

        out.push(SynthesisEvent::SynthesisTargetsDispatched {
            run_id,
            role: SynthesisRole::Investigation,
            count,
        });

        for target in &targets {
            out.push(SynthesisEvent::InvestigationTargetRequested {
                run_id,
                signal_id: target.signal_id,
                signal_title: target.title.clone(),
                signal_type: target.node_type.to_string(),
            });
        }

        Ok(out)
    }

    #[handle(on = SynthesisEvent, id = "synthesis:process_investigation_target", filter = is_investigation_target_requested)]
    async fn process_investigation_target(
        event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (run_id, signal_id) = match &event {
            SynthesisEvent::InvestigationTargetRequested { run_id, signal_id, .. } => (*run_id, *signal_id),
            _ => unreachable!(),
        };

        let deps = ctx.deps();
        let cancelled = deps.cancelled.clone().unwrap_or_else(|| Arc::new(AtomicBool::new(false)));

        if cancelled.load(Ordering::Relaxed) {
            return Ok(events![SynthesisEvent::InvestigationTargetCompleted {
                run_id,
                signal_id,
                evidence_created: 0,
                confidence_adjusted: false,
            }]);
        }

        let region = deps.region.as_ref().expect("guarded by trigger");
        let graph_client = deps.graph_client.as_ref().expect("guarded by trigger");
        let ai = deps.ai.as_ref().expect("guarded by trigger");
        let archive = deps.archive.as_ref().expect("guarded by fan-out").clone();
        let graph = GraphReader::new(graph_client.clone());

        let investigator = activities::investigator::Investigator::new(
            &graph,
            archive,
            ai.as_ref(),
            region,
            cancelled,
        );

        // Re-fetch targets from graph to find this specific one
        let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();
        let targets = graph
            .find_investigation_targets(min_lat, max_lat, min_lng, max_lng)
            .await
            .unwrap_or_default();

        let target = targets.iter().find(|t| t.signal_id == signal_id);
        let Some(target) = target else {
            warn!(%signal_id, "Investigation target not found in graph, skipping");
            return Ok(events![SynthesisEvent::InvestigationTargetCompleted {
                run_id,
                signal_id,
                evidence_created: 0,
                confidence_adjusted: false,
            }]);
        };

        let (target_events, target_stats) = investigator
            .investigate_single_signal(target)
            .await;

        let mut out = target_events;
        out.push(SynthesisEvent::InvestigationTargetCompleted {
            run_id,
            signal_id,
            evidence_created: target_stats.evidence_created,
            confidence_adjusted: target_stats.confidence_adjusted,
        });

        Ok(out)
    }

    #[handle(on = SynthesisEvent, id = "synthesis:check_investigation_complete", filter = is_investigation_target_completed)]
    async fn check_investigation_complete(
        _event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (_, state) = ctx.singleton::<PipelineState>();
        let total = state.synthesis_role_totals.get(&SynthesisRole::Investigation).copied().unwrap_or(0);
        let completed = state.synthesis_role_completed.get(&SynthesisRole::Investigation).copied().unwrap_or(0);

        if completed >= total && total > 0 {
            let run_id = Uuid::parse_str(&ctx.deps().run_id).unwrap_or_else(|_| Uuid::new_v4());
            info!(total, completed, "All investigation targets complete");
            Ok(events![SynthesisEvent::SynthesisRoleCompleted {
                run_id,
                role: SynthesisRole::Investigation,
            }])
        } else {
            Ok(events![PipelineEvent::HandlerSkipped {
                handler_id: "synthesis:check_investigation_complete".into(),
                reason: format!("waiting for Investigation: {completed}/{total} targets complete"),
            }])
        }
    }

    // ===============================================================
    // ResponseMapping: fan-out → per-target → completion check
    // ===============================================================

    #[handle(on = SynthesisEvent, id = "synthesis:fan_out_response_mapping", filter = is_synthesis_triggered)]
    async fn fan_out_response_mapping(
        event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let run_id = event.run_id();
        let deps = ctx.deps();
        let region = deps.region.as_ref().expect("guarded by trigger");
        let graph_client = deps.graph_client.as_ref().expect("guarded by trigger");
        let budget = deps.budget.as_ref().expect("guarded by trigger");
        let graph = GraphReader::new(graph_client.clone());

        let mut out = Events::new();
        if !budget.has_budget(OperationCost::CLAUDE_HAIKU_SYNTHESIS * 10) {
            out.push(TelemetryEvent::SystemLog {
                message: "Skipped response mapping: insufficient budget".into(),
                context: Some(serde_json::json!({
                    "handler": "synthesis:fan_out_response_mapping",
                    "reason": "budget_exhausted",
                })),
            });
            out.push(SynthesisEvent::SynthesisRoleCompleted {
                run_id,
                role: SynthesisRole::ResponseMapping,
            });
            return Ok(out);
        }

        let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();
        let tensions = match graph
            .get_active_tensions(min_lat, max_lat, min_lng, max_lng)
            .await
        {
            Ok(t) => t,
            Err(e) => {
                warn!(error = %e, "Failed to get active tensions for response mapping");
                out.push(SynthesisEvent::SynthesisRoleCompleted {
                    run_id,
                    role: SynthesisRole::ResponseMapping,
                });
                return Ok(out);
            }
        };

        if tensions.is_empty() {
            info!("No active tensions for response mapping");
            out.push(SynthesisEvent::SynthesisRoleCompleted {
                run_id,
                role: SynthesisRole::ResponseMapping,
            });
            return Ok(out);
        }

        let count = tensions.len() as u32;
        info!(count, "Dispatching response mapping targets");

        out.push(SynthesisEvent::SynthesisTargetsDispatched {
            run_id,
            role: SynthesisRole::ResponseMapping,
            count,
        });

        // We need to get tension titles for the events
        for (concern_id, _embedding) in &tensions {
            let title = match graph.get_signal_info(*concern_id).await {
                Ok(Some((t, _))) => t,
                _ => format!("tension-{}", concern_id),
            };
            out.push(SynthesisEvent::ResponseMappingTargetRequested {
                run_id,
                concern_id: *concern_id,
                concern_title: title,
            });
        }

        Ok(out)
    }

    #[handle(on = SynthesisEvent, id = "synthesis:process_response_mapping_target", filter = is_response_mapping_target_requested)]
    async fn process_response_mapping_target(
        event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (run_id, concern_id) = match &event {
            SynthesisEvent::ResponseMappingTargetRequested { run_id, concern_id, .. } => (*run_id, *concern_id),
            _ => unreachable!(),
        };

        let deps = ctx.deps();
        let region = deps.region.as_ref().expect("guarded by trigger");
        let graph_client = deps.graph_client.as_ref().expect("guarded by trigger");
        let ai = deps.ai.as_ref().expect("guarded by trigger");
        let graph = GraphReader::new(graph_client.clone());

        let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();

        // Re-fetch the tension embedding for this specific one
        let tensions = graph
            .get_active_tensions(min_lat, max_lat, min_lng, max_lng)
            .await
            .unwrap_or_default();

        let tension_embedding = tensions.iter().find(|(id, _)| *id == concern_id);
        let Some((_id, embedding)) = tension_embedding else {
            warn!(%concern_id, "Response mapping target not found, skipping");
            return Ok(events![SynthesisEvent::ResponseMappingTargetCompleted {
                run_id,
                concern_id,
                edges_created: 0,
            }]);
        };

        let (target_events, edges_created) = activities::response_mapper::map_single_tension(
            &graph,
            ai.as_ref(),
            concern_id,
            embedding,
            min_lat,
            max_lat,
            min_lng,
            max_lng,
        )
        .await;

        let mut out = target_events;
        out.push(SynthesisEvent::ResponseMappingTargetCompleted {
            run_id,
            concern_id,
            edges_created,
        });

        Ok(out)
    }

    #[handle(on = SynthesisEvent, id = "synthesis:check_response_mapping_complete", filter = is_response_mapping_target_completed)]
    async fn check_response_mapping_complete(
        _event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (_, state) = ctx.singleton::<PipelineState>();
        let total = state.synthesis_role_totals.get(&SynthesisRole::ResponseMapping).copied().unwrap_or(0);
        let completed = state.synthesis_role_completed.get(&SynthesisRole::ResponseMapping).copied().unwrap_or(0);

        if completed >= total && total > 0 {
            let run_id = Uuid::parse_str(&ctx.deps().run_id).unwrap_or_else(|_| Uuid::new_v4());
            info!(total, completed, "All response mapping targets complete");
            Ok(events![SynthesisEvent::SynthesisRoleCompleted {
                run_id,
                role: SynthesisRole::ResponseMapping,
            }])
        } else {
            Ok(events![PipelineEvent::HandlerSkipped {
                handler_id: "synthesis:check_response_mapping_complete".into(),
                reason: format!("waiting for ResponseMapping: {completed}/{total} targets complete"),
            }])
        }
    }

    // ---------------------------------------------------------------
    // Completion: all 6 roles done → PhaseCompleted(Synthesis)
    // ---------------------------------------------------------------

    #[handle(on = SynthesisEvent, id = "synthesis:phase_complete", filter = is_synthesis_role_completed)]
    async fn phase_complete(
        _event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let (_, state) = ctx.singleton::<PipelineState>();

        if state
            .completed_synthesis_roles
            .is_superset(&all_synthesis_roles())
        {
            info!("All synthesis roles complete, emitting PhaseCompleted");
            Ok(events![LifecycleEvent::PhaseCompleted {
                phase: PipelinePhase::Synthesis,
            }])
        } else {
            let completed: Vec<_> = state.completed_synthesis_roles.iter().collect();
            let expected: Vec<_> = all_synthesis_roles().into_iter().collect();
            Ok(events![PipelineEvent::HandlerSkipped {
                handler_id: "synthesis:phase_complete".into(),
                reason: format!("waiting for Synthesis: completed {completed:?}, need {expected:?}"),
            }])
        }
    }

    // ---------------------------------------------------------------
    // Severity inference: unchanged, triggers on PhaseCompleted(Synthesis)
    // ---------------------------------------------------------------

    /// PhaseCompleted(Synthesis) → re-evaluate Notice severity now that
    /// EVIDENCE_OF edges have been projected to Neo4j by the graph projector.
    #[handle(on = LifecycleEvent, id = "synthesis:severity_inference", filter = is_synthesis_completed)]
    async fn severity_inference(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();

        let (region, graph_client) = match (deps.region.as_ref(), deps.graph_client.as_ref()) {
            (Some(r), Some(g)) => (r, g),
            _ => {
                return Ok(events![TelemetryEvent::SystemLog {
                    message: "Skipped severity inference: missing region or graph_client".into(),
                    context: Some(serde_json::json!({
                        "handler": "synthesis:severity_inference",
                        "reason": "missing_deps",
                    })),
                }]);
            }
        };

        let graph = GraphReader::new(graph_client.clone());
        let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();

        match rootsignal_graph::severity_inference::compute_severity_inference(
            &graph, min_lat, max_lat, min_lng, max_lng,
        )
        .await
        {
            Ok((updated, severity_events)) => {
                if updated > 0 {
                    info!(updated, "Severity inference updated notices");
                }
                let mut all_events = Events::new();
                for ev in severity_events {
                    all_events.push(ev);
                }
                Ok(all_events)
            }
            Err(e) => {
                warn!(error = %e, "Severity inference failed (non-fatal)");
                Ok(events![TelemetryEvent::SystemLog {
                    message: format!("Severity inference failed: {e}"),
                    context: Some(serde_json::json!({
                        "handler": "synthesis:severity_inference",
                        "error": e.to_string(),
                    })),
                }])
            }
        }
    }
}
