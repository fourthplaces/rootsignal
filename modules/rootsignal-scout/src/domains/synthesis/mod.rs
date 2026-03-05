// Synthesis domain: similarity edges, parallel finders, severity inference.

pub mod activities;
pub mod events;
pub mod util;

#[cfg(test)]
mod completion_tests;

use anyhow::Result;
use futures::stream::{self, StreamExt};
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

#[handlers]
pub mod handlers {
    use super::*;

    // ---------------------------------------------------------------
    // Similarity: single graph-wide operation, not atomized
    // ---------------------------------------------------------------

    #[handle(on = LifecycleEvent, id = "synthesis:similarity", filter = is_signal_expansion_completed)]
    async fn similarity(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let run_id = Uuid::parse_str(&deps.run_id).unwrap_or_else(|_| Uuid::new_v4());

        let graph_client = match deps.graph_client.as_ref() {
            Some(g) => g,
            None => {
                return Ok(events![SynthesisEvent::SynthesisRoleCompleted {
                    run_id,
                    role: SynthesisRole::Similarity,
                }]);
            }
        };

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
    // ConcernLinker: guards deps, loads targets, processes all, emits SynthesisRoleCompleted
    // ===============================================================

    #[handle(on = LifecycleEvent, id = "synthesis:concern_linker", filter = is_signal_expansion_completed)]
    async fn concern_linker(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let run_id = Uuid::parse_str(&deps.run_id).unwrap_or_else(|_| Uuid::new_v4());

        let (region, graph_client, budget, archive) = match (
            deps.run_scope.region(),
            deps.graph_client.as_ref(),
            deps.budget.as_ref(),
            deps.archive.as_ref(),
        ) {
            (Some(r), Some(g), Some(b), Some(a)) => (r, g, b, a.clone()),
            _ => {
                return Ok(events![SynthesisEvent::SynthesisRoleCompleted {
                    run_id,
                    role: SynthesisRole::ConcernLinker,
                }]);
            }
        };

        let graph = GraphReader::new(graph_client.clone());

        let mut out = Events::new();
        if !budget.has_budget(
            OperationCost::CLAUDE_HAIKU_TENSION_LINKER + OperationCost::SEARCH_TENSION_LINKER,
        ) {
            out.push(TelemetryEvent::SystemLog {
                message: "Skipped concern linker: insufficient budget".into(),
                context: Some(serde_json::json!({
                    "handler": "synthesis:concern_linker",
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

        info!(count = targets.len(), "Processing concern linker targets");

        let ai = deps.ai.as_ref().expect("ai required for synthesis");
        let tl = activities::concern_linker::ConcernLinker::new(
            &graph,
            archive,
            &*deps.embedder,
            ai.as_ref(),
            region.clone(),
            deps.run_id.clone(),
        );

        // Load shared landscapes once for all targets
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

        let futures: Vec<_> = targets
            .iter()
            .map(|target| tl.process_single_target(target, &tension_landscape, &situation_landscape))
            .collect();
        let results: Vec<_> = stream::iter(futures).buffer_unordered(5).collect().await;

        for (target_events, _target_stats) in results {
            out.extend(target_events);
        }

        out.push(SynthesisEvent::SynthesisRoleCompleted {
            run_id,
            role: SynthesisRole::ConcernLinker,
        });

        Ok(out)
    }

    // ===============================================================
    // ResponseFinder: guards deps, loads targets, processes all, emits SynthesisRoleCompleted
    // ===============================================================

    #[handle(on = LifecycleEvent, id = "synthesis:response_finder", filter = is_signal_expansion_completed)]
    async fn response_finder(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let run_id = Uuid::parse_str(&deps.run_id).unwrap_or_else(|_| Uuid::new_v4());

        let (region, graph_client, budget, archive) = match (
            deps.run_scope.region(),
            deps.graph_client.as_ref(),
            deps.budget.as_ref(),
            deps.archive.as_ref(),
        ) {
            (Some(r), Some(g), Some(b), Some(a)) => (r, g, b, a.clone()),
            _ => {
                return Ok(events![SynthesisEvent::SynthesisRoleCompleted {
                    run_id,
                    role: SynthesisRole::ResponseFinder,
                }]);
            }
        };

        let graph = GraphReader::new(graph_client.clone());

        let mut out = Events::new();
        if !budget.has_budget(
            OperationCost::CLAUDE_HAIKU_RESPONSE_FINDER + OperationCost::SEARCH_RESPONSE_FINDER,
        ) {
            out.push(TelemetryEvent::SystemLog {
                message: "Skipped response finder: insufficient budget".into(),
                context: Some(serde_json::json!({
                    "handler": "synthesis:response_finder",
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

        info!(count = targets.len(), "Processing response finder targets");

        let ai = deps.ai.as_ref().expect("ai required for synthesis");
        let rf = activities::response_finder::ResponseFinder::new(
            &graph,
            archive,
            &*deps.embedder,
            ai.as_ref(),
            region.clone(),
            deps.run_id.clone(),
        );

        // Load situation context once for all targets
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

        let futures: Vec<_> = targets
            .iter()
            .map(|target| rf.process_single_target(target, &situation_context))
            .collect();
        let results: Vec<_> = stream::iter(futures).buffer_unordered(3).collect().await;

        for (target_events, target_sources, _target_stats) in results {
            out.extend(target_events);
            for source in target_sources {
                out.push(DiscoveryEvent::SourceDiscovered {
                    source,
                    discovered_by: "synthesis".into(),
                });
            }
        }

        out.push(SynthesisEvent::SynthesisRoleCompleted {
            run_id,
            role: SynthesisRole::ResponseFinder,
        });

        Ok(out)
    }

    // ===============================================================
    // GatheringFinder: guards deps, loads targets, processes all, emits SynthesisRoleCompleted
    // ===============================================================

    #[handle(on = LifecycleEvent, id = "synthesis:gathering_finder", filter = is_signal_expansion_completed)]
    async fn gathering_finder(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let run_id = Uuid::parse_str(&deps.run_id).unwrap_or_else(|_| Uuid::new_v4());

        let (region, graph_client, budget, archive) = match (
            deps.run_scope.region(),
            deps.graph_client.as_ref(),
            deps.budget.as_ref(),
            deps.archive.as_ref(),
        ) {
            (Some(r), Some(g), Some(b), Some(a)) => (r, g, b, a.clone()),
            _ => {
                return Ok(events![SynthesisEvent::SynthesisRoleCompleted {
                    run_id,
                    role: SynthesisRole::GatheringFinder,
                }]);
            }
        };

        let graph = GraphReader::new(graph_client.clone());

        let mut out = Events::new();
        if !budget.has_budget(
            OperationCost::CLAUDE_HAIKU_GATHERING_FINDER + OperationCost::SEARCH_GATHERING_FINDER,
        ) {
            out.push(TelemetryEvent::SystemLog {
                message: "Skipped gathering finder: insufficient budget".into(),
                context: Some(serde_json::json!({
                    "handler": "synthesis:gathering_finder",
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

        info!(count = targets.len(), "Processing gathering finder targets");

        let ai = deps.ai.as_ref().expect("ai required for synthesis");
        let gf_deps = activities::gathering_finder::GatheringFinderDeps::new(
            &graph,
            archive,
            &*deps.embedder,
            ai.as_ref(),
            region.clone(),
            deps.run_id.clone(),
        );

        let futures: Vec<_> = targets
            .iter()
            .map(|target| activities::gathering_finder::investigate_single_target(&gf_deps, target))
            .collect();
        let results: Vec<_> = stream::iter(futures).buffer_unordered(3).collect().await;

        for (target_events, target_sources, _target_stats) in results {
            out.extend(target_events);
            for source in target_sources {
                out.push(DiscoveryEvent::SourceDiscovered {
                    source,
                    discovered_by: "synthesis".into(),
                });
            }
        }

        out.push(SynthesisEvent::SynthesisRoleCompleted {
            run_id,
            role: SynthesisRole::GatheringFinder,
        });

        Ok(out)
    }

    // ===============================================================
    // Investigation: guards deps, loads targets, processes all, emits SynthesisRoleCompleted
    // ===============================================================

    #[handle(on = LifecycleEvent, id = "synthesis:investigation", filter = is_signal_expansion_completed)]
    async fn investigation(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let run_id = Uuid::parse_str(&deps.run_id).unwrap_or_else(|_| Uuid::new_v4());

        let (region, graph_client, budget, archive) = match (
            deps.run_scope.region(),
            deps.graph_client.as_ref(),
            deps.budget.as_ref(),
            deps.archive.as_ref(),
        ) {
            (Some(r), Some(g), Some(b), Some(a)) => (r, g, b, a.clone()),
            _ => {
                return Ok(events![SynthesisEvent::SynthesisRoleCompleted {
                    run_id,
                    role: SynthesisRole::Investigation,
                }]);
            }
        };

        let graph = GraphReader::new(graph_client.clone());

        let mut out = Events::new();
        if !budget.has_budget(
            OperationCost::CLAUDE_HAIKU_INVESTIGATION + OperationCost::SEARCH_INVESTIGATION,
        ) {
            out.push(TelemetryEvent::SystemLog {
                message: "Skipped investigation: insufficient budget".into(),
                context: Some(serde_json::json!({
                    "handler": "synthesis:investigation",
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

        info!(count = targets.len(), "Processing investigation targets");

        let ai = deps.ai.as_ref().expect("ai required for synthesis");
        let investigator = activities::investigator::Investigator::new(
            &graph,
            archive,
            ai.as_ref(),
            region,
        );

        let futures: Vec<_> = targets
            .iter()
            .map(|target| investigator.investigate_single_signal(target))
            .collect();
        let results: Vec<_> = stream::iter(futures).buffer_unordered(4).collect().await;

        for (target_events, _target_stats) in results {
            out.extend(target_events);
        }

        out.push(SynthesisEvent::SynthesisRoleCompleted {
            run_id,
            role: SynthesisRole::Investigation,
        });

        Ok(out)
    }

    // ===============================================================
    // ResponseMapping: guards deps, loads targets, processes all, emits SynthesisRoleCompleted
    // ===============================================================

    #[handle(on = LifecycleEvent, id = "synthesis:response_mapping", filter = is_signal_expansion_completed)]
    async fn response_mapping(
        _event: LifecycleEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let deps = ctx.deps();
        let run_id = Uuid::parse_str(&deps.run_id).unwrap_or_else(|_| Uuid::new_v4());

        let (region, graph_client, budget) = match (
            deps.run_scope.region(),
            deps.graph_client.as_ref(),
            deps.budget.as_ref(),
        ) {
            (Some(r), Some(g), Some(b)) => (r, g, b),
            _ => {
                return Ok(events![SynthesisEvent::SynthesisRoleCompleted {
                    run_id,
                    role: SynthesisRole::ResponseMapping,
                }]);
            }
        };

        let graph = GraphReader::new(graph_client.clone());

        let mut out = Events::new();
        if !budget.has_budget(OperationCost::CLAUDE_HAIKU_SYNTHESIS * 10) {
            out.push(TelemetryEvent::SystemLog {
                message: "Skipped response mapping: insufficient budget".into(),
                context: Some(serde_json::json!({
                    "handler": "synthesis:response_mapping",
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

        info!(count = tensions.len(), "Processing response mapping targets");

        let ai = deps.ai.as_ref().expect("ai required for synthesis");

        let futures: Vec<_> = tensions
            .iter()
            .map(|(concern_id, embedding)| {
                activities::response_mapper::map_single_tension(
                    &graph,
                    ai.as_ref(),
                    *concern_id,
                    embedding,
                    min_lat,
                    max_lat,
                    min_lng,
                    max_lng,
                )
            })
            .collect();
        let results: Vec<_> = stream::iter(futures).buffer_unordered(5).collect().await;

        for (target_events, _edges_created) in results {
            out.extend(target_events);
        }

        out.push(SynthesisEvent::SynthesisRoleCompleted {
            run_id,
            role: SynthesisRole::ResponseMapping,
        });

        Ok(out)
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

        // Idempotency: if this phase already completed, skip
        if state.completed_phases.contains(&PipelinePhase::Synthesis) {
            return Ok(events![PipelineEvent::HandlerSkipped {
                handler_id: "synthesis:phase_complete".into(),
                reason: "Synthesis already completed".into(),
            }]);
        }

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

        let (region, graph_client) = match (deps.run_scope.region(), deps.graph_client.as_ref()) {
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
