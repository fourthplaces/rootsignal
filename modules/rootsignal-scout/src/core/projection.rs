//! Infrastructure projections: project events to Neo4j, maintain scout_runs table.
//!
//! Persistence is handled by seesaw's unified `Store` trait via
//! `PostgresStore`. Aggregator state is handled by seesaw's
//! registered aggregators (priority 1).
//!
//! Projections here:
//! - `neo4j_projection_handler` — project events to Neo4j graph (Handler, needs priority control)
//! - `scout_runs_projection` — maintain the scout_runs lookup table
//! - `system_log_projection` — print SystemLog events to stdout

use std::sync::Arc;

use chrono::Utc;
use rootsignal_graph::{query, GraphClient, GraphProjector};
use seesaw_core::{events, on_any, project, AnyEvent, Context, Events, Handler, Projection};
use tracing::info;

use rootsignal_common::events::{Event, EventDomain, Eventlike, SystemEvent, WorldEvent};
use rootsignal_common::telemetry_events::TelemetryEvent;

use crate::core::aggregate::PipelineState;
use crate::core::engine::ScoutEngineDeps;
use crate::core::pipeline_events::PipelineEvent;
use crate::domains::discovery::events::DiscoveryEvent;
use crate::domains::enrichment::events::EnrichmentEvent;
use crate::domains::expansion::events::ExpansionEvent;
use crate::domains::lifecycle::events::LifecycleEvent;
use crate::domains::scrape::events::ScrapeEvent;
use crate::domains::signals::events::{DedupOutcome, SignalEvent};
use crate::domains::situation_weaving::events::SituationWeavingEvent;
use crate::domains::supervisor::events::SupervisorEvent;
use crate::domains::synthesis::events::SynthesisEvent;

// Priority-0: event persistence — handled by seesaw's unified Store (PostgresStore in production).
// Priority-1: aggregate state — handled by seesaw aggregators.

/// Priority-2 handler: project events to Neo4j graph.
///
/// Captures `GraphProjector` via closure — not on `ScoutEngineDeps`.
/// Routes events by `EventDomain` — exhaustive match ensures compile-time
/// safety when new domains are added.
pub fn neo4j_projection_handler(projector: GraphProjector) -> Handler<ScoutEngineDeps> {
    let projector = Arc::new(projector);
    on_any()
        .id("neo4j_projection")
        .priority(2)
        .then(move |event: AnyEvent, ctx: Context<ScoutEngineDeps>| {
            let projector = projector.clone();
            async move {
                // Classify the event into its domain. Each arm is explicit —
                // adding a new EventDomain variant without handling it here
                // will fail to compile.
                let (domain, event_type, payload) = classify_event(&event);

                // Exhaustive match — no wildcard. Mirrors GraphProjector::project().
                match domain {
                    EventDomain::Fact => {}
                    EventDomain::Discovery | EventDomain::Pipeline => {}
                    EventDomain::Scrape => return Ok(events![]),
                    EventDomain::Signal => {
                        if let Some(SignalEvent::DedupCompleted { ref verdicts, .. }) = event.downcast_ref::<SignalEvent>() {
                            project_dedup_verdicts(projector.client(), verdicts).await?;
                        }
                        return Ok(events![]);
                    }
                    EventDomain::Lifecycle => return Ok(events![]),
                    EventDomain::Enrichment => return Ok(events![]),
                    EventDomain::Expansion => return Ok(events![]),
                    EventDomain::Synthesis => return Ok(events![]),
                    EventDomain::SituationWeaving => return Ok(events![]),
                    EventDomain::Supervisor => return Ok(events![]),
                }

                let (event_type, payload) = match (event_type, payload) {
                    (Some(t), Some(p)) => (t, p),
                    _ => return Ok(events![]),
                };

                let deps = ctx.deps();
                let stored = rootsignal_events::StoredEvent {
                    seq: 0,
                    ts: Utc::now(),
                    event_type,
                    parent_seq: None,
                    caused_by_seq: None,
                    run_id: Some(deps.run_id.to_string()),
                    actor: None,
                    payload,
                    schema_v: 1,
                    id: Some(ctx.current_event_id()),
                    parent_id: ctx.parent_event_id(),
                    correlation_id: None,
                    aggregate_type: None,
                    aggregate_id: None,
                    handler_id: None,
                };
                projector.project(&stored).await?;
                Ok(events![])
            }
        })
}

