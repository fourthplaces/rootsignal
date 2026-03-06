//! GraphProjector — projection of facts into Neo4j nodes and edges.
//!
//! Each event is either acted upon (MERGE/SET/DELETE) or ignored (no-op).
//! Embeddings are computed at projection time via an optional EmbeddingStore
//! (backed by a Postgres cache, so replay gets 100% cache hits).
//!
//! Idempotency: all writes use MERGE or conditional SET with the event's seq as a guard.
//! Replaying the same event twice produces the same graph state.

use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Utc};
use neo4rs::query;
use tracing::{debug, warn};
use uuid::Uuid;

use rootsignal_common::events::{
    AnnouncementCorrection, ConcernCorrection, Event, GatheringCorrection,
    HelpRequestCorrection, Location, ResourceCorrection, Schedule, SignalDiversityScore,
    SituationChange, SourceChange, SystemEvent, SystemSourceChange, WorldEvent,
};
use rootsignal_common::types::{NodeType, SourceNode};
use rootsignal_common::EmbeddingLookup;
use rootsignal_events::StoredEvent;
use crate::GraphClient;


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

impl GraphProjector {
    pub fn new(client: GraphClient) -> Self {
        Self { client, embedding_store: None }
    }

    /// Attach an embedding store for computing embeddings at projection time.
    pub fn with_embedding_store(mut self, store: Arc<dyn EmbeddingLookup>) -> Self {
        self.embedding_store = Some(store);
        self
    }

