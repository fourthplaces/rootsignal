pub mod activities;
pub mod aggregates;
pub mod events;
pub mod materializer;
pub mod util;

use anyhow::Result;
use chrono::Utc;
use causal::{reactor, reactors, Context, Events};
use tracing::{info, warn};

use rootsignal_common::events::{SystemEvent, WorldEvent};
use rootsignal_common::types::NodeType;
use rootsignal_graph::{
    ConcernLinkerOutcome, ConcernLinkerTarget, GatheringFinderTarget, InvestigationTarget,
    ResponseFinderTarget,
};

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::domains::curiosity::activities::concern_linker::{
    ConcernLinker, TensionClassification,
};
use crate::domains::curiosity::activities::gathering_finder::{
    self as gathering_finder, GatheringClassification, GatheringFinderDeps,
};
use crate::domains::curiosity::activities::investigator::Investigator;
use crate::domains::curiosity::activities::response_finder::{
    self as response_finder, ResponseClassification, ResponseFinder,
};
use crate::domains::curiosity::aggregates::{ConcernLifecycle, SignalLifecycle};
use crate::domains::curiosity::events::{CuriosityEvent, ResolvedEdge};
use crate::domains::curiosity::materializer::ConcernEdge;
use crate::domains::curiosity::util::build_future_query_source;
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::scheduling::activities::budget::OperationCost;
use crate::store::event_sourced::{node_system_events, node_to_world_event};

const MAX_TENSIONS_PER_SIGNAL: usize = 3;
const MAX_RESPONSES_PER_CONCERN: usize = 8;
const MAX_GATHERINGS_PER_CONCERN: usize = 8;

fn signal_not_investigated(e: &WorldEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    let signal_id = match e.signal_id() {
        Some(id) => id,
        None => return false,
    };
    let lifecycle = ctx.aggregate_of::<SignalLifecycle>(signal_id).curr;
    !lifecycle.investigated
}

fn signal_not_concern_linked(e: &WorldEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    let signal_id = match e.signal_id() {
        Some(id) => id,
        None => return false,
    };
    let lifecycle = ctx.aggregate_of::<SignalLifecycle>(signal_id).curr;
    !lifecycle.concern_linked
}

fn concern_not_response_scouted(e: &WorldEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    let concern_id = match e {
        WorldEvent::ConcernRaised { id, .. } => *id,
        _ => return false,
    };
    let lifecycle = ctx.aggregate_of::<ConcernLifecycle>(concern_id).curr;
    !lifecycle.responses_scouted
}

fn concern_not_gathering_scouted(e: &WorldEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    let concern_id = match e {
        WorldEvent::ConcernRaised { id, .. } => *id,
        _ => return false,
    };
    let lifecycle = ctx.aggregate_of::<ConcernLifecycle>(concern_id).curr;
    !lifecycle.gatherings_scouted
}

fn is_curiosity_discovery(e: &CuriosityEvent, _ctx: &Context<ScoutEngineDeps>) -> bool {
    e.is_discovery()
}

fn emit_concern_edge(out: &mut Events, edge: ConcernEdge) {
    if edge.is_gravity {
        out.push(SystemEvent::ConcernLinked {
            signal_id: edge.signal_id,
            concern_id: edge.concern_id,
            strength: edge.strength,
            explanation: edge.explanation,
            source_url: None,
        });
    } else {
        out.push(SystemEvent::ResponseLinked {
            signal_id: edge.signal_id,
            concern_id: edge.concern_id,
            strength: edge.strength,
            explanation: edge.explanation,
            source_url: None,
        });
    }
}

#[reactors]
pub mod reactors {
    use super::*;

