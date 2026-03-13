//! GraphProjector — projection of facts into Neo4j nodes and edges.
//!
//! Architecture: plan/execute separation.
//!
//! `plan()` is pure — it maps an event to a list of `Op`s (queries to run),
//! without touching Neo4j. `execute()` runs the ops individually (live mode).
//! `execute_batch()` groups `SetSignalProp` ops into UNWIND queries and wraps
//! everything in a single transaction (replay mode).
//!
//! Idempotency: all writes use MERGE or conditional SET.
//! Replaying the same event twice produces the same graph state.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Utc};
use neo4rs::query;
use tracing::{debug, info, warn};
use uuid::Uuid;

use rootsignal_common::events::{
    AnnouncementCorrection, ConcernCorrection, ConditionCorrection, Event, GatheringCorrection,
    HelpRequestCorrection, Location, ResourceCorrection, Schedule, SignalDiversityScore,
    SituationChange, SourceChange, SystemEvent, SystemSourceChange, WorldEvent,
};
use rootsignal_common::types::{Entity, NodeType, SourceNode};
use rootsignal_common::EmbeddingLookup;
use causal::types::PersistedEvent;
use crate::GraphClient;

fn schema_v(event: &PersistedEvent) -> i16 {
    event.metadata.get("schema_v").and_then(|v| v.as_i64()).unwrap_or(1) as i16
}
fn run_id_str(event: &PersistedEvent) -> String {
    event.metadata.get("run_id").and_then(|v| v.as_str()).unwrap_or("").to_string()
}
fn actor_str(event: &PersistedEvent) -> String {
    event.metadata.get("actor").and_then(|v| v.as_str()).unwrap_or("").to_string()
}


// ---------------------------------------------------------------------------
// GraphProjector
// ---------------------------------------------------------------------------

/// Pure projection of facts into Neo4j nodes and edges.
pub struct GraphProjector {
    client: GraphClient,
    embedding_store: Option<Arc<dyn EmbeddingLookup>>,
}

/// Result of applying a single event.
#[derive(Debug)]
pub enum ApplyResult {
    /// The event produced a graph mutation.
    Applied,
    /// The event was a no-op (observability, informational, or unknown type).
    NoOp,
    /// The event payload could not be deserialized.
    DeserializeError(String),
}

/// A single planned graph operation. Pure data — no I/O.
pub enum Op {
    /// Run a single pre-built query.
    Run(neo4rs::Query),
    /// Run multiple queries in sequence (multi-query events).
    RunAll(Vec<neo4rs::Query>),
    /// SET a property on a signal node via multi-label coalesce.
    /// The batch executor collapses these into UNWIND queries by property name.
    SetSignalProp {
        signal_id: String,
        property: &'static str,
        value: neo4rs::BoltType,
        /// Include Condition in the coalesce lookup.
        include_condition: bool,
    },
    /// Compute embedding then SET. Deferred — requires async I/O.
    Embed {
        label: &'static str,
        id: Uuid,
        title: String,
        summary: String,
    },
}

/// The result of planning a single event's projection.
pub struct Plan {
    pub ops: Vec<Op>,
    pub result: ApplyResult,
}

impl Plan {
    fn applied(ops: Vec<Op>) -> Self {
        Plan { ops, result: ApplyResult::Applied }
    }
    fn single(op: Op) -> Self {
        Plan { ops: vec![op], result: ApplyResult::Applied }
    }
    fn skip() -> Self {
        Plan { ops: vec![], result: ApplyResult::NoOp }
    }
    fn error(msg: String) -> Self {
        Plan { ops: vec![], result: ApplyResult::DeserializeError(msg) }
    }
}

impl GraphProjector {
    pub fn new(client: GraphClient) -> Self {
        Self { client, embedding_store: None }
    }

    /// Access the underlying graph client for direct Cypher queries.
    pub fn client(&self) -> &GraphClient {
        &self.client
    }

    /// Attach an embedding store for computing embeddings at projection time.
    pub fn with_embedding_store(mut self, store: Arc<dyn EmbeddingLookup>) -> Self {
        self.embedding_store = Some(store);
        self
    }