    /// Compute and set embedding on a signal node. Skips silently on failure.
    async fn set_embedding(&self, label: &str, id: &Uuid, title: &str, summary: &str) {
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
                Ok(_) => {} // NoOp embedder returns empty
                Err(e) => {
                    warn!(error = %e, %label, %id, "Embedding lookup failed, skipping");
                }
            }
        }
    }

    /// Project a single fact to the graph. Idempotent.
    ///
    /// Routes by `EventDomain` — Rust forces exhaustive match arms, so adding
    /// a new domain variant produces a compile error here until handled.
    pub async fn project(&self, event: &StoredEvent) -> Result<ApplyResult> {
        use rootsignal_common::events::EventDomain;

        let domain = match EventDomain::from_event_type(&event.event_type) {
            Some(d) => d,
            None => {
                warn!(
                    seq = event.seq,
                    event_type = event.event_type,
                    "Unknown event domain — update EventDomain enum"
                );
                return Ok(ApplyResult::DeserializeError(format!(
                    "unknown event domain: {}",
                    event.event_type
                )));
            }
        };

        // Exhaustive match — no wildcard. Adding a new EventDomain variant
        // will fail to compile here until the projector handles it.
        match domain {
            EventDomain::Fact => self.project_fact(event).await,
            EventDomain::Discovery | EventDomain::Pipeline => {
                self.project_pipeline(event).await
            }
            EventDomain::Scrape => Ok(ApplyResult::NoOp),
            EventDomain::Signal => Ok(ApplyResult::NoOp),
            EventDomain::Lifecycle => Ok(ApplyResult::NoOp),
            EventDomain::Enrichment => Ok(ApplyResult::NoOp),
            EventDomain::Expansion => Ok(ApplyResult::NoOp),
            EventDomain::Synthesis => Ok(ApplyResult::NoOp),
            EventDomain::SituationWeaving => Ok(ApplyResult::NoOp),
            EventDomain::Supervisor => Ok(ApplyResult::NoOp),
        }
    }

    /// Project a World/System/Telemetry fact event to the graph.
    async fn project_fact(&self, event: &StoredEvent) -> Result<ApplyResult> {
        let mut payload = event.payload.clone();
        rootsignal_events::upcast(&event.event_type, event.schema_v, &mut payload);

        let parsed = match Event::from_payload(&payload) {
            Ok(e) => e,
            Err(e) => {
                warn!(seq = event.seq, error = %e, "Failed to deserialize fact event payload");
                return Ok(ApplyResult::DeserializeError(e.to_string()));
            }
        };

        match parsed {
            Event::Telemetry(_) => {
                debug!(
                    seq = event.seq,
                    event_type = event.event_type,
                    "No-op (telemetry)"
                );
                Ok(ApplyResult::NoOp)
            }
            Event::World(world) => self.project_world(world, event).await,
            Event::System(system) => self.project_system(system, event).await,
        }
    }

    // =================================================================
    // Pipeline events — only projectable variants
    // =================================================================

    async fn project_pipeline(&self, event: &StoredEvent) -> Result<ApplyResult> {
        match event.event_type.as_str() {
            "pipeline:source_discovered" | "discovery:source_discovered" => {
                #[derive(serde::Deserialize)]
                struct Payload {
                    source: SourceNode,
                    #[allow(dead_code)]
                    discovered_by: String,
                }
                let payload: Payload = serde_json::from_value(event.payload.clone())
                    .map_err(|e| anyhow::anyhow!("source_discovered deser: {e}"))?;
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
                         s.sources_discovered = 0
                     ON MATCH SET
                         s.active = CASE WHEN s.active = false AND $discovery_method = 'curated' THEN true ELSE s.active END,
                         s.url = CASE WHEN $url <> '' THEN $url ELSE s.url END",
                )
                .param("id", s.id.to_string())
                .param("canonical_key", s.canonical_key.as_str())
                .param("canonical_value", s.canonical_value.as_str())
                .param("url", s.url.as_deref().unwrap_or(""))
                .param("discovery_method", s.discovery_method.to_string())
                .param("ts", format_dt_from_stored(event))
                .param("weight", s.weight)
                .param("source_role", s.source_role.to_string())
                .param("gap_context", s.gap_context.clone().unwrap_or_default());

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }
            // SourcesDiscovered is a proposal — the domain_filter handler decides
            // which become SourcesRegistered. SourceRejected is audit-only.
            "discovery:sources_discovered" | "discovery:source_rejected" => {
                Ok(ApplyResult::NoOp)
            }
            _ => {
                debug!(seq = event.seq, event_type = %event.event_type, "No-op (pipeline)");
                Ok(ApplyResult::NoOp)
            }
        }
    }

    // =================================================================
    // World events — observed facts
    // =================================================================

    async fn project_world(&self, world: WorldEvent, event: &StoredEvent) -> Result<ApplyResult> {
        match world {
            // ---------------------------------------------------------
            // Discovery facts — 5 typed variants
            // ---------------------------------------------------------
            WorldEvent::GatheringAnnounced {
                id,
                title,
                summary,
                source_url,
                published_at,
                extraction_id: _,
                locations,
                mentioned_entities: _,
                references: _,
                schedule,
                action_url,
            } => {
                let location = locations.into_iter().next();
                let (starts_at, ends_at, rrule, all_day, timezone) = extract_schedule(&schedule);
                let q = build_discovery_query(
                    "Gathering",
                    ", n.starts_at = CASE WHEN $starts_at = '' THEN null ELSE datetime($starts_at) END,
                       n.ends_at = CASE WHEN $ends_at = '' THEN null ELSE datetime($ends_at) END,
                       n.rrule = $rrule, n.all_day = $all_day, n.timezone = $timezone,
                       n.action_url = $action_url,
                       n.is_recurring = CASE WHEN $rrule <> '' THEN true ELSE false END",
                    id, &title, &summary, 0.5, &source_url,
                    &event.ts, published_at, &location, event,
                )
                .param("starts_at", starts_at)
                .param("ends_at", ends_at)
                .param("rrule", rrule)
                .param("all_day", all_day)
                .param("timezone", timezone)
                .param("action_url", action_url.as_deref().unwrap_or(""));

                self.client.run(q).await?;
                self.set_embedding("Gathering", &id, &title, &summary).await;
                Ok(ApplyResult::Applied)
            }

            WorldEvent::ResourceOffered {
                id,
                title,
                summary,
                source_url,
                published_at,
                extraction_id: _,
                locations,
                mentioned_entities: _,
                references: _,
                schedule,
                action_url,
                availability,
                eligibility,
            } => {
                let location = locations.into_iter().next();
                let (starts_at, ends_at, rrule, all_day, timezone) = extract_schedule(&schedule);
                let q = build_discovery_query(
                    "Resource",
                    ", n.action_url = $action_url, n.availability = $availability, n.eligibility = $eligibility,
                       n.starts_at = CASE WHEN $starts_at = '' THEN null ELSE datetime($starts_at) END,
                       n.ends_at = CASE WHEN $ends_at = '' THEN null ELSE datetime($ends_at) END,
                       n.rrule = $rrule, n.all_day = $all_day, n.timezone = $timezone",
                    id,
                    &title,
                    &summary,
                    0.5,
                    &source_url,
                    &event.ts,
                    published_at,
                    &location,
                    event,
                )
                .param("action_url", action_url.as_deref().unwrap_or(""))
                .param("availability", availability.as_deref().unwrap_or(""))
                .param("eligibility", eligibility.as_deref().unwrap_or(""))
                .param("starts_at", starts_at)
                .param("ends_at", ends_at)
                .param("rrule", rrule)
                .param("all_day", all_day)
                .param("timezone", timezone);

                self.client.run(q).await?;
                self.set_embedding("Resource", &id, &title, &summary).await;
                Ok(ApplyResult::Applied)
            }

            WorldEvent::HelpRequested {
                id,
                title,
                summary,
                source_url,
                published_at,
                extraction_id: _,
                locations,
                mentioned_entities: _,
                references: _,
                schedule,
                what_needed,
                stated_goal,
            } => {
                let location = locations.into_iter().next();
                let (starts_at, ends_at, rrule, all_day, timezone) = extract_schedule(&schedule);
                let q = build_discovery_query(
                    "HelpRequest",
                    ", n.what_needed = $what_needed, n.stated_goal = $stated_goal,
                       n.starts_at = CASE WHEN $starts_at = '' THEN null ELSE datetime($starts_at) END,
                       n.ends_at = CASE WHEN $ends_at = '' THEN null ELSE datetime($ends_at) END,
                       n.rrule = $rrule, n.all_day = $all_day, n.timezone = $timezone",
                    id,
                    &title,
                    &summary,
                    0.5,
                    &source_url,
                    &event.ts,
                    published_at,
                    &location,
                    event,
                )
                .param("what_needed", what_needed.as_deref().unwrap_or(""))
                .param("stated_goal", stated_goal.unwrap_or_default())
                .param("starts_at", starts_at)
                .param("ends_at", ends_at)
                .param("rrule", rrule)
                .param("all_day", all_day)
                .param("timezone", timezone);

                self.client.run(q).await?;
                self.set_embedding("HelpRequest", &id, &title, &summary).await;
                Ok(ApplyResult::Applied)
            }

            WorldEvent::AnnouncementShared {
                id,
                title,
                summary,
                source_url,
                published_at,
                extraction_id: _,
                locations,
                mentioned_entities: _,
                references: _,
                schedule,
                subject,
                effective_date,
            } => {
                let location = locations.into_iter().next();
                let (starts_at, ends_at, rrule, all_day, timezone) = extract_schedule(&schedule);
                let q = build_discovery_query(
                    "Announcement",
                    ", n.subject = $subject,
                       n.effective_date = CASE WHEN $effective_date = '' THEN null ELSE datetime($effective_date) END,
                       n.starts_at = CASE WHEN $starts_at = '' THEN null ELSE datetime($starts_at) END,
                       n.ends_at = CASE WHEN $ends_at = '' THEN null ELSE datetime($ends_at) END,
                       n.rrule = $rrule, n.all_day = $all_day, n.timezone = $timezone",
                    id, &title, &summary, 0.5, &source_url,
                    &event.ts, published_at, &location, event,
                )
                .param("subject", subject.as_deref().unwrap_or(""))
                .param("effective_date", effective_date.map(|dt| format_dt(&dt)).unwrap_or_default())
                .param("starts_at", starts_at)
                .param("ends_at", ends_at)
                .param("rrule", rrule)
                .param("all_day", all_day)
                .param("timezone", timezone);

                self.client.run(q).await?;
                self.set_embedding("Announcement", &id, &title, &summary).await;
                Ok(ApplyResult::Applied)
            }

            WorldEvent::ConcernRaised {
                id,
                title,
                summary,
                source_url,
                published_at,
                extraction_id: _,
                locations,
                mentioned_entities: _,
                references: _,
                schedule,
                subject,
                opposing,
            } => {
                let location = locations.into_iter().next();
                let (starts_at, ends_at, rrule, all_day, timezone) = extract_schedule(&schedule);
                let q = build_discovery_query(
                    "Concern",
                    ", n.subject = $subject, n.opposing = $opposing,
                       n.starts_at = CASE WHEN $starts_at = '' THEN null ELSE datetime($starts_at) END,
                       n.ends_at = CASE WHEN $ends_at = '' THEN null ELSE datetime($ends_at) END,
                       n.rrule = $rrule, n.all_day = $all_day, n.timezone = $timezone",
                    id,
                    &title,
                    &summary,
                    0.5,
                    &source_url,
                    &event.ts,
                    published_at,
                    &location,
                    event,
                )
                .param("subject", subject.as_deref().unwrap_or(""))
                .param("opposing", opposing.as_deref().unwrap_or(""))
                .param("starts_at", starts_at)
                .param("ends_at", ends_at)
                .param("rrule", rrule)
                .param("all_day", all_day)
                .param("timezone", timezone);

                self.client.run(q).await?;
                self.set_embedding("Concern", &id, &title, &summary).await;
                Ok(ApplyResult::Applied)
            }

            WorldEvent::ConditionObserved {
                id,
                title,
                summary,
                source_url,
                published_at,
                extraction_id: _,
                locations,
                mentioned_entities: _,
                references: _,
                schedule,
                subject,
                observed_by,
                measurement,
                affected_scope,
            } => {
                let location = locations.into_iter().next();
                let (starts_at, ends_at, rrule, all_day, timezone) = extract_schedule(&schedule);
                let q = build_discovery_query(
                    "Condition",
                    ", n.subject = $subject, n.observed_by = $observed_by,
                       n.measurement = $measurement, n.affected_scope = $affected_scope,
                       n.starts_at = CASE WHEN $starts_at = '' THEN null ELSE datetime($starts_at) END,
                       n.ends_at = CASE WHEN $ends_at = '' THEN null ELSE datetime($ends_at) END,
                       n.rrule = $rrule, n.all_day = $all_day, n.timezone = $timezone",
                    id, &title, &summary, 0.5, &source_url,
                    &event.ts, published_at, &location, event,
                )
                .param("subject", subject.as_deref().unwrap_or(""))
                .param("observed_by", observed_by.as_deref().unwrap_or(""))
                .param("measurement", measurement.as_deref().unwrap_or(""))
                .param("affected_scope", affected_scope.as_deref().unwrap_or(""))
                .param("starts_at", starts_at)
                .param("ends_at", ends_at)
                .param("rrule", rrule)
                .param("all_day", all_day)
                .param("timezone", timezone);

                self.client.run(q).await?;
                self.set_embedding("Condition", &id, &title, &summary).await;
                Ok(ApplyResult::Applied)
            }

            // ---------------------------------------------------------
            // Lifecycle events — placeholder (log only, no graph action yet)
            // ---------------------------------------------------------
            WorldEvent::GatheringCancelled { signal_id, reason, .. } => {
                debug!(signal_id = %signal_id, reason = %reason, "GatheringCancelled (no-op placeholder)");
                Ok(ApplyResult::NoOp)
            }
            WorldEvent::ResourceDepleted { signal_id, reason, .. } => {
                debug!(signal_id = %signal_id, reason = %reason, "ResourceDepleted (no-op placeholder)");
                Ok(ApplyResult::NoOp)
            }
            WorldEvent::AnnouncementRetracted { signal_id, reason, .. } => {
                debug!(signal_id = %signal_id, reason = %reason, "AnnouncementRetracted (no-op placeholder)");
                Ok(ApplyResult::NoOp)
            }
            WorldEvent::CitationRetracted { citation_id, reason, .. } => {
                debug!(citation_id = %citation_id, reason = %reason, "CitationRetracted (no-op placeholder)");
                Ok(ApplyResult::NoOp)
            }
            WorldEvent::DetailsChanged { signal_id, summary, .. } => {
                debug!(signal_id = %signal_id, summary = %summary, "DetailsChanged (no-op placeholder)");
                Ok(ApplyResult::NoOp)
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
                .param("ts", format_dt_from_stored(event))
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

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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
                         r.signal_count = 1,
                         r.created_at = datetime($ts),
                         r.last_seen = datetime($ts)
                     ON MATCH SET
                         r.signal_count = r.signal_count + 1,
                         r.last_seen = datetime($ts)",
                )
                .param("slug", slug.as_str())
                .param("id", resource_id.to_string())
                .param("name", name.as_str())
                .param("description", description.as_str())
                .param("ts", format_dt_from_stored(event));

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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
                        return Ok(ApplyResult::NoOp);
                    }
                };

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            // ---------------------------------------------------------
            // Provenance links
            // ---------------------------------------------------------
            WorldEvent::SourceLinkDiscovered { .. } => {
                debug!(seq = event.seq, "No-op (source link — informational)");
                Ok(ApplyResult::NoOp)
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

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

        }
    }

    // =================================================================
    // System decisions — editorial judgments
    // =================================================================

    async fn project_system(
        &self,
        system: SystemEvent,
        event: &StoredEvent,
    ) -> Result<ApplyResult> {
        match system {
            // ---------------------------------------------------------
            // Sensitivity + implied queries (paired with discoveries)
            // ---------------------------------------------------------
            SystemEvent::SensitivityClassified { signal_id, level } => {
                let q = query(
                    "OPTIONAL MATCH (g:Gathering {id: $id})
                     OPTIONAL MATCH (a:Resource {id: $id})
                     OPTIONAL MATCH (n:HelpRequest {id: $id})
                     OPTIONAL MATCH (nc:Announcement {id: $id})
                     OPTIONAL MATCH (t:Concern {id: $id})
                     WITH coalesce(g, a, n, nc, t) AS node
                     WHERE node IS NOT NULL
                     SET node.sensitivity = $level",
                )
                .param("id", signal_id.to_string())
                .param("level", level.as_str());

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemEvent::ToneClassified { signal_id, tone } => {
                let q = query(
                    "OPTIONAL MATCH (g:Gathering {id: $id})
                     OPTIONAL MATCH (a:Resource {id: $id})
                     OPTIONAL MATCH (n:HelpRequest {id: $id})
                     OPTIONAL MATCH (nc:Announcement {id: $id})
                     OPTIONAL MATCH (t:Concern {id: $id})
                     WITH coalesce(g, a, n, nc, t) AS node
                     WHERE node IS NOT NULL
                     SET node.tone = $value",
                )
                .param("id", signal_id.to_string())
                .param("value", tone.to_string());
                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemEvent::SeverityClassified { signal_id, severity } => {
                let q = query(
                    "OPTIONAL MATCH (g:Gathering {id: $id})
                     OPTIONAL MATCH (a:Resource {id: $id})
                     OPTIONAL MATCH (n:HelpRequest {id: $id})
                     OPTIONAL MATCH (nc:Announcement {id: $id})
                     OPTIONAL MATCH (t:Concern {id: $id})
                     WITH coalesce(g, a, n, nc, t) AS node
                     WHERE node IS NOT NULL
                     SET node.severity = $value",
                )
                .param("id", signal_id.to_string())
                .param("value", severity.to_string());
                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemEvent::UrgencyClassified { signal_id, urgency } => {
                let q = query(
                    "OPTIONAL MATCH (g:Gathering {id: $id})
                     OPTIONAL MATCH (a:Resource {id: $id})
                     OPTIONAL MATCH (n:HelpRequest {id: $id})
                     OPTIONAL MATCH (nc:Announcement {id: $id})
                     OPTIONAL MATCH (t:Concern {id: $id})
                     WITH coalesce(g, a, n, nc, t) AS node
                     WHERE node IS NOT NULL
                     SET node.urgency = $value",
                )
                .param("id", signal_id.to_string())
                .param("value", urgency.to_string());
                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemEvent::CategoryClassified { signal_id, category } => {
                let q = query(
                    "OPTIONAL MATCH (g:Gathering {id: $id})
                     OPTIONAL MATCH (a:Resource {id: $id})
                     OPTIONAL MATCH (n:HelpRequest {id: $id})
                     OPTIONAL MATCH (nc:Announcement {id: $id})
                     OPTIONAL MATCH (t:Concern {id: $id})
                     OPTIONAL MATCH (cond:Condition {id: $id})
                     WITH coalesce(g, a, n, nc, t, cond) AS node
                     WHERE node IS NOT NULL
                     SET node.category = $value",
                )
                .param("id", signal_id.to_string())
                .param("value", category.as_str());
                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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
                let q = query(&format!(
                    "MATCH (n:{label} {{id: $id}})
                     SET n.last_confirmed_active = datetime($ts),
                         n.corroboration_count = coalesce(n.corroboration_count, 0) + 1"
                ))
                .param("id", signal_id.to_string())
                .param("ts", format_dt_from_stored(event));

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            // ---------------------------------------------------------
            // Corroboration scoring
            // ---------------------------------------------------------
            SystemEvent::CorroborationScored {
                signal_id,
                new_corroboration_count,
                ..
            } => {
                // Find entity across all signal types and set count
                let q = query(
                    "OPTIONAL MATCH (g:Gathering {id: $id})
                     OPTIONAL MATCH (a:Resource {id: $id})
                     OPTIONAL MATCH (n:HelpRequest {id: $id})
                     OPTIONAL MATCH (nc:Announcement {id: $id})
                     OPTIONAL MATCH (t:Concern {id: $id})
                     WITH coalesce(g, a, n, nc, t) AS node
                     WHERE node IS NOT NULL
                     SET node.corroboration_count = $count",
                )
                .param("id", signal_id.to_string())
                .param("count", new_corroboration_count as i64);

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemEvent::ConfidenceScored {
                signal_id,
                new_confidence,
                ..
            } => {
                let q = query(
                    "OPTIONAL MATCH (g:Gathering {id: $id})
                     OPTIONAL MATCH (a:Resource {id: $id})
                     OPTIONAL MATCH (n:HelpRequest {id: $id})
                     OPTIONAL MATCH (nc:Announcement {id: $id})
                     OPTIONAL MATCH (t:Concern {id: $id})
                     WITH coalesce(g, a, n, nc, t) AS node
                     WHERE node IS NOT NULL
                     SET node.confidence = $confidence",
                )
                .param("id", signal_id.to_string())
                .param("confidence", new_confidence as f64);

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemEvent::ObservationRejected { .. } => {
                debug!(
                    seq = event.seq,
                    "No-op (observation rejected — informational)"
                );
                Ok(ApplyResult::NoOp)
            }

            SystemEvent::SignalsExpired { signals } => {
                let ts = format_dt_from_stored(event);
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

                    self.client.run(q).await?;
                }
                if signals.is_empty() {
                    Ok(ApplyResult::NoOp)
                } else {
                    Ok(ApplyResult::Applied)
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

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemEvent::DuplicateDetected { .. } => {
                debug!(
                    seq = event.seq,
                    "No-op (duplicate detected — informational)"
                );
                Ok(ApplyResult::NoOp)
            }

            SystemEvent::ExtractionDroppedNoDate { .. } => {
                debug!(
                    seq = event.seq,
                    "No-op (extraction dropped — informational)"
                );
                Ok(ApplyResult::NoOp)
            }

            SystemEvent::ReviewVerdictReached {
                signal_id,
                new_status,
                ..
            } => {
                // Update the signal's review status
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

                self.client.run(q).await?;

                // Reactively promote situations: if all constituent signals
                // are now 'live', promote the situation too.
                if new_status == "live" {
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
                             WHERE other.review_status <> 'live'
                           }
                         SET sit.review_status = 'live'",
                    )
                    .param("id", signal_id.to_string());

                    self.client.run(promote).await?;
                }

                Ok(ApplyResult::Applied)
            }

            SystemEvent::ImpliedQueriesConsumed { signal_ids } => {
                let ids: Vec<String> = signal_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS id
                     MATCH (n) WHERE n.id = id AND (n:Resource OR n:Gathering)
                     SET n.implied_queries = null",
                )
                .param("ids", ids);

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            // ---------------------------------------------------------
            // Corrections
            // ---------------------------------------------------------
            SystemEvent::GatheringCorrected {
                signal_id,
                correction,
                ..
            } => {
                match correction {
                    GatheringCorrection::Title { new, .. } => {
                        self.set_str("Gathering", signal_id, "title", &new).await?
                    }
                    GatheringCorrection::Summary { new, .. } => {
                        self.set_str("Gathering", signal_id, "summary", &new)
                            .await?
                    }
                    GatheringCorrection::Sensitivity { new, .. } => {
                        self.set_str("Gathering", signal_id, "sensitivity", new.as_str())
                            .await?
                    }
                    GatheringCorrection::Location { new, .. } => {
                        self.set_location("Gathering", signal_id, &new).await?
                    }
                    GatheringCorrection::Schedule { new, .. } => {
                        self.set_schedule("Gathering", signal_id, &new).await?
                    }
                    GatheringCorrection::Organizer { new, .. } => {
                        self.set_str(
                            "Gathering",
                            signal_id,
                            "organizer",
                            new.as_deref().unwrap_or(""),
                        )
                        .await?
                    }
                    GatheringCorrection::ActionUrl { new, .. } => {
                        self.set_str(
                            "Gathering",
                            signal_id,
                            "action_url",
                            new.as_deref().unwrap_or(""),
                        )
                        .await?
                    }
                    GatheringCorrection::Unknown => {
                        debug!("Ignoring unknown gathering correction field");
                        return Ok(ApplyResult::NoOp);
                    }
                }
                Ok(ApplyResult::Applied)
            }

            SystemEvent::ResourceCorrected {
                signal_id,
                correction,
                ..
            } => {
                match correction {
                    ResourceCorrection::Title { new, .. } => {
                        self.set_str("Resource", signal_id, "title", &new).await?
                    }
                    ResourceCorrection::Summary { new, .. } => {
                        self.set_str("Resource", signal_id, "summary", &new).await?
                    }
                    ResourceCorrection::Sensitivity { new, .. } => {
                        self.set_str("Resource", signal_id, "sensitivity", new.as_str())
                            .await?
                    }
                    ResourceCorrection::Location { new, .. } => {
                        self.set_location("Resource", signal_id, &new).await?
                    }
                    ResourceCorrection::ActionUrl { new, .. } => {
                        self.set_str("Resource", signal_id, "action_url", new.as_deref().unwrap_or(""))
                            .await?
                    }
                    ResourceCorrection::Availability { new, .. } => {
                        self.set_str(
                            "Resource",
                            signal_id,
                            "availability",
                            new.as_deref().unwrap_or(""),
                        )
                        .await?
                    }
                    ResourceCorrection::IsOngoing { new, .. } => {
                        self.set_bool("Resource", signal_id, "is_ongoing", new.unwrap_or(false))
                            .await?
                    }
                    ResourceCorrection::Unknown => {
                        debug!("Ignoring unknown resource correction field");
                        return Ok(ApplyResult::NoOp);
                    }
                }
                Ok(ApplyResult::Applied)
            }

            SystemEvent::HelpRequestCorrected {
                signal_id,
                correction,
                ..
            } => {
                match correction {
                    HelpRequestCorrection::Title { new, .. } => {
                        self.set_str("HelpRequest", signal_id, "title", &new).await?
                    }
                    HelpRequestCorrection::Summary { new, .. } => {
                        self.set_str("HelpRequest", signal_id, "summary", &new).await?
                    }
                    HelpRequestCorrection::Sensitivity { new, .. } => {
                        self.set_str("HelpRequest", signal_id, "sensitivity", new.as_str())
                            .await?
                    }
                    HelpRequestCorrection::Location { new, .. } => {
                        self.set_location("HelpRequest", signal_id, &new).await?
                    }
                    HelpRequestCorrection::Urgency { new, .. } => {
                        self.set_str(
                            "HelpRequest",
                            signal_id,
                            "urgency",
                            new.map(|u| urgency_str(u)).unwrap_or(""),
                        )
                        .await?
                    }
                    HelpRequestCorrection::WhatNeeded { new, .. } => {
                        self.set_str(
                            "HelpRequest",
                            signal_id,
                            "what_needed",
                            new.as_deref().unwrap_or(""),
                        )
                        .await?
                    }
                    HelpRequestCorrection::StatedGoal { new, .. } => {
                        self.set_str("HelpRequest", signal_id, "stated_goal", new.as_deref().unwrap_or(""))
                            .await?
                    }
                    HelpRequestCorrection::Unknown => {
                        debug!("Ignoring unknown help request correction field");
                        return Ok(ApplyResult::NoOp);
                    }
                }
                Ok(ApplyResult::Applied)
            }

            SystemEvent::AnnouncementCorrected {
                signal_id,
                correction,
                ..
            } => {
                match correction {
                    AnnouncementCorrection::Title { new, .. } => {
                        self.set_str("Announcement", signal_id, "title", &new).await?
                    }
                    AnnouncementCorrection::Summary { new, .. } => {
                        self.set_str("Announcement", signal_id, "summary", &new).await?
                    }
                    AnnouncementCorrection::Sensitivity { new, .. } => {
                        self.set_str("Announcement", signal_id, "sensitivity", new.as_str())
                            .await?
                    }
                    AnnouncementCorrection::Location { new, .. } => {
                        self.set_location("Announcement", signal_id, &new).await?
                    }
                    AnnouncementCorrection::Category { new, .. } => {
                        self.set_str(
                            "Announcement",
                            signal_id,
                            "category",
                            new.as_deref().unwrap_or(""),
                        )
                        .await?
                    }
                    AnnouncementCorrection::EffectiveDate { new, .. } => {
                        let val = new.map(|dt| format_dt(&dt)).unwrap_or_default();
                        let q = query("MATCH (n:Announcement {id: $id}) SET n.effective_date = CASE WHEN $value = '' THEN null ELSE datetime($value) END")
                            .param("id", signal_id.to_string())
                            .param("value", val);
                        self.client.run(q).await?;
                    }
                    AnnouncementCorrection::Unknown => {
                        debug!("Ignoring unknown announcement correction field");
                        return Ok(ApplyResult::NoOp);
                    }
                }
                Ok(ApplyResult::Applied)
            }

            SystemEvent::ConcernCorrected {
                signal_id,
                correction,
                ..
            } => {
                match correction {
                    ConcernCorrection::Title { new, .. } => {
                        self.set_str("Concern", signal_id, "title", &new).await?
                    }
                    ConcernCorrection::Summary { new, .. } => {
                        self.set_str("Concern", signal_id, "summary", &new).await?
                    }
                    ConcernCorrection::Sensitivity { new, .. } => {
                        self.set_str("Concern", signal_id, "sensitivity", new.as_str())
                            .await?
                    }
                    ConcernCorrection::Location { new, .. } => {
                        self.set_location("Concern", signal_id, &new).await?
                    }
                    ConcernCorrection::Opposing { new, .. } => {
                        self.set_str(
                            "Concern",
                            signal_id,
                            "opposing",
                            new.as_deref().unwrap_or(""),
                        )
                        .await?
                    }
                    ConcernCorrection::Unknown => {
                        debug!("Ignoring unknown concern correction field");
                        return Ok(ApplyResult::NoOp);
                    }
                }
                Ok(ApplyResult::Applied)
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
                .param("ts", format_dt_from_stored(event));

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemEvent::OrphanedActorsCleaned { actor_ids } => {
                let ids: Vec<String> = actor_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS id
                     MATCH (a:Actor {id: id})
                     DETACH DELETE a",
                )
                .param("ids", ids);

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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
                .param("ts", format_dt_from_stored(event));

                self.client.run(q).await?;

                // SET optional enrichment fields when present
                let id_str = situation_id.to_string();
                if let Some(th) = tension_heat {
                    let q = query("MATCH (s:Situation {id: $id}) SET s.tension_heat = $v")
                        .param("id", id_str.clone()).param("v", th);
                    self.client.run(q).await?;
                }
                if let Some(ref cl) = clarity {
                    let q = query("MATCH (s:Situation {id: $id}) SET s.clarity = $v")
                        .param("id", id_str.clone()).param("v", cl.as_str());
                    self.client.run(q).await?;
                }
                if let Some(sc) = signal_count {
                    let q = query("MATCH (s:Situation {id: $id}) SET s.signal_count = $v")
                        .param("id", id_str.clone()).param("v", sc as i64);
                    self.client.run(q).await?;
                }
                if let Some(ref ne) = narrative_embedding {
                    let vals: Vec<f64> = ne.iter().map(|v| *v as f64).collect();
                    let q = query("MATCH (s:Situation {id: $id}) SET s.narrative_embedding = $v")
                        .param("id", id_str.clone()).param("v", vals);
                    self.client.run(q).await?;
                }
                if let Some(ref ce) = causal_embedding {
                    let vals: Vec<f64> = ce.iter().map(|v| *v as f64).collect();
                    let q = query("MATCH (s:Situation {id: $id}) SET s.causal_embedding = $v")
                        .param("id", id_str).param("v", vals);
                    self.client.run(q).await?;
                }

                Ok(ApplyResult::Applied)
            }

            SystemEvent::SituationChanged {
                situation_id,
                change,
            } => {
                let id_str = situation_id.to_string();
                let ts = format_dt_from_stored(event);
                match change {
                    SituationChange::Headline { new, .. } => {
                        let q = query("MATCH (s:Situation {id: $id}) SET s.headline = $value, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("value", new.as_str()).param("ts", ts);
                        self.client.run(q).await?;
                    }
                    SituationChange::Lede { new, .. } => {
                        let q = query("MATCH (s:Situation {id: $id}) SET s.lede = $value, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("value", new.as_str()).param("ts", ts);
                        self.client.run(q).await?;
                    }
                    SituationChange::Arc { new, .. } => {
                        let q = query("MATCH (s:Situation {id: $id}) SET s.arc = $value, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("value", new.to_string()).param("ts", ts);
                        self.client.run(q).await?;
                    }
                    SituationChange::Temperature { new, .. } => {
                        let q = query("MATCH (s:Situation {id: $id}) SET s.temperature = $value, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("value", new).param("ts", ts);
                        self.client.run(q).await?;
                    }
                    SituationChange::Location { new, .. } => {
                        let (lat, lng) = location_lat_lng(&new);
                        let name = location_name_str(&new);
                        let q = query("MATCH (s:Situation {id: $id}) SET s.centroid_lat = $lat, s.centroid_lng = $lng, s.location_name = $name, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("lat", lat).param("lng", lng).param("name", name).param("ts", ts);
                        self.client.run(q).await?;
                    }
                    SituationChange::Sensitivity { new, .. } => {
                        let q = query("MATCH (s:Situation {id: $id}) SET s.sensitivity = $value, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("value", new.as_str()).param("ts", ts);
                        self.client.run(q).await?;
                    }
                    SituationChange::Category { new, .. } => {
                        let q = query("MATCH (s:Situation {id: $id}) SET s.category = $value, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("value", new.as_deref().unwrap_or("")).param("ts", ts);
                        self.client.run(q).await?;
                    }
                    SituationChange::StructuredState { new, .. } => {
                        let q = query("MATCH (s:Situation {id: $id}) SET s.structured_state = $value, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("value", new.as_str()).param("ts", ts);
                        self.client.run(q).await?;
                    }
                }
                Ok(ApplyResult::Applied)
            }

            SystemEvent::SituationPromoted { situation_ids } => {
                let ids: Vec<String> = situation_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS id
                     MATCH (s:Situation {id: id})
                     SET s.review_status = 'live'",
                )
                .param("ids", ids);

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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
                let ts = format_dt_from_stored(event);
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

                self.client.run(q).await?;

                // Supersedes edge
                if let Some(ref sup_id) = supersedes {
                    let q = query(
                        "MATCH (d:Dispatch {id: $id}), (old:Dispatch {id: $old_id})
                         MERGE (d)-[:SUPERSEDES]->(old)",
                    )
                    .param("id", dispatch_id.to_string())
                    .param("old_id", sup_id.to_string());
                    self.client.run(q).await?;
                }

                // CITES edges to signals
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
                    self.client.run(q).await?;
                }

                // Update dispatch count on situation
                let q = query(
                    "MATCH (s:Situation {id: $sid})
                     SET s.dispatch_count = coalesce(s.dispatch_count, 0) + 1",
                )
                .param("sid", situation_id.to_string());
                self.client.run(q).await?;

                Ok(ApplyResult::Applied)
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
                     SET s.signal_count = coalesce(s.signal_count, 0) + 1
                     WITH s
                     OPTIONAL MATCH (t:Concern)-[:PART_OF]->(s)
                     WITH s, count(t) AS tc
                     SET s.tension_count = tc",
                )
                .param("signal_id", signal_id.to_string())
                .param("situation_id", situation_id.to_string())
                .param("confidence", confidence)
                .param("reasoning", reasoning.as_str())
                .param("label", signal_label.as_str());

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemEvent::SituationTagsAggregated {
                situation_id,
                tag_slugs,
            } => {
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
                    self.client.run(q).await?;
                }
                Ok(ApplyResult::Applied)
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

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            // ---------------------------------------------------------
            // Tags
            // ---------------------------------------------------------
            SystemEvent::SignalTagged {
                signal_id,
                tag_slugs,
            } => {
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

                    self.client.run(q).await?;
                }
                Ok(ApplyResult::Applied)
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

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemEvent::TagsMerged {
                source_slug,
                target_slug,
            } => {
                // Step 1: Repoint TAGGED edges
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
                self.client.run(q1).await?;

                // Step 2: Repoint SUPPRESSED_TAG edges
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
                self.client.run(q2).await?;

                // Step 3: Delete source tag
                let q3 = query(
                    "MATCH (t:Tag {slug: $source}) DETACH DELETE t",
                )
                .param("source", source_slug.as_str());
                self.client.run(q3).await?;

                Ok(ApplyResult::Applied)
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

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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
                     SET node.lat = null, node.lng = null",
                )
                .param("ids", ids);

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemEvent::OrphanedCitationsCleaned { citation_ids } => {
                let ids: Vec<String> = citation_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS id
                     MATCH (ev:Citation {id: id})
                     DETACH DELETE ev",
                )
                .param("ids", ids);

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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
                match change {
                    SystemSourceChange::QualityPenalty { new, .. } => {
                        let q = query(
                            "MATCH (s:Source {canonical_key: $key}) SET s.quality_penalty = $value",
                        )
                        .param("key", key)
                        .param("value", new);
                        self.client.run(q).await?;
                    }
                    SystemSourceChange::GapContext { new, .. } => {
                        let q = query(
                            "MATCH (s:Source {canonical_key: $key}) SET s.gap_context = $value",
                        )
                        .param("key", key)
                        .param("value", new.as_deref().unwrap_or(""));
                        self.client.run(q).await?;
                    }
                }
                Ok(ApplyResult::Applied)
            }

            // ---------------------------------------------------------
            // Source registry
            // ---------------------------------------------------------
            SystemEvent::SourcesRegistered { sources } => {
                let ts = format_dt_from_stored(event);
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
                             s.sources_discovered = 0
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
                    .param("gap_context", source.gap_context.unwrap_or_default());

                    self.client.run(q).await?;
                }
                Ok(ApplyResult::Applied)
            }

            SystemEvent::SourceChanged {
                canonical_key,
                change,
                ..
            } => {
                let key = canonical_key.as_str();
                match change {
                    SourceChange::Weight { new, .. } => {
                        let q =
                            query("MATCH (s:Source {canonical_key: $key}) SET s.weight = $value")
                                .param("key", key)
                                .param("value", new);
                        self.client.run(q).await?;
                    }
                    SourceChange::Url { new, .. } => {
                        let q = query("MATCH (s:Source {canonical_key: $key}) SET s.url = $value")
                            .param("key", key)
                            .param("value", new.as_str());
                        self.client.run(q).await?;
                    }
                    SourceChange::Role { new, .. } => {
                        let q = query(
                            "MATCH (s:Source {canonical_key: $key}) SET s.source_role = $value",
                        )
                        .param("key", key)
                        .param("value", new.to_string());
                        self.client.run(q).await?;
                    }
                    SourceChange::Active { new, .. } => {
                        let q =
                            query("MATCH (s:Source {canonical_key: $key}) SET s.active = $value")
                                .param("key", key)
                                .param("value", new);
                        self.client.run(q).await?;
                    }
                    SourceChange::Cadence { new, .. } => {
                        if let Some(hours) = new {
                            let q = query("MATCH (s:Source {canonical_key: $key}) SET s.cadence_hours = $value")
                                .param("key", key)
                                .param("value", hours as i64);
                            self.client.run(q).await?;
                        }
                    }
                }
                Ok(ApplyResult::Applied)
            }

            SystemEvent::SourceDeactivated { source_ids, .. } => {
                let ids: Vec<String> = source_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS id
                     MATCH (s:Source {id: id})
                     SET s.active = false",
                )
                .param("ids", ids);

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemEvent::SourceDeleted { canonical_key, .. } => {
                let q = query(
                    "MATCH (s:Source {canonical_key: $key})
                     DETACH DELETE s",
                )
                .param("key", canonical_key.as_str());

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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
                .param("ts", format_dt_from_stored(event));

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemEvent::PinsConsumed { pin_ids } => {
                if pin_ids.is_empty() {
                    return Ok(ApplyResult::NoOp);
                }
                let ids: Vec<String> = pin_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS pid
                     MATCH (p:Pin {id: pid})
                     DETACH DELETE p",
                )
                .param("ids", ids);
                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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
                .param("ts", format_dt_from_stored(event));

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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
                .param("ts", format_dt_from_stored(event))
                .param("canonical_key", source_canonical_key.unwrap_or_default());

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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
                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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
                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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
                .param("ts", format_dt_from_stored(event));
                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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
                    NodeType::Citation => return Ok(ApplyResult::NoOp),
                };
                let q = query(&format!(
                    "MATCH (n:{label} {{id: $id}})
                     SET n.investigated_at = datetime($ts)"
                ))
                .param("id", signal_id.to_string())
                .param("ts", format_dt(&investigated_at));
                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemEvent::ExhaustedRetriesPromoted { .. } => {
                let q = query(
                    "MATCH (n)
                     WHERE (n:Resource OR n:Gathering OR n:HelpRequest OR n:Announcement)
                       AND n.curiosity_investigated = 'failed'
                       AND n.curiosity_retry_count >= 3
                     SET n.curiosity_investigated = 'abandoned'",
                );
                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemEvent::ConcernLinkerOutcomeRecorded {
                signal_id,
                label,
                outcome,
                increment_retry,
            } => {
                let label = match label.as_str() {
                    "Gathering" | "Resource" | "HelpRequest" | "Announcement" => label.as_str(),
                    _ => return Ok(ApplyResult::NoOp),
                };
                let cypher = if increment_retry {
                    format!(
                        "MATCH (n:{label} {{id: $id}})
                         SET n.curiosity_investigated = $outcome,
                             n.curiosity_retry_count = coalesce(n.curiosity_retry_count, 0) + 1"
                    )
                } else {
                    format!(
                        "MATCH (n:{label} {{id: $id}})
                         SET n.curiosity_investigated = $outcome"
                    )
                };
                let q = query(&cypher)
                    .param("id", signal_id.to_string())
                    .param("outcome", outcome.as_str());
                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemEvent::GatheringScouted {
                concern_id,
                found_gatherings,
                scouted_at,
            } => {
                let q = query(
                    "MATCH (t:Concern {id: $id})
                     SET t.gravity_scouted_at = datetime($ts),
                         t.gravity_scout_miss_count = CASE
                             WHEN $found THEN 0
                             ELSE coalesce(t.gravity_scout_miss_count, 0) + 1
                         END",
                )
                .param("id", concern_id.to_string())
                .param("ts", format_dt(&scouted_at))
                .param("found", found_gatherings);
                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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
                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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
                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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

                // Re-point RESPONDS_TO edges
                let q = query(
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
                self.client.run(q).await?;

                // Re-point DRAWN_TO edges
                let q = query(
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
                self.client.run(q).await?;

                // Re-point PART_OF edges
                let q = query(
                    "MATCH (dup:Concern {id: $dup_id})-[r:PART_OF]->(s:Situation)
                     MATCH (survivor:Concern {id: $survivor_id})
                     WHERE NOT (survivor)-[:PART_OF]->(s)
                     CREATE (survivor)-[:PART_OF]->(s)
                     WITH r
                     DELETE r",
                )
                .param("dup_id", did.as_str())
                .param("survivor_id", sid.as_str());
                self.client.run(q).await?;

                // Bump corroboration count
                let q = query(
                    "MATCH (t:Concern {id: $survivor_id})
                     SET t.corroboration_count = coalesce(t.corroboration_count, 0) + 1",
                )
                .param("survivor_id", sid.as_str());
                self.client.run(q).await?;

                // Delete duplicate
                let q = query("MATCH (t:Concern {id: $dup_id}) DETACH DELETE t")
                    .param("dup_id", did.as_str());
                self.client.run(q).await?;

                Ok(ApplyResult::Applied)
            }

            // ---------------------------------------------------------
            // System curiosity
            // ---------------------------------------------------------
            SystemEvent::ExpansionQueryCollected { .. } => {
                debug!(seq = event.seq, "No-op (expansion query — informational)");
                Ok(ApplyResult::NoOp)
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
                if signals_produced > 0 {
                    let q = query(
                        "MATCH (s:Source {canonical_key: $key})
                         SET s.last_scraped = datetime($now),
                             s.last_produced_signal = datetime($now),
                             s.signals_produced = s.signals_produced + $count,
                             s.consecutive_empty_runs = 0,
                             s.scrape_count = coalesce(s.scrape_count, 0) + 1",
                    )
                    .param("key", canonical_key.as_str())
                    .param("now", now.as_str())
                    .param("count", signals_produced as i64);
                    self.client.run(q).await?;
                } else {
                    let q = query(
                        "MATCH (s:Source {canonical_key: $key})
                         SET s.last_scraped = datetime($now),
                             s.consecutive_empty_runs = s.consecutive_empty_runs + 1,
                             s.scrape_count = coalesce(s.scrape_count, 0) + 1",
                    )
                    .param("key", canonical_key.as_str())
                    .param("now", now.as_str());
                    self.client.run(q).await?;
                }
                Ok(ApplyResult::Applied)
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
                     SET s.sources_discovered = coalesce(s.sources_discovered, 0) + $count",
                )
                .param("key", canonical_key.as_str())
                .param("count", sources_discovered as i64);
                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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
                     WITH collect(DISTINCT sig.source_url) AS urls
                     UNWIND urls AS url
                     MATCH (src:Source {active: true})
                     WHERE src.url = url AND src.weight IS NOT NULL
                     SET src.weight = CASE WHEN src.weight * $factor > 5.0 THEN 5.0 ELSE src.weight * $factor END",
                )
                .param("headline", headline.as_str())
                .param("factor", factor);
                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
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
                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemEvent::CauseHeatComputed { scores } => {
                for score in &scores {
                    let q = query(&format!(
                        "MATCH (n:{} {{id: $id}}) SET n.cause_heat = $heat",
                        score.label
                    ))
                    .param("id", score.signal_id.to_string())
                    .param("heat", score.cause_heat);
                    self.client.run(q).await?;
                }
                Ok(ApplyResult::Applied)
            }

            SystemEvent::SignalDiversityComputed { metrics } => {
                if metrics.is_empty() {
                    return Ok(ApplyResult::NoOp);
                }
                // Group by label for efficient batch writes
                let mut by_label: std::collections::HashMap<String, Vec<&SignalDiversityScore>> =
                    std::collections::HashMap::new();
                for m in &metrics {
                    by_label.entry(m.label.clone()).or_default().push(m);
                }
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

                    self.client.run(q).await?;
                }
                Ok(ApplyResult::Applied)
            }

            SystemEvent::ActorStatsComputed { stats } => {
                if stats.is_empty() {
                    return Ok(ApplyResult::NoOp);
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

                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemEvent::SimilarityEdgesRebuilt { edges } => {
                // Delete all existing SIMILAR_TO edges
                let q = query(
                    "MATCH ()-[e:SIMILAR_TO]->() DELETE e",
                );
                self.client.run(q).await?;

                if !edges.is_empty() {
                    // UNWIND + MERGE new edges in batches
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
                        self.client.run(q).await?;
                    }
                }
                Ok(ApplyResult::Applied)
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
                self.client.run(q).await?;
                Ok(ApplyResult::Applied)
            }

        }
    }

    // -----------------------------------------------------------------------
    // Private helpers for typed correction handlers
    // -----------------------------------------------------------------------

    async fn set_str(&self, label: &str, id: uuid::Uuid, prop: &str, value: &str) -> Result<()> {
        let q = query(&format!(
            "MATCH (n:{label} {{id: $id}}) SET n.{prop} = $value"
        ))
        .param("id", id.to_string())
        .param("value", value);
        self.client.run(q).await?;
        Ok(())
    }

    async fn set_f64(&self, label: &str, id: uuid::Uuid, prop: &str, value: f64) -> Result<()> {
        let q = query(&format!(
            "MATCH (n:{label} {{id: $id}}) SET n.{prop} = $value"
        ))
        .param("id", id.to_string())
        .param("value", value);
        self.client.run(q).await?;
        Ok(())
    }

    async fn set_bool(&self, label: &str, id: uuid::Uuid, prop: &str, value: bool) -> Result<()> {
        let q = query(&format!(
            "MATCH (n:{label} {{id: $id}}) SET n.{prop} = $value"
        ))
        .param("id", id.to_string())
        .param("value", value);
        self.client.run(q).await?;
        Ok(())
    }

    async fn set_location(
        &self,
        label: &str,
        id: uuid::Uuid,
        loc: &Option<Location>,
    ) -> Result<()> {
        let (lat, lng) = location_lat_lng(loc);
        let name = location_name_str(loc);
        let address = location_address_str(loc);
        let q = query(&format!(
            "MATCH (n:{label} {{id: $id}}) SET n.lat = $lat, n.lng = $lng, n.location_name = $name, n.address = $address"
        ))
        .param("id", id.to_string())
        .param("lat", lat)
        .param("lng", lng)
        .param("name", name)
        .param("address", address);
        self.client.run(q).await?;
        Ok(())
    }

    async fn set_schedule(
        &self,
        label: &str,
        id: uuid::Uuid,
        schedule: &Option<Schedule>,
    ) -> Result<()> {
        let (starts_at, ends_at, rrule, all_day, timezone) = extract_schedule(schedule);
        let q = query(&format!(
            "MATCH (n:{label} {{id: $id}}) SET
             n.starts_at = CASE WHEN $starts_at = '' THEN null ELSE datetime($starts_at) END,
             n.ends_at = CASE WHEN $ends_at = '' THEN null ELSE datetime($ends_at) END,
             n.rrule = $rrule, n.all_day = $all_day, n.timezone = $timezone"
        ))
        .param("id", id.to_string())
        .param("starts_at", starts_at)
        .param("ends_at", ends_at)
        .param("rrule", rrule)
        .param("all_day", all_day)
        .param("timezone", timezone);
        self.client.run(q).await?;
        Ok(())
    }

    /// Replay events from seq_start in order. Returns the last seq applied.
    pub async fn replay_from(
        &self,
        store: &rootsignal_events::EventStore,
        seq_start: i64,
    ) -> Result<i64> {
        let batch_size = 1000;
        let mut cursor = seq_start;
        let mut last_applied = seq_start.saturating_sub(1);

        loop {
            let events = store.read_from(cursor, batch_size).await?;
            if events.is_empty() {
                break;
            }

            for event in &events {
                self.project(event).await?;
                last_applied = event.seq;
            }

            cursor = last_applied + 1;

            if events.len() < batch_size {
                break;
            }
        }

        Ok(last_applied)
    }

    /// Full rebuild: wipe graph, replay all facts from the beginning.
    pub async fn rebuild(&self, store: &rootsignal_events::EventStore) -> Result<i64> {
        self.client
            .run(query("MATCH (n) DETACH DELETE n"))
            .await?;

        self.replay_from(store, 1).await
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
fn format_dt_from_stored(event: &StoredEvent) -> String {
    format_dt(&event.ts)
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

fn location_address_str(loc: &Option<Location>) -> String {
    loc.as_ref()
        .and_then(|l| l.address.clone())
        .unwrap_or_default()
}

fn extract_schedule(schedule: &Option<Schedule>) -> (String, String, String, bool, String) {
    match schedule {
        Some(s) => (
            s.starts_at.map(|dt| format_dt(&dt)).unwrap_or_default(),
            s.ends_at.map(|dt| format_dt(&dt)).unwrap_or_default(),
            s.rrule.clone().unwrap_or_default(),
            s.all_day,
            s.timezone.clone().unwrap_or_default(),
        ),
        None => (
            String::new(),
            String::new(),
            String::new(),
            false,
            String::new(),
        ),
    }
}

/// Build the common MERGE/ON CREATE SET query for all 5 discovery event types.
/// No sensitivity or implied_queries — those come from separate SystemEvent events.
fn build_discovery_query(
    label: &str,
    type_specific_set: &str,
    id: uuid::Uuid,
    title: &str,
    summary: &str,
    confidence: f32,
    source_url: &str,
    extracted_at: &DateTime<Utc>,
    published_at: Option<DateTime<Utc>>,
    location: &Option<Location>,
    event: &StoredEvent,
) -> neo4rs::Query {
    let (lat, lng) = location_lat_lng(location);
    let loc_name = location_name_str(location);
    let loc_address = location_address_str(location);
    let actor_str = event.actor.as_deref().unwrap_or("").to_string();
    let run_id = event.run_id.as_deref().unwrap_or("").to_string();

    let cypher = format!(
        "MERGE (n:{label} {{id: $id}})
         ON CREATE SET
             n.title = $title,
             n.summary = $summary,
             n.confidence = $confidence,
             n.source_url = $source_url,
             n.extracted_at = datetime($extracted_at),
             n.last_confirmed_active = datetime($extracted_at),
             n.published_at = CASE WHEN $published_at = '' THEN null ELSE datetime($published_at) END,
             n.location_name = $location_name,
             n.address = $address,
             n.lat = $lat,
             n.lng = $lng,
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
        .param("source_url", source_url)
        .param("extracted_at", format_dt(extracted_at))
        .param(
            "published_at",
            published_at.map(|dt| format_dt(&dt)).unwrap_or_default(),
        )
        .param("location_name", loc_name)
        .param("address", loc_address)
        .param("lat", lat)
        .param("lng", lng)
        .param("created_by", actor_str)
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