    #[reactor(on = WorldEvent, id = "curiosity:investigate", filter = signal_not_investigated)]
    async fn investigate(
        event: WorldEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let signal_id = event.signal_id().unwrap();
        let mut out = Events::new();

        let deps = ctx.deps();
        let state = ctx.aggregate::<PipelineState>().curr;

        let (region, graph, archive) = match (
            state.run_scope.region(),
            deps.graph.as_deref(),
            deps.archive.as_ref(),
        ) {
            (Some(r), Some(g), Some(a)) => (r, g, a.clone()),
            _ => {
                out.push(CuriosityEvent::SignalInvestigated { signal_id });
                return Ok(out);
            }
        };

        if !state.has_budget(
            OperationCost::CLAUDE_HAIKU_INVESTIGATION + OperationCost::SEARCH_INVESTIGATION,
        ) {
            ctx.logger.debug("Skipped investigation: insufficient budget");
            out.push(CuriosityEvent::SignalInvestigated { signal_id });
            return Ok(out);
        }

        let target = InvestigationTarget {
            signal_id,
            node_type: event.node_type().unwrap_or(NodeType::Announcement),
            title: event.title().unwrap_or_default().to_string(),
            summary: event.summary().unwrap_or_default().to_string(),
            url: event.url().unwrap_or_default().to_string(),
            is_sensitive: false,
        };

        info!(
            signal_id = %signal_id,
            node_type = %target.node_type,
            title = target.title.as_str(),
            "Investigating signal"
        );

        let ai = deps.ai.as_ref().expect("ai required for investigation");
        let investigator = Investigator::new(graph, archive, ai.as_ref(), region);
        let result = investigator.investigate_single_signal(&target).await;

        // Evidence citations
        for ev in result.evidence {
            out.push(WorldEvent::CitationPublished {
                citation_id: ev.citation_id,
                signal_id: ev.signal_id,
                url: ev.source_url,
                content_hash: ev.content_hash,
                snippet: ev.snippet,
                relevance: ev.relevance,
                channel_type: ev.channel_type,
                evidence_confidence: ev.evidence_confidence,
            });
        }

        // Confidence revision
        if let Some(rev) = result.confidence_revision {
            out.push(SystemEvent::ConfidenceScored {
                signal_id: rev.signal_id,
                old_confidence: rev.old_confidence,
                new_confidence: rev.new_confidence,
            });
        }

        // Always mark investigated
        out.push(SystemEvent::SignalInvestigated {
            signal_id: target.signal_id,
            node_type: target.node_type,
            investigated_at: Utc::now(),
        });

        out.push(CuriosityEvent::SignalInvestigated { signal_id });
        Ok(out)
    }