    /// Execute an embedding write. Async I/O — called by the executor, not the planner.
    async fn run_embedding(&self, label: &str, id: &Uuid, title: &str, summary: &str) {
        if let Some(ref store) = self.embedding_store {
            let text = format!("{title} {summary}");
            let text = if text.len() > 500 { &text[..500] } else { &text };
            match store.get(text).await {
                Ok(embedding) if !embedding.is_empty() => {
                    let emb_f64: Vec<f64> = embedding.iter().map(|v| *v as f64).collect();
                    let q = query(&format!(
                        "MATCH (n:{label} {{id: $id}}) SET n.embedding = $embedding"
                    ))
                    .param("id", id.to_string())
                    .param("embedding", emb_f64);
                    if let Err(e) = self.client.run(q).await {
                        warn!(error = %e, %label, %id, "Failed to write embedding");
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    warn!(error = %e, %label, %id, "Embedding lookup failed, skipping");
                }
            }
        }
    }

    // =================================================================
    // Public API — plan/execute
    // =================================================================

    /// Plan operations for a single event. Pure — no I/O.
    pub fn plan(&self, event: &PersistedEvent) -> Plan {
        use rootsignal_common::events::EventDomain;

        let domain = match EventDomain::from_event_type(&event.event_type) {
            Some(d) => d,
            None => {
                warn!(
                    seq = event.position.raw(),
                    event_type = event.event_type,
                    "Unknown event domain — update EventDomain enum"
                );
                return Plan::error(format!("unknown event domain: {}", event.event_type));
            }
        };

        match domain {
            EventDomain::Fact => self.plan_fact(event),
            EventDomain::Discovery | EventDomain::Pipeline => self.plan_pipeline(event),
            EventDomain::Scrape => Plan::skip(),
            EventDomain::Signal => Plan::skip(),
            EventDomain::Lifecycle => Plan::skip(),
            EventDomain::Enrichment => Plan::skip(),
            EventDomain::Expansion => Plan::skip(),
            EventDomain::Synthesis => Plan::skip(),
            EventDomain::SituationWeaving => Plan::skip(),
            EventDomain::Supervisor => Plan::skip(),
            EventDomain::Scheduling => Plan::skip(),
            EventDomain::Curiosity => Plan::skip(),
        }
    }

    /// Execute a single event's plan. Used by live mode.
    pub async fn execute(&self, plan: Plan) -> Result<ApplyResult> {
        for op in plan.ops {
            match op {
                Op::Run(q) => { self.client.run(q).await?; }
                Op::RunAll(qs) => {
                    for q in qs {
                        self.client.run(q).await?;
                    }
                }
                Op::SetSignalProp { signal_id, property, value, include_condition } => {
                    let q = build_signal_set_query(property, &signal_id, value, include_condition);
                    self.client.run(q).await?;
                }
                Op::Embed { label, id, title, summary } => {
                    self.run_embedding(label, &id, &title, &summary).await;
                }
            }
        }
        Ok(plan.result)
    }

    /// Execute a batch of plans with UNWIND optimization. Used by replay mode.
    pub async fn execute_batch(&self, plans: Vec<Plan>) -> Result<()> {
        let mut prop_batches: HashMap<&'static str, Vec<(String, neo4rs::BoltType, bool)>> = HashMap::new();
        let mut sequential: Vec<Op> = Vec::new();
        let mut embeds: Vec<(&'static str, Uuid, String, String)> = Vec::new();

        for plan in plans {
            for op in plan.ops {
                match op {
                    Op::SetSignalProp { signal_id, property, value, include_condition } => {
                        prop_batches.entry(property)
                            .or_default()
                            .push((signal_id, value, include_condition));
                    }
                    Op::Embed { label, id, title, summary } => {
                        embeds.push((label, id, title, summary));
                    }
                    other => sequential.push(other),
                }
            }
        }

        let mut txn = self.client.start_txn().await
            .map_err(|e| anyhow::anyhow!("Failed to start Neo4j transaction: {e}"))?;

        // UNWIND batched property SETs — one query per property name.
        for (property, rows) in &prop_batches {
            if rows.is_empty() { continue; }
            let include_condition = rows.iter().any(|(_, _, ic)| *ic);
            let params: Vec<neo4rs::BoltType> = rows.iter().map(|(id, val, _)| {
                neo4rs::BoltType::Map(neo4rs::BoltMap::from_iter(vec![
                    (neo4rs::BoltString::from("id"), neo4rs::BoltType::String(neo4rs::BoltString::from(id.as_str()))),
                    (neo4rs::BoltString::from("val"), val.clone()),
                ]))
            }).collect();

            let q = query(&build_signal_set_unwind_cypher(property, include_condition))
                .param("rows", params);
            txn.run(q).await
                .map_err(|e| anyhow::anyhow!("UNWIND SET {property}: {e}"))?;
        }

        // Sequential ops within the same transaction.
        for op in sequential {
            match op {
                Op::Run(q) => { txn.run(q).await.map_err(|e| anyhow::anyhow!("batch run: {e}"))?; }
                Op::RunAll(qs) => {
                    for q in qs {
                        txn.run(q).await.map_err(|e| anyhow::anyhow!("batch run_all: {e}"))?;
                    }
                }
                _ => unreachable!(),
            }
        }

        txn.commit().await
            .map_err(|e| anyhow::anyhow!("Failed to commit batch transaction: {e}"))?;

        // Embeddings after txn commit — requires async I/O.
        for (label, id, title, summary) in &embeds {
            self.run_embedding(label, id, title, summary).await;
        }

        Ok(())
    }

    /// Project a single event. Plan + execute in one call (live mode).
    pub async fn project(&self, event: &PersistedEvent) -> Result<ApplyResult> {
        let plan = self.plan(event);
        self.execute(plan).await
    }

    /// Project a batch of events with UNWIND optimization (replay mode).
    pub async fn project_batch(&self, events: &[PersistedEvent]) -> Result<()> {
        let plans: Vec<Plan> = events.iter().map(|e| self.plan(e)).collect();
        self.execute_batch(plans).await
    }

    // =================================================================
    // Planning — pure, no I/O
    // =================================================================

    fn plan_fact(&self, event: &PersistedEvent) -> Plan {
        let mut payload = event.payload.clone();
        rootsignal_events::upcast(&event.event_type, schema_v(event), &mut payload);

        let parsed = match Event::from_payload(&payload) {
            Ok(e) => e,
            Err(e) => {
                warn!(seq = event.position.raw(), error = %e, "Failed to deserialize fact event payload");
                return Plan::error(e.to_string());
            }
        };

        match parsed {
            Event::Telemetry(_) => {
                debug!(
                    seq = event.position.raw(),
                    event_type = event.event_type,
                    "No-op (telemetry)"
                );
                Plan::skip()
            }
            Event::World(world) => self.plan_world(world, event),
            Event::System(system) => self.plan_system(system, event),
        }
    }

    // =================================================================
    // Pipeline events — only projectable variants
    // =================================================================

    fn plan_pipeline(&self, event: &PersistedEvent) -> Plan {
        match event.event_type.as_str() {
            "pipeline:source_discovered" | "discovery:source_discovered" => {
                #[derive(serde::Deserialize)]
                struct Payload {
                    source: SourceNode,
                    #[allow(dead_code)]
                    discovered_by: String,
                }
                let payload: Payload = match serde_json::from_value(event.payload.clone()) {
                    Ok(p) => p,
                    Err(e) => return Plan::error(format!("source_discovered deser: {e}")),
                };
                let s = &payload.source;

                let q = query(
                    "MERGE (s:Source {canonical_key: $canonical_key})
                     ON CREATE SET
                         s.id = $id,
                         s.canonical_value = $canonical_value,
                         s.url = $url,
                         s.discovery_method = $discovery_method,
                         s.created_at = datetime($ts),
                         s.signals_produced = 0,
                         s.signals_corroborated = 0,
                         s.consecutive_empty_runs = 0,
                         s.active = true,
                         s.gap_context = $gap_context,
                         s.weight = $weight,
                         s.avg_signals_per_scrape = 0.0,
                         s.quality_penalty = 1.0,
                         s.source_role = $source_role,
                         s.scrape_count = 0,
                         s.sources_discovered = 0,
                         s.cw_page = $cw_page,
                         s.cw_feed = $cw_feed,
                         s.cw_media = $cw_media,
                         s.cw_discussion = $cw_discussion,
                         s.cw_events = $cw_events
                     ON MATCH SET
                         s.active = CASE WHEN s.active = false AND $discovery_method = 'curated' THEN true ELSE s.active END,
                         s.url = CASE WHEN $url <> '' THEN $url ELSE s.url END",
                )
                .param("id", s.id.to_string())
                .param("canonical_key", s.canonical_key.as_str())
                .param("canonical_value", s.canonical_value.as_str())
                .param("url", s.url.as_deref().unwrap_or(""))
                .param("discovery_method", s.discovery_method.to_string())
                .param("ts", format_dt_from_event(event))
                .param("weight", s.weight)
                .param("source_role", s.source_role.to_string())
                .param("gap_context", s.gap_context.clone().unwrap_or_default())
                .param("cw_page", s.channel_weights.page)
                .param("cw_feed", s.channel_weights.feed)
                .param("cw_media", s.channel_weights.media)
                .param("cw_discussion", s.channel_weights.discussion)
                .param("cw_events", s.channel_weights.events);

                Plan::single(Op::Run(q))
            }
            "discovery:sources_discovered" => {
                Plan::skip()
            }
            _ => {
                debug!(seq = event.position.raw(), event_type = %event.event_type, "No-op (pipeline)");
                Plan::skip()
            }
        }
    }

    // =================================================================
    // World events — observed facts
    // =================================================================

    fn plan_world(&self, world: WorldEvent, event: &PersistedEvent) -> Plan {
        match world {
            // ---------------------------------------------------------
            // Discovery facts — 5 typed variants
            // ---------------------------------------------------------
            WorldEvent::GatheringAnnounced {
                id,
                title,
                summary,
                url,
                published_at,
                extraction_id: _,
                locations,
                mentioned_entities,
                references: _,
                schedule,
                action_url,
            } => {
                let sp = extract_schedule(&schedule);
                let q = build_discovery_query(
                    "Gathering",
                    ", n.starts_at = CASE WHEN $starts_at = '' THEN null ELSE datetime($starts_at) END,
                       n.ends_at = CASE WHEN $ends_at = '' THEN null ELSE datetime($ends_at) END,
                       n.rrule = $rrule, n.all_day = $all_day, n.timezone = $timezone,
                       n.schedule_text = $schedule_text,
                       n.rdates = [d IN $rdates | datetime(d)],
                       n.exdates = [d IN $exdates | datetime(d)],
                       n.action_url = $action_url,
                       n.is_recurring = CASE WHEN $rrule <> '' THEN true ELSE false END",
                    id, &title, &summary, 0.5, &url,
                    &event.created_at, published_at, &locations, event,
                )
                .param("starts_at", sp.starts_at)
                .param("ends_at", sp.ends_at)
                .param("rrule", sp.rrule)
                .param("all_day", sp.all_day)
                .param("timezone", sp.timezone)
                .param("schedule_text", sp.schedule_text)
                .param("rdates", sp.rdates)
                .param("exdates", sp.exdates)
                .param("action_url", action_url.as_deref().unwrap_or(""));

                let mut ops = vec![Op::Run(q)];
                ops.extend(self.plan_entities(&id, "Gathering", &mentioned_entities));
                ops.extend(self.plan_locations(&id, "Gathering", &locations));
                ops.push(Op::Embed { label: "Gathering", id, title, summary });
                Plan::applied(ops)
            }

            WorldEvent::ResourceOffered {
                id,
                title,
                summary,
                url,
                published_at,
                extraction_id: _,
                locations,
                mentioned_entities,
                references: _,
                schedule,
                action_url,
                availability,
                eligibility,
            } => {
                let sp = extract_schedule(&schedule);
                let q = build_discovery_query(
                    "Resource",
                    ", n.action_url = $action_url, n.availability = $availability, n.eligibility = $eligibility,
                       n.starts_at = CASE WHEN $starts_at = '' THEN null ELSE datetime($starts_at) END,
                       n.ends_at = CASE WHEN $ends_at = '' THEN null ELSE datetime($ends_at) END,
                       n.rrule = $rrule, n.all_day = $all_day, n.timezone = $timezone,
                       n.schedule_text = $schedule_text,
                       n.rdates = [d IN $rdates | datetime(d)],
                       n.exdates = [d IN $exdates | datetime(d)]",
                    id,
                    &title,
                    &summary,
                    0.5,
                    &url,
                    &event.created_at,
                    published_at,
                    &locations,
                    event,
                )
                .param("action_url", action_url.as_deref().unwrap_or(""))
                .param("availability", availability.as_deref().unwrap_or(""))
                .param("eligibility", eligibility.as_deref().unwrap_or(""))
                .param("starts_at", sp.starts_at)
                .param("ends_at", sp.ends_at)
                .param("rrule", sp.rrule)
                .param("all_day", sp.all_day)
                .param("timezone", sp.timezone)
                .param("schedule_text", sp.schedule_text)
                .param("rdates", sp.rdates)
                .param("exdates", sp.exdates);

                let mut ops = vec![Op::Run(q)];
                ops.extend(self.plan_entities(&id, "Resource", &mentioned_entities));
                ops.extend(self.plan_locations(&id, "Resource", &locations));
                ops.push(Op::Embed { label: "Resource", id, title, summary });
                Plan::applied(ops)
            }

            WorldEvent::HelpRequested {
                id,
                title,
                summary,
                url,
                published_at,
                extraction_id: _,
                locations,
                mentioned_entities,
                references: _,
                schedule,
                action_url: _,
                what_needed,
                stated_goal,
            } => {
                let sp = extract_schedule(&schedule);
                let q = build_discovery_query(
                    "HelpRequest",
                    ", n.what_needed = $what_needed, n.stated_goal = $stated_goal,
                       n.starts_at = CASE WHEN $starts_at = '' THEN null ELSE datetime($starts_at) END,
                       n.ends_at = CASE WHEN $ends_at = '' THEN null ELSE datetime($ends_at) END,
                       n.rrule = $rrule, n.all_day = $all_day, n.timezone = $timezone,
                       n.schedule_text = $schedule_text,
                       n.rdates = [d IN $rdates | datetime(d)],
                       n.exdates = [d IN $exdates | datetime(d)]",
                    id,
                    &title,
                    &summary,
                    0.5,
                    &url,
                    &event.created_at,
                    published_at,
                    &locations,
                    event,
                )
                .param("what_needed", what_needed.as_deref().unwrap_or(""))
                .param("stated_goal", stated_goal.unwrap_or_default())
                .param("starts_at", sp.starts_at)
                .param("ends_at", sp.ends_at)
                .param("rrule", sp.rrule)
                .param("all_day", sp.all_day)
                .param("timezone", sp.timezone)
                .param("schedule_text", sp.schedule_text)
                .param("rdates", sp.rdates)
                .param("exdates", sp.exdates);

                let mut ops = vec![Op::Run(q)];
                ops.extend(self.plan_entities(&id, "HelpRequest", &mentioned_entities));
                ops.extend(self.plan_locations(&id, "HelpRequest", &locations));
                ops.push(Op::Embed { label: "HelpRequest", id, title, summary });
                Plan::applied(ops)
            }

            WorldEvent::AnnouncementShared {
                id,
                title,
                summary,
                url,
                published_at,
                extraction_id: _,
                locations,
                mentioned_entities,
                references: _,
                schedule,
                action_url: _,
                subject,
                effective_date,
            } => {
                let sp = extract_schedule(&schedule);
                let q = build_discovery_query(
                    "Announcement",
                    ", n.subject = $subject,
                       n.effective_date = CASE WHEN $effective_date = '' THEN null ELSE datetime($effective_date) END,
                       n.starts_at = CASE WHEN $starts_at = '' THEN null ELSE datetime($starts_at) END,
                       n.ends_at = CASE WHEN $ends_at = '' THEN null ELSE datetime($ends_at) END,
                       n.rrule = $rrule, n.all_day = $all_day, n.timezone = $timezone,
                       n.schedule_text = $schedule_text,
                       n.rdates = [d IN $rdates | datetime(d)],
                       n.exdates = [d IN $exdates | datetime(d)]",
                    id, &title, &summary, 0.5, &url,
                    &event.created_at, published_at, &locations, event,
                )
                .param("subject", subject.as_deref().unwrap_or(""))
                .param("effective_date", effective_date.map(|dt| format_dt(&dt)).unwrap_or_default())
                .param("starts_at", sp.starts_at)
                .param("ends_at", sp.ends_at)
                .param("rrule", sp.rrule)
                .param("all_day", sp.all_day)
                .param("timezone", sp.timezone)
                .param("schedule_text", sp.schedule_text)
                .param("rdates", sp.rdates)
                .param("exdates", sp.exdates);

                let mut ops = vec![Op::Run(q)];
                ops.extend(self.plan_entities(&id, "Announcement", &mentioned_entities));
                ops.extend(self.plan_locations(&id, "Announcement", &locations));
                ops.push(Op::Embed { label: "Announcement", id, title, summary });
                Plan::applied(ops)
            }

            WorldEvent::ConcernRaised {
                id,
                title,
                summary,
                url,
                published_at,
                extraction_id: _,
                locations,
                mentioned_entities,
                references: _,
                schedule,
                action_url: _,
                subject,
                opposing,
            } => {
                let sp = extract_schedule(&schedule);
                let q = build_discovery_query(
                    "Concern",
                    ", n.subject = $subject, n.opposing = $opposing,
                       n.starts_at = CASE WHEN $starts_at = '' THEN null ELSE datetime($starts_at) END,
                       n.ends_at = CASE WHEN $ends_at = '' THEN null ELSE datetime($ends_at) END,
                       n.rrule = $rrule, n.all_day = $all_day, n.timezone = $timezone,
                       n.schedule_text = $schedule_text,
                       n.rdates = [d IN $rdates | datetime(d)],
                       n.exdates = [d IN $exdates | datetime(d)]",
                    id,
                    &title,
                    &summary,
                    0.5,
                    &url,
                    &event.created_at,
                    published_at,
                    &locations,
                    event,
                )
                .param("subject", subject.as_deref().unwrap_or(""))
                .param("opposing", opposing.as_deref().unwrap_or(""))
                .param("starts_at", sp.starts_at)
                .param("ends_at", sp.ends_at)
                .param("rrule", sp.rrule)
                .param("all_day", sp.all_day)
                .param("timezone", sp.timezone)
                .param("schedule_text", sp.schedule_text)
                .param("rdates", sp.rdates)
                .param("exdates", sp.exdates);

                let mut ops = vec![Op::Run(q)];
                ops.extend(self.plan_entities(&id, "Concern", &mentioned_entities));
                ops.extend(self.plan_locations(&id, "Concern", &locations));
                ops.push(Op::Embed { label: "Concern", id, title, summary });
                Plan::applied(ops)
            }

            WorldEvent::ConditionObserved {
                id,
                title,
                summary,
                url,
                published_at,
                extraction_id: _,
                locations,
                mentioned_entities,
                references: _,
                schedule,
                action_url: _,
                subject,
                observed_by,
                measurement,
                affected_scope,
            } => {
                let sp = extract_schedule(&schedule);
                let q = build_discovery_query(
                    "Condition",
                    ", n.subject = $subject, n.observed_by = $observed_by,
                       n.measurement = $measurement, n.affected_scope = $affected_scope,
                       n.starts_at = CASE WHEN $starts_at = '' THEN null ELSE datetime($starts_at) END,
                       n.ends_at = CASE WHEN $ends_at = '' THEN null ELSE datetime($ends_at) END,
                       n.rrule = $rrule, n.all_day = $all_day, n.timezone = $timezone,
                       n.schedule_text = $schedule_text,
                       n.rdates = [d IN $rdates | datetime(d)],
                       n.exdates = [d IN $exdates | datetime(d)]",
                    id, &title, &summary, 0.5, &url,
                    &event.created_at, published_at, &locations, event,
                )
                .param("subject", subject.as_deref().unwrap_or(""))
                .param("observed_by", observed_by.as_deref().unwrap_or(""))
                .param("measurement", measurement.as_deref().unwrap_or(""))
                .param("affected_scope", affected_scope.as_deref().unwrap_or(""))
                .param("starts_at", sp.starts_at)
                .param("ends_at", sp.ends_at)
                .param("rrule", sp.rrule)
                .param("all_day", sp.all_day)
                .param("timezone", sp.timezone)
                .param("schedule_text", sp.schedule_text)
                .param("rdates", sp.rdates)
                .param("exdates", sp.exdates);

                let mut ops = vec![Op::Run(q)];
                ops.extend(self.plan_entities(&id, "Condition", &mentioned_entities));
                ops.extend(self.plan_locations(&id, "Condition", &locations));
                ops.push(Op::Embed { label: "Condition", id, title, summary });
                Plan::applied(ops)
            }

            // ---------------------------------------------------------
            // Lifecycle events — placeholder (log only, no graph action yet)
            // ---------------------------------------------------------
            WorldEvent::GatheringCancelled { signal_id, reason, .. } => {
                debug!(signal_id = %signal_id, reason = %reason, "GatheringCancelled (no-op placeholder)");
                Plan::skip()
            }
            WorldEvent::ResourceDepleted { signal_id, reason, .. } => {
                debug!(signal_id = %signal_id, reason = %reason, "ResourceDepleted (no-op placeholder)");
                Plan::skip()
            }
            WorldEvent::AnnouncementRetracted { signal_id, reason, .. } => {
                debug!(signal_id = %signal_id, reason = %reason, "AnnouncementRetracted (no-op placeholder)");
                Plan::skip()
            }
            WorldEvent::CitationRetracted { citation_id, reason, .. } => {
                debug!(citation_id = %citation_id, reason = %reason, "CitationRetracted (no-op placeholder)");
                Plan::skip()
            }
            WorldEvent::DetailsChanged { signal_id, node_type, title, summary, .. } => {
                let label = match node_type {
                    NodeType::Gathering => "Gathering",
                    NodeType::Resource => "Resource",
                    NodeType::HelpRequest => "HelpRequest",
                    NodeType::Announcement => "Announcement",
                    NodeType::Concern => "Concern",
                    NodeType::Condition => "Condition",
                    NodeType::Citation => {
                        debug!(signal_id = %signal_id, "DetailsChanged on Citation — skipping");
                        return Plan::skip();
                    }
                };

                let q = query(&format!(
                    "MATCH (n:{label} {{id: $signal_id}})
                     SET n.title = $title, n.summary = $summary,
                         n.last_confirmed_active = datetime($ts)"
                ))
                .param("signal_id", signal_id.to_string())
                .param("title", title.as_str())
                .param("summary", summary.as_str())
                .param("ts", format_dt_from_event(event));

                info!(signal_id = %signal_id, label = label, "DetailsChanged projected");
                Plan::applied(vec![
                    Op::Run(q),
                    Op::Embed { label, id: signal_id, title, summary },
                ])
            }

            // ---------------------------------------------------------
            // Citations
            // ---------------------------------------------------------
            WorldEvent::CitationPublished {
                citation_id,
                signal_id,
                url,
                content_hash,
                snippet,
                relevance,
                channel_type,
                evidence_confidence,
            } => {
                let q = query(
                    "OPTIONAL MATCH (g:Gathering {id: $signal_id})
                     OPTIONAL MATCH (a:Resource {id: $signal_id})
                     OPTIONAL MATCH (n:HelpRequest {id: $signal_id})
                     OPTIONAL MATCH (nc:Announcement {id: $signal_id})
                     OPTIONAL MATCH (t:Concern {id: $signal_id})
                     OPTIONAL MATCH (cond:Condition {id: $signal_id})
                     WITH coalesce(g, a, n, nc, t, cond) AS node
                     WHERE node IS NOT NULL
                     MERGE (node)-[:SOURCED_FROM]->(ev:Citation {source_url: $url})
                     ON CREATE SET
                         ev.id = $ev_id,
                         ev.retrieved_at = datetime($ts),
                         ev.content_hash = $content_hash,
                         ev.snippet = $snippet,
                         ev.relevance = $relevance,
                         ev.evidence_confidence = $evidence_confidence,
                         ev.channel_type = $channel_type
                     ON MATCH SET
                         ev.retrieved_at = datetime($ts),
                         ev.content_hash = $content_hash",
                )
                .param("ev_id", citation_id.to_string())
                .param("signal_id", signal_id.to_string())
                .param("url", url.as_str())
                .param("ts", format_dt_from_event(event))
                .param("content_hash", content_hash.as_str())
                .param("snippet", snippet.unwrap_or_default())
                .param("relevance", relevance.unwrap_or_default())
                .param(
                    "evidence_confidence",
                    evidence_confidence.unwrap_or(0.0) as f64,
                )
                .param(
                    "channel_type",
                    channel_type.map(|ct| ct.as_str()).unwrap_or("press"),
                );

                Plan::single(Op::Run(q))
            }

            // ---------------------------------------------------------
            // Resource identification (replay-safe MERGE)
            // ---------------------------------------------------------
            WorldEvent::ResourceIdentified {
                resource_id,
                name,
                slug,
                description,
            } => {
                let q = query(
                    "MERGE (r:Resource {slug: $slug})
                     ON CREATE SET
                         r.id = $id,
                         r.name = $name,
                         r.description = $description,
                         r.sensitivity = 'general',
                         r.confidence = 1.0,
                         r.signal_count = 1,
                         r.created_at = datetime($ts),
                         r.last_seen = datetime($ts)
                     ON MATCH SET
                         r.last_seen = datetime($ts)",
                )
                .param("slug", slug.as_str())
                .param("id", resource_id.to_string())
                .param("name", name.as_str())
                .param("description", description.as_str())
                .param("ts", format_dt_from_event(event));

                Plan::single(Op::Run(q))
            }

            // ---------------------------------------------------------
            // Edge facts
            // ---------------------------------------------------------
            WorldEvent::ResourceLinked {
                signal_id,
                resource_slug,
                role,
                confidence,
                quantity,
                notes,
                capacity,
            } => {
                let q = match role.as_str() {
                    "requires" => {
                        query(
                            "MATCH (s) WHERE s.id = $sid AND (s:HelpRequest OR s:Gathering)
                             MATCH (r:Resource {slug: $slug})
                             MERGE (s)-[e:REQUIRES]->(r)
                             ON CREATE SET e.confidence = $confidence, e.quantity = $quantity, e.notes = $notes
                             ON MATCH SET e.confidence = $confidence, e.quantity = $quantity, e.notes = $notes"
                        )
                        .param("sid", signal_id.to_string())
                        .param("slug", resource_slug.as_str())
                        .param("confidence", confidence as f64)
                        .param("quantity", quantity.unwrap_or_default())
                        .param("notes", notes.unwrap_or_default())
                    }
                    "prefers" => {
                        query(
                            "MATCH (s) WHERE s.id = $sid AND (s:HelpRequest OR s:Gathering)
                             MATCH (r:Resource {slug: $slug})
                             MERGE (s)-[e:PREFERS]->(r)
                             ON CREATE SET e.confidence = $confidence
                             ON MATCH SET e.confidence = $confidence"
                        )
                        .param("sid", signal_id.to_string())
                        .param("slug", resource_slug.as_str())
                        .param("confidence", confidence as f64)
                    }
                    "offers" => {
                        query(
                            "MATCH (s:Resource {id: $sid})
                             MATCH (r:Resource {slug: $slug})
                             MERGE (s)-[e:OFFERS]->(r)
                             ON CREATE SET e.confidence = $confidence, e.capacity = $capacity
                             ON MATCH SET e.confidence = $confidence, e.capacity = $capacity"
                        )
                        .param("sid", signal_id.to_string())
                        .param("slug", resource_slug.as_str())
                        .param("confidence", confidence as f64)
                        .param("capacity", capacity.unwrap_or_default())
                    }
                    _ => {
                        warn!(role = role.as_str(), "Unknown resource edge role, skipping");
                        return Plan::skip();
                    }
                };

                Plan::single(Op::Run(q))
            }

            // ---------------------------------------------------------
            // Provenance links
            // ---------------------------------------------------------
            WorldEvent::SourceLinkDiscovered { .. } => {
                debug!(seq = event.position.raw(), "No-op (source link — informational)");
                Plan::skip()
            }

            WorldEvent::ActorLinkedToSource {
                actor_id,
                source_id,
            } => {
                let q = query(
                    "MATCH (a:Actor {id: $actor_id})
                     MATCH (s:Source {id: $source_id})
                     MERGE (a)-[:HAS_SOURCE]->(s)",
                )
                .param("actor_id", actor_id.to_string())
                .param("source_id", source_id.to_string());

                Plan::single(Op::Run(q))
            }

            WorldEvent::SignalLinkedToSource {
                signal_id,
                source_id,
            } => {
                let q = query(
                    "MATCH (n)
                     WHERE n.id = $signal_id
                       AND (n:Gathering OR n:Resource OR n:HelpRequest OR n:Announcement OR n:Concern OR n:Condition)
                     MATCH (s:Source {id: $source_id})
                     MERGE (n)-[:PRODUCED_BY]->(s)",
                )
                .param("signal_id", signal_id.to_string())
                .param("source_id", source_id.to_string());

                Plan::single(Op::Run(q))
            }

        }
    }

    // =================================================================
    // System decisions — editorial judgments
    // =================================================================

    fn plan_system(
        &self,
        system: SystemEvent,
        event: &PersistedEvent,
    ) -> Plan {
        match system {
            // ---------------------------------------------------------
            // Sensitivity + implied queries (paired with discoveries)
            // ---------------------------------------------------------
            SystemEvent::SensitivityClassified { signal_id, level } => {
                Plan::single(Op::SetSignalProp {
                    signal_id: signal_id.to_string(),
                    property: "sensitivity",
                    value: neo4rs::BoltType::String(neo4rs::BoltString::from(level.as_str())),
                    include_condition: false,
                })
            }

            SystemEvent::ToneClassified { signal_id, tone } => {
                Plan::single(Op::SetSignalProp {
                    signal_id: signal_id.to_string(),
                    property: "tone",
                    value: neo4rs::BoltType::String(neo4rs::BoltString::from(tone.to_string().as_str())),
                    include_condition: false,
                })
            }

            SystemEvent::SeverityClassified { signal_id, severity } => {
                Plan::single(Op::SetSignalProp {
                    signal_id: signal_id.to_string(),
                    property: "severity",
                    value: neo4rs::BoltType::String(neo4rs::BoltString::from(severity.to_string().as_str())),
                    include_condition: false,
                })
            }

            SystemEvent::UrgencyClassified { signal_id, urgency } => {
                Plan::single(Op::SetSignalProp {
                    signal_id: signal_id.to_string(),
                    property: "urgency",
                    value: neo4rs::BoltType::String(neo4rs::BoltString::from(urgency.to_string().as_str())),
                    include_condition: false,
                })
            }

            SystemEvent::CategoryClassified { signal_id, category } => {
                Plan::single(Op::SetSignalProp {
                    signal_id: signal_id.to_string(),
                    property: "category",
                    value: neo4rs::BoltType::String(neo4rs::BoltString::from(category.as_str())),
                    include_condition: true,
                })
            }

            SystemEvent::ImpliedQueriesExtracted { signal_id, queries } => {
                let q = query(
                    "OPTIONAL MATCH (g:Gathering {id: $id})
                     OPTIONAL MATCH (a:Resource {id: $id})
                     OPTIONAL MATCH (n:HelpRequest {id: $id})
                     OPTIONAL MATCH (nc:Announcement {id: $id})
                     OPTIONAL MATCH (t:Concern {id: $id})
                     WITH coalesce(g, a, n, nc, t) AS node
                     WHERE node IS NOT NULL
                     SET node.implied_queries = $queries",
                )
                .param("id", signal_id.to_string())
                .param("queries", queries);

                Plan::single(Op::Run(q))
            }

            // ---------------------------------------------------------
            // Corroboration — system judgment that sources confirm the same thing
            // ---------------------------------------------------------
            SystemEvent::ObservationCorroborated {
                signal_id,
                node_type,
                ..
            } => {
                let label = node_type_label(node_type);
                // Guard on a dedicated property so GatheringAnnounced's last_confirmed_active
                // doesn't interfere. Replaying the same event (same timestamp) is a no-op.
                let q = query(&format!(
                    "MATCH (n:{label} {{id: $id}})
                     WHERE n.last_corroboration_ts IS NULL OR n.last_corroboration_ts < datetime($ts)
                     SET n.last_corroboration_ts = datetime($ts),
                         n.last_confirmed_active = datetime($ts),
                         n.corroboration_count = coalesce(n.corroboration_count, 0) + 1"
                ))
                .param("id", signal_id.to_string())
                .param("ts", format_dt_from_event(event));

                Plan::single(Op::Run(q))
            }

            // ---------------------------------------------------------
            // Corroboration scoring
            // ---------------------------------------------------------
            SystemEvent::CorroborationScored {
                signal_id,
                new_corroboration_count,
                ..
            } => {
                Plan::single(Op::SetSignalProp {
                    signal_id: signal_id.to_string(),
                    property: "corroboration_count",
                    value: neo4rs::BoltType::Integer(neo4rs::BoltInteger::new(new_corroboration_count as i64)),
                    include_condition: false,
                })
            }

            // ---------------------------------------------------------
            // Signal lifecycle decisions
            // ---------------------------------------------------------
            SystemEvent::FreshnessConfirmed {
                signal_ids,
                node_type,
                confirmed_at,
            } => {
                let label = node_type_label(node_type);
                let ids: Vec<String> = signal_ids.iter().map(|id| id.to_string()).collect();
                let q = query(&format!(
                    "UNWIND $ids AS id
                     MATCH (n:{label} {{id: id}})
                     SET n.last_confirmed_active = datetime($ts)"
                ))
                .param("ids", ids)
                .param("ts", format_dt(&confirmed_at));

                Plan::single(Op::Run(q))
            }

            SystemEvent::ConfidenceScored {
                signal_id,
                new_confidence,
                ..
            } => {
                Plan::single(Op::SetSignalProp {
                    signal_id: signal_id.to_string(),
                    property: "confidence",
                    value: neo4rs::BoltType::Float(neo4rs::BoltFloat::new(new_confidence as f64)),
                    include_condition: false,
                })
            }

            SystemEvent::ObservationRejected { .. } => {
                Plan::skip()
            }

            SystemEvent::SignalsExpired { signals } => {
                let ts = format_dt_from_event(event);
                let mut ops = Vec::new();
                for s in signals.iter() {
                    let label = node_type_label(s.node_type);
                    let q = query(&format!(
                        "MATCH (n:{label} {{id: $id}})
                         SET n.expired = true,
                             n.expired_at = datetime($ts),
                             n.expired_reason = $reason"
                    ))
                    .param("id", s.signal_id.to_string())
                    .param("ts", ts.clone())
                    .param("reason", s.reason.as_str());

                    ops.push(Op::Run(q));
                }
                if ops.is_empty() {
                    Plan::skip()
                } else {
                    Plan::applied(ops)
                }
            }

            SystemEvent::EntityPurged {
                signal_id,
                node_type,
                ..
            } => {
                let label = node_type_label(node_type);
                let q = query(&format!(
                    "MATCH (n:{label} {{id: $id}})
                     OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Citation)
                     DETACH DELETE n, ev"
                ))
                .param("id", signal_id.to_string());

                Plan::single(Op::Run(q))
            }

            SystemEvent::DuplicateDetected { .. } => {
                Plan::skip()
            }

            SystemEvent::ExtractionDroppedNoDate { .. } => {
                Plan::skip()
            }

            SystemEvent::ReviewVerdictReached {
                signal_id,
                new_status,
                ..
            } => {
                let q = query(
                    "OPTIONAL MATCH (g:Gathering {id: $id})
                     OPTIONAL MATCH (a:Resource {id: $id})
                     OPTIONAL MATCH (n:HelpRequest {id: $id})
                     OPTIONAL MATCH (nc:Announcement {id: $id})
                     OPTIONAL MATCH (t:Concern {id: $id})
                     OPTIONAL MATCH (c:Condition {id: $id})
                     WITH coalesce(g, a, n, nc, t, c) AS node
                     WHERE node IS NOT NULL
                     SET node.review_status = $status",
                )
                .param("id", signal_id.to_string())
                .param("status", new_status.as_str());

                let mut ops = vec![Op::Run(q)];

                if new_status == "accepted" {
                    let promote = query(
                        "OPTIONAL MATCH (g:Gathering {id: $id})
                         OPTIONAL MATCH (a:Resource {id: $id})
                         OPTIONAL MATCH (n:HelpRequest {id: $id})
                         OPTIONAL MATCH (nc:Announcement {id: $id})
                         OPTIONAL MATCH (t:Concern {id: $id})
                         OPTIONAL MATCH (c:Condition {id: $id})
                         WITH coalesce(g, a, n, nc, t, c) AS node
                         WHERE node IS NOT NULL
                         OPTIONAL MATCH (node)-[:PART_OF]->(sit:Situation)
                         WHERE sit.review_status = 'staged'
                           AND NOT EXISTS {
                             MATCH (other)-[:PART_OF]->(sit)
                             WHERE other.review_status <> 'accepted'
                           }
                         SET sit.review_status = 'accepted'",
                    )
                    .param("id", signal_id.to_string());

                    ops.push(Op::Run(promote));
                }

                Plan::applied(ops)
            }

            SystemEvent::ImpliedQueriesConsumed { signal_ids } => {
                let ids: Vec<String> = signal_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS id
                     MATCH (n) WHERE n.id = id AND (n:Resource OR n:Gathering)
                     SET n.implied_queries = null",
                )
                .param("ids", ids);

                Plan::single(Op::Run(q))
            }

            // ---------------------------------------------------------
            // Corrections
            // ---------------------------------------------------------
            SystemEvent::GatheringCorrected {
                signal_id,
                correction,
                ..
            } => {
                let ops = match correction {
                    GatheringCorrection::Title { new, .. } => {
                        vec![self.plan_set_str("Gathering", signal_id, "title", &new)]
                    }
                    GatheringCorrection::Summary { new, .. } => {
                        vec![self.plan_set_str("Gathering", signal_id, "summary", &new)]
                    }
                    GatheringCorrection::Sensitivity { new, .. } => {
                        vec![self.plan_set_str("Gathering", signal_id, "sensitivity", new.as_str())]
                    }
                    GatheringCorrection::Location { new, .. } => {
                        self.plan_update_location("Gathering", signal_id, &new)
                    }
                    GatheringCorrection::Schedule { new, .. } => {
                        vec![self.plan_set_schedule("Gathering", signal_id, &new)]
                    }
                    GatheringCorrection::Organizer { new, .. } => {
                        vec![self.plan_set_str(
                            "Gathering",
                            signal_id,
                            "organizer",
                            new.as_deref().unwrap_or(""),
                        )]
                    }
                    GatheringCorrection::ActionUrl { new, .. } => {
                        vec![self.plan_set_str(
                            "Gathering",
                            signal_id,
                            "action_url",
                            new.as_deref().unwrap_or(""),
                        )]
                    }
                    GatheringCorrection::Unknown => {
                        return Plan::skip();
                    }
                };
                Plan::applied(ops)
            }

            SystemEvent::ResourceCorrected {
                signal_id,
                correction,
                ..
            } => {
                let ops = match correction {
                    ResourceCorrection::Title { new, .. } => {
                        vec![self.plan_set_str("Resource", signal_id, "title", &new)]
                    }
                    ResourceCorrection::Summary { new, .. } => {
                        vec![self.plan_set_str("Resource", signal_id, "summary", &new)]
                    }
                    ResourceCorrection::Sensitivity { new, .. } => {
                        vec![self.plan_set_str("Resource", signal_id, "sensitivity", new.as_str())]
                    }
                    ResourceCorrection::Location { new, .. } => {
                        self.plan_update_location("Resource", signal_id, &new)
                    }
                    ResourceCorrection::ActionUrl { new, .. } => {
                        vec![self.plan_set_str("Resource", signal_id, "action_url", new.as_deref().unwrap_or(""))]
                    }
                    ResourceCorrection::Availability { new, .. } => {
                        vec![self.plan_set_str(
                            "Resource",
                            signal_id,
                            "availability",
                            new.as_deref().unwrap_or(""),
                        )]
                    }
                    ResourceCorrection::IsOngoing { new, .. } => {
                        vec![self.plan_set_bool("Resource", signal_id, "is_ongoing", new.unwrap_or(false))]
                    }
                    ResourceCorrection::Unknown => {
                        return Plan::skip();
                    }
                };
                Plan::applied(ops)
            }

            SystemEvent::HelpRequestCorrected {
                signal_id,
                correction,
                ..
            } => {
                let ops = match correction {
                    HelpRequestCorrection::Title { new, .. } => {
                        vec![self.plan_set_str("HelpRequest", signal_id, "title", &new)]
                    }
                    HelpRequestCorrection::Summary { new, .. } => {
                        vec![self.plan_set_str("HelpRequest", signal_id, "summary", &new)]
                    }
                    HelpRequestCorrection::Sensitivity { new, .. } => {
                        vec![self.plan_set_str("HelpRequest", signal_id, "sensitivity", new.as_str())]
                    }
                    HelpRequestCorrection::Location { new, .. } => {
                        self.plan_update_location("HelpRequest", signal_id, &new)
                    }
                    HelpRequestCorrection::Urgency { new, .. } => {
                        vec![self.plan_set_str(
                            "HelpRequest",
                            signal_id,
                            "urgency",
                            new.map(|u| urgency_str(u)).unwrap_or(""),
                        )]
                    }
                    HelpRequestCorrection::WhatNeeded { new, .. } => {
                        vec![self.plan_set_str(
                            "HelpRequest",
                            signal_id,
                            "what_needed",
                            new.as_deref().unwrap_or(""),
                        )]
                    }
                    HelpRequestCorrection::StatedGoal { new, .. } => {
                        vec![self.plan_set_str("HelpRequest", signal_id, "stated_goal", new.as_deref().unwrap_or(""))]
                    }
                    HelpRequestCorrection::Unknown => {
                        return Plan::skip();
                    }
                };
                Plan::applied(ops)
            }

            SystemEvent::AnnouncementCorrected {
                signal_id,
                correction,
                ..
            } => {
                let ops = match correction {
                    AnnouncementCorrection::Title { new, .. } => {
                        vec![self.plan_set_str("Announcement", signal_id, "title", &new)]
                    }
                    AnnouncementCorrection::Summary { new, .. } => {
                        vec![self.plan_set_str("Announcement", signal_id, "summary", &new)]
                    }
                    AnnouncementCorrection::Sensitivity { new, .. } => {
                        vec![self.plan_set_str("Announcement", signal_id, "sensitivity", new.as_str())]
                    }
                    AnnouncementCorrection::Location { new, .. } => {
                        self.plan_update_location("Announcement", signal_id, &new)
                    }
                    AnnouncementCorrection::Category { new, .. } => {
                        vec![self.plan_set_str(
                            "Announcement",
                            signal_id,
                            "category",
                            new.as_deref().unwrap_or(""),
                        )]
                    }
                    AnnouncementCorrection::EffectiveDate { new, .. } => {
                        let val = new.map(|dt| format_dt(&dt)).unwrap_or_default();
                        let q = query("MATCH (n:Announcement {id: $id}) SET n.effective_date = CASE WHEN $value = '' THEN null ELSE datetime($value) END")
                            .param("id", signal_id.to_string())
                            .param("value", val);
                        vec![Op::Run(q)]
                    }
                    AnnouncementCorrection::Unknown => {
                        return Plan::skip();
                    }
                };
                Plan::applied(ops)
            }

            SystemEvent::ConcernCorrected {
                signal_id,
                correction,
                ..
            } => {
                let ops = match correction {
                    ConcernCorrection::Title { new, .. } => {
                        vec![self.plan_set_str("Concern", signal_id, "title", &new)]
                    }
                    ConcernCorrection::Summary { new, .. } => {
                        vec![self.plan_set_str("Concern", signal_id, "summary", &new)]
                    }
                    ConcernCorrection::Sensitivity { new, .. } => {
                        vec![self.plan_set_str("Concern", signal_id, "sensitivity", new.as_str())]
                    }
                    ConcernCorrection::Location { new, .. } => {
                        self.plan_update_location("Concern", signal_id, &new)
                    }
                    ConcernCorrection::Opposing { new, .. } => {
                        vec![self.plan_set_str(
                            "Concern",
                            signal_id,
                            "opposing",
                            new.as_deref().unwrap_or(""),
                        )]
                    }
                    ConcernCorrection::Unknown => {
                        return Plan::skip();
                    }
                };
                Plan::applied(ops)
            }

            SystemEvent::ConditionCorrected {
                signal_id,
                correction,
                ..
            } => {
                let ops = match correction {
                    ConditionCorrection::Title { new, .. } => {
                        vec![self.plan_set_str("Condition", signal_id, "title", &new)]
                    }
                    ConditionCorrection::Summary { new, .. } => {
                        vec![self.plan_set_str("Condition", signal_id, "summary", &new)]
                    }
                    ConditionCorrection::Sensitivity { new, .. } => {
                        vec![self.plan_set_str("Condition", signal_id, "sensitivity", new.as_str())]
                    }
                    ConditionCorrection::Location { new, .. } => {
                        self.plan_update_location("Condition", signal_id, &new)
                    }
                    ConditionCorrection::Subject { new, .. } => {
                        vec![self.plan_set_str(
                            "Condition",
                            signal_id,
                            "subject",
                            new.as_deref().unwrap_or(""),
                        )]
                    }
                    ConditionCorrection::ObservedBy { new, .. } => {
                        vec![self.plan_set_str(
                            "Condition",
                            signal_id,
                            "observed_by",
                            new.as_deref().unwrap_or(""),
                        )]
                    }
                    ConditionCorrection::Measurement { new, .. } => {
                        vec![self.plan_set_str(
                            "Condition",
                            signal_id,
                            "measurement",
                            new.as_deref().unwrap_or(""),
                        )]
                    }
                    ConditionCorrection::AffectedScope { new, .. } => {
                        vec![self.plan_set_str(
                            "Condition",
                            signal_id,
                            "affected_scope",
                            new.as_deref().unwrap_or(""),
                        )]
                    }
                    ConditionCorrection::Unknown => {
                        return Plan::skip();
                    }
                };
                Plan::applied(ops)
            }

            // ---------------------------------------------------------
            // Actor identification
            // ---------------------------------------------------------
            SystemEvent::ActorIdentified {
                actor_id,
                name,
                actor_type,
                canonical_key,
                domains,
                social_urls,
                description,
                bio,
                location_lat,
                location_lng,
                location_name,
            } => {
                let q = query(
                    "MERGE (a:Actor {canonical_key: $canonical_key})
                     ON CREATE SET
                         a.id = $id,
                         a.name = $name,
                         a.actor_type = $actor_type,
                         a.domains = $domains,
                         a.social_urls = $social_urls,
                         a.description = $description,
                         a.bio = $bio,
                         a.location_lat = $location_lat,
                         a.location_lng = $location_lng,
                         a.location_name = $location_name,
                         a.signal_count = 0,
                         a.first_seen = datetime($ts),
                         a.last_active = datetime($ts)
                     ON MATCH SET
                         a.name = $name,
                         a.last_active = datetime($ts)",
                )
                .param("id", actor_id.to_string())
                .param("canonical_key", canonical_key.as_str())
                .param("name", name.as_str())
                .param("actor_type", actor_type.to_string())
                .param("domains", domains)
                .param("social_urls", social_urls)
                .param("description", description.as_str())
                .param::<Option<String>>("bio", bio)
                .param::<Option<f64>>("location_lat", location_lat)
                .param::<Option<f64>>("location_lng", location_lng)
                .param::<Option<String>>("location_name", location_name)
                .param("ts", format_dt_from_event(event));

                Plan::single(Op::Run(q))
            }

            SystemEvent::ActorLinkedToSignal {
                actor_id,
                signal_id,
                role,
            } => {
                let q = query(
                    "MATCH (a:Actor {id: $actor_id})
                     MATCH (n) WHERE n.id = $signal_id AND (n:Gathering OR n:Resource OR n:HelpRequest OR n:Announcement OR n:Concern OR n:Condition)
                     MERGE (a)-[:ACTED_IN {role: $role}]->(n)"
                )
                .param("actor_id", actor_id.to_string())
                .param("signal_id", signal_id.to_string())
                .param("role", role.as_str());

                Plan::single(Op::Run(q))
            }

            SystemEvent::ActorLocationIdentified {
                actor_id,
                location_lat,
                location_lng,
                location_name,
            } => {
                let q = query(
                    "MATCH (a:Actor {id: $id})
                     SET a.location_lat = $lat,
                         a.location_lng = $lng,
                         a.location_name = $name",
                )
                .param("id", actor_id.to_string())
                .param("lat", location_lat)
                .param("lng", location_lng)
                .param("name", location_name.unwrap_or_default());

                Plan::single(Op::Run(q))
            }

            SystemEvent::ActorProfileEnriched {
                actor_id,
                bio,
                external_url,
            } => {
                let q = query(
                    "MATCH (a:Actor {id: $id})
                     SET a.bio = $bio,
                         a.external_url = $url",
                )
                .param("id", actor_id.to_string())
                .param("bio", bio.unwrap_or_default())
                .param("url", external_url.unwrap_or_default());

                Plan::single(Op::Run(q))
            }

            // ---------------------------------------------------------
            // Actor decisions
            // ---------------------------------------------------------
            SystemEvent::DuplicateActorsMerged {
                kept_id,
                merged_ids,
            } => {
                let ids: Vec<String> = merged_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS merged_id
                     MATCH (old:Actor {id: merged_id})
                     MATCH (kept:Actor {id: $kept_id})
                     // Move ACTED_IN edges
                     OPTIONAL MATCH (old)-[r:ACTED_IN]->(signal)
                     FOREACH (_ IN CASE WHEN r IS NOT NULL THEN [1] ELSE [] END |
                         MERGE (kept)-[:ACTED_IN {role: r.role}]->(signal)
                     )
                     // Move HAS_SOURCE edges
                     OPTIONAL MATCH (old)-[s:HAS_SOURCE]->(source)
                     FOREACH (_ IN CASE WHEN s IS NOT NULL THEN [1] ELSE [] END |
                         MERGE (kept)-[:HAS_SOURCE]->(source)
                     )
                     DETACH DELETE old",
                )
                .param("kept_id", kept_id.to_string())
                .param("ids", ids);

                Plan::single(Op::Run(q))
            }

            SystemEvent::OrphanedActorsCleaned { actor_ids } => {
                let ids: Vec<String> = actor_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS id
                     MATCH (a:Actor {id: id})
                     DETACH DELETE a",
                )
                .param("ids", ids);

                Plan::single(Op::Run(q))
            }

