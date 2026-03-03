// Synthesis domain: similarity edges, parallel finders, severity inference.

pub mod activities;
pub mod events;
pub mod util;

#[cfg(test)]
mod completion_tests;

use std::sync::atomic::AtomicBool;
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
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::scheduling::activities::budget::{BudgetTracker, OperationCost};
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
    // Role handlers: each listens for SynthesisTriggered, runs one activity
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

    #[handle(on = SynthesisEvent, id = "synthesis:response_mapping", filter = is_synthesis_triggered)]
    async fn response_mapping(
        event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let run_id = event.run_id();
        let deps = ctx.deps();
        let region = deps.region.as_ref().expect("guarded by trigger");
        let graph_client = deps.graph_client.as_ref().expect("guarded by trigger");
        let budget = deps.budget.as_ref().expect("guarded by trigger");
        let ai = deps.ai.as_ref().expect("guarded by trigger");
        let graph = GraphReader::new(graph_client.clone());

        let mut out = Events::new();
        if budget.has_budget(OperationCost::CLAUDE_HAIKU_SYNTHESIS * 10) {
            info!("Starting response mapping...");
            match activities::response_mapper::map_responses(
                &graph,
                ai.as_ref(),
                region.center_lat,
                region.center_lng,
                region.radius_km,
                &mut out,
            ).await {
                Ok(rm_stats) => info!("{rm_stats}"),
                Err(e) => warn!(error = %e, "Response mapping failed (non-fatal)"),
            }
        } else {
            out.push(TelemetryEvent::SystemLog {
                message: "Skipped response mapping: insufficient budget".into(),
                context: Some(serde_json::json!({
                    "handler": "synthesis:response_mapping",
                    "reason": "budget_exhausted",
                })),
            });
        }
        out.push(SynthesisEvent::SynthesisRoleCompleted {
            run_id,
            role: SynthesisRole::ResponseMapping,
        });
        Ok(out)
    }

    #[handle(on = SynthesisEvent, id = "synthesis:tension_linker", filter = is_synthesis_triggered)]
    async fn tension_linker(
        event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let run_id = event.run_id();
        let deps = ctx.deps();
        let region = deps.region.as_ref().expect("guarded by trigger");
        let graph_client = deps.graph_client.as_ref().expect("guarded by trigger");
        let budget = deps.budget.as_ref().expect("guarded by trigger");
        let ai = deps.ai.as_ref().expect("guarded by trigger");
        let graph = GraphReader::new(graph_client.clone());
        let cancelled = deps
            .cancelled
            .clone()
            .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
        let archive = match deps.archive.as_ref() {
            Some(a) => a.clone(),
            None => {
                let mut skip = events![SynthesisEvent::SynthesisRoleCompleted {
                    run_id,
                    role: SynthesisRole::ConcernLinker,
                }];
                skip.push(TelemetryEvent::SystemLog {
                    message: "Skipped concern linker: missing archive".into(),
                    context: Some(serde_json::json!({
                        "handler": "synthesis:tension_linker",
                        "reason": "missing_deps",
                    })),
                });
                return Ok(skip);
            }
        };

        let mut out = Events::new();
        if budget.has_budget(
            OperationCost::CLAUDE_HAIKU_TENSION_LINKER + OperationCost::SEARCH_TENSION_LINKER,
        ) {
            info!("Starting tension linker...");
            let tl = activities::concern_linker::ConcernLinker::new(
                &graph,
                archive,
                &*deps.embedder,
                ai.as_ref(),
                region.clone(),
                cancelled,
                deps.run_id.clone(),
            );
            let tl_stats = tl.run(&mut out).await;
            info!("{tl_stats}");
        } else {
            out.push(TelemetryEvent::SystemLog {
                message: "Skipped concern linker: insufficient budget".into(),
                context: Some(serde_json::json!({
                    "handler": "synthesis:tension_linker",
                    "reason": "budget_exhausted",
                })),
            });
        }
        out.push(SynthesisEvent::SynthesisRoleCompleted {
            run_id,
            role: SynthesisRole::ConcernLinker,
        });
        Ok(out)
    }

    #[handle(on = SynthesisEvent, id = "synthesis:response_finder", filter = is_synthesis_triggered)]
    async fn response_finder(
        event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let run_id = event.run_id();
        let deps = ctx.deps();
        let region = deps.region.as_ref().expect("guarded by trigger");
        let graph_client = deps.graph_client.as_ref().expect("guarded by trigger");
        let budget = deps.budget.as_ref().expect("guarded by trigger");
        let ai = deps.ai.as_ref().expect("guarded by trigger");
        let graph = GraphReader::new(graph_client.clone());
        let cancelled = deps
            .cancelled
            .clone()
            .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
        let archive = match deps.archive.as_ref() {
            Some(a) => a.clone(),
            None => {
                let mut skip = events![SynthesisEvent::SynthesisRoleCompleted {
                    run_id,
                    role: SynthesisRole::ResponseFinder,
                }];
                skip.push(TelemetryEvent::SystemLog {
                    message: "Skipped response finder: missing archive".into(),
                    context: Some(serde_json::json!({
                        "handler": "synthesis:response_finder",
                        "reason": "missing_deps",
                    })),
                });
                return Ok(skip);
            }
        };

        let mut out = Events::new();
        if budget.has_budget(
            OperationCost::CLAUDE_HAIKU_RESPONSE_FINDER + OperationCost::SEARCH_RESPONSE_FINDER,
        ) {
            info!("Starting response finder...");
            let rf = activities::response_finder::ResponseFinder::new(
                &graph,
                archive,
                &*deps.embedder,
                ai.as_ref(),
                region.clone(),
                cancelled,
                deps.run_id.clone(),
            );
            let (rf_stats, rf_sources) = rf.run(&mut out).await;
            info!("{rf_stats}");
            for source in rf_sources {
                out.push(DiscoveryEvent::SourceDiscovered {
                    source,
                    discovered_by: "synthesis".into(),
                });
            }
        } else {
            out.push(TelemetryEvent::SystemLog {
                message: "Skipped response finder: insufficient budget".into(),
                context: Some(serde_json::json!({
                    "handler": "synthesis:response_finder",
                    "reason": "budget_exhausted",
                })),
            });
        }
        out.push(SynthesisEvent::SynthesisRoleCompleted {
            run_id,
            role: SynthesisRole::ResponseFinder,
        });
        Ok(out)
    }

    #[handle(on = SynthesisEvent, id = "synthesis:gathering_finder", filter = is_synthesis_triggered)]
    async fn gathering_finder(
        event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let run_id = event.run_id();
        let deps = ctx.deps();
        let region = deps.region.as_ref().expect("guarded by trigger");
        let graph_client = deps.graph_client.as_ref().expect("guarded by trigger");
        let budget = deps.budget.as_ref().expect("guarded by trigger");
        let ai = deps.ai.as_ref().expect("guarded by trigger");
        let graph = GraphReader::new(graph_client.clone());
        let cancelled = deps
            .cancelled
            .clone()
            .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
        let archive = match deps.archive.as_ref() {
            Some(a) => a.clone(),
            None => {
                let mut skip = events![SynthesisEvent::SynthesisRoleCompleted {
                    run_id,
                    role: SynthesisRole::GatheringFinder,
                }];
                skip.push(TelemetryEvent::SystemLog {
                    message: "Skipped gathering finder: missing archive".into(),
                    context: Some(serde_json::json!({
                        "handler": "synthesis:gathering_finder",
                        "reason": "missing_deps",
                    })),
                });
                return Ok(skip);
            }
        };

        let mut out = Events::new();
        if budget.has_budget(
            OperationCost::CLAUDE_HAIKU_GATHERING_FINDER + OperationCost::SEARCH_GATHERING_FINDER,
        ) {
            info!("Starting gathering finder...");
            let gf_deps = activities::gathering_finder::GatheringFinderDeps::new(
                &graph,
                archive,
                &*deps.embedder,
                ai.as_ref(),
                region.clone(),
                cancelled,
                deps.run_id.clone(),
            );
            let (gf_stats, gf_sources) =
                activities::gathering_finder::find_gatherings(&gf_deps, &mut out).await;
            info!("{gf_stats}");
            for source in gf_sources {
                out.push(DiscoveryEvent::SourceDiscovered {
                    source,
                    discovered_by: "synthesis".into(),
                });
            }
        } else {
            out.push(TelemetryEvent::SystemLog {
                message: "Skipped gathering finder: insufficient budget".into(),
                context: Some(serde_json::json!({
                    "handler": "synthesis:gathering_finder",
                    "reason": "budget_exhausted",
                })),
            });
        }
        out.push(SynthesisEvent::SynthesisRoleCompleted {
            run_id,
            role: SynthesisRole::GatheringFinder,
        });
        Ok(out)
    }

    #[handle(on = SynthesisEvent, id = "synthesis:investigation", filter = is_synthesis_triggered)]
    async fn investigation(
        event: SynthesisEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let run_id = event.run_id();
        let deps = ctx.deps();
        let region = deps.region.as_ref().expect("guarded by trigger");
        let graph_client = deps.graph_client.as_ref().expect("guarded by trigger");
        let budget = deps.budget.as_ref().expect("guarded by trigger");
        let ai = deps.ai.as_ref().expect("guarded by trigger");
        let graph = GraphReader::new(graph_client.clone());
        let cancelled = deps
            .cancelled
            .clone()
            .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
        let archive = match deps.archive.as_ref() {
            Some(a) => a.clone(),
            None => {
                let mut skip = events![SynthesisEvent::SynthesisRoleCompleted {
                    run_id,
                    role: SynthesisRole::Investigation,
                }];
                skip.push(TelemetryEvent::SystemLog {
                    message: "Skipped investigation: missing archive".into(),
                    context: Some(serde_json::json!({
                        "handler": "synthesis:investigation",
                        "reason": "missing_deps",
                    })),
                });
                return Ok(skip);
            }
        };

        let mut out = Events::new();
        if budget.has_budget(
            OperationCost::CLAUDE_HAIKU_INVESTIGATION + OperationCost::SEARCH_INVESTIGATION,
        ) {
            info!("Starting investigation phase...");
            let investigator = activities::investigator::Investigator::new(
                &graph,
                archive,
                ai.as_ref(),
                region,
                cancelled,
            );
            let inv_stats = investigator.run(&mut out).await;
            info!("{inv_stats}");
        } else {
            out.push(TelemetryEvent::SystemLog {
                message: "Skipped investigation: insufficient budget".into(),
                context: Some(serde_json::json!({
                    "handler": "synthesis:investigation",
                    "reason": "budget_exhausted",
                })),
            });
        }
        out.push(SynthesisEvent::SynthesisRoleCompleted {
            run_id,
            role: SynthesisRole::Investigation,
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

        if state
            .completed_synthesis_roles
            .is_superset(&all_synthesis_roles())
        {
            info!("All synthesis roles complete, emitting PhaseCompleted");
            Ok(events![LifecycleEvent::PhaseCompleted {
                phase: PipelinePhase::Synthesis,
            }])
        } else {
            Ok(Events::new())
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
                Ok(events![])
            }
        }
    }
}