    #[reactor(on = WorldEvent, id = "curiosity:link_concerns", filter = signal_not_concern_linked)]
    async fn link_concerns(
        event: WorldEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let signal_id = event.signal_id().unwrap();
        let mut out = Events::new();

        let deps = ctx.deps();
        let state = ctx.aggregate::<PipelineState>().curr;

        let (region, graph, archive) = match (
            state.run_scope.region(),
            deps.graph.as_deref(),
            deps.archive.as_ref(),
        ) {
            (Some(r), Some(g), Some(a)) => (r, g, a.clone()),
            _ => {
                out.push(CuriosityEvent::SignalConcernLinked { signal_id });
                return Ok(out);
            }
        };

        if !state.has_budget(
            OperationCost::CLAUDE_HAIKU_TENSION_LINKER + OperationCost::SEARCH_TENSION_LINKER,
        ) {
            ctx.logger
                .debug("Skipped concern linker: insufficient budget");
            out.push(CuriosityEvent::SignalConcernLinked { signal_id });
            return Ok(out);
        }

        let label = event
            .node_type_label()
            .unwrap_or("Announcement")
            .to_string();
        let target = ConcernLinkerTarget {
            signal_id,
            title: event.title().unwrap_or_default().to_string(),
            summary: event.summary().unwrap_or_default().to_string(),
            label,
            url: event.url().unwrap_or_default().to_string(),
        };

        info!(
            signal_id = %signal_id,
            title = target.title.as_str(),
            "Linking concerns for signal"
        );

        let ai = deps.ai.as_ref().expect("ai required for concern linking");
        let cl = ConcernLinker::new(
            graph,
            archive,
            &*deps.embedder,
            ai.as_ref(),
            region.clone(),
            deps.run_id.to_string(),
        );

        let (min_lat, max_lat, min_lng, max_lng) = region.bounding_box();

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
                        .map(|(i, (title, summary))| {
                            format!("{}. {} — {}", i + 1, title, summary)
                        })
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
                                i + 1,
                                s.headline,
                                s.arc,
                                s.temperature,
                                s.clarity,
                                s.signal_count,
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

        let outcome = match cl
            .investigate_signal(&target, &tension_landscape, &situation_landscape)
            .await
        {
            Ok(finding) => {
                if !finding.curious {
                    info!(
                        signal_id = %signal_id,
                        reason = finding.skip_reason.as_deref().unwrap_or("self-explanatory"),
                        "Signal not curious, skipping"
                    );
                    ConcernLinkerOutcome::Skipped
                } else {
                    let mut any_failed = false;
                    for tension in finding.tensions.into_iter().take(MAX_TENSIONS_PER_SIGNAL) {
                        match cl.classify_tension(&tension).await {
                            Ok(TensionClassification::New { tension_id }) => {
                                out.push(CuriosityEvent::TensionDiscovered {
                                    tension_id,
                                    title: tension.title,
                                    summary: tension.summary,
                                    severity: tension.severity,
                                    category: tension.category,
                                    opposing: tension.opposing,
                                    url: tension.url,
                                    parent_signal_id: signal_id,
                                    match_strength: tension.match_strength,
                                    explanation: tension.explanation,
                                });
                            }
                            Ok(TensionClassification::Duplicate { existing_id }) => {
                                out.push(SystemEvent::ResponseLinked {
                                    signal_id,
                                    concern_id: existing_id,
                                    strength: tension.match_strength.clamp(0.0, 1.0),
                                    explanation: tension.explanation,
                                    source_url: None,
                                });
                            }
                            Err(e) => {
                                any_failed = true;
                                warn!(
                                    signal_id = %signal_id,
                                    tension_title = tension.title.as_str(),
                                    error = %e,
                                    "Failed to classify tension"
                                );
                            }
                        }
                    }
                    if any_failed {
                        ConcernLinkerOutcome::Failed
                    } else {
                        ConcernLinkerOutcome::Done
                    }
                }
            }
            Err(e) => {
                warn!(
                    signal_id = %signal_id,
                    error = %e,
                    "Concern linking investigation failed"
                );
                ConcernLinkerOutcome::Failed
            }
        };

        out.push(SystemEvent::ConcernLinkerOutcomeRecorded {
            signal_id,
            label: target.label,
            outcome: outcome.as_str().to_string(),
            increment_retry: outcome == ConcernLinkerOutcome::Failed,
        });

        out.push(CuriosityEvent::SignalConcernLinked { signal_id });
        Ok(out)
    }