            // ---------------------------------------------------------
            // Relationship linking — system judgments
            // ---------------------------------------------------------
            SystemEvent::ResponseLinked {
                signal_id,
                concern_id,
                strength,
                explanation,
                ..
            } => {
                let q = query(
                    "MATCH (resp) WHERE resp.id = $resp_id AND (resp:Resource OR resp:Gathering OR resp:HelpRequest)
                     MATCH (t:Concern {id: $tid})
                     MERGE (resp)-[r:RESPONDS_TO]->(t)
                     ON CREATE SET r.match_strength = $strength, r.explanation = $explanation
                     ON MATCH SET r.match_strength = $strength, r.explanation = $explanation"
                )
                .param("resp_id", signal_id.to_string())
                .param("tid", concern_id.to_string())
                .param("strength", strength)
                .param("explanation", explanation.as_str());

                Plan::single(Op::Run(q))
            }

            SystemEvent::ConcernLinked {
                signal_id,
                concern_id,
                strength,
                explanation,
                ..
            } => {
                let q = query(
                    "MATCH (resp) WHERE resp.id = $resp_id AND (resp:Resource OR resp:Gathering OR resp:HelpRequest)
                     MATCH (t:Concern {id: $tid})
                     MERGE (resp)-[r:DRAWN_TO]->(t)
                     ON CREATE SET r.match_strength = $strength, r.explanation = $explanation
                     ON MATCH SET r.match_strength = $strength, r.explanation = $explanation"
                )
                .param("resp_id", signal_id.to_string())
                .param("tid", concern_id.to_string())
                .param("strength", strength)
                .param("explanation", explanation.as_str());

                Plan::single(Op::Run(q))
            }

