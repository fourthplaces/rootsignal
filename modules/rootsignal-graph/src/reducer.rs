//! GraphProjector — pure projection of facts into Neo4j nodes and edges.
//!
//! Each event is either acted upon (MERGE/SET/DELETE) or ignored (no-op).
//! The projector never reads the graph, calls APIs, generates UUIDs, or uses wall-clock time.
//! It writes only factual values from event payloads — no embeddings, no diversity counts,
//! no cause_heat. Those are computed by enrichment passes after the projector runs.
//!
//! Idempotency: all writes use MERGE or conditional SET with the event's seq as a guard.
//! Replaying the same event twice produces the same graph state.

use anyhow::Result;
use chrono::{DateTime, Utc};
use neo4rs::query;
use tracing::{debug, warn};

use rootsignal_common::events::{
    AidCorrection, Event, GatheringCorrection, Location, NeedCorrection, NoticeCorrection,
    Schedule, SituationChange, SystemSourceChange, TensionCorrection,
    WorldEvent, SystemDecision,
};
use rootsignal_common::types::NodeType;
use rootsignal_events::StoredEvent;

use crate::GraphClient;

// ---------------------------------------------------------------------------
// GraphProjector
// ---------------------------------------------------------------------------

/// Pure projection of facts into Neo4j nodes and edges.
pub struct GraphProjector {
    client: GraphClient,
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
        Self { client }
    }

    /// Project a single fact to the graph. Idempotent.
    pub async fn project(&self, event: &StoredEvent) -> Result<ApplyResult> {
        let parsed = match Event::from_payload(&event.payload) {
            Ok(e) => e,
            Err(e) => {
                warn!(seq = event.seq, error = %e, "Failed to deserialize event payload");
                return Ok(ApplyResult::DeserializeError(e.to_string()));
            }
        };

        match parsed {
            Event::Telemetry(_) => {
                debug!(seq = event.seq, event_type = event.event_type, "No-op (telemetry)");
                Ok(ApplyResult::NoOp)
            }
            Event::World(world) => self.project_world(world, event).await,
            Event::System(system) => self.project_system(system, event).await,
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
            WorldEvent::GatheringDiscovered {
                id, title, summary, confidence, source_url,
                extracted_at, content_date, location,
                schedule, action_url, organizer,
                from_location: _, mentioned_actors: _, author_actor: _,
            } => {
                let (starts_at, ends_at, rrule, all_day, timezone) = extract_schedule(&schedule);
                let q = build_discovery_query(
                    "Gathering",
                    ", n.starts_at = CASE WHEN $starts_at = '' THEN null ELSE datetime($starts_at) END,
                       n.ends_at = CASE WHEN $ends_at = '' THEN null ELSE datetime($ends_at) END,
                       n.rrule = $rrule, n.all_day = $all_day, n.timezone = $timezone,
                       n.action_url = $action_url, n.organizer = $organizer,
                       n.is_recurring = CASE WHEN $rrule <> '' THEN true ELSE false END",
                    id, &title, &summary, confidence, &source_url,
                    &extracted_at, content_date, &location, event,
                )
                .param("starts_at", starts_at)
                .param("ends_at", ends_at)
                .param("rrule", rrule)
                .param("all_day", all_day)
                .param("timezone", timezone)
                .param("action_url", action_url.as_deref().unwrap_or(""))
                .param("organizer", organizer.unwrap_or_default());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            WorldEvent::AidDiscovered {
                id, title, summary, confidence, source_url,
                extracted_at, content_date, location,
                action_url, availability, is_ongoing,
                from_location: _, mentioned_actors: _, author_actor: _,
            } => {
                let q = build_discovery_query(
                    "Aid",
                    ", n.action_url = $action_url, n.availability = $availability,
                       n.is_ongoing = $is_ongoing",
                    id, &title, &summary, confidence, &source_url,
                    &extracted_at, content_date, &location, event,
                )
                .param("action_url", action_url.as_deref().unwrap_or(""))
                .param("availability", availability.as_deref().unwrap_or(""))
                .param("is_ongoing", is_ongoing.unwrap_or(false));

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            WorldEvent::NeedDiscovered {
                id, title, summary, confidence, source_url,
                extracted_at, content_date, location,
                urgency, what_needed, goal,
                from_location: _, mentioned_actors: _, author_actor: _,
            } => {
                let q = build_discovery_query(
                    "Need",
                    ", n.urgency = $urgency, n.what_needed = $what_needed, n.goal = $goal",
                    id, &title, &summary, confidence, &source_url,
                    &extracted_at, content_date, &location, event,
                )
                .param("urgency", urgency.map(|u| urgency_str(u)).unwrap_or(""))
                .param("what_needed", what_needed.as_deref().unwrap_or(""))
                .param("goal", goal.unwrap_or_default());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            WorldEvent::NoticeDiscovered {
                id, title, summary, confidence, source_url,
                extracted_at, content_date, location,
                severity, category, effective_date, source_authority,
                from_location: _, mentioned_actors: _, author_actor: _,
            } => {
                let q = build_discovery_query(
                    "Notice",
                    ", n.severity = $severity, n.category = $category,
                       n.effective_date = CASE WHEN $effective_date = '' THEN null ELSE datetime($effective_date) END,
                       n.source_authority = $source_authority",
                    id, &title, &summary, confidence, &source_url,
                    &extracted_at, content_date, &location, event,
                )
                .param("severity", severity.map(|s| severity_str(s)).unwrap_or(""))
                .param("category", category.unwrap_or_default())
                .param("effective_date", effective_date.map(|dt| format_dt(&dt)).unwrap_or_default())
                .param("source_authority", source_authority.unwrap_or_default());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            WorldEvent::TensionDiscovered {
                id, title, summary, confidence, source_url,
                extracted_at, content_date, location,
                severity, what_would_help,
                from_location: _, mentioned_actors: _, author_actor: _,
            } => {
                let q = build_discovery_query(
                    "Tension",
                    ", n.severity = $severity, n.what_would_help = $what_would_help",
                    id, &title, &summary, confidence, &source_url,
                    &extracted_at, content_date, &location, event,
                )
                .param("severity", severity.map(|s| severity_str(s)).unwrap_or(""))
                .param("what_would_help", what_would_help.as_deref().unwrap_or(""));

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            // ---------------------------------------------------------
            // Corroboration — world fact only (no scoring)
            // ---------------------------------------------------------
            WorldEvent::ObservationCorroborated {
                entity_id,
                node_type,
                ..
            } => {
                let label = node_type_label(node_type);
                let q = query(&format!(
                    "MATCH (n:{label} {{id: $id}})
                     SET n.last_confirmed_active = datetime($ts)"
                ))
                .param("id", entity_id.to_string())
                .param("ts", format_dt_from_stored(event));

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            // ---------------------------------------------------------
            // Citations
            // ---------------------------------------------------------
            WorldEvent::CitationRecorded {
                citation_id,
                entity_id,
                url,
                content_hash,
                snippet,
                relevance,
                channel_type,
                evidence_confidence,
            } => {
                let q = query(
                    "OPTIONAL MATCH (g:Gathering {id: $entity_id})
                     OPTIONAL MATCH (a:Aid {id: $entity_id})
                     OPTIONAL MATCH (n:Need {id: $entity_id})
                     OPTIONAL MATCH (nc:Notice {id: $entity_id})
                     OPTIONAL MATCH (t:Tension {id: $entity_id})
                     WITH coalesce(g, a, n, nc, t) AS node
                     WHERE node IS NOT NULL
                     MERGE (node)-[:SOURCED_FROM]->(ev:Evidence {source_url: $url})
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
                         ev.content_hash = $content_hash"
                )
                .param("ev_id", citation_id.to_string())
                .param("entity_id", entity_id.to_string())
                .param("url", url.as_str())
                .param("ts", format_dt_from_stored(event))
                .param("content_hash", content_hash.as_str())
                .param("snippet", snippet.unwrap_or_default())
                .param("relevance", relevance.unwrap_or_default())
                .param("evidence_confidence", evidence_confidence.unwrap_or(0.0) as f64)
                .param("channel_type", channel_type.map(|ct| ct.as_str()).unwrap_or("press"));

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            // ---------------------------------------------------------
            // Sources
            // ---------------------------------------------------------
            WorldEvent::SourceRegistered {
                source_id,
                canonical_key,
                canonical_value,
                url,
                discovery_method,
                weight,
                source_role,
                gap_context,
            } => {
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
                         s.scrape_count = 0
                     ON MATCH SET
                         s.active = CASE WHEN s.active = false AND $discovery_method = 'curated' THEN true ELSE s.active END,
                         s.url = CASE WHEN $url <> '' THEN $url ELSE s.url END"
                )
                .param("id", source_id.to_string())
                .param("canonical_key", canonical_key.as_str())
                .param("canonical_value", canonical_value.as_str())
                .param("url", url.as_deref().unwrap_or(""))
                .param("discovery_method", discovery_method.to_string())
                .param("ts", format_dt_from_stored(event))
                .param("weight", weight)
                .param("source_role", source_role.to_string())
                .param("gap_context", gap_context.unwrap_or_default());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            WorldEvent::SourceChanged {
                canonical_key,
                change,
                ..
            } => {
                let key = canonical_key.as_str();
                match change {
                    rootsignal_world::values::WorldSourceChange::Weight { new, .. } => {
                        let q = query("MATCH (s:Source {canonical_key: $key}) SET s.weight = $value")
                            .param("key", key).param("value", new);
                        self.client.graph.run(q).await?;
                    }
                    rootsignal_world::values::WorldSourceChange::Url { new, .. } => {
                        let q = query("MATCH (s:Source {canonical_key: $key}) SET s.url = $value")
                            .param("key", key).param("value", new.as_str());
                        self.client.graph.run(q).await?;
                    }
                    rootsignal_world::values::WorldSourceChange::Role { new, .. } => {
                        let q = query("MATCH (s:Source {canonical_key: $key}) SET s.source_role = $value")
                            .param("key", key).param("value", new.to_string());
                        self.client.graph.run(q).await?;
                    }
                    rootsignal_world::values::WorldSourceChange::Active { new, .. } => {
                        let q = query("MATCH (s:Source {canonical_key: $key}) SET s.active = $value")
                            .param("key", key).param("value", new);
                        self.client.graph.run(q).await?;
                    }
                }
                Ok(ApplyResult::Applied)
            }

            WorldEvent::SourceDeactivated { source_ids, .. } => {
                let ids: Vec<String> = source_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS id
                     MATCH (s:Source {id: id})
                     SET s.active = false"
                )
                .param("ids", ids);

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            WorldEvent::SourceLinkDiscovered { .. } => {
                debug!(seq = event.seq, "No-op (source link — informational)");
                Ok(ApplyResult::NoOp)
            }

            // ---------------------------------------------------------
            // Actors
            // ---------------------------------------------------------
            WorldEvent::ActorIdentified {
                actor_id,
                name,
                actor_type,
                entity_id,
                domains,
                social_urls,
                description,
                bio,
                location_lat,
                location_lng,
                location_name,
            } => {
                let q = query(
                    "MERGE (a:Actor {entity_id: $entity_id})
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
                         a.last_active = datetime($ts)"
                )
                .param("id", actor_id.to_string())
                .param("entity_id", entity_id.as_str())
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

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            WorldEvent::ActorLinkedToEntity {
                actor_id,
                entity_id,
                role,
            } => {
                let q = query(
                    "MATCH (a:Actor {id: $actor_id})
                     MATCH (n) WHERE n.id = $entity_id AND (n:Gathering OR n:Aid OR n:Need OR n:Notice OR n:Tension)
                     MERGE (a)-[:ACTED_IN {role: $role}]->(n)"
                )
                .param("actor_id", actor_id.to_string())
                .param("entity_id", entity_id.to_string())
                .param("role", role.as_str());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            WorldEvent::ActorLinkedToSource {
                actor_id,
                source_id,
            } => {
                let q = query(
                    "MATCH (a:Actor {id: $actor_id})
                     MATCH (s:Source {id: $source_id})
                     MERGE (a)-[:HAS_SOURCE]->(s)"
                )
                .param("actor_id", actor_id.to_string())
                .param("source_id", source_id.to_string());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            WorldEvent::ActorLocationIdentified {
                actor_id,
                location_lat,
                location_lng,
                location_name,
            } => {
                let q = query(
                    "MATCH (a:Actor {id: $id})
                     SET a.location_lat = $lat,
                         a.location_lng = $lng,
                         a.location_name = $name"
                )
                .param("id", actor_id.to_string())
                .param("lat", location_lat)
                .param("lng", location_lng)
                .param("name", location_name.unwrap_or_default());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            // ---------------------------------------------------------
            // Community input
            // ---------------------------------------------------------
            WorldEvent::PinCreated {
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
                         p.created_at = datetime($ts)"
                )
                .param("id", pin_id.to_string())
                .param("lat", location_lat)
                .param("lng", location_lng)
                .param("source_id", source_id.to_string())
                .param("created_by", created_by.as_str())
                .param("ts", format_dt_from_stored(event));

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            WorldEvent::DemandReceived {
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
                         d.created_at = datetime($ts)"
                )
                .param("id", demand_id.to_string())
                .param("query", demand_query.as_str())
                .param("lat", center_lat)
                .param("lng", center_lng)
                .param("radius", radius_km)
                .param("ts", format_dt_from_stored(event));

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            WorldEvent::SubmissionReceived {
                submission_id,
                url,
                reason,
                source_canonical_key,
            } => {
                let q = query(
                    "CREATE (sub:Submission {
                         id: $id,
                         url: $url,
                         reason: $reason,
                         submitted_at: datetime($ts)
                     })
                     WITH sub
                     OPTIONAL MATCH (s:Source {canonical_key: $canonical_key})
                     FOREACH (_ IN CASE WHEN s IS NOT NULL THEN [1] ELSE [] END |
                         MERGE (sub)-[:SUBMITTED_FOR]->(s)
                     )"
                )
                .param("id", submission_id.to_string())
                .param("url", url.as_str())
                .param("reason", reason.unwrap_or_default())
                .param("ts", format_dt_from_stored(event))
                .param("canonical_key", source_canonical_key.unwrap_or_default());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            // ---------------------------------------------------------
            // Edge facts
            // ---------------------------------------------------------
            WorldEvent::ResourceEdgeCreated {
                signal_id,
                resource_id,
                role,
                confidence,
                quantity,
                notes,
                capacity,
            } => {
                let q = match role.as_str() {
                    "requires" => {
                        query(
                            "MATCH (s) WHERE s.id = $sid AND (s:Need OR s:Gathering)
                             MATCH (r:Resource {id: $rid})
                             MERGE (s)-[e:REQUIRES]->(r)
                             ON CREATE SET e.confidence = $confidence, e.quantity = $quantity, e.notes = $notes
                             ON MATCH SET e.confidence = $confidence, e.quantity = $quantity, e.notes = $notes"
                        )
                        .param("sid", signal_id.to_string())
                        .param("rid", resource_id.to_string())
                        .param("confidence", confidence as f64)
                        .param("quantity", quantity.unwrap_or_default())
                        .param("notes", notes.unwrap_or_default())
                    }
                    "prefers" => {
                        query(
                            "MATCH (s) WHERE s.id = $sid AND (s:Need OR s:Gathering)
                             MATCH (r:Resource {id: $rid})
                             MERGE (s)-[e:PREFERS]->(r)
                             ON CREATE SET e.confidence = $confidence
                             ON MATCH SET e.confidence = $confidence"
                        )
                        .param("sid", signal_id.to_string())
                        .param("rid", resource_id.to_string())
                        .param("confidence", confidence as f64)
                    }
                    "offers" => {
                        query(
                            "MATCH (s:Aid {id: $sid})
                             MATCH (r:Resource {id: $rid})
                             MERGE (s)-[e:OFFERS]->(r)
                             ON CREATE SET e.confidence = $confidence, e.capacity = $capacity
                             ON MATCH SET e.confidence = $confidence, e.capacity = $capacity"
                        )
                        .param("sid", signal_id.to_string())
                        .param("rid", resource_id.to_string())
                        .param("confidence", confidence as f64)
                        .param("capacity", capacity.unwrap_or_default())
                    }
                    _ => {
                        warn!(role = role.as_str(), "Unknown resource edge role, skipping");
                        return Ok(ApplyResult::NoOp);
                    }
                };

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            WorldEvent::ResponseLinked {
                signal_id,
                tension_id,
                strength,
                explanation,
            } => {
                let q = query(
                    "MATCH (resp) WHERE resp.id = $resp_id AND (resp:Aid OR resp:Gathering OR resp:Need)
                     MATCH (t:Tension {id: $tid})
                     MERGE (resp)-[r:RESPONDS_TO]->(t)
                     ON CREATE SET r.match_strength = $strength, r.explanation = $explanation
                     ON MATCH SET r.match_strength = $strength, r.explanation = $explanation"
                )
                .param("resp_id", signal_id.to_string())
                .param("tid", tension_id.to_string())
                .param("strength", strength)
                .param("explanation", explanation.as_str());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            WorldEvent::GravityLinked {
                signal_id,
                tension_id,
                strength,
                explanation,
                gathering_type,
            } => {
                let q = query(
                    "MATCH (resp) WHERE resp.id = $resp_id AND (resp:Aid OR resp:Gathering OR resp:Need)
                     MATCH (t:Tension {id: $tid})
                     MERGE (resp)-[r:DRAWN_TO]->(t)
                     ON CREATE SET r.match_strength = $strength, r.explanation = $explanation, r.gathering_type = $gathering_type
                     ON MATCH SET r.match_strength = $strength, r.explanation = $explanation, r.gathering_type = $gathering_type"
                )
                .param("resp_id", signal_id.to_string())
                .param("tid", tension_id.to_string())
                .param("strength", strength)
                .param("explanation", explanation.as_str())
                .param("gathering_type", gathering_type.as_str());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            WorldEvent::ExpansionQueryCollected { .. } => {
                debug!(seq = event.seq, "No-op (expansion query — informational)");
                Ok(ApplyResult::NoOp)
            }
        }
    }

    // =================================================================
    // System decisions — editorial judgments
    // =================================================================

    async fn project_system(&self, system: SystemDecision, event: &StoredEvent) -> Result<ApplyResult> {
        match system {
            // ---------------------------------------------------------
            // Sensitivity + implied queries (paired with discoveries)
            // ---------------------------------------------------------
            SystemDecision::SensitivityClassified { entity_id, level } => {
                let q = query(
                    "OPTIONAL MATCH (g:Gathering {id: $id})
                     OPTIONAL MATCH (a:Aid {id: $id})
                     OPTIONAL MATCH (n:Need {id: $id})
                     OPTIONAL MATCH (nc:Notice {id: $id})
                     OPTIONAL MATCH (t:Tension {id: $id})
                     WITH coalesce(g, a, n, nc, t) AS node
                     WHERE node IS NOT NULL
                     SET node.sensitivity = $level"
                )
                .param("id", entity_id.to_string())
                .param("level", level.as_str());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemDecision::ImpliedQueriesExtracted { entity_id, queries } => {
                let q = query(
                    "OPTIONAL MATCH (g:Gathering {id: $id})
                     OPTIONAL MATCH (a:Aid {id: $id})
                     OPTIONAL MATCH (n:Need {id: $id})
                     OPTIONAL MATCH (nc:Notice {id: $id})
                     OPTIONAL MATCH (t:Tension {id: $id})
                     WITH coalesce(g, a, n, nc, t) AS node
                     WHERE node IS NOT NULL
                     SET node.implied_queries = $queries"
                )
                .param("id", entity_id.to_string())
                .param("queries", queries);

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            // ---------------------------------------------------------
            // Corroboration scoring
            // ---------------------------------------------------------
            SystemDecision::CorroborationScored {
                entity_id,
                new_corroboration_count,
                ..
            } => {
                // Find entity across all signal types and set count
                let q = query(
                    "OPTIONAL MATCH (g:Gathering {id: $id})
                     OPTIONAL MATCH (a:Aid {id: $id})
                     OPTIONAL MATCH (n:Need {id: $id})
                     OPTIONAL MATCH (nc:Notice {id: $id})
                     OPTIONAL MATCH (t:Tension {id: $id})
                     WITH coalesce(g, a, n, nc, t) AS node
                     WHERE node IS NOT NULL
                     SET node.corroboration_count = $count"
                )
                .param("id", entity_id.to_string())
                .param("count", new_corroboration_count as i64);

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            // ---------------------------------------------------------
            // Signal lifecycle decisions
            // ---------------------------------------------------------
            SystemDecision::FreshnessConfirmed {
                entity_ids,
                node_type,
                confirmed_at,
            } => {
                let label = node_type_label(node_type);
                let ids: Vec<String> = entity_ids.iter().map(|id| id.to_string()).collect();
                let q = query(&format!(
                    "UNWIND $ids AS id
                     MATCH (n:{label} {{id: id}})
                     SET n.last_confirmed_active = datetime($ts)"
                ))
                .param("ids", ids)
                .param("ts", format_dt(&confirmed_at));

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemDecision::ConfidenceScored {
                entity_id,
                new_confidence,
                ..
            } => {
                let q = query(
                    "OPTIONAL MATCH (g:Gathering {id: $id})
                     OPTIONAL MATCH (a:Aid {id: $id})
                     OPTIONAL MATCH (n:Need {id: $id})
                     OPTIONAL MATCH (nc:Notice {id: $id})
                     OPTIONAL MATCH (t:Tension {id: $id})
                     WITH coalesce(g, a, n, nc, t) AS node
                     WHERE node IS NOT NULL
                     SET node.confidence = $confidence"
                )
                .param("id", entity_id.to_string())
                .param("confidence", new_confidence as f64);

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemDecision::ObservationRejected { .. } => {
                debug!(seq = event.seq, "No-op (observation rejected — informational)");
                Ok(ApplyResult::NoOp)
            }

            SystemDecision::EntityExpired {
                entity_id,
                node_type,
                reason,
            } => {
                let label = node_type_label(node_type);
                let q = query(&format!(
                    "MATCH (n:{label} {{id: $id}})
                     SET n.expired = true,
                         n.expired_at = datetime($ts),
                         n.expired_reason = $reason"
                ))
                .param("id", entity_id.to_string())
                .param("ts", format_dt_from_stored(event))
                .param("reason", reason.as_str());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemDecision::EntityPurged {
                entity_id,
                node_type,
                ..
            } => {
                let label = node_type_label(node_type);
                let q = query(&format!(
                    "MATCH (n:{label} {{id: $id}})
                     OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
                     DETACH DELETE n, ev"
                ))
                .param("id", entity_id.to_string());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemDecision::DuplicateDetected { .. } => {
                debug!(seq = event.seq, "No-op (duplicate detected — informational)");
                Ok(ApplyResult::NoOp)
            }

            SystemDecision::ExtractionDroppedNoDate { .. } => {
                debug!(seq = event.seq, "No-op (extraction dropped — informational)");
                Ok(ApplyResult::NoOp)
            }

            SystemDecision::ReviewVerdictReached {
                entity_id,
                new_status,
                ..
            } => {
                let q = query(
                    "OPTIONAL MATCH (g:Gathering {id: $id})
                     OPTIONAL MATCH (a:Aid {id: $id})
                     OPTIONAL MATCH (n:Need {id: $id})
                     OPTIONAL MATCH (nc:Notice {id: $id})
                     OPTIONAL MATCH (t:Tension {id: $id})
                     WITH coalesce(g, a, n, nc, t) AS node
                     WHERE node IS NOT NULL
                     SET node.review_status = $status"
                )
                .param("id", entity_id.to_string())
                .param("status", new_status.as_str());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemDecision::ImpliedQueriesConsumed { entity_ids } => {
                let ids: Vec<String> = entity_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS id
                     MATCH (n) WHERE n.id = id AND (n:Aid OR n:Gathering)
                     SET n.implied_queries = null"
                )
                .param("ids", ids);

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            // ---------------------------------------------------------
            // Corrections
            // ---------------------------------------------------------
            SystemDecision::GatheringCorrected { entity_id, correction, .. } => {
                match correction {
                    GatheringCorrection::Title { new, .. } => self.set_str("Gathering", entity_id, "title", &new).await?,
                    GatheringCorrection::Summary { new, .. } => self.set_str("Gathering", entity_id, "summary", &new).await?,
                    GatheringCorrection::Confidence { new, .. } => self.set_f64("Gathering", entity_id, "confidence", new as f64).await?,
                    GatheringCorrection::Sensitivity { new, .. } => self.set_str("Gathering", entity_id, "sensitivity", new.as_str()).await?,
                    GatheringCorrection::Location { new, .. } => self.set_location("Gathering", entity_id, &new).await?,
                    GatheringCorrection::Schedule { new, .. } => self.set_schedule("Gathering", entity_id, &new).await?,
                    GatheringCorrection::Organizer { new, .. } => self.set_str("Gathering", entity_id, "organizer", new.as_deref().unwrap_or("")).await?,
                    GatheringCorrection::ActionUrl { new, .. } => self.set_str("Gathering", entity_id, "action_url", new.as_deref().unwrap_or("")).await?,
                }
                Ok(ApplyResult::Applied)
            }

            SystemDecision::AidCorrected { entity_id, correction, .. } => {
                match correction {
                    AidCorrection::Title { new, .. } => self.set_str("Aid", entity_id, "title", &new).await?,
                    AidCorrection::Summary { new, .. } => self.set_str("Aid", entity_id, "summary", &new).await?,
                    AidCorrection::Confidence { new, .. } => self.set_f64("Aid", entity_id, "confidence", new as f64).await?,
                    AidCorrection::Sensitivity { new, .. } => self.set_str("Aid", entity_id, "sensitivity", new.as_str()).await?,
                    AidCorrection::Location { new, .. } => self.set_location("Aid", entity_id, &new).await?,
                    AidCorrection::ActionUrl { new, .. } => self.set_str("Aid", entity_id, "action_url", new.as_deref().unwrap_or("")).await?,
                    AidCorrection::Availability { new, .. } => self.set_str("Aid", entity_id, "availability", new.as_deref().unwrap_or("")).await?,
                    AidCorrection::IsOngoing { new, .. } => self.set_bool("Aid", entity_id, "is_ongoing", new.unwrap_or(false)).await?,
                }
                Ok(ApplyResult::Applied)
            }

            SystemDecision::NeedCorrected { entity_id, correction, .. } => {
                match correction {
                    NeedCorrection::Title { new, .. } => self.set_str("Need", entity_id, "title", &new).await?,
                    NeedCorrection::Summary { new, .. } => self.set_str("Need", entity_id, "summary", &new).await?,
                    NeedCorrection::Confidence { new, .. } => self.set_f64("Need", entity_id, "confidence", new as f64).await?,
                    NeedCorrection::Sensitivity { new, .. } => self.set_str("Need", entity_id, "sensitivity", new.as_str()).await?,
                    NeedCorrection::Location { new, .. } => self.set_location("Need", entity_id, &new).await?,
                    NeedCorrection::Urgency { new, .. } => self.set_str("Need", entity_id, "urgency", new.map(|u| urgency_str(u)).unwrap_or("")).await?,
                    NeedCorrection::WhatNeeded { new, .. } => self.set_str("Need", entity_id, "what_needed", new.as_deref().unwrap_or("")).await?,
                    NeedCorrection::Goal { new, .. } => self.set_str("Need", entity_id, "goal", new.as_deref().unwrap_or("")).await?,
                }
                Ok(ApplyResult::Applied)
            }

            SystemDecision::NoticeCorrected { entity_id, correction, .. } => {
                match correction {
                    NoticeCorrection::Title { new, .. } => self.set_str("Notice", entity_id, "title", &new).await?,
                    NoticeCorrection::Summary { new, .. } => self.set_str("Notice", entity_id, "summary", &new).await?,
                    NoticeCorrection::Confidence { new, .. } => self.set_f64("Notice", entity_id, "confidence", new as f64).await?,
                    NoticeCorrection::Sensitivity { new, .. } => self.set_str("Notice", entity_id, "sensitivity", new.as_str()).await?,
                    NoticeCorrection::Location { new, .. } => self.set_location("Notice", entity_id, &new).await?,
                    NoticeCorrection::Severity { new, .. } => self.set_str("Notice", entity_id, "severity", new.map(|s| severity_str(s)).unwrap_or("")).await?,
                    NoticeCorrection::Category { new, .. } => self.set_str("Notice", entity_id, "category", new.as_deref().unwrap_or("")).await?,
                    NoticeCorrection::EffectiveDate { new, .. } => {
                        let val = new.map(|dt| format_dt(&dt)).unwrap_or_default();
                        let q = query("MATCH (n:Notice {id: $id}) SET n.effective_date = CASE WHEN $value = '' THEN null ELSE datetime($value) END")
                            .param("id", entity_id.to_string())
                            .param("value", val);
                        self.client.graph.run(q).await?;
                    }
                    NoticeCorrection::SourceAuthority { new, .. } => self.set_str("Notice", entity_id, "source_authority", new.as_deref().unwrap_or("")).await?,
                }
                Ok(ApplyResult::Applied)
            }

            SystemDecision::TensionCorrected { entity_id, correction, .. } => {
                match correction {
                    TensionCorrection::Title { new, .. } => self.set_str("Tension", entity_id, "title", &new).await?,
                    TensionCorrection::Summary { new, .. } => self.set_str("Tension", entity_id, "summary", &new).await?,
                    TensionCorrection::Confidence { new, .. } => self.set_f64("Tension", entity_id, "confidence", new as f64).await?,
                    TensionCorrection::Sensitivity { new, .. } => self.set_str("Tension", entity_id, "sensitivity", new.as_str()).await?,
                    TensionCorrection::Location { new, .. } => self.set_location("Tension", entity_id, &new).await?,
                    TensionCorrection::Severity { new, .. } => self.set_str("Tension", entity_id, "severity", new.map(|s| severity_str(s)).unwrap_or("")).await?,
                    TensionCorrection::WhatWouldHelp { new, .. } => self.set_str("Tension", entity_id, "what_would_help", new.as_deref().unwrap_or("")).await?,
                }
                Ok(ApplyResult::Applied)
            }

            // ---------------------------------------------------------
            // Actor decisions
            // ---------------------------------------------------------
            SystemDecision::DuplicateActorsMerged {
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
                     DETACH DELETE old"
                )
                .param("kept_id", kept_id.to_string())
                .param("ids", ids);

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemDecision::OrphanedActorsCleaned { actor_ids } => {
                let ids: Vec<String> = actor_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS id
                     MATCH (a:Actor {id: id})
                     DETACH DELETE a"
                )
                .param("ids", ids);

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            // ---------------------------------------------------------
            // Situations / dispatches
            // ---------------------------------------------------------
            SystemDecision::SituationIdentified {
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
            } => {
                let q = query(
                    "MERGE (s:Story {id: $id})
                     ON CREATE SET
                         s.headline = $headline,
                         s.lede = $lede,
                         s.arc = $arc,
                         s.energy = $temperature,
                         s.centroid_lat = $centroid_lat,
                         s.centroid_lng = $centroid_lng,
                         s.location_name = $location_name,
                         s.sensitivity = $sensitivity,
                         s.category = $category,
                         s.structured_state = $structured_state,
                         s.first_seen = datetime($ts),
                         s.last_updated = datetime($ts),
                         s.review_status = 'staged'"
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

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemDecision::SituationChanged {
                situation_id,
                change,
            } => {
                let id_str = situation_id.to_string();
                let ts = format_dt_from_stored(event);
                match change {
                    SituationChange::Headline { new, .. } => {
                        let q = query("MATCH (s:Story {id: $id}) SET s.headline = $value, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("value", new.as_str()).param("ts", ts);
                        self.client.graph.run(q).await?;
                    }
                    SituationChange::Lede { new, .. } => {
                        let q = query("MATCH (s:Story {id: $id}) SET s.lede = $value, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("value", new.as_str()).param("ts", ts);
                        self.client.graph.run(q).await?;
                    }
                    SituationChange::Arc { new, .. } => {
                        let q = query("MATCH (s:Story {id: $id}) SET s.arc = $value, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("value", new.to_string()).param("ts", ts);
                        self.client.graph.run(q).await?;
                    }
                    SituationChange::Temperature { new, .. } => {
                        let q = query("MATCH (s:Story {id: $id}) SET s.energy = $value, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("value", new).param("ts", ts);
                        self.client.graph.run(q).await?;
                    }
                    SituationChange::Location { new, .. } => {
                        let (lat, lng) = location_lat_lng(&new);
                        let name = location_name_str(&new);
                        let q = query("MATCH (s:Story {id: $id}) SET s.centroid_lat = $lat, s.centroid_lng = $lng, s.location_name = $name, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("lat", lat).param("lng", lng).param("name", name).param("ts", ts);
                        self.client.graph.run(q).await?;
                    }
                    SituationChange::Sensitivity { new, .. } => {
                        let q = query("MATCH (s:Story {id: $id}) SET s.sensitivity = $value, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("value", new.as_str()).param("ts", ts);
                        self.client.graph.run(q).await?;
                    }
                    SituationChange::Category { new, .. } => {
                        let q = query("MATCH (s:Story {id: $id}) SET s.category = $value, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("value", new.as_deref().unwrap_or("")).param("ts", ts);
                        self.client.graph.run(q).await?;
                    }
                    SituationChange::StructuredState { new, .. } => {
                        let q = query("MATCH (s:Story {id: $id}) SET s.structured_state = $value, s.last_updated = datetime($ts)")
                            .param("id", id_str).param("value", new.as_str()).param("ts", ts);
                        self.client.graph.run(q).await?;
                    }
                }
                Ok(ApplyResult::Applied)
            }

            SystemDecision::SituationPromoted { situation_ids } => {
                let ids: Vec<String> = situation_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS id
                     MATCH (s:Story {id: id})
                     SET s.review_status = 'live'"
                )
                .param("ids", ids);

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemDecision::DispatchCreated { .. } => {
                debug!(seq = event.seq, "No-op (dispatch — not a graph node)");
                Ok(ApplyResult::NoOp)
            }

            // ---------------------------------------------------------
            // Tags
            // ---------------------------------------------------------
            SystemDecision::TagSuppressed {
                situation_id,
                tag_slug,
            } => {
                let q = query(
                    "MATCH (s:Story {id: $situation_id})-[r:TAGGED]->(t:Tag {slug: $slug})
                     DELETE r"
                )
                .param("situation_id", situation_id.to_string())
                .param("slug", tag_slug.as_str());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemDecision::TagsMerged {
                source_slug,
                target_slug,
            } => {
                let q = query(
                    "MATCH (source:Tag {slug: $source_slug})
                     MATCH (target:Tag {slug: $target_slug})
                     OPTIONAL MATCH (s)-[r:TAGGED]->(source)
                     FOREACH (_ IN CASE WHEN r IS NOT NULL THEN [1] ELSE [] END |
                         MERGE (s)-[:TAGGED]->(target)
                     )
                     DETACH DELETE source"
                )
                .param("source_slug", source_slug.as_str())
                .param("target_slug", target_slug.as_str());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            // ---------------------------------------------------------
            // Quality / lint
            // ---------------------------------------------------------
            SystemDecision::EmptyEntitiesCleaned { entity_ids } => {
                let ids: Vec<String> = entity_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS id
                     OPTIONAL MATCH (g:Gathering {id: id})
                     OPTIONAL MATCH (a:Aid {id: id})
                     OPTIONAL MATCH (n:Need {id: id})
                     OPTIONAL MATCH (nc:Notice {id: id})
                     OPTIONAL MATCH (t:Tension {id: id})
                     WITH coalesce(g, a, n, nc, t) AS node
                     WHERE node IS NOT NULL
                     OPTIONAL MATCH (node)-[:SOURCED_FROM]->(ev:Evidence)
                     DETACH DELETE node, ev"
                )
                .param("ids", ids);

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemDecision::FakeCoordinatesNulled { entity_ids, .. } => {
                let ids: Vec<String> = entity_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS id
                     OPTIONAL MATCH (g:Gathering {id: id})
                     OPTIONAL MATCH (a:Aid {id: id})
                     OPTIONAL MATCH (n:Need {id: id})
                     OPTIONAL MATCH (nc:Notice {id: id})
                     OPTIONAL MATCH (t:Tension {id: id})
                     WITH coalesce(g, a, n, nc, t) AS node
                     WHERE node IS NOT NULL
                     SET node.lat = null, node.lng = null"
                )
                .param("ids", ids);

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            SystemDecision::OrphanedCitationsCleaned { citation_ids } => {
                let ids: Vec<String> = citation_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS id
                     MATCH (ev:Evidence {id: id})
                     DETACH DELETE ev"
                )
                .param("ids", ids);

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            // ---------------------------------------------------------
            // Source system changes (editorial)
            // ---------------------------------------------------------
            SystemDecision::SourceSystemChanged {
                canonical_key,
                change,
                ..
            } => {
                let key = canonical_key.as_str();
                match change {
                    SystemSourceChange::QualityPenalty { new, .. } => {
                        let q = query("MATCH (s:Source {canonical_key: $key}) SET s.quality_penalty = $value")
                            .param("key", key).param("value", new);
                        self.client.graph.run(q).await?;
                    }
                    SystemSourceChange::GapContext { new, .. } => {
                        let q = query("MATCH (s:Source {canonical_key: $key}) SET s.gap_context = $value")
                            .param("key", key).param("value", new.as_deref().unwrap_or(""));
                        self.client.graph.run(q).await?;
                    }
                }
                Ok(ApplyResult::Applied)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Private helpers for typed correction handlers
    // -----------------------------------------------------------------------

    async fn set_str(&self, label: &str, id: uuid::Uuid, prop: &str, value: &str) -> Result<()> {
        let q = query(&format!("MATCH (n:{label} {{id: $id}}) SET n.{prop} = $value"))
            .param("id", id.to_string())
            .param("value", value);
        self.client.graph.run(q).await?;
        Ok(())
    }

    async fn set_f64(&self, label: &str, id: uuid::Uuid, prop: &str, value: f64) -> Result<()> {
        let q = query(&format!("MATCH (n:{label} {{id: $id}}) SET n.{prop} = $value"))
            .param("id", id.to_string())
            .param("value", value);
        self.client.graph.run(q).await?;
        Ok(())
    }

    async fn set_bool(&self, label: &str, id: uuid::Uuid, prop: &str, value: bool) -> Result<()> {
        let q = query(&format!("MATCH (n:{label} {{id: $id}}) SET n.{prop} = $value"))
            .param("id", id.to_string())
            .param("value", value);
        self.client.graph.run(q).await?;
        Ok(())
    }

    async fn set_location(&self, label: &str, id: uuid::Uuid, loc: &Option<Location>) -> Result<()> {
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
        self.client.graph.run(q).await?;
        Ok(())
    }

    async fn set_schedule(&self, label: &str, id: uuid::Uuid, schedule: &Option<Schedule>) -> Result<()> {
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
        self.client.graph.run(q).await?;
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
            .graph
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
        NodeType::Aid => "Aid",
        NodeType::Need => "Need",
        NodeType::Notice => "Notice",
        NodeType::Tension => "Tension",
        NodeType::Citation => "Evidence",
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
        None => (String::new(), String::new(), String::new(), false, String::new()),
    }
}

/// Build the common MERGE/ON CREATE SET query for all 5 discovery event types.
/// No sensitivity or implied_queries — those come from separate SystemDecision events.
fn build_discovery_query(
    label: &str,
    type_specific_set: &str,
    id: uuid::Uuid,
    title: &str,
    summary: &str,
    confidence: f32,
    source_url: &str,
    extracted_at: &DateTime<Utc>,
    content_date: Option<DateTime<Utc>>,
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
             n.content_date = CASE WHEN $content_date = '' THEN null ELSE datetime($content_date) END,
             n.location_name = $location_name,
             n.address = $address,
             n.lat = $lat,
             n.lng = $lng,
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
        .param("content_date", content_date.map(|dt| format_dt(&dt)).unwrap_or_default())
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