    #[reactor(on = WorldEvent, id = "curiosity:find_responses", filter = concern_not_response_scouted)]
    async fn find_responses(
        event: WorldEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let concern_id = match &event {
            WorldEvent::ConcernRaised { id, .. } => *id,
            _ => return Ok(Events::new()),
        };
        let mut out = Events::new();

        let deps = ctx.deps();
        let state = ctx.aggregate::<PipelineState>().curr;

        let (region, graph, archive) = match (
            state.run_scope.region(),
            deps.graph.as_deref(),
            deps.archive.as_ref(),
        ) {
            (Some(r), Some(g), Some(a)) => (r, g, a.clone()),
            _ => {
                out.push(CuriosityEvent::ConcernResponsesScouted { concern_id });
                return Ok(out);
            }
        };

        if !state.has_budget(
            OperationCost::CLAUDE_HAIKU_RESPONSE_FINDER + OperationCost::SEARCH_RESPONSE_FINDER,
        ) {
            ctx.logger
                .debug("Skipped response finder: insufficient budget");
            out.push(CuriosityEvent::ConcernResponsesScouted { concern_id });
            return Ok(out);
        }

        let target = ResponseFinderTarget {
            concern_id,
            title: event.title().unwrap_or_default().to_string(),
            summary: event.summary().unwrap_or_default().to_string(),
            severity: "medium".to_string(),
            category: None,
            opposing: event.opposing().map(|s| s.to_string()),
            cause_heat: 0.0,
            response_count: 0,
        };

        info!(
            concern_id = %concern_id,
            title = target.title.as_str(),
            "Finding responses for concern"
        );

        let ai = deps.ai.as_ref().expect("ai required for response finder");
        let rf = ResponseFinder::new(
            graph,
            archive,
            &*deps.embedder,
            ai.as_ref(),
            region.clone(),
            deps.run_id.to_string(),
        );

        let situation_context = match graph.get_situation_landscape(15).await {
            Ok(situations) => response_finder::format_situation_context(&situations),
            Err(e) => {
                warn!(error = %e, "Failed to load situation landscape for response finder");
                String::new()
            }
        };

        let finding = match rf.investigate_target(&target, &situation_context).await {
            Ok(f) => f,
            Err(e) => {
                warn!(
                    concern_id = %concern_id,
                    error = %e,
                    "Response finder investigation failed"
                );
                out.push(CuriosityEvent::ConcernResponsesScouted { concern_id });
                return Ok(out);
            }
        };

        for response in finding.responses.into_iter().take(MAX_RESPONSES_PER_CONCERN) {
            match rf.classify_response(&response).await {
                Ok(ResponseClassification::New { signal_id }) => {
                    let also_addresses: Vec<ResolvedEdge> = rf
                        .resolve_also_addresses(&response.also_addresses)
                        .await
                        .into_iter()
                        .map(|(concern_id, similarity)| ResolvedEdge {
                            concern_id,
                            similarity,
                        })
                        .collect();

                    out.push(CuriosityEvent::SignalDiscovered {
                        signal_id,
                        title: response.title,
                        summary: response.summary,
                        signal_type: response.signal_type,
                        url: response.url,
                        parent_concern_id: concern_id,
                        match_strength: response.match_strength,
                        explanation: response.explanation,
                        is_gravity: false,
                        event_date: response.event_date,
                        is_recurring: response.is_recurring,
                        venue: None,
                        organizer: None,
                        gathering_type: None,
                        what_needed: None,
                        stated_goal: None,
                        availability: None,
                        eligibility: None,
                        also_addresses,
                        resources: response.resources,
                        diffusion_mechanism: Some(response.diffusion_mechanism),
                    });
                }
                Ok(ResponseClassification::Duplicate { existing_id }) => {
                    out.push(SystemEvent::ResponseLinked {
                        signal_id: existing_id,
                        concern_id,
                        strength: response.match_strength.clamp(0.0, 1.0),
                        explanation: response.explanation,
                        source_url: None,
                    });
                }
                Err(e) => {
                    warn!(
                        concern_id = %concern_id,
                        response_title = response.title.as_str(),
                        error = %e,
                        "Failed to classify response"
                    );
                }
            }
        }

        for tension in &finding.emergent_tensions {
            match rf.classify_emergent_tension(tension).await {
                Ok(Some(tension_id)) => {
                    out.push(CuriosityEvent::EmergentTensionDiscovered {
                        tension_id,
                        title: tension.title.clone(),
                        summary: tension.summary.clone(),
                        severity: tension.severity.clone(),
                        opposing: tension.opposing.clone(),
                        url: tension.source_url.clone(),
                        parent_concern_id: concern_id,
                    });
                }
                Ok(None) => {}
                Err(e) => {
                    warn!(
                        tension_title = tension.title.as_str(),
                        error = %e,
                        "Failed to classify emergent tension"
                    );
                }
            }
        }

        let sources: Vec<_> = finding
            .future_queries
            .iter()
            .take(5)
            .map(|q| build_future_query_source(q, &target.title, "Response finder"))
            .collect();
        if !sources.is_empty() {
            out.push(DiscoveryEvent::SourcesDiscovered {
                sources,
                discovered_by: "response_finder".to_string(),
            });
        }

        out.push(CuriosityEvent::ConcernResponsesScouted { concern_id });

        info!(
            concern_id = %concern_id,
            title = target.title.as_str(),
            "Response finding complete"
        );

        Ok(out)
    }