            // ---------------------------------------------------------
            // Situations / dispatches
            // ---------------------------------------------------------
            SystemEvent::SituationIdentified {
                situation_id,
                headline,
                lede,
                arc,
                temperature,
                centroid_lat,
                centroid_lng,
                location_name,
                sensitivity,
                category,
                structured_state,
                tension_heat,
                clarity,
                signal_count,
                narrative_embedding,
                causal_embedding,
            } => {
                let q = query(
                    "MERGE (s:Situation {id: $id})
                     ON CREATE SET
                         s.headline = $headline,
                         s.lede = $lede,
                         s.arc = $arc,
                         s.temperature = $temperature,
                         s.centroid_lat = $centroid_lat,
                         s.centroid_lng = $centroid_lng,
                         s.location_name = $location_name,
                         s.sensitivity = $sensitivity,
                         s.category = $category,
                         s.structured_state = $structured_state,
                         s.first_seen = datetime($ts),
                         s.last_updated = datetime($ts),
                         s.review_status = 'staged'",
                )
                .param("id", situation_id.to_string())
                .param("headline", headline.as_str())
                .param("lede", lede.as_str())
                .param("arc", arc.to_string())
                .param("temperature", temperature)
                .param::<Option<f64>>("centroid_lat", centroid_lat)
                .param::<Option<f64>>("centroid_lng", centroid_lng)
                .param("location_name", location_name.unwrap_or_default())
                .param("sensitivity", sensitivity.as_str())
                .param("category", category.unwrap_or_default())
                .param("structured_state", structured_state.as_str())
                .param("ts", format_dt_from_event(event));

                let mut ops = vec![Op::Run(q)];

                let id_str = situation_id.to_string();
                if let Some(th) = tension_heat {
                    let q = query("MATCH (s:Situation {id: $id}) SET s.tension_heat = $v")
                        .param("id", id_str.clone()).param("v", th);
                    ops.push(Op::Run(q));
                }
                if let Some(ref cl) = clarity {
                    let q = query("MATCH (s:Situation {id: $id}) SET s.clarity = $v")
                        .param("id", id_str.clone()).param("v", cl.as_str());
                    ops.push(Op::Run(q));
                }
                if let Some(sc) = signal_count {
                    let q = query("MATCH (s:Situation {id: $id}) SET s.signal_count = $v")
                        .param("id", id_str.clone()).param("v", sc as i64);
                    ops.push(Op::Run(q));
                }
                if let Some(ref ne) = narrative_embedding {
                    let vals: Vec<f64> = ne.iter().map(|v| *v as f64).collect();
                    let q = query("MATCH (s:Situation {id: $id}) SET s.narrative_embedding = $v")
                        .param("id", id_str.clone()).param("v", vals);
                    ops.push(Op::Run(q));
                }
                if let Some(ref ce) = causal_embedding {
                    let vals: Vec<f64> = ce.iter().map(|v| *v as f64).collect();
                    let q = query("MATCH (s:Situation {id: $id}) SET s.causal_embedding = $v")
                        .param("id", id_str).param("v", vals);
                    ops.push(Op::Run(q));
                }

                Plan::applied(ops)
            }

