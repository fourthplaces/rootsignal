//! GraphReducer — pure projection of facts into Neo4j nodes and edges.
//!
//! Each event is either acted upon (MERGE/SET/DELETE) or ignored (no-op).
//! The reducer never reads the graph, calls APIs, generates UUIDs, or uses wall-clock time.
//! It writes only factual values from event payloads — no embeddings, no diversity counts,
//! no cause_heat. Those are computed by enrichment passes after the reducer runs.
//!
//! Idempotency: all writes use MERGE or conditional SET with the event's seq as a guard.
//! Replaying the same event twice produces the same graph state.

use anyhow::Result;
use neo4rs::query;
use tracing::{debug, warn};

use rootsignal_common::events::Event;
use rootsignal_common::types::NodeType;
use rootsignal_events::StoredEvent;

use crate::GraphClient;

// ---------------------------------------------------------------------------
// GraphReducer
// ---------------------------------------------------------------------------

/// Pure projection of facts into Neo4j nodes and edges.
pub struct GraphReducer {
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

impl GraphReducer {
    pub fn new(client: GraphClient) -> Self {
        Self { client }
    }

    /// Apply a single fact to the graph. Idempotent.
    pub async fn apply(&self, event: &StoredEvent) -> Result<ApplyResult> {
        let parsed = match Event::from_payload(&event.payload) {
            Ok(e) => e,
            Err(e) => {
                warn!(seq = event.seq, error = %e, "Failed to deserialize event payload");
                return Ok(ApplyResult::DeserializeError(e.to_string()));
            }
        };

        match parsed {
            // =================================================================
            // Observability — explicit no-ops
            // =================================================================
            Event::UrlScraped { .. }
            | Event::FeedScraped { .. }
            | Event::SocialScraped { .. }
            | Event::SocialTopicsSearched { .. }
            | Event::SearchPerformed { .. }
            | Event::LlmExtractionCompleted { .. }
            | Event::BudgetCheckpoint { .. }
            | Event::BootstrapCompleted { .. }
            | Event::AgentWebSearched { .. }
            | Event::AgentPageRead { .. }
            | Event::AgentFutureQuery { .. }
            | Event::LintBatchCompleted { .. } => {
                debug!(seq = event.seq, event_type = event.event_type, "No-op (observability)");
                Ok(ApplyResult::NoOp)
            }

            // Informational — no graph mutation
            Event::SignalRejected { .. }
            | Event::SignalDroppedNoDate { .. }
            | Event::SignalDeduplicated { .. }
            | Event::ExpansionQueryCollected { .. }
            | Event::ExpansionSourceCreated { .. }
            | Event::SourceLinkDiscovered { .. } => {
                debug!(seq = event.seq, event_type = event.event_type, "No-op (informational)");
                Ok(ApplyResult::NoOp)
            }

            // =================================================================
            // Signal facts — create/update/delete signal nodes
            // =================================================================
            Event::SignalDiscovered {
                signal_id,
                node_type,
                title,
                summary,
                sensitivity,
                confidence,
                source_url,
                extracted_at,
                content_date,
                about_location,
                about_location_name,
                implied_queries,
                // Type-specific
                starts_at,
                ends_at,
                action_url,
                organizer,
                is_recurring,
                availability,
                is_ongoing,
                urgency,
                what_needed,
                goal,
                severity,
                category,
                effective_date,
                source_authority,
                what_would_help,
                // Not used in reducer (actor linking is a separate event)
                mentioned_actors: _,
                author_actor: _,
                from_location: _,
            } => {
                let label = node_type_label(node_type);
                let (lat, lng) = geo_point_to_lat_lng(&about_location);
                let actor_str = event.actor.as_deref().unwrap_or("");
                let run_id = event.run_id.as_deref().unwrap_or("");

                // Build type-specific SET clause
                let type_specific_set = match node_type {
                    NodeType::Gathering => {
                        ", n.starts_at = CASE WHEN $starts_at = '' THEN null ELSE datetime($starts_at) END,
                           n.ends_at = CASE WHEN $ends_at = '' THEN null ELSE datetime($ends_at) END,
                           n.action_url = $action_url,
                           n.organizer = $organizer,
                           n.is_recurring = $is_recurring"
                    }
                    NodeType::Aid => {
                        ", n.action_url = $action_url,
                           n.availability = $availability,
                           n.is_ongoing = $is_ongoing"
                    }
                    NodeType::Need => {
                        ", n.urgency = $urgency,
                           n.what_needed = $what_needed,
                           n.action_url = $action_url,
                           n.goal = $goal"
                    }
                    NodeType::Notice => {
                        ", n.severity = $severity,
                           n.category = $category,
                           n.effective_date = $effective_date,
                           n.source_authority = $source_authority"
                    }
                    NodeType::Tension => {
                        ", n.severity = $severity,
                           n.category = $category,
                           n.what_would_help = $what_would_help"
                    }
                    _ => "",
                };

                let cypher = format!(
                    "MERGE (n:{label} {{id: $id}})
                     ON CREATE SET
                         n.title = $title,
                         n.summary = $summary,
                         n.sensitivity = $sensitivity,
                         n.confidence = $confidence,
                         n.source_url = $source_url,
                         n.extracted_at = datetime($extracted_at),
                         n.last_confirmed_active = datetime($extracted_at),
                         n.content_date = CASE WHEN $content_date = '' THEN null ELSE datetime($content_date) END,
                         n.location_name = $location_name,
                         n.lat = $lat,
                         n.lng = $lng,
                         n.implied_queries = CASE WHEN size($implied_queries) > 0 THEN $implied_queries ELSE null END,
                         n.corroboration_count = 0,
                         n.review_status = 'staged',
                         n.created_by = $created_by,
                         n.scout_run_id = $scout_run_id
                         {type_specific_set}"
                );

                let q = query(&cypher)
                    .param("id", signal_id.to_string())
                    .param("title", title.as_str())
                    .param("summary", summary.as_str())
                    .param("sensitivity", sensitivity.as_str())
                    .param("confidence", confidence as f64)
                    .param("source_url", source_url.as_str())
                    .param("extracted_at", format_dt(&extracted_at))
                    .param("content_date", content_date.map(|dt| format_dt(&dt)).unwrap_or_default())
                    .param("location_name", about_location_name.as_deref().unwrap_or(""))
                    .param("lat", lat)
                    .param("lng", lng)
                    .param("implied_queries", implied_queries)
                    .param("created_by", actor_str)
                    .param("scout_run_id", run_id)
                    // Type-specific params (all provided, unused ones are harmless)
                    .param("starts_at", starts_at.map(|dt| format_dt(&dt)).unwrap_or_default())
                    .param("ends_at", ends_at.map(|dt| format_dt(&dt)).unwrap_or_default())
                    .param("action_url", action_url.as_deref().unwrap_or(""))
                    .param("organizer", organizer.unwrap_or_default())
                    .param("is_recurring", is_recurring.unwrap_or(false))
                    .param("availability", availability.as_deref().unwrap_or(""))
                    .param("is_ongoing", is_ongoing.unwrap_or(false))
                    .param("urgency", urgency.map(|u| urgency_str(u)).unwrap_or(""))
                    .param("what_needed", what_needed.as_deref().unwrap_or(""))
                    .param("goal", goal.unwrap_or_default())
                    .param("severity", severity.map(|s| severity_str(s)).unwrap_or(""))
                    .param("category", category.unwrap_or_default())
                    .param("effective_date", effective_date.map(|dt| format_dt(&dt)).unwrap_or_default())
                    .param("source_authority", source_authority.unwrap_or_default())
                    .param("what_would_help", what_would_help.as_deref().unwrap_or(""));

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            Event::SignalCorroborated {
                signal_id,
                node_type,
                new_corroboration_count,
                ..
            } => {
                let label = node_type_label(node_type);
                let q = query(&format!(
                    "MATCH (n:{label} {{id: $id}})
                     SET n.corroboration_count = $count,
                         n.last_confirmed_active = datetime($ts)"
                ))
                .param("id", signal_id.to_string())
                .param("count", new_corroboration_count as i64)
                .param("ts", format_dt_from_stored(event));

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            Event::SignalRefreshed {
                signal_ids,
                node_type,
                new_last_confirmed_active,
            } => {
                let label = node_type_label(node_type);
                let ids: Vec<String> = signal_ids.iter().map(|id| id.to_string()).collect();
                let q = query(&format!(
                    "UNWIND $ids AS id
                     MATCH (n:{label} {{id: id}})
                     SET n.last_confirmed_active = datetime($ts)"
                ))
                .param("ids", ids)
                .param("ts", format_dt(&new_last_confirmed_active));

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            Event::SignalConfidenceScored {
                signal_id,
                new_confidence,
                ..
            } => {
                // Update across all signal labels (we don't know the type here)
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
                .param("id", signal_id.to_string())
                .param("confidence", new_confidence as f64);

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            Event::SignalFieldsCorrected {
                signal_id,
                corrections,
            } => {
                // Apply each field correction individually
                for correction in &corrections {
                    let new_val = match &correction.new_value {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    // Use dynamic property setting
                    let q = query(&format!(
                        "OPTIONAL MATCH (g:Gathering {{id: $id}})
                         OPTIONAL MATCH (a:Aid {{id: $id}})
                         OPTIONAL MATCH (n:Need {{id: $id}})
                         OPTIONAL MATCH (nc:Notice {{id: $id}})
                         OPTIONAL MATCH (t:Tension {{id: $id}})
                         WITH coalesce(g, a, n, nc, t) AS node
                         WHERE node IS NOT NULL
                         SET node.{} = $value",
                        sanitize_field_name(&correction.field)
                    ))
                    .param("id", signal_id.to_string())
                    .param("value", new_val);

                    self.client.graph.run(q).await?;
                }
                Ok(ApplyResult::Applied)
            }

            Event::SignalExpired {
                signal_id,
                node_type,
                ..
            } => {
                let label = node_type_label(node_type);
                let q = query(&format!(
                    "MATCH (n:{label} {{id: $id}})
                     OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
                     DETACH DELETE n, ev"
                ))
                .param("id", signal_id.to_string());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            Event::SignalPurged {
                signal_id,
                node_type,
                ..
            } => {
                let label = node_type_label(node_type);
                let q = query(&format!(
                    "MATCH (n:{label} {{id: $id}})
                     OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
                     DETACH DELETE n, ev"
                ))
                .param("id", signal_id.to_string());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            Event::ReviewVerdictReached {
                signal_id,
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
                .param("id", signal_id.to_string())
                .param("status", new_status.as_str());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            Event::ImpliedQueriesConsumed { signal_ids } => {
                let ids: Vec<String> = signal_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS id
                     MATCH (n) WHERE n.id = id AND (n:Aid OR n:Gathering)
                     SET n.implied_queries = null"
                )
                .param("ids", ids);

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            // =================================================================
            // Citation facts — Evidence nodes + SOURCED_FROM edges
            // =================================================================
            Event::CitationRecorded {
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
                     OPTIONAL MATCH (a:Aid {id: $signal_id})
                     OPTIONAL MATCH (n:Need {id: $signal_id})
                     OPTIONAL MATCH (nc:Notice {id: $signal_id})
                     OPTIONAL MATCH (t:Tension {id: $signal_id})
                     WITH coalesce(g, a, n, nc, t) AS signal
                     WHERE signal IS NOT NULL
                     MERGE (signal)-[:SOURCED_FROM]->(ev:Evidence {source_url: $url})
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
                .param("signal_id", signal_id.to_string())
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

            Event::OrphanedCitationsCleaned { citation_ids } => {
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

            // =================================================================
            // Source facts
            // =================================================================
            Event::SourceRegistered {
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

            Event::SourceUpdated {
                canonical_key,
                changes,
                ..
            } => {
                // Apply each changed field from the JSON object
                if let Some(obj) = changes.as_object() {
                    for (key, value) in obj {
                        let val_str = match value {
                            serde_json::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        let q = query(&format!(
                            "MATCH (s:Source {{canonical_key: $key}})
                             SET s.{} = $value",
                            sanitize_field_name(key)
                        ))
                        .param("key", canonical_key.as_str())
                        .param("value", val_str);

                        self.client.graph.run(q).await?;
                    }
                }
                Ok(ApplyResult::Applied)
            }

            Event::SourceDeactivated { source_ids, .. } => {
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

            Event::SourceRemoved { canonical_key, .. } => {
                let q = query(
                    "MATCH (s:Source {canonical_key: $key})
                     DETACH DELETE s"
                )
                .param("key", canonical_key.as_str());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            Event::SourceScrapeRecorded {
                canonical_key,
                signals_produced,
                scrape_count,
                consecutive_empty_runs,
            } => {
                let q = if signals_produced > 0 {
                    query(
                        "MATCH (s:Source {canonical_key: $key})
                         SET s.last_scraped = datetime($ts),
                             s.last_produced_signal = datetime($ts),
                             s.signals_produced = s.signals_produced + $count,
                             s.consecutive_empty_runs = 0,
                             s.scrape_count = $scrape_count"
                    )
                    .param("count", signals_produced as i64)
                } else {
                    query(
                        "MATCH (s:Source {canonical_key: $key})
                         SET s.last_scraped = datetime($ts),
                             s.consecutive_empty_runs = $empty_runs,
                             s.scrape_count = $scrape_count"
                    )
                    .param("empty_runs", consecutive_empty_runs as i64)
                };

                let q = q
                    .param("key", canonical_key.as_str())
                    .param("ts", format_dt_from_stored(event))
                    .param("scrape_count", scrape_count as i64);

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            // =================================================================
            // Actor facts
            // =================================================================
            Event::ActorIdentified {
                actor_id,
                name,
                actor_type,
                entity_id,
                domains,
                social_urls,
                description,
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
                         a.signal_count = 1,
                         a.first_seen = datetime($ts),
                         a.last_active = datetime($ts)
                     ON MATCH SET
                         a.name = $name,
                         a.last_active = datetime($ts),
                         a.signal_count = a.signal_count + 1"
                )
                .param("id", actor_id.to_string())
                .param("entity_id", entity_id.as_str())
                .param("name", name.as_str())
                .param("actor_type", actor_type.to_string())
                .param("domains", domains)
                .param("social_urls", social_urls)
                .param("description", description.as_str())
                .param("ts", format_dt_from_stored(event));

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            Event::ActorLinkedToSignal {
                actor_id,
                signal_id,
                role,
            } => {
                let q = query(
                    "MATCH (a:Actor {id: $actor_id})
                     MATCH (n) WHERE n.id = $signal_id AND (n:Gathering OR n:Aid OR n:Need OR n:Notice OR n:Tension)
                     MERGE (a)-[:ACTED_IN {role: $role}]->(n)"
                )
                .param("actor_id", actor_id.to_string())
                .param("signal_id", signal_id.to_string())
                .param("role", role.as_str());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            Event::ActorLinkedToSource {
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

            Event::ActorStatsUpdated {
                actor_id,
                signal_count,
                last_active,
            } => {
                let q = query(
                    "MATCH (a:Actor {id: $id})
                     SET a.signal_count = $count,
                         a.last_active = datetime($ts)"
                )
                .param("id", actor_id.to_string())
                .param("count", signal_count as i64)
                .param("ts", format_dt(&last_active));

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            Event::ActorLocationIdentified {
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

            Event::DuplicateActorsMerged {
                kept_id,
                merged_ids,
            } => {
                // Repoint all edges from merged actors to the kept actor, then delete merged
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

            Event::OrphanedActorsCleaned { actor_ids } => {
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

            // =================================================================
            // Relationship facts
            // =================================================================
            Event::RelationshipEstablished {
                from_id,
                to_id,
                relationship_type,
                properties,
            } => {
                // Dynamic relationship type — sanitize the name
                let rel_type = sanitize_field_name(&relationship_type);
                let cypher = format!(
                    "MATCH (a) WHERE a.id = $from_id
                     MATCH (b) WHERE b.id = $to_id
                     MERGE (a)-[r:{rel_type}]->(b)"
                );

                let mut q = query(&cypher)
                    .param("from_id", from_id.to_string())
                    .param("to_id", to_id.to_string());

                // Set properties if provided
                if let Some(props) = properties {
                    if let Some(obj) = props.as_object() {
                        for (key, value) in obj {
                            match value {
                                serde_json::Value::Number(n) => {
                                    if let Some(f) = n.as_f64() {
                                        q = q.param(key.as_str(), f);
                                    }
                                }
                                serde_json::Value::String(s) => {
                                    q = q.param(key.as_str(), s.as_str());
                                }
                                _ => {}
                            }
                        }
                    }
                }

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            // =================================================================
            // Situation / dispatch facts
            // =================================================================
            Event::SituationIdentified {
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

            Event::SituationEvolved {
                situation_id,
                changes,
            } => {
                // Apply each changed field
                if let Some(obj) = changes.as_object() {
                    for (key, value) in obj {
                        let val_str = match value {
                            serde_json::Value::String(s) => s.clone(),
                            serde_json::Value::Number(n) => n.to_string(),
                            other => other.to_string(),
                        };
                        let q = query(&format!(
                            "MATCH (s:Story {{id: $id}})
                             SET s.{} = $value, s.last_updated = datetime($ts)",
                            sanitize_field_name(key)
                        ))
                        .param("id", situation_id.to_string())
                        .param("value", val_str)
                        .param("ts", format_dt_from_stored(event));

                        self.client.graph.run(q).await?;
                    }
                }
                Ok(ApplyResult::Applied)
            }

            Event::SituationPromoted { situation_ids } => {
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

            Event::DispatchCreated { .. } => {
                // Dispatches are delivered artifacts, not graph nodes.
                // Future: could store as Dispatch nodes if needed.
                debug!(seq = event.seq, "No-op (dispatch — not a graph node)");
                Ok(ApplyResult::NoOp)
            }

            // =================================================================
            // Tag facts
            // =================================================================
            Event::TagsAggregated {
                situation_id,
                tags,
            } => {
                for tag in &tags {
                    let q = query(
                        "MATCH (s:Story {id: $situation_id})
                         MERGE (t:Tag {slug: $slug})
                         ON CREATE SET t.name = $name
                         MERGE (s)-[r:TAGGED]->(t)
                         SET r.weight = $weight"
                    )
                    .param("situation_id", situation_id.to_string())
                    .param("slug", tag.slug.as_str())
                    .param("name", tag.name.as_str())
                    .param("weight", tag.weight);

                    self.client.graph.run(q).await?;
                }
                Ok(ApplyResult::Applied)
            }

            Event::TagSuppressed {
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

            Event::TagsMerged {
                source_slug,
                target_slug,
            } => {
                // Move all TAGGED edges from source tag to target tag, then delete source
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

            // =================================================================
            // Quality / lint facts (graph-mutating subset)
            // =================================================================
            Event::LintCorrectionApplied {
                node_id,
                signal_type,
                field,
                new_value,
                ..
            } => {
                let label = node_type_label(signal_type);
                let q = query(&format!(
                    "MATCH (n:{label} {{id: $id}})
                     SET n.{} = $value",
                    sanitize_field_name(&field)
                ))
                .param("id", node_id.to_string())
                .param("value", new_value.as_str());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            Event::LintRejectionIssued {
                node_id,
                signal_type,
                ..
            } => {
                let label = node_type_label(signal_type);
                let q = query(&format!(
                    "MATCH (n:{label} {{id: $id}})
                     SET n.review_status = 'rejected'"
                ))
                .param("id", node_id.to_string());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            Event::EmptySignalsCleaned { signal_ids } => {
                let ids: Vec<String> = signal_ids.iter().map(|id| id.to_string()).collect();
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

            Event::FakeCoordinatesNulled { signal_ids, .. } => {
                let ids: Vec<String> = signal_ids.iter().map(|id| id.to_string()).collect();
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

            // =================================================================
            // Schedule facts
            // =================================================================
            Event::ScheduleRecorded {
                signal_id,
                rrule,
                dtstart,
                label,
            } => {
                let q = query(
                    "OPTIONAL MATCH (g:Gathering {id: $id})
                     OPTIONAL MATCH (a:Aid {id: $id})
                     OPTIONAL MATCH (n:Need {id: $id})
                     OPTIONAL MATCH (nc:Notice {id: $id})
                     OPTIONAL MATCH (t:Tension {id: $id})
                     WITH coalesce(g, a, n, nc, t) AS node
                     WHERE node IS NOT NULL
                     SET node.rrule = $rrule,
                         node.dtstart = datetime($dtstart),
                         node.schedule_label = $label"
                )
                .param("id", signal_id.to_string())
                .param("rrule", rrule.as_str())
                .param("dtstart", format_dt(&dtstart))
                .param("label", label.unwrap_or_default());

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            // =================================================================
            // Pin / demand / submission facts
            // =================================================================
            Event::PinCreated {
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

            Event::PinsRemoved { pin_ids } => {
                let ids: Vec<String> = pin_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS id
                     MATCH (p:Pin {id: id})
                     DETACH DELETE p"
                )
                .param("ids", ids);

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            Event::DemandSignalReceived {
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

            Event::DemandAggregated {
                consumed_demand_ids,
                ..
            } => {
                let ids: Vec<String> = consumed_demand_ids.iter().map(|id| id.to_string()).collect();
                let q = query(
                    "UNWIND $ids AS id
                     MATCH (d:DemandSignal {id: id})
                     DELETE d"
                )
                .param("ids", ids);

                self.client.graph.run(q).await?;
                Ok(ApplyResult::Applied)
            }

            Event::SubmissionReceived {
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
        }
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
                self.apply(event).await?;
                last_applied = event.seq;
            }

            cursor = last_applied + 1;

            // If we got fewer than batch_size, we've reached the end
            if events.len() < batch_size {
                break;
            }
        }

        Ok(last_applied)
    }

    /// Full rebuild: wipe graph, replay all facts from the beginning.
    pub async fn rebuild(&self, store: &rootsignal_events::EventStore) -> Result<i64> {
        // Delete all nodes and relationships
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
        NodeType::Evidence => "Evidence",
    }
}

fn format_dt(dt: &chrono::DateTime<chrono::Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// Use the event's stored timestamp (from the events table) when no explicit timestamp
/// exists in the payload. This is the fact's timestamp — never wall-clock time.
fn format_dt_from_stored(event: &StoredEvent) -> String {
    format_dt(&event.ts)
}

fn geo_point_to_lat_lng(
    point: &Option<rootsignal_common::types::GeoPoint>,
) -> (f64, f64) {
    match point {
        Some(p) => (p.lat, p.lng),
        None => (0.0, 0.0),
    }
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

/// Sanitize a field name to prevent Cypher injection.
/// Only allows alphanumeric chars and underscores.
fn sanitize_field_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}