    #[reactor(on = WorldEvent, id = "curiosity:find_gatherings", filter = concern_not_gathering_scouted)]
    async fn find_gatherings(
        event: WorldEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let concern_id = match &event {
            WorldEvent::ConcernRaised { id, .. } => *id,
            _ => return Ok(Events::new()),
        };
        let mut out = Events::new();

        let deps = ctx.deps();
        let state = ctx.aggregate::<PipelineState>().curr;

        let (region, graph, archive) = match (
            state.run_scope.region(),
            deps.graph.as_deref(),
            deps.archive.as_ref(),
        ) {
            (Some(r), Some(g), Some(a)) => (r, g, a.clone()),
            _ => {
                out.push(CuriosityEvent::ConcernGatheringsScouted { concern_id });
                return Ok(out);
            }
        };

        if !state.has_budget(
            OperationCost::CLAUDE_HAIKU_GATHERING_FINDER
                + OperationCost::SEARCH_GATHERING_FINDER,
        ) {
            ctx.logger
                .debug("Skipped gathering finder: insufficient budget");
            out.push(CuriosityEvent::ConcernGatheringsScouted { concern_id });
            return Ok(out);
        }

        let target = GatheringFinderTarget {
            concern_id,
            title: event.title().unwrap_or_default().to_string(),
            summary: event.summary().unwrap_or_default().to_string(),
            severity: "medium".to_string(),
            category: None,
            opposing: event.opposing().map(|s| s.to_string()),
            cause_heat: 0.0,
        };

        info!(
            concern_id = %concern_id,
            title = target.title.as_str(),
            "Finding gatherings for concern"
        );

        let ai = deps.ai.as_ref().expect("ai required for gathering finder");
        let gf_deps = GatheringFinderDeps::new(
            graph,
            archive,
            &*deps.embedder,
            ai.as_ref(),
            region.clone(),
            deps.run_id.to_string(),
        );

        let finding = match gathering_finder::investigate_target(&gf_deps, &target).await {
            Ok(f) => f,
            Err(e) => {
                warn!(
                    concern_id = %concern_id,
                    error = %e,
                    "Gathering finder investigation failed"
                );
                out.push(CuriosityEvent::ConcernGatheringsScouted { concern_id });
                return Ok(out);
            }
        };

        if finding.no_gravity {
            info!(
                concern_id = %concern_id,
                reason = finding.no_gravity_reason.as_deref().unwrap_or("unknown"),
                "No gravity found — early termination"
            );

            let sources: Vec<_> = finding
                .future_queries
                .iter()
                .take(5)
                .map(|q| build_future_query_source(q, &target.title, "Gathering finder"))
                .collect();
            if !sources.is_empty() {
                out.push(DiscoveryEvent::SourcesDiscovered {
                    sources,
                    discovered_by: "gathering_finder".to_string(),
                });
            }

            out.push(CuriosityEvent::ConcernGatheringsScouted { concern_id });
            return Ok(out);
        }

        for gathering in finding.gatherings.into_iter().take(MAX_GATHERINGS_PER_CONCERN) {
            match gathering_finder::classify_gathering(&gf_deps, &gathering).await {
                Ok(GatheringClassification::New { signal_id }) => {
                    let also_addresses: Vec<ResolvedEdge> = gathering_finder::resolve_also_addresses(
                        &gf_deps,
                        &gathering.also_addresses,
                    )
                    .await
                    .into_iter()
                    .map(|(concern_id, similarity)| ResolvedEdge {
                        concern_id,
                        similarity,
                    })
                    .collect();

                    out.push(CuriosityEvent::SignalDiscovered {
                        signal_id,
                        title: gathering.title,
                        summary: gathering.summary,
                        signal_type: gathering.signal_type,
                        url: gathering.url,
                        parent_concern_id: concern_id,
                        match_strength: gathering.match_strength,
                        explanation: gathering.explanation,
                        is_gravity: true,
                        event_date: gathering.event_date,
                        is_recurring: gathering.is_recurring,
                        venue: gathering.venue,
                        organizer: gathering.organizer,
                        gathering_type: Some(gathering.gathering_type),
                        what_needed: None,
                        stated_goal: None,
                        availability: None,
                        eligibility: None,
                        also_addresses,
                        resources: vec![],
                        diffusion_mechanism: None,
                    });
                }
                Ok(GatheringClassification::Duplicate { existing_id }) => {
                    out.push(SystemEvent::ConcernLinked {
                        signal_id: existing_id,
                        concern_id,
                        strength: gathering.match_strength.clamp(0.0, 1.0),
                        explanation: gathering.explanation,
                        source_url: None,
                    });
                    out.push(SystemEvent::FreshnessConfirmed {
                        signal_ids: vec![existing_id],
                        node_type: NodeType::Gathering,
                        confirmed_at: Utc::now(),
                    });
                }
                Err(e) => {
                    warn!(
                        concern_id = %concern_id,
                        gathering_title = gathering.title.as_str(),
                        error = %e,
                        "Failed to classify gathering"
                    );
                }
            }
        }

        let sources: Vec<_> = finding
            .future_queries
            .iter()
            .take(5)
            .map(|q| build_future_query_source(q, &target.title, "Gathering finder"))
            .collect();
        if !sources.is_empty() {
            out.push(DiscoveryEvent::SourcesDiscovered {
                sources,
                discovered_by: "gathering_finder".to_string(),
            });
        }

        out.push(CuriosityEvent::ConcernGatheringsScouted { concern_id });

        info!(
            concern_id = %concern_id,
            title = target.title.as_str(),
            "Gathering finding complete"
        );

        Ok(out)
    }