            SystemEvent::SituationChanged {
                situation_id,
                change,
            } => {
                let id_str = situation_id.to_string();
                let ts = format_dt_from_event(event);
                let q = match change {
                    SituationChange::Headline { new, .. } => {
                        query("MATCH (s:Situation {id: $id}) SET s.headline = $value, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("value", new.as_str()).param("ts", ts)
                    }
                    SituationChange::Lede { new, .. } => {
                        query("MATCH (s:Situation {id: $id}) SET s.lede = $value, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("value", new.as_str()).param("ts", ts)
                    }
                    SituationChange::Arc { new, .. } => {
                        query("MATCH (s:Situation {id: $id}) SET s.arc = $value, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("value", new.to_string()).param("ts", ts)
                    }
                    SituationChange::Temperature { new, .. } => {
                        query("MATCH (s:Situation {id: $id}) SET s.temperature = $value, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("value", new).param("ts", ts)
                    }
                    SituationChange::Location { new, .. } => {
                        let (lat, lng) = location_lat_lng(&new);
                        let name = location_name_str(&new);
                        query("MATCH (s:Situation {id: $id}) SET s.centroid_lat = $lat, s.centroid_lng = $lng, s.location_name = $name, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("lat", lat).param("lng", lng).param("name", name).param("ts", ts)
                    }
                    SituationChange::Sensitivity { new, .. } => {
                        query("MATCH (s:Situation {id: $id}) SET s.sensitivity = $value, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("value", new.as_str()).param("ts", ts)
                    }
                    SituationChange::Category { new, .. } => {
                        query("MATCH (s:Situation {id: $id}) SET s.category = $value, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("value", new.as_deref().unwrap_or("")).param("ts", ts)
                    }
                    SituationChange::StructuredState { new, .. } => {
                        query("MATCH (s:Situation {id: $id}) SET s.structured_state = $value, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("value", new.as_str()).param("ts", ts)
                    }
                };
                Plan::single(Op::Run(q))
            }

            SystemEvent::SituationPromoted { situation_ids } => {
                let ids: Vec<String> = situation_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS id
                     MATCH (s:Situation {id: id})
                     SET s.review_status = 'accepted'",
                )
                .param("ids", ids);

                Plan::single(Op::Run(q))
            }

            SystemEvent::DispatchCreated {
                dispatch_id,
                situation_id,
                body,
                signal_ids,
                dispatch_type,
                supersedes,
                fidelity_score,
                flagged_for_review,
                flag_reason,
            } => {
                let ts = format_dt_from_event(event);
                let q = query(
                    "MERGE (d:Dispatch {id: $id})
                     ON CREATE SET
                         d.situation_id = $situation_id,
                         d.body = $body,
                         d.dispatch_type = $dispatch_type,
                         d.created_at = datetime($ts),
                         d.flagged_for_review = $flagged,
                         d.flag_reason = $flag_reason,
                         d.fidelity_score = $fidelity
                     ON MATCH SET
                         d.body = $body,
                         d.dispatch_type = $dispatch_type,
                         d.flagged_for_review = $flagged,
                         d.flag_reason = $flag_reason,
                         d.fidelity_score = $fidelity
                     WITH d
                     MATCH (s:Situation {id: $situation_id})
                     MERGE (d)-[:BELONGS_TO]->(s)",
                )
                .param("id", dispatch_id.to_string())
                .param("situation_id", situation_id.to_string())
                .param("body", body.as_str())
                .param("dispatch_type", dispatch_type.to_string())
                .param("ts", ts)
                .param("flagged", flagged_for_review.unwrap_or(false))
                .param("flag_reason", flag_reason.unwrap_or_default())
                .param("fidelity", fidelity_score.unwrap_or(-1.0));

                let mut ops = vec![Op::Run(q)];

                if let Some(ref sup_id) = supersedes {
                    let q = query(
                        "MATCH (d:Dispatch {id: $id}), (old:Dispatch {id: $old_id})
                         MERGE (d)-[:SUPERSEDES]->(old)",
                    )
                    .param("id", dispatch_id.to_string())
                    .param("old_id", sup_id.to_string());
                    ops.push(Op::Run(q));
                }

                if !signal_ids.is_empty() {
                    let ids: Vec<String> = signal_ids.iter().map(|id| id.to_string()).collect();
                    let q = query(
                        "MATCH (d:Dispatch {id: $did})
                         UNWIND $sids AS sid
                         MATCH (sig) WHERE sig.id = sid
                           AND (sig:Gathering OR sig:Resource OR sig:HelpRequest OR sig:Announcement OR sig:Concern OR sig:Condition)
                         MERGE (d)-[:CITES]->(sig)",
                    )
                    .param("did", dispatch_id.to_string())
                    .param("sids", ids);
                    ops.push(Op::Run(q));
                }

                let q = query(
                    "MATCH (s:Situation {id: $sid})
                     OPTIONAL MATCH (d:Dispatch)-[:BELONGS_TO]->(s)
                     WITH s, count(d) AS dc
                     SET s.dispatch_count = dc",
                )
                .param("sid", situation_id.to_string());
                ops.push(Op::Run(q));

                Plan::applied(ops)
            }

            SystemEvent::SignalAssignedToSituation {
                signal_id,
                situation_id,
                signal_label,
                confidence,
                reasoning,
            } => {
                let q = query(
                    "MATCH (sig) WHERE sig.id = $signal_id
                       AND (sig:Gathering OR sig:Resource OR sig:HelpRequest OR sig:Announcement OR sig:Concern OR sig:Condition)
                     MATCH (s:Situation {id: $situation_id})
                     MERGE (sig)-[e:PART_OF]->(s)
                     ON CREATE SET e.confidence = $confidence, e.reasoning = $reasoning, e.label = $label
                     ON MATCH SET e.confidence = $confidence, e.reasoning = $reasoning
                     WITH s
                     OPTIONAL MATCH (any)-[:PART_OF]->(s)
                     WITH s, count(any) AS sc
                     OPTIONAL MATCH (t:Concern)-[:PART_OF]->(s)
                     WITH s, sc, count(t) AS tc
                     SET s.signal_count = sc, s.tension_count = tc",
                )
                .param("signal_id", signal_id.to_string())
                .param("situation_id", situation_id.to_string())
                .param("confidence", confidence)
                .param("reasoning", reasoning.as_str())
                .param("label", signal_label.as_str());

                Plan::single(Op::Run(q))
            }

            SystemEvent::SituationTagsAggregated {
                situation_id,
                tag_slugs,
            } => {
                let mut ops = Vec::new();
                for slug in &tag_slugs {
                    let name = slug.replace('-', " ");
                    let q = query(
                        "MATCH (s:Situation {id: $sid})
                         MERGE (t:Tag {slug: $slug})
                         ON CREATE SET t.name = $name
                         MERGE (s)-[:TAGGED]->(t)",
                    )
                    .param("sid", situation_id.to_string())
                    .param("slug", slug.as_str())
                    .param("name", name.as_str());
                    ops.push(Op::Run(q));
                }
                Plan::applied(ops)
            }

            SystemEvent::DispatchFlaggedForReview {
                dispatch_id,
                reason,
            } => {
                let q = query(
                    "MATCH (d:Dispatch {id: $id})
                     SET d.flagged_for_review = true,
                         d.flag_reason = $reason",
                )
                .param("id", dispatch_id.to_string())
                .param("reason", reason.as_str());

                Plan::single(Op::Run(q))
            }

            SystemEvent::SignalsPendingWeaving {
                signal_ids,
                scout_run_id: _,
            } => {
                let ids: Vec<String> = signal_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS sid
                     MATCH (n) WHERE n.id = sid
                       AND (n:Gathering OR n:Resource OR n:HelpRequest OR n:Announcement OR n:Concern OR n:Condition)
                     SET n.situation_pending = true",
                )
                .param("ids", ids);

                Plan::single(Op::Run(q))
            }

            // ---------------------------------------------------------
            // Tags
            // ---------------------------------------------------------
            SystemEvent::SignalTagged {
                signal_id,
                tag_slugs,
            } => {
                let mut ops = Vec::new();
                for slug in &tag_slugs {
                    let name = slug.replace('-', " ");
                    let q = query(
                        "MATCH (s)
                         WHERE s.id = $signal_id
                           AND (s:Gathering OR s:Resource OR s:HelpRequest OR s:Announcement OR s:Concern OR s:Condition)
                         MERGE (t:Tag {slug: $slug})
                         ON CREATE SET t.name = $name
                         MERGE (s)-[r:TAGGED]->(t)
                         SET r.weight = 1.0",
                    )
                    .param("signal_id", signal_id.to_string())
                    .param("slug", slug.as_str())
                    .param("name", name.as_str());

                    ops.push(Op::Run(q));
                }
                Plan::applied(ops)
            }

            SystemEvent::TagSuppressed {
                situation_id,
                tag_slug,
            } => {
                let q = query(
                    "MATCH (s:Situation {id: $situation_id})-[r:TAGGED]->(t:Tag {slug: $slug})
                     DELETE r
                     MERGE (s)-[sup:SUPPRESSED_TAG]->(t)
                       ON CREATE SET sup.suppressed_at = datetime()",
                )
                .param("situation_id", situation_id.to_string())
                .param("slug", tag_slug.as_str());

                Plan::single(Op::Run(q))
            }

            SystemEvent::TagsMerged {
                source_slug,
                target_slug,
            } => {
                let q1 = query(
                    "MATCH (src:Tag {slug: $source}), (tgt:Tag {slug: $target})
                     WITH src, tgt
                     OPTIONAL MATCH (n)-[old:TAGGED]->(src)
                     WITH src, tgt, n, old
                     WHERE old IS NOT NULL
                     MERGE (n)-[:TAGGED]->(tgt)
                     DELETE old",
                )
                .param("source", source_slug.as_str())
                .param("target", target_slug.as_str());

                let q2 = query(
                    "MATCH (src:Tag {slug: $source}), (tgt:Tag {slug: $target})
                     WITH src, tgt
                     OPTIONAL MATCH (s)-[old:SUPPRESSED_TAG]->(src)
                     WITH src, tgt, s, old
                     WHERE old IS NOT NULL
                     MERGE (s)-[:SUPPRESSED_TAG]->(tgt)
                     DELETE old",
                )
                .param("source", source_slug.as_str())
                .param("target", target_slug.as_str());

                let q3 = query(
                    "MATCH (t:Tag {slug: $source}) DETACH DELETE t",
                )
                .param("source", source_slug.as_str());

                Plan::single(Op::RunAll(vec![q1, q2, q3]))
            }

            // ---------------------------------------------------------
            // Quality / lint
            // ---------------------------------------------------------
            SystemEvent::EmptyEntitiesCleaned { signal_ids } => {
                let ids: Vec<String> = signal_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS id
                     OPTIONAL MATCH (g:Gathering {id: id})
                     OPTIONAL MATCH (a:Resource {id: id})
                     OPTIONAL MATCH (n:HelpRequest {id: id})
                     OPTIONAL MATCH (nc:Announcement {id: id})
                     OPTIONAL MATCH (t:Concern {id: id})
                     WITH coalesce(g, a, n, nc, t) AS node
                     WHERE node IS NOT NULL
                     OPTIONAL MATCH (node)-[:SOURCED_FROM]->(ev:Citation)
                     DETACH DELETE node, ev",
                )
                .param("ids", ids);

                Plan::single(Op::Run(q))
            }

            SystemEvent::FakeCoordinatesNulled { signal_ids, .. } => {
                let ids: Vec<String> = signal_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS id
                     OPTIONAL MATCH (g:Gathering {id: id})
                     OPTIONAL MATCH (a:Resource {id: id})
                     OPTIONAL MATCH (n:HelpRequest {id: id})
                     OPTIONAL MATCH (nc:Announcement {id: id})
                     OPTIONAL MATCH (t:Concern {id: id})
                     WITH coalesce(g, a, n, nc, t) AS node
                     WHERE node IS NOT NULL
                     SET node.locations_json = ''
                     WITH node
                     OPTIONAL MATCH (node)-[r:HELD_AT|AVAILABLE_AT|NEEDED_AT|RELEVANT_TO|AFFECTS|OBSERVED_AT|REFERENCES_LOCATION]->(l:Location)
                     DELETE r",
                )
                .param("ids", ids);

                Plan::single(Op::Run(q))
            }

            SystemEvent::OrphanedCitationsCleaned { citation_ids } => {
                let ids: Vec<String> = citation_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS id
                     MATCH (ev:Citation {id: id})
                     DETACH DELETE ev",
                )
                .param("ids", ids);

                Plan::single(Op::Run(q))
            }

            // ---------------------------------------------------------
            // Source system changes (editorial)
            // ---------------------------------------------------------
            SystemEvent::SourceSystemChanged {
                canonical_key,
                change,
                ..
            } => {
                let key = canonical_key.as_str();
                let q = match change {
                    SystemSourceChange::QualityPenalty { new, .. } => {
                        query(
                            "MATCH (s:Source {canonical_key: $key}) SET s.quality_penalty = $value",
                        )
                        .param("key", key)
                        .param("value", new)
                    }
                    SystemSourceChange::GapContext { new, .. } => {
                        query(
                            "MATCH (s:Source {canonical_key: $key}) SET s.gap_context = $value",
                        )
                        .param("key", key)
                        .param("value", new.as_deref().unwrap_or(""))
                    }
                };
                Plan::single(Op::Run(q))
            }

            // ---------------------------------------------------------
            // Source registry
            // ---------------------------------------------------------
            SystemEvent::SourcesRegistered { sources } => {
                let ts = format_dt_from_event(event);
                let mut ops = Vec::new();
                for source in sources {
                    let q = query(
                        "MERGE (s:Source {canonical_key: $canonical_key})
                         ON CREATE SET
                             s.id = $id,
                             s.canonical_value = $canonical_value,
                             s.url = $url,
                             s.discovery_method = $discovery_method,
                             s.created_at = datetime($ts),
                             s.signals_produced = 0,
                             s.signals_corroborated = 0,
                             s.consecutive_empty_runs = 0,
                             s.active = true,
                             s.gap_context = $gap_context,
                             s.weight = $weight,
                             s.avg_signals_per_scrape = 0.0,
                             s.quality_penalty = 1.0,
                             s.source_role = $source_role,
                             s.scrape_count = 0,
                             s.sources_discovered = 0,
                             s.cw_page = $cw_page,
                             s.cw_feed = $cw_feed,
                             s.cw_media = $cw_media,
                             s.cw_discussion = $cw_discussion,
                             s.cw_events = $cw_events
                         ON MATCH SET
                             s.active = CASE WHEN s.active = false AND $discovery_method = 'curated' THEN true ELSE s.active END,
                             s.url = CASE WHEN $url <> '' THEN $url ELSE s.url END"
                    )
                    .param("id", source.id.to_string())
                    .param("canonical_key", source.canonical_key.as_str())
                    .param("canonical_value", source.canonical_value.as_str())
                    .param("url", source.url.as_deref().unwrap_or(""))
                    .param("discovery_method", source.discovery_method.to_string())
                    .param("ts", ts.as_str())
                    .param("weight", source.weight)
                    .param("source_role", source.source_role.to_string())
                    .param("gap_context", source.gap_context.clone().unwrap_or_default())
                    .param("cw_page", source.channel_weights.page)
                    .param("cw_feed", source.channel_weights.feed)
                    .param("cw_media", source.channel_weights.media)
                    .param("cw_discussion", source.channel_weights.discussion)
                    .param("cw_events", source.channel_weights.events);

                    ops.push(Op::Run(q));

                    if let Some(parent_key) = &source.discovered_from_key {
                        let link_q = query(
                            "MATCH (child:Source {canonical_key: $child_key})
                             MATCH (parent:Source {canonical_key: $parent_key})
                             MERGE (child)-[:LINKED_FROM]->(parent)",
                        )
                        .param("child_key", source.canonical_key.as_str())
                        .param("parent_key", parent_key.as_str());
                        ops.push(Op::Run(link_q));
                    }
                }
                Plan::applied(ops)
            }

            SystemEvent::SourceChanged {
                canonical_key,
                change,
                ..
            } => {
                let key = canonical_key.as_str();
                let q = match change {
                    SourceChange::Weight { new, .. } => {
                        query("MATCH (s:Source {canonical_key: $key}) SET s.weight = $value")
                            .param("key", key)
                            .param("value", new)
                    }
                    SourceChange::Url { new, .. } => {
                        query("MATCH (s:Source {canonical_key: $key}) SET s.url = $value")
                            .param("key", key)
                            .param("value", new.as_str())
                    }
                    SourceChange::Role { new, .. } => {
                        query(
                            "MATCH (s:Source {canonical_key: $key}) SET s.source_role = $value",
                        )
                        .param("key", key)
                        .param("value", new.to_string())
                    }
                    SourceChange::Active { new, .. } => {
                        query("MATCH (s:Source {canonical_key: $key}) SET s.active = $value")
                            .param("key", key)
                            .param("value", new)
                    }
                    SourceChange::Cadence { new, .. } => {
                        if let Some(hours) = new {
                            query("MATCH (s:Source {canonical_key: $key}) SET s.cadence_hours = $value")
                                .param("key", key)
                                .param("value", hours as i64)
                        } else {
                            return Plan::skip();
                        }
                    }
                    SourceChange::ChannelWeight { channel, new, .. } => {
                        let prop = format!("cw_{channel}");
                        let cypher = format!(
                            "MATCH (s:Source {{canonical_key: $key}}) SET s.{prop} = $value"
                        );
                        query(&cypher)
                            .param("key", key)
                            .param("value", new)
                    }
                };
                Plan::single(Op::Run(q))
            }

            SystemEvent::SourceDeactivated { source_ids, .. } => {
                let ids: Vec<String> = source_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS id
                     MATCH (s:Source {id: id})
                     SET s.active = false",
                )
                .param("ids", ids);

                Plan::single(Op::Run(q))
            }

            SystemEvent::SourceSignalsCleared { canonical_key, .. } => {
                let q = query(
                    "MATCH (s:Source {canonical_key: $key})<-[:PRODUCED_BY]-(n)
                     OPTIONAL MATCH (n)-[:SOURCED_FROM]->(c:Citation)
                     DETACH DELETE n, c",
                )
                .param("key", canonical_key.as_str());

                Plan::single(Op::Run(q))
            }

            SystemEvent::SourceDeleted { canonical_key, .. } => {
                let q = query(
                    "MATCH (s:Source {canonical_key: $key})
                     DETACH DELETE s",
                )
                .param("key", canonical_key.as_str());

                Plan::single(Op::Run(q))
            }

            // ---------------------------------------------------------
            // App user actions
            // ---------------------------------------------------------
            SystemEvent::PinCreated {
                pin_id,
                location_lat,
                location_lng,
                source_id,
                created_by,
            } => {
                let q = query(
                    "MERGE (p:Pin {id: $id})
                     ON CREATE SET
                         p.location_lat = $lat,
                         p.location_lng = $lng,
                         p.source_id = $source_id,
                         p.created_by = $created_by,
                         p.created_at = datetime($ts)",
                )
                .param("id", pin_id.to_string())
                .param("lat", location_lat)
                .param("lng", location_lng)
                .param("source_id", source_id.to_string())
                .param("created_by", created_by.as_str())
                .param("ts", format_dt_from_event(event));

                Plan::single(Op::Run(q))
            }

            SystemEvent::PinsConsumed { pin_ids } => {
                if pin_ids.is_empty() {
                    return Plan::skip();
                }
                let ids: Vec<String> = pin_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS pid
                     MATCH (p:Pin {id: pid})
                     DETACH DELETE p",
                )
                .param("ids", ids);
                Plan::single(Op::Run(q))
            }

            SystemEvent::DemandReceived {
                demand_id,
                query: demand_query,
                center_lat,
                center_lng,
                radius_km,
            } => {
                let q = query(
                    "MERGE (d:DemandSignal {id: $id})
                     SET d.query = $query,
                         d.center_lat = $lat,
                         d.center_lng = $lng,
                         d.radius_km = $radius,
                         d.created_at = datetime($ts)",
                )
                .param("id", demand_id.to_string())
                .param("query", demand_query.as_str())
                .param("lat", center_lat)
                .param("lng", center_lng)
                .param("radius", radius_km)
                .param("ts", format_dt_from_event(event));

                Plan::single(Op::Run(q))
            }

            SystemEvent::SubmissionReceived {
                submission_id,
                url,
                reason,
                source_canonical_key,
            } => {
                let q = query(
                    "MERGE (sub:Submission {id: $id})
                     ON CREATE SET
                         sub.url = $url,
                         sub.reason = $reason,
                         sub.submitted_at = datetime($ts)
                     WITH sub
                     OPTIONAL MATCH (s:Source {canonical_key: $canonical_key})
                     FOREACH (_ IN CASE WHEN s IS NOT NULL THEN [1] ELSE [] END |
                         MERGE (sub)-[:SUBMITTED_FOR]->(s)
                     )",
                )
                .param("id", submission_id.to_string())
                .param("url", url.as_str())
                .param("reason", reason.unwrap_or_default())
                .param("ts", format_dt_from_event(event))
                .param("canonical_key", source_canonical_key.unwrap_or_default());

                Plan::single(Op::Run(q))
            }

            // ---------------------------------------------------------
            // Response scouting
            // ---------------------------------------------------------
            SystemEvent::ResponseScouted {
                concern_id,
                scouted_at,
            } => {
                let ts = format_dt(&scouted_at);
                let q = query(
                    "MATCH (t:Concern {id: $id})
                     SET t.response_scouted_at = datetime($ts)",
                )
                .param("id", concern_id.to_string())
                .param("ts", ts.as_str());
                Plan::single(Op::Run(q))
            }

            // ---------------------------------------------------------
            // Query embedding storage
            // ---------------------------------------------------------
            SystemEvent::QueryEmbeddingStored {
                canonical_key,
                embedding,
            } => {
                let q = query(
                    "MATCH (s:Source {canonical_key: $key})
                     SET s.query_embedding = $embedding",
                )
                .param("key", canonical_key.as_str())
                .param("embedding", embedding);
                Plan::single(Op::Run(q))
            }

            // ---------------------------------------------------------
            // Situation curiosity
            // ---------------------------------------------------------
            SystemEvent::CuriosityTriggered {
                situation_id,
                signal_ids,
            } => {
                let sig_id_strings: Vec<String> =
                    signal_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "MATCH (s:Situation {id: $sit_id})
                     SET s.curiosity_triggered_at = datetime($ts)
                     WITH s
                     UNWIND $sig_ids AS sid
                     MATCH (sig {id: sid})-[:PART_OF]->(s)
                     SET sig.curiosity_investigated = NULL",
                )
                .param("sit_id", situation_id.to_string())
                .param("sig_ids", sig_id_strings)
                .param("ts", format_dt_from_event(event));
                Plan::single(Op::Run(q))
            }