/// Classify a live event into its domain, event_type string, and payload.
///
/// Returns `(domain, Option<event_type>, Option<payload>)`.
/// For non-projectable events within a projectable domain, event_type/payload
/// are None (the handler skips them).
fn classify_event(
    event: &AnyEvent,
) -> (EventDomain, Option<String>, Option<serde_json::Value>) {
    if let Some(e) = event.downcast_ref::<WorldEvent>() {
        (
            EventDomain::Fact,
            Some(e.event_type().to_string()),
            Some(Event::World(e.clone()).to_payload()),
        )
    } else if let Some(e) = event.downcast_ref::<SystemEvent>() {
        (
            EventDomain::Fact,
            Some(e.event_type().to_string()),
            Some(Event::System(e.clone()).to_payload()),
        )
    } else if let Some(e) = event.downcast_ref::<TelemetryEvent>() {
        (
            EventDomain::Fact,
            Some(e.event_type().to_string()),
            Some(Event::Telemetry(e.clone()).to_payload()),
        )
    } else if let Some(e) = event.downcast_ref::<DiscoveryEvent>() {
        if e.is_projectable() {
            (EventDomain::Discovery, Some(e.event_type_str()), Some(e.to_persist_payload()))
        } else {
            (EventDomain::Discovery, None, None)
        }
    } else if let Some(e) = event.downcast_ref::<PipelineEvent>() {
        if e.is_projectable() {
            (EventDomain::Pipeline, Some(e.event_type_str()), Some(e.to_persist_payload()))
        } else {
            (EventDomain::Pipeline, None, None)
        }
    } else if event.downcast_ref::<ScrapeEvent>().is_some() {
        (EventDomain::Scrape, None, None)
    } else if let Some(e) = event.downcast_ref::<SignalEvent>() {
        (
            EventDomain::Signal,
            Some(e.event_type_str()),
            Some(e.to_persist_payload()),
        )
    } else if event.downcast_ref::<LifecycleEvent>().is_some() {
        (EventDomain::Lifecycle, None, None)
    } else if event.downcast_ref::<EnrichmentEvent>().is_some() {
        (EventDomain::Enrichment, None, None)
    } else if event.downcast_ref::<ExpansionEvent>().is_some() {
        (EventDomain::Expansion, None, None)
    } else if event.downcast_ref::<SynthesisEvent>().is_some() {
        (EventDomain::Synthesis, None, None)
    } else if event.downcast_ref::<SituationWeavingEvent>().is_some() {
        (EventDomain::SituationWeaving, None, None)
    } else if event.downcast_ref::<SupervisorEvent>().is_some() {
        (EventDomain::Supervisor, None, None)
    } else {
        // Genuinely unknown event type — log at debug, not warn.
        // If this fires frequently, a new event type needs adding above.
        tracing::debug!("neo4j_projection: unrecognized event type (not in any known domain)");
        (EventDomain::Fact, None, None)
    }
}

/// Detect terminal events that mean a run is finished.
///
/// Terminal events are domain facts — the projection observes them
/// and writes stats to the scout_runs table.
fn is_terminal_event(event: &AnyEvent, ctx: &Context<ScoutEngineDeps>) -> bool {
    if event.downcast_ref::<SynthesisEvent>()
        .is_some_and(|e| matches!(e, SynthesisEvent::SynthesisCompleted { .. }))
    {
        return true;
    }

    if event.downcast_ref::<SupervisorEvent>()
        .is_some_and(|e| matches!(e, SupervisorEvent::SupervisionCompleted | SupervisorEvent::NothingToSupervise { .. }))
    { return true; }

    false
}

/// Projection: maintain the `scout_runs` lookup table.
///
/// On `ScoutRunRequested`: INSERT a new row.
/// On terminal events: UPDATE with stats JSONB (finished_at handled by post_settle_cleanup).
pub fn scout_runs_projection() -> Projection<ScoutEngineDeps> {
    project("scout_runs_projection").then(move |event: AnyEvent, ctx: Context<ScoutEngineDeps>| {
        async move {
            // INSERT on ScoutRunRequested
            if let Some(LifecycleEvent::ScoutRunRequested { run_id, scope }) = event.downcast_ref::<LifecycleEvent>() {
                let deps = ctx.deps();
                if let Some(pool) = &deps.pg_pool {
                    let region = scope
                        .region()
                        .map(|r| r.name.as_str())
                        .unwrap_or("unknown");
                    let scope_json = scope
                        .region()
                        .and_then(|r| serde_json::to_value(r).ok());
                    sqlx::query(
                        "INSERT INTO scout_runs (run_id, region, scope, started_at) \
                         VALUES ($1, $2, $3, now()) \
                         ON CONFLICT (run_id) DO NOTHING",
                    )
                    .bind(run_id.to_string())
                    .bind(region)
                    .bind(&scope_json)
                    .execute(pool)
                    .await?;
                }
                return Ok(());
            }

            // UPDATE stats on terminal events
            if is_terminal_event(&event, &ctx) {
                let deps = ctx.deps();

                if let Some(ref budget) = deps.budget {
                    budget.log_status();
                }

                let (_, state) = ctx.singleton::<PipelineState>();
                info!("{}", state.stats);

                if let Some(pool) = &deps.pg_pool {
                    let stats_json = serde_json::to_value(&state.stats)?;
                    sqlx::query(
                        "UPDATE scout_runs SET stats = $2 WHERE run_id = $1",
                    )
                    .bind(deps.run_id.to_string())
                    .bind(stats_json)
                    .execute(pool)
                    .await?;
                }
            }

            Ok(())
        }
    })
}