    #[reactor(on = CuriosityEvent, id = "curiosity:materialize", filter = is_curiosity_discovery)]
    async fn materialize(
        event: CuriosityEvent,
        ctx: Context<ScoutEngineDeps>,
    ) -> Result<Events> {
        let state = ctx.aggregate::<PipelineState>().curr;
        let region = state.run_scope.region();

        let result = match materializer::materialize(event, region) {
            Some(r) => r,
            None => return Ok(Events::new()),
        };

        let mut out = Events::new();

        // World fact + system classifications
        out.push(node_to_world_event(&result.node, None));
        for sys in node_system_events(&result.node) {
            out.push(sys);
        }

        // Concern edges (responds-to or drawn-to)
        for edge in result.concern_edges {
            emit_concern_edge(&mut out, edge);
        }

        // Resource edges
        for res in result.resources {
            out.push(WorldEvent::ResourceIdentified {
                resource_id: uuid::Uuid::new_v4(),
                name: res.name,
                slug: res.slug.clone(),
                description: res.description,
            });
            out.push(WorldEvent::ResourceLinked {
                signal_id: res.signal_id,
                resource_slug: res.slug,
                role: res.role,
                confidence: res.confidence,
                quantity: res.quantity,
                notes: None,
                capacity: res.capacity,
            });
        }

        // Venue edges
        if let Some(venue) = result.venue {
            if let Some(r) = region {
                out.push(SystemEvent::PlaceDiscovered {
                    place_id: uuid::Uuid::new_v4(),
                    name: venue.name,
                    slug: venue.slug.clone(),
                    lat: r.center_lat,
                    lng: r.center_lng,
                    discovered_at: Utc::now(),
                });
            }
            out.push(SystemEvent::GathersAtPlaceLinked {
                signal_id: venue.signal_id,
                place_slug: venue.slug,
            });
        }

        Ok(out)
    }
}