            // ---------------------------------------------------------
            // Investigation & curiosity bookkeeping
            // ---------------------------------------------------------
            SystemEvent::SignalInvestigated {
                signal_id,
                node_type,
                investigated_at,
            } => {
                let label = match node_type {
                    NodeType::Gathering => "Gathering",
                    NodeType::Resource => "Resource",
                    NodeType::HelpRequest => "HelpRequest",
                    NodeType::Announcement => "Announcement",
                    NodeType::Concern => "Concern",
                    NodeType::Condition => "Condition",
                    NodeType::Citation => return Plan::skip(),
                };
                let q = query(&format!(
                    "MATCH (n:{label} {{id: $id}})
                     SET n.investigated_at = datetime($ts)"
                ))
                .param("id", signal_id.to_string())
                .param("ts", format_dt(&investigated_at));
                Plan::single(Op::Run(q))
            }

            SystemEvent::ExhaustedRetriesPromoted { .. } => {
                let q = query(
                    "MATCH (n)
                     WHERE (n:Resource OR n:Gathering OR n:HelpRequest OR n:Announcement)
                       AND n.curiosity_investigated = 'failed'
                       AND n.curiosity_retry_count >= 3
                     SET n.curiosity_investigated = 'abandoned'",
                );
                Plan::single(Op::Run(q))
            }

            SystemEvent::ConcernLinkerOutcomeRecorded {
                signal_id,
                label,
                outcome,
                increment_retry,
            } => {
                let label = match label.as_str() {
                    "Gathering" | "Resource" | "HelpRequest" | "Announcement" => label.as_str(),
                    _ => return Plan::skip(),
                };
                let cypher = if increment_retry {
                    format!(
                        "MATCH (n:{label} {{id: $id}})
                         WHERE n.last_retry_ts IS NULL OR n.last_retry_ts < datetime($ts)
                         SET n.curiosity_investigated = $outcome,
                             n.curiosity_retry_count = coalesce(n.curiosity_retry_count, 0) + 1,
                             n.last_retry_ts = datetime($ts)"
                    )
                } else {
                    format!(
                        "MATCH (n:{label} {{id: $id}})
                         SET n.curiosity_investigated = $outcome"
                    )
                };
                let q = query(&cypher)
                    .param("id", signal_id.to_string())
                    .param("outcome", outcome.as_str())
                    .param("ts", format_dt_from_event(event));
                Plan::single(Op::Run(q))
            }

            SystemEvent::GatheringScouted {
                concern_id,
                found_gatherings,
                scouted_at,
            } => {
                let q = query(
                    "MATCH (t:Concern {id: $id})
                     WHERE t.gravity_scouted_at IS NULL OR t.gravity_scouted_at < datetime($ts)
                     SET t.gravity_scouted_at = datetime($ts),
                         t.gravity_scout_miss_count = CASE
                             WHEN $found THEN 0
                             ELSE coalesce(t.gravity_scout_miss_count, 0) + 1
                         END",
                )
                .param("id", concern_id.to_string())
                .param("ts", format_dt(&scouted_at))
                .param("found", found_gatherings);
                Plan::single(Op::Run(q))
            }

            // ---------------------------------------------------------
            // Place & gathering geography
            // ---------------------------------------------------------
            SystemEvent::PlaceDiscovered {
                place_id,
                name,
                slug,
                lat,
                lng,
                discovered_at,
            } => {
                let q = query(
                    "MERGE (p:Place {slug: $slug})
                     ON CREATE SET
                         p.id = $id,
                         p.name = $name,
                         p.lat = $lat,
                         p.lng = $lng,
                         p.geocoded = false,
                         p.created_at = datetime($ts)",
                )
                .param("slug", slug.as_str())
                .param("id", place_id.to_string())
                .param("name", name.as_str())
                .param("lat", lat)
                .param("lng", lng)
                .param("ts", format_dt(&discovered_at));
                Plan::single(Op::Run(q))
            }

            SystemEvent::GathersAtPlaceLinked {
                signal_id,
                place_slug,
            } => {
                let q = query(
                    "MATCH (s) WHERE s.id = $sid AND (s:Resource OR s:Gathering OR s:HelpRequest)
                     MATCH (p:Place {slug: $slug})
                     MERGE (s)-[:GATHERS_AT]->(p)",
                )
                .param("sid", signal_id.to_string())
                .param("slug", place_slug.as_str());
                Plan::single(Op::Run(q))
            }

            // ---------------------------------------------------------
            // Tension deduplication
            // ---------------------------------------------------------
            SystemEvent::DuplicateConcernMerged {
                survivor_id,
                duplicate_id,
            } => {
                let sid = survivor_id.to_string();
                let did = duplicate_id.to_string();

                let q1 = query(
                    "MATCH (sig)-[r:RESPONDS_TO]->(dup:Concern {id: $dup_id})
                     MATCH (survivor:Concern {id: $survivor_id})
                     WITH sig, r, survivor, dup
                     WHERE NOT (sig)-[:RESPONDS_TO]->(survivor)
                     CREATE (sig)-[:RESPONDS_TO {match_strength: r.match_strength, explanation: r.explanation}]->(survivor)
                     WITH r, dup
                     DELETE r"
                )
                .param("dup_id", did.as_str())
                .param("survivor_id", sid.as_str());

                let q2 = query(
                    "MATCH (sig)-[r:DRAWN_TO]->(dup:Concern {id: $dup_id})
                     MATCH (survivor:Concern {id: $survivor_id})
                     WITH sig, r, survivor, dup
                     WHERE NOT (sig)-[:DRAWN_TO]->(survivor)
                     CREATE (sig)-[:DRAWN_TO {match_strength: r.match_strength, explanation: r.explanation, gathering_type: r.gathering_type}]->(survivor)
                     WITH r, dup
                     DELETE r"
                )
                .param("dup_id", did.as_str())
                .param("survivor_id", sid.as_str());

                let q3 = query(
                    "MATCH (dup:Concern {id: $dup_id})-[r:PART_OF]->(s:Situation)
                     MATCH (survivor:Concern {id: $survivor_id})
                     WHERE NOT (survivor)-[:PART_OF]->(s)
                     CREATE (survivor)-[:PART_OF]->(s)
                     WITH r
                     DELETE r",
                )
                .param("dup_id", did.as_str())
                .param("survivor_id", sid.as_str());

                // Only increment if the duplicate still exists (first run).
                // On replay the dup was already deleted by q5, so MATCH finds nothing.
                let q4 = query(
                    "MATCH (dup:Concern {id: $dup_id})
                     MATCH (t:Concern {id: $survivor_id})
                     SET t.corroboration_count = coalesce(t.corroboration_count, 0) + 1",
                )
                .param("dup_id", did.as_str())
                .param("survivor_id", sid.as_str());

                let q5 = query("MATCH (t:Concern {id: $dup_id}) DETACH DELETE t")
                    .param("dup_id", did.as_str());

                Plan::single(Op::RunAll(vec![q1, q2, q3, q4, q5]))
            }

            // ---------------------------------------------------------
            // System curiosity
            // ---------------------------------------------------------
            SystemEvent::ExpansionQueryCollected { .. } => {
                Plan::skip()
            }

            // ---------------------------------------------------------
            // Source scrape recording
            // ---------------------------------------------------------
            SystemEvent::SourceScraped {
                canonical_key,
                signals_produced,
                scraped_at,
            } => {
                let now = format_dt(&scraped_at);
                // Guard: skip if this scrape timestamp was already applied (replay idempotency).
                // The WHERE clause ensures additive counters only fire once per distinct scrape.
                let q = if signals_produced > 0 {
                    query(
                        "MATCH (s:Source {canonical_key: $key})
                         WHERE s.last_scraped IS NULL OR s.last_scraped < datetime($now)
                         SET s.last_scraped = datetime($now),
                             s.last_produced_signal = datetime($now),
                             s.signals_produced = s.signals_produced + $count,
                             s.consecutive_empty_runs = 0,
                             s.scrape_count = coalesce(s.scrape_count, 0) + 1",
                    )
                    .param("key", canonical_key.as_str())
                    .param("now", now.as_str())
                    .param("count", signals_produced as i64)
                } else {
                    query(
                        "MATCH (s:Source {canonical_key: $key})
                         WHERE s.last_scraped IS NULL OR s.last_scraped < datetime($now)
                         SET s.last_scraped = datetime($now),
                             s.consecutive_empty_runs = s.consecutive_empty_runs + 1,
                             s.scrape_count = coalesce(s.scrape_count, 0) + 1",
                    )
                    .param("key", canonical_key.as_str())
                    .param("now", now.as_str())
                };
                Plan::single(Op::Run(q))
            }

            // ---------------------------------------------------------
            // Source discovery credit
            // ---------------------------------------------------------
            SystemEvent::SourceDiscoveryCredit {
                canonical_key,
                sources_discovered,
            } => {
                let q = query(
                    "MATCH (s:Source {canonical_key: $key})
                     WHERE s.last_discovery_credit_ts IS NULL OR s.last_discovery_credit_ts < datetime($ts)
                     SET s.sources_discovered = coalesce(s.sources_discovered, 0) + $count,
                         s.last_discovery_credit_ts = datetime($ts)",
                )
                .param("key", canonical_key.as_str())
                .param("count", sources_discovered as i64)
                .param("ts", format_dt_from_event(event));
                Plan::single(Op::Run(q))
            }