/// Projection: print SystemLog events to stdout via tracing.
pub fn system_log_projection() -> Projection<ScoutEngineDeps> {
    project("system_log_stdout").then(move |event: AnyEvent, _ctx: Context<ScoutEngineDeps>| {
        async move {
            if let Some(TelemetryEvent::SystemLog { message, .. }) =
                event.downcast_ref::<TelemetryEvent>()
            {
                tracing::info!(target: "system_log", "{}", message);
            }
            Ok(())
        }
    })
}

/// Test-only handler: capture every event into a shared Vec for inspection.
///
/// Only registered when `ScoutEngineDeps.captured_events` is Some.
/// Stores raw `AnyEvent`s — test code uses `downcast_ref` to inspect.
pub fn capture_handler(
    sink: Arc<std::sync::Mutex<Vec<AnyEvent>>>,
) -> Handler<ScoutEngineDeps> {
    on_any()
        .id("test_capture")
        .priority(0)
        .then(move |event: AnyEvent, _ctx: Context<ScoutEngineDeps>| {
            let sink = sink.clone();
            async move {
                sink.lock().unwrap().push(event);
                Ok::<Events, anyhow::Error>(events![])
            }
        })
}

/// Project DedupCompleted verdicts to Neo4j.
///
/// Created: wire edges (source, resources, tags, actor).
/// Corroborated: set corroboration_count + last_confirmed_active.
/// Refreshed: set last_confirmed_active.
///
/// World/System events (NodeCreated, CitationPublished, ActorIdentified, etc.)
/// are projected by the existing project_world/project_system arms — this
/// function only handles the wiring data packed into DedupOutcome.
async fn project_dedup_verdicts(
    client: &GraphClient,
    verdicts: &[DedupOutcome],
) -> anyhow::Result<()> {
    use crate::core::extractor::ResourceRole;

    for verdict in verdicts {
        match verdict {
            DedupOutcome::Created {
                node_id,
                source_id,
                resource_tags,
                signal_tags,
                actor,
                ..
            } => {
                // PRODUCED_BY edge
                if let Some(sid) = source_id {
                    let q = query(
                        "MATCH (n)
                         WHERE n.id = $signal_id
                           AND (n:Gathering OR n:Resource OR n:HelpRequest OR n:Announcement OR n:Concern OR n:Condition)
                         MATCH (s:Source {id: $source_id})
                         MERGE (n)-[:PRODUCED_BY]->(s)",
                    )
                    .param("signal_id", node_id.to_string())
                    .param("source_id", sid.to_string());
                    client.run(q).await?;
                }

                // Resource nodes + role edges
                for tag in resource_tags.iter().filter(|t| t.confidence >= 0.3) {
                    let slug = rootsignal_common::slugify(&tag.slug);
                    let rq = query(
                        "MERGE (r:Resource {slug: $slug})
                         ON CREATE SET
                             r.id = $id,
                             r.name = $name,
                             r.description = $description,
                             r.sensitivity = 'general',
                             r.confidence = 1.0,
                             r.signal_count = 1,
                             r.created_at = datetime(),
                             r.last_seen = datetime()
                         ON MATCH SET
                             r.signal_count = r.signal_count + 1,
                             r.last_seen = datetime()",
                    )
                    .param("slug", slug.as_str())
                    .param("id", uuid::Uuid::new_v4().to_string())
                    .param("name", tag.slug.as_str())
                    .param("description", tag.context.as_deref().unwrap_or(""));
                    client.run(rq).await?;

                    let (quantity, capacity) = match tag.role {
                        ResourceRole::Requires => (tag.context.clone(), None),
                        ResourceRole::Prefers => (None, None),
                        ResourceRole::Offers => (None, tag.context.clone()),
                    };
                    let role_str = tag.role.to_string();
                    let edge_q = match role_str.as_str() {
                        "requires" => {
                            query(
                                "MATCH (s) WHERE s.id = $sid AND (s:HelpRequest OR s:Gathering)
                                 MATCH (r:Resource {slug: $slug})
                                 MERGE (s)-[e:REQUIRES]->(r)
                                 ON CREATE SET e.confidence = $confidence, e.quantity = $quantity, e.notes = ''
                                 ON MATCH SET e.confidence = $confidence, e.quantity = $quantity"
                            )
                            .param("sid", node_id.to_string())
                            .param("slug", slug.as_str())
                            .param("confidence", tag.confidence.clamp(0.0, 1.0))
                            .param("quantity", quantity.unwrap_or_default())
                        }
                        "prefers" => {
                            query(
                                "MATCH (s) WHERE s.id = $sid AND (s:HelpRequest OR s:Gathering)
                                 MATCH (r:Resource {slug: $slug})
                                 MERGE (s)-[e:PREFERS]->(r)
                                 ON CREATE SET e.confidence = $confidence
                                 ON MATCH SET e.confidence = $confidence"
                            )
                            .param("sid", node_id.to_string())
                            .param("slug", slug.as_str())
                            .param("confidence", tag.confidence.clamp(0.0, 1.0))
                        }
                        "offers" => {
                            query(
                                "MATCH (s:Resource {id: $sid})
                                 MATCH (r:Resource {slug: $slug})
                                 MERGE (s)-[e:OFFERS]->(r)
                                 ON CREATE SET e.confidence = $confidence, e.capacity = $capacity
                                 ON MATCH SET e.confidence = $confidence, e.capacity = $capacity"
                            )
                            .param("sid", node_id.to_string())
                            .param("slug", slug.as_str())
                            .param("confidence", tag.confidence.clamp(0.0, 1.0))
                            .param("capacity", capacity.unwrap_or_default())
                        }
                        _ => continue,
                    };
                    client.run(edge_q).await?;
                }

                // Tag nodes + TAGGED edges
                for slug in signal_tags {
                    let name = slug.replace('-', " ");
                    let tq = query(
                        "MATCH (s)
                         WHERE s.id = $signal_id
                           AND (s:Gathering OR s:Resource OR s:HelpRequest OR s:Announcement OR s:Concern OR s:Condition)
                         MERGE (t:Tag {slug: $slug})
                         ON CREATE SET t.name = $name
                         MERGE (s)-[r:TAGGED]->(t)
                         SET r.weight = 1.0",
                    )
                    .param("signal_id", node_id.to_string())
                    .param("slug", slug.as_str())
                    .param("name", name.as_str());
                    client.run(tq).await?;
                }

                // Actor → Source edge (ActorIdentified + ActorLinkedToSignal are
                // emitted as SystemEvents and projected via project_system)
                if let Some(ref resolved) = actor {
                    if resolved.is_new {
                        if let Some(sid) = resolved.source_id {
                            let aq = query(
                                "MATCH (a:Actor {id: $actor_id})
                                 MATCH (s:Source {id: $source_id})
                                 MERGE (a)-[:HAS_SOURCE]->(s)",
                            )
                            .param("actor_id", resolved.actor_id.to_string())
                            .param("source_id", sid.to_string());
                            client.run(aq).await?;
                        }
                    }
                }
            }

            DedupOutcome::Corroborated {
                existing_id,
                node_type,
                new_corroboration_count,
                ..
            } => {
                let label = node_type_label(*node_type);
                let q = query(&format!(
                    "MATCH (n:{label} {{id: $id}})
                     SET n.corroboration_count = $count,
                         n.last_confirmed_active = datetime()"
                ))
                .param("id", existing_id.to_string())
                .param("count", *new_corroboration_count as i64);
                client.run(q).await?;
            }

            DedupOutcome::Refreshed {
                existing_id,
                node_type,
                ..
            } => {
                let label = node_type_label(*node_type);
                let q = query(&format!(
                    "MATCH (n:{label} {{id: $id}})
                     SET n.last_confirmed_active = datetime()"
                ))
                .param("id", existing_id.to_string());
                client.run(q).await?;
            }
        }
    }
    Ok(())
}

fn node_type_label(node_type: rootsignal_common::types::NodeType) -> &'static str {
    match node_type {
        rootsignal_common::types::NodeType::Gathering => "Gathering",
        rootsignal_common::types::NodeType::Resource => "Resource",
        rootsignal_common::types::NodeType::HelpRequest => "HelpRequest",
        rootsignal_common::types::NodeType::Announcement => "Announcement",
        rootsignal_common::types::NodeType::Concern => "Concern",
        rootsignal_common::types::NodeType::Condition => "Condition",
        rootsignal_common::types::NodeType::Citation => "Citation",
    }
}