            // ---------------------------------------------------------
            // Source weight adjustments
            // ---------------------------------------------------------
            SystemEvent::SourcesBoostedForSituation {
                headline,
                factor,
            } => {
                let q = query(
                    "MATCH (sig)-[:PART_OF]->(s:Situation {headline: $headline})
                     WITH collect(DISTINCT sig.url) AS urls
                     UNWIND urls AS url
                     MATCH (src:Source {active: true})
                     WHERE src.url = url AND src.weight IS NOT NULL
                       AND (src.last_boost_ts IS NULL OR src.last_boost_ts < datetime($ts))
                     SET src.weight = CASE WHEN src.weight * $factor > 5.0 THEN 5.0 ELSE src.weight * $factor END,
                         src.last_boost_ts = datetime($ts)",
                )
                .param("headline", headline.as_str())
                .param("factor", factor)
                .param("ts", format_dt_from_event(event));
                Plan::single(Op::Run(q))
            }

            // ---------------------------------------------------------
            // Supervisor analytics
            // ---------------------------------------------------------
            SystemEvent::EchoScored {
                situation_id,
                echo_score,
            } => {
                let q = query(
                    "MATCH (s:Situation {id: $id}) SET s.echo_score = $score",
                )
                .param("id", situation_id.to_string())
                .param("score", echo_score);
                Plan::single(Op::Run(q))
            }

            SystemEvent::CauseHeatComputed { scores } => {
                let mut queries = Vec::new();
                for score in &scores {
                    let q = query(&format!(
                        "MATCH (n:{} {{id: $id}}) SET n.cause_heat = $heat",
                        score.label
                    ))
                    .param("id", score.signal_id.to_string())
                    .param("heat", score.cause_heat);
                    queries.push(q);
                }
                Plan::single(Op::RunAll(queries))
            }

            SystemEvent::SignalDiversityComputed { metrics } => {
                if metrics.is_empty() {
                    return Plan::skip();
                }
                let mut by_label: std::collections::HashMap<String, Vec<&SignalDiversityScore>> =
                    std::collections::HashMap::new();
                for m in &metrics {
                    by_label.entry(m.label.clone()).or_default().push(m);
                }
                let mut ops = Vec::new();
                for (label, rows) in &by_label {
                    let params: Vec<neo4rs::BoltType> = rows
                        .iter()
                        .map(|m| {
                            neo4rs::BoltType::Map(neo4rs::BoltMap::from_iter(vec![
                                (
                                    neo4rs::BoltString::from("id"),
                                    neo4rs::BoltType::String(neo4rs::BoltString::from(
                                        m.signal_id.to_string().as_str(),
                                    )),
                                ),
                                (
                                    neo4rs::BoltString::from("src_div"),
                                    neo4rs::BoltType::Integer(neo4rs::BoltInteger::new(
                                        m.source_diversity,
                                    )),
                                ),
                                (
                                    neo4rs::BoltString::from("ch_div"),
                                    neo4rs::BoltType::Integer(neo4rs::BoltInteger::new(
                                        m.channel_diversity,
                                    )),
                                ),
                                (
                                    neo4rs::BoltString::from("ext_ratio"),
                                    neo4rs::BoltType::Float(neo4rs::BoltFloat::new(
                                        m.external_ratio,
                                    )),
                                ),
                            ]))
                        })
                        .collect();

                    let q = query(&format!(
                        "UNWIND $rows AS row
                         MATCH (n:{label} {{id: row.id}})
                         SET n.source_diversity = row.src_div,
                             n.channel_diversity = row.ch_div,
                             n.external_ratio = row.ext_ratio"
                    ))
                    .param("rows", params);

                    ops.push(Op::Run(q));
                }
                Plan::applied(ops)
            }

            SystemEvent::ActorStatsComputed { stats } => {
                if stats.is_empty() {
                    return Plan::skip();
                }
                let params: Vec<neo4rs::BoltType> = stats
                    .iter()
                    .map(|s| {
                        neo4rs::BoltType::Map(neo4rs::BoltMap::from_iter(vec![
                            (
                                neo4rs::BoltString::from("id"),
                                neo4rs::BoltType::String(neo4rs::BoltString::from(
                                    s.actor_id.to_string().as_str(),
                                )),
                            ),
                            (
                                neo4rs::BoltString::from("cnt"),
                                neo4rs::BoltType::Integer(neo4rs::BoltInteger::new(
                                    s.signal_count as i64,
                                )),
                            ),
                        ]))
                    })
                    .collect();

                let q = query(
                    "UNWIND $rows AS row
                     MATCH (a:Actor {id: row.id})
                     SET a.signal_count = row.cnt",
                )
                .param("rows", params);

                Plan::single(Op::Run(q))
            }

            SystemEvent::SimilarityEdgesRebuilt { edges } => {
                let mut queries = vec![query(
                    "MATCH ()-[e:SIMILAR_TO]->() DELETE e",
                )];

                if !edges.is_empty() {
                    for batch in edges.chunks(500) {
                        let edge_data: Vec<neo4rs::BoltType> = batch
                            .iter()
                            .map(|e| {
                                neo4rs::BoltType::Map(neo4rs::BoltMap::from_iter(vec![
                                    (
                                        neo4rs::BoltString::from("from"),
                                        neo4rs::BoltType::String(neo4rs::BoltString::from(
                                            e.from_id.to_string().as_str(),
                                        )),
                                    ),
                                    (
                                        neo4rs::BoltString::from("to"),
                                        neo4rs::BoltType::String(neo4rs::BoltString::from(
                                            e.to_id.to_string().as_str(),
                                        )),
                                    ),
                                    (
                                        neo4rs::BoltString::from("weight"),
                                        neo4rs::BoltType::Float(neo4rs::BoltFloat::new(e.weight)),
                                    ),
                                ]))
                            })
                            .collect();

                        let q = query(
                            "UNWIND $edges AS edge
                             MATCH (a) WHERE a.id = edge.from AND (a:Gathering OR a:Resource OR a:HelpRequest OR a:Announcement OR a:Concern OR a:Condition)
                             MATCH (b) WHERE b.id = edge.to AND (b:Gathering OR b:Resource OR b:HelpRequest OR b:Announcement OR b:Concern OR b:Condition)
                             MERGE (a)-[r:SIMILAR_TO]->(b)
                             SET r.weight = edge.weight",
                        )
                        .param("edges", edge_data);
                        queries.push(q);
                    }
                }
                Plan::single(Op::RunAll(queries))
            }

            // ---------------------------------------------------------
            // Admin actions
            // ---------------------------------------------------------
            SystemEvent::ValidationIssueDismissed { issue_id } => {
                let q = query(
                    "MATCH (v:ValidationIssue {id: $id})
                     WHERE v.status = 'open'
                     SET v.status = 'dismissed',
                         v.resolved_at = datetime(),
                         v.resolution = 'dismissed by admin'",
                )
                .param("id", issue_id.as_str());
                Plan::single(Op::Run(q))
            }

            // ---------------------------------------------------------
            // Location geocoding — MERGE canonical Location by Mapbox address,
            // then merge any stub (created by WorldEvent projection) into it.
            // ---------------------------------------------------------
            SystemEvent::LocationGeocoded {
                signal_id,
                location_name,
                lat,
                lng,
                address,
                precision,
                timezone,
                city,
                state,
                country_code,
                country_name,
            } => {
                let canonical_address = address.as_deref().unwrap_or("").to_string();
                let normalized_name = location_name.trim().to_lowercase();
                let lat_bucket = (lat * 1000.0).round() / 1000.0;
                let lng_bucket = (lng * 1000.0).round() / 1000.0;

                // Step 1: MERGE canonical Location by address, SET geocoded properties
                let merge_q = query(
                    "MERGE (canonical:Location {canonical_address: $canonical_address})
                     ON CREATE SET canonical.lat = $lat, canonical.lng = $lng,
                                   canonical.lat_bucket = $lat_bucket, canonical.lng_bucket = $lng_bucket,
                                   canonical.name = $location_name,
                                   canonical.normalized_name = $normalized_name,
                                   canonical.precision = $precision,
                                   canonical.timezone = $timezone,
                                   canonical.city = $city,
                                   canonical.state = $state,
                                   canonical.country_code = $country_code,
                                   canonical.country_name = $country_name,
                                   canonical.geocoded = true
                     ON MATCH SET canonical.lat = $lat, canonical.lng = $lng,
                                  canonical.lat_bucket = $lat_bucket, canonical.lng_bucket = $lng_bucket,
                                  canonical.precision = $precision,
                                  canonical.timezone = $timezone,
                                  canonical.city = $city,
                                  canonical.state = $state,
                                  canonical.country_code = $country_code,
                                  canonical.country_name = $country_name,
                                  canonical.geocoded = true",
                )
                .param("canonical_address", canonical_address.as_str())
                .param("lat", lat)
                .param("lng", lng)
                .param("lat_bucket", lat_bucket)
                .param("lng_bucket", lng_bucket)
                .param("location_name", location_name.as_str())
                .param("normalized_name", normalized_name.as_str())
                .param("precision", precision.as_str())
                .param("timezone", timezone.as_deref().unwrap_or(""))
                .param("city", city.as_deref().unwrap_or(""))
                .param("state", state.as_deref().unwrap_or(""))
                .param("country_code", country_code.as_deref().unwrap_or(""))
                .param("country_name", country_name.as_deref().unwrap_or(""));

                // Step 2: Find un-geocoded stub by normalized_name and merge into canonical.
                // Only targets stubs (no canonical_address yet) to avoid merging
                // two already-geocoded canonical nodes that share a normalized_name.
                let merge_stub_q = query(
                    "MATCH (stub:Location {normalized_name: $normalized_name})
                     WHERE stub.canonical_address IS NULL
                     MATCH (canonical:Location {canonical_address: $canonical_address})
                     WHERE stub <> canonical
                     CALL apoc.refactor.mergeNodes([canonical, stub], {
                       properties: 'discard',
                       mergeRels: true
                     }) YIELD node
                     RETURN node",
                )
                .param("normalized_name", normalized_name.as_str())
                .param("canonical_address", canonical_address.as_str());

                // Step 3: If a signal was already connected to a wrong canonical
                // (e.g. two signals shared a stub, first geocode merged it into
                // canonical A, now this geocode resolves to canonical B), redirect
                // only the edge that was for this location name.
                let redirect_q = query(
                    "MATCH (s {id: $signal_id})-[old_r]->(wrong:Location)
                     WHERE wrong.canonical_address IS NOT NULL
                       AND wrong.canonical_address <> $canonical_address
                       AND wrong.normalized_name = $normalized_name
                     MATCH (canonical:Location {canonical_address: $canonical_address})
                     CALL apoc.refactor.to(old_r, canonical)
                     YIELD input, output
                     RETURN input, output",
                )
                .param("signal_id", signal_id.to_string())
                .param("canonical_address", canonical_address.as_str())
                .param("normalized_name", normalized_name.as_str());

                debug!(signal_id = %signal_id, location = location_name, "LocationGeocoded projected");
                Plan::applied(vec![Op::Run(merge_q), Op::Run(merge_stub_q), Op::Run(redirect_q)])
            }

            // ---------------------------------------------------------
            // Region auto-discovery — MERGE Region + CONTAINS nesting
            // ---------------------------------------------------------
            SystemEvent::RegionDiscovered {
                region_id,
                name,
                center_lat,
                center_lng,
                radius_km,
                city,
                state,
                country_code,
                scale,
                parent_region_id,
            } => {
                let merge_q = query(
                    "MERGE (r:Region {name: $name})
                     ON CREATE SET r.id = $id,
                                   r.center_lat = $lat,
                                   r.center_lng = $lng,
                                   r.radius_km = $radius_km,
                                   r.review_status = 'discovered',
                                   r.is_leaf = true,
                                   r.created_at = datetime(),
                                   r.city = $city,
                                   r.state = $state,
                                   r.country_code = $country_code,
                                   r.scale = $scale,
                                   r.geo_terms = [$name]",
                )
                .param("name", name.as_str())
                .param("id", region_id.to_string())
                .param("lat", center_lat)
                .param("lng", center_lng)
                .param("radius_km", radius_km)
                .param("city", city.as_deref().unwrap_or(""))
                .param("state", state.as_deref().unwrap_or(""))
                .param("country_code", country_code.as_deref().unwrap_or(""))
                .param("scale", scale.as_str());

                let mut ops = vec![Op::Run(merge_q)];

                // CONTAINS nesting: parent_region_id links to parent discovered in same batch
                if let Some(parent_id) = parent_region_id {
                    let nest_q = query(
                        "MATCH (p:Region {id: $parent_id}), (c:Region {name: $child_name})
                         MERGE (p)-[:CONTAINS]->(c)",
                    )
                    .param("parent_id", parent_id.to_string())
                    .param("child_name", name.as_str());
                    ops.push(Op::Run(nest_q));
                }

                debug!(name = name, scale = scale, "RegionDiscovered projected");
                Plan::applied(ops)
            }

            // ---------------------------------------------------------
            // Signal groups (coalescing)
            // ---------------------------------------------------------
            SystemEvent::GroupCreated {
                group_id,
                label,
                queries,
                seed_signal_id,
            } => {
                let ts = format_dt_from_event(event);
                let queries_json: Vec<String> = queries;
                let mut ops = vec![];

                let q = query(
                    "MERGE (g:SignalGroup {id: $id})
                     ON CREATE SET g.label = $label,
                                   g.queries = $queries,
                                   g.created_at = datetime($ts)
                     ON MATCH SET g.label = $label,
                                  g.queries = $queries",
                )
                .param("id", group_id.to_string())
                .param("label", label.as_str())
                .param("queries", queries_json)
                .param("ts", ts);
                ops.push(Op::Run(q));

                if let Some(seed_id) = seed_signal_id {
                    let seed_q = query(
                        "MATCH (sig) WHERE sig.id = $signal_id
                           AND (sig:Gathering OR sig:Resource OR sig:HelpRequest OR sig:Announcement OR sig:Concern OR sig:Condition)
                         MATCH (g:SignalGroup {id: $group_id})
                         MERGE (sig)-[r:MEMBER_OF]->(g)
                         ON CREATE SET r.confidence = 1.0",
                    )
                    .param("signal_id", seed_id.to_string())
                    .param("group_id", group_id.to_string());
                    ops.push(Op::Run(seed_q));
                }

                Plan::applied(ops)
            }

            SystemEvent::SignalAddedToGroup {
                signal_id,
                group_id,
                confidence,
            } => {
                let q = query(
                    "MATCH (sig) WHERE sig.id = $signal_id
                       AND (sig:Gathering OR sig:Resource OR sig:HelpRequest OR sig:Announcement OR sig:Concern OR sig:Condition)
                     MATCH (g:SignalGroup {id: $group_id})
                     MERGE (sig)-[r:MEMBER_OF]->(g)
                     ON CREATE SET r.confidence = $confidence
                     ON MATCH SET r.confidence = $confidence",
                )
                .param("signal_id", signal_id.to_string())
                .param("group_id", group_id.to_string())
                .param("confidence", confidence);

                Plan::single(Op::Run(q))
            }

            SystemEvent::GroupQueriesRefined {
                group_id,
                queries,
            } => {
                let ts = format_dt_from_event(event);
                let queries_json: Vec<String> = queries;
                let q = query(
                    "MATCH (g:SignalGroup {id: $id})
                     SET g.queries = $queries,
                         g.last_refined = datetime($ts)",
                )
                .param("id", group_id.to_string())
                .param("queries", queries_json)
                .param("ts", ts);

                Plan::single(Op::Run(q))
            }

        }
    }

    // -----------------------------------------------------------------------
    // Plan helpers — pure Op constructors, no I/O
    // -----------------------------------------------------------------------

    fn plan_set_str(&self, label: &str, id: uuid::Uuid, prop: &str, value: &str) -> Op {
        let q = query(&format!(
            "MATCH (n:{label} {{id: $id}}) SET n.{prop} = $value"
        ))
        .param("id", id.to_string())
        .param("value", value);
        Op::Run(q)
    }

    fn plan_set_f64(&self, label: &str, id: uuid::Uuid, prop: &str, value: f64) -> Op {
        let q = query(&format!(
            "MATCH (n:{label} {{id: $id}}) SET n.{prop} = $value"
        ))
        .param("id", id.to_string())
        .param("value", value);
        Op::Run(q)
    }

    fn plan_set_bool(&self, label: &str, id: uuid::Uuid, prop: &str, value: bool) -> Op {
        let q = query(&format!(
            "MATCH (n:{label} {{id: $id}}) SET n.{prop} = $value"
        ))
        .param("id", id.to_string())
        .param("value", value);
        Op::Run(q)
    }

    fn plan_set_schedule(&self, label: &str, id: uuid::Uuid, schedule: &Option<Schedule>) -> Op {
        let sp = extract_schedule(schedule);
        let q = query(&format!(
            "MATCH (n:{label} {{id: $id}}) SET
             n.starts_at = CASE WHEN $starts_at = '' THEN null ELSE datetime($starts_at) END,
             n.ends_at = CASE WHEN $ends_at = '' THEN null ELSE datetime($ends_at) END,
             n.rrule = $rrule, n.all_day = $all_day, n.timezone = $timezone,
             n.schedule_text = $schedule_text,
             n.rdates = [d IN $rdates | datetime(d)],
             n.exdates = [d IN $exdates | datetime(d)]"
        ))
        .param("id", id.to_string())
        .param("starts_at", sp.starts_at)
        .param("ends_at", sp.ends_at)
        .param("rrule", sp.rrule)
        .param("all_day", sp.all_day)
        .param("timezone", sp.timezone)
        .param("schedule_text", sp.schedule_text)
        .param("rdates", sp.rdates)
        .param("exdates", sp.exdates);
        Op::Run(q)
    }

    fn plan_update_location(&self, label: &str, id: Uuid, loc: &Option<Location>) -> Vec<Op> {
        // Delete ALL location edge types, not just the default — the original
        // edge may have used a role override (e.g. affected_area → AFFECTS).
        let delete_q = query(&format!(
            "MATCH (n:{label} {{id: $id}})-[r:HELD_AT|AVAILABLE_AT|NEEDED_AT|RELEVANT_TO|AFFECTS|OBSERVED_AT|REFERENCES_LOCATION]->(l:Location)
             DELETE r"
        ))
        .param("id", id.to_string());

        let locations_json = match loc {
            Some(l) => serde_json::to_string(&[l]).unwrap_or_default(),
            None => String::new(),
        };
        let json_q = query(&format!(
            "MATCH (n:{label} {{id: $id}}) SET n.locations_json = $locations_json"
        ))
        .param("id", id.to_string())
        .param("locations_json", locations_json);

        let mut ops = vec![Op::Run(delete_q), Op::Run(json_q)];
        if let Some(loc) = loc {
            ops.extend(self.plan_locations(&id, label, std::slice::from_ref(loc)));
        }
        ops
    }

    fn plan_entities(&self, signal_id: &Uuid, signal_label: &str, entities: &[Entity]) -> Vec<Op> {
        if entities.is_empty() {
            return vec![];
        }

        let signal_id_str = signal_id.to_string();

        let entity_params: Vec<neo4rs::BoltType> = entities
            .iter()
            .map(|e| {
                neo4rs::BoltType::Map(neo4rs::BoltMap::from_iter(vec![
                    (
                        neo4rs::BoltString::from("name"),
                        neo4rs::BoltType::String(neo4rs::BoltString::from(e.name.as_str())),
                    ),
                    (
                        neo4rs::BoltString::from("entity_type"),
                        neo4rs::BoltType::String(neo4rs::BoltString::from(
                            e.entity_type.to_string().as_str(),
                        )),
                    ),
                    (
                        neo4rs::BoltString::from("role"),
                        neo4rs::BoltType::String(neo4rs::BoltString::from(
                            e.role.as_deref().unwrap_or(""),
                        )),
                    ),
                ]))
            })
            .collect();

        let q = query(&format!(
            "MATCH (s:{signal_label} {{id: $signal_id}})
             UNWIND $entities AS ent
             MERGE (e:Entity {{name: ent.name, entity_type: ent.entity_type}})
             MERGE (e)-[r:MENTIONED_IN]->(s)
             SET r.role = ent.role
             WITH e
             OPTIONAL MATCH (a:Actor)
               WHERE a.name = e.name OR a.canonical_key = e.name
             FOREACH (_ IN CASE WHEN a IS NOT NULL THEN [1] ELSE [] END |
               MERGE (e)-[:SAME_AS]->(a)
             )"
        ))
        .param("signal_id", signal_id_str)
        .param("entities", entity_params);

        vec![Op::Run(q)]
    }

    fn plan_locations(&self, signal_id: &Uuid, signal_label: &str, locations: &[Location]) -> Vec<Op> {
        if locations.is_empty() {
            return vec![];
        }

        let signal_id_str = signal_id.to_string();
        let mut ops = Vec::new();

        for loc in locations {
            let name = match loc.name.as_deref() {
                Some(n) if !n.is_empty() => n,
                _ => continue,
            };

            let normalized_name = name.trim().to_lowercase();
            let edge_type = location_edge_type(signal_label, loc.role.as_deref());

            // Create a stub Location by normalized_name only.
            // LocationGeocoded (from the geocoder handler) fills in
            // deterministic coordinates and merges duplicates via canonical_address.
            let q = query(&format!(
                "MATCH (s:{signal_label} {{id: $signal_id}})
                 MERGE (l:Location {{normalized_name: $normalized_name}})
                 ON CREATE SET l.name = $name, l.address = $address
                 MERGE (s)-[:{edge_type}]->(l)"
            ))
            .param("signal_id", signal_id_str.clone())
            .param("normalized_name", normalized_name)
            .param("name", name)
            .param("address", loc.address.as_deref().unwrap_or(""));

            ops.push(Op::Run(q));
        }

        ops
    }
}

// ---------------------------------------------------------------------------
// Helpers — no graph reads, no wall-clock time
// ---------------------------------------------------------------------------

fn node_type_label(node_type: NodeType) -> &'static str {
    match node_type {
        NodeType::Gathering => "Gathering",
        NodeType::Resource => "Resource",
        NodeType::HelpRequest => "HelpRequest",
        NodeType::Announcement => "Announcement",
        NodeType::Concern => "Concern",
        NodeType::Condition => "Condition",
        NodeType::Citation => "Citation",
    }
}

fn format_dt(dt: &DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// Use the event's stored timestamp (from the events table) when no explicit timestamp
/// exists in the payload. This is the fact's timestamp — never wall-clock time.
fn format_dt_from_event(event: &PersistedEvent) -> String {
    format_dt(&event.created_at)
}


/// Map signal label + location role to a typed Neo4j edge.
fn location_edge_type(signal_label: &str, role: Option<&str>) -> &'static str {
    match role {
        Some("affected_area") | Some("epicenter") => "AFFECTS",
        Some("origin") | Some("destination") => "REFERENCES_LOCATION",
        _ => match signal_label {
            "Gathering" => "HELD_AT",
            "Resource" => "AVAILABLE_AT",
            "HelpRequest" => "NEEDED_AT",
            "Announcement" => "RELEVANT_TO",
            "Concern" => "AFFECTS",
            "Condition" => "OBSERVED_AT",
            _ => "REFERENCES_LOCATION",
        },
    }
}

fn location_lat_lng(loc: &Option<Location>) -> (f64, f64) {
    loc.as_ref()
        .and_then(|l| l.point.as_ref())
        .map(|p| (p.lat, p.lng))
        .unwrap_or((0.0, 0.0))
}

fn location_name_str(loc: &Option<Location>) -> String {
    loc.as_ref()
        .and_then(|l| l.name.clone())
        .unwrap_or_default()
}


struct ScheduleProps {
    starts_at: String,
    ends_at: String,
    rrule: String,
    all_day: bool,
    timezone: String,
    schedule_text: String,
    rdates: Vec<String>,
    exdates: Vec<String>,
}

fn extract_schedule(schedule: &Option<Schedule>) -> ScheduleProps {
    match schedule {
        Some(s) => ScheduleProps {
            starts_at: s.starts_at.map(|dt| format_dt(&dt)).unwrap_or_default(),
            ends_at: s.ends_at.map(|dt| format_dt(&dt)).unwrap_or_default(),
            rrule: s.rrule.clone().unwrap_or_default(),
            all_day: s.all_day,
            timezone: s.timezone.clone().unwrap_or_default(),
            schedule_text: s.schedule_text.clone().unwrap_or_default(),
            rdates: s.rdates.iter().map(|dt| format_dt(dt)).collect(),
            exdates: s.exdates.iter().map(|dt| format_dt(dt)).collect(),
        },
        None => ScheduleProps {
            starts_at: String::new(),
            ends_at: String::new(),
            rrule: String::new(),
            all_day: false,
            timezone: String::new(),
            schedule_text: String::new(),
            rdates: Vec::new(),
            exdates: Vec::new(),
        },
    }
}

/// Build the common MERGE/ON CREATE SET query for all 6 discovery event types.
/// Location data is projected as :Location nodes via `project_locations`.
/// Flat lat/lng/location_name/address properties are kept temporarily for reader compat
/// and will be removed once readers are migrated to traverse Location edges.
fn build_discovery_query(
    label: &str,
    type_specific_set: &str,
    id: uuid::Uuid,
    title: &str,
    summary: &str,
    confidence: f32,
    url: &str,
    extracted_at: &DateTime<Utc>,
    published_at: Option<DateTime<Utc>>,
    locations: &[Location],
    event: &PersistedEvent,
) -> neo4rs::Query {
    let locations_json = if locations.is_empty() {
        String::new()
    } else {
        serde_json::to_string(locations).unwrap_or_default()
    };
    let actor = actor_str(event);
    let run_id = run_id_str(event);

    let cypher = format!(
        "MERGE (n:{label} {{id: $id}})
         ON CREATE SET
             n.title = $title,
             n.summary = $summary,
             n.confidence = $confidence,
             n.url = $url,
             n.extracted_at = datetime($extracted_at),
             n.last_confirmed_active = datetime($extracted_at),
             n.published_at = CASE WHEN $published_at = '' THEN null ELSE datetime($published_at) END,
             n.locations_json = $locations_json,
             n.sensitivity = 'general',
             n.corroboration_count = 0,
             n.review_status = 'staged',
             n.created_by = $created_by,
             n.scout_run_id = $scout_run_id
             {type_specific_set}"
    );

    query(&cypher)
        .param("id", id.to_string())
        .param("title", title)
        .param("summary", summary)
        .param("confidence", confidence as f64)
        .param("url", url)
        .param("extracted_at", format_dt(extracted_at))
        .param(
            "published_at",
            published_at.map(|dt| format_dt(&dt)).unwrap_or_default(),
        )
        .param("locations_json", locations_json)
        .param("created_by", actor)
        .param("scout_run_id", run_id)
}

fn urgency_str(u: rootsignal_common::types::Urgency) -> &'static str {
    match u {
        rootsignal_common::types::Urgency::Low => "low",
        rootsignal_common::types::Urgency::Medium => "medium",
        rootsignal_common::types::Urgency::High => "high",
        rootsignal_common::types::Urgency::Critical => "critical",
    }
}

fn severity_str(s: rootsignal_common::types::Severity) -> &'static str {
    match s {
        rootsignal_common::types::Severity::Low => "low",
        rootsignal_common::types::Severity::Medium => "medium",
        rootsignal_common::types::Severity::High => "high",
        rootsignal_common::types::Severity::Critical => "critical",
    }
}

// ---------------------------------------------------------------------------
// Signal property SET helpers — used by execute() and execute_batch()
// ---------------------------------------------------------------------------

/// Build a single-row SET query for a signal property via multi-label coalesce.
fn build_signal_set_query(
    property: &str,
    signal_id: &str,
    value: neo4rs::BoltType,
    include_condition: bool,
) -> neo4rs::Query {
    let cypher = build_signal_set_cypher(property, include_condition);
    query(&cypher)
        .param("id", signal_id)
        .param("value", value)
}

/// Cypher for single-row signal property SET.
fn build_signal_set_cypher(property: &str, include_condition: bool) -> String {
    if include_condition {
        format!(
            "OPTIONAL MATCH (g:Gathering {{id: $id}})
             OPTIONAL MATCH (a:Resource {{id: $id}})
             OPTIONAL MATCH (n:HelpRequest {{id: $id}})
             OPTIONAL MATCH (nc:Announcement {{id: $id}})
             OPTIONAL MATCH (t:Concern {{id: $id}})
             OPTIONAL MATCH (cond:Condition {{id: $id}})
             WITH coalesce(g, a, n, nc, t, cond) AS node
             WHERE node IS NOT NULL
             SET node.{property} = $value"
        )
    } else {
        format!(
            "OPTIONAL MATCH (g:Gathering {{id: $id}})
             OPTIONAL MATCH (a:Resource {{id: $id}})
             OPTIONAL MATCH (n:HelpRequest {{id: $id}})
             OPTIONAL MATCH (nc:Announcement {{id: $id}})
             OPTIONAL MATCH (t:Concern {{id: $id}})
             WITH coalesce(g, a, n, nc, t) AS node
             WHERE node IS NOT NULL
             SET node.{property} = $value"
        )
    }
}

/// Cypher for UNWIND batch signal property SET.
fn build_signal_set_unwind_cypher(property: &str, include_condition: bool) -> String {
    if include_condition {
        format!(
            "UNWIND $rows AS row
             OPTIONAL MATCH (g:Gathering {{id: row.id}})
             OPTIONAL MATCH (a:Resource {{id: row.id}})
             OPTIONAL MATCH (n:HelpRequest {{id: row.id}})
             OPTIONAL MATCH (nc:Announcement {{id: row.id}})
             OPTIONAL MATCH (t:Concern {{id: row.id}})
             OPTIONAL MATCH (cond:Condition {{id: row.id}})
             WITH coalesce(g, a, n, nc, t, cond) AS node, row
             WHERE node IS NOT NULL
             SET node.{property} = row.val"
        )
    } else {
        format!(
            "UNWIND $rows AS row
             OPTIONAL MATCH (g:Gathering {{id: row.id}})
             OPTIONAL MATCH (a:Resource {{id: row.id}})
             OPTIONAL MATCH (n:HelpRequest {{id: row.id}})
             OPTIONAL MATCH (nc:Announcement {{id: row.id}})
             OPTIONAL MATCH (t:Concern {{id: row.id}})
             WITH coalesce(g, a, n, nc, t) AS node, row
             WHERE node IS NOT NULL
             SET node.{property} = row.val"
        )
    }
}

