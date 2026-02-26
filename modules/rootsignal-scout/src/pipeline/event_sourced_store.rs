//! EventSourcedStore — events are the source of truth, projector writes the graph.
//!
//! Every write method does exactly two things:
//!   1. Build an Event from the method args → append to EventStore (Postgres)
//!   2. Project the stored event to the graph via GraphProjector (Neo4j)
//!
//! The events table is the single source of truth. The graph is a projection.
//!
//! Read methods delegate to GraphWriter (graph is always current).
//! Actor/source/resource methods pass through to GraphWriter (Phase 2 scope).

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use rootsignal_common::events::{Event, Location, Schedule};
use rootsignal_common::types::{
    ActorNode, EvidenceNode, GeoPoint, Node, NodeType, SourceNode,
};
use rootsignal_common::{
    EntityMappingOwned, FRESHNESS_MAX_DAYS, GATHERING_PAST_GRACE_HOURS, NEED_EXPIRE_DAYS,
    NOTICE_EXPIRE_DAYS,
};
use rootsignal_events::{AppendEvent, EventStore};
use rootsignal_graph::{DuplicateMatch, GraphProjector, GraphWriter, ReapStats};

use super::traits::SignalStore;

/// SignalStore that appends events then projects them to the graph.
pub struct EventSourcedStore {
    writer: GraphWriter,         // kept for READ methods + Phase 2 pass-through
    projector: GraphProjector,   // sole write path for signal lifecycle
    event_store: EventStore,
    run_id: String,
}

impl EventSourcedStore {
    pub fn new(
        writer: GraphWriter,
        projector: GraphProjector,
        event_store: EventStore,
        run_id: String,
    ) -> Self {
        Self {
            writer,
            projector,
            event_store,
            run_id,
        }
    }
}

// ---------------------------------------------------------------------------
// Node → Event conversion helpers
// ---------------------------------------------------------------------------

fn meta_to_location(meta: &rootsignal_common::types::NodeMeta) -> Option<Location> {
    meta.about_location.as_ref().map(|point| Location {
        point: Some(GeoPoint {
            lat: point.lat,
            lng: point.lng,
            precision: point.precision,
        }),
        name: meta.about_location_name.clone(),
        address: None,
    })
}

fn meta_to_from_location(meta: &rootsignal_common::types::NodeMeta) -> Option<Location> {
    meta.from_location.as_ref().map(|point| Location {
        point: Some(GeoPoint {
            lat: point.lat,
            lng: point.lng,
            precision: point.precision,
        }),
        name: None,
        address: None,
    })
}

fn node_to_event(node: &Node) -> Event {
    match node {
        Node::Gathering(n) => Event::GatheringDiscovered {
            id: n.meta.id,
            title: n.meta.title.clone(),
            summary: n.meta.summary.clone(),
            sensitivity: n.meta.sensitivity,
            confidence: n.meta.confidence,
            source_url: n.meta.source_url.clone(),
            extracted_at: n.meta.extracted_at,
            content_date: n.meta.content_date,
            location: meta_to_location(&n.meta),
            from_location: meta_to_from_location(&n.meta),
            implied_queries: n.meta.implied_queries.clone(),
            mentioned_actors: n.meta.mentioned_actors.clone(),
            author_actor: n.meta.author_actor.clone(),
            schedule: schedule_from_gathering(n),
            action_url: if n.action_url.is_empty() {
                None
            } else {
                Some(n.action_url.clone())
            },
            organizer: n.organizer.clone(),
        },
        Node::Aid(n) => Event::AidDiscovered {
            id: n.meta.id,
            title: n.meta.title.clone(),
            summary: n.meta.summary.clone(),
            sensitivity: n.meta.sensitivity,
            confidence: n.meta.confidence,
            source_url: n.meta.source_url.clone(),
            extracted_at: n.meta.extracted_at,
            content_date: n.meta.content_date,
            location: meta_to_location(&n.meta),
            from_location: meta_to_from_location(&n.meta),
            implied_queries: n.meta.implied_queries.clone(),
            mentioned_actors: n.meta.mentioned_actors.clone(),
            author_actor: n.meta.author_actor.clone(),
            action_url: if n.action_url.is_empty() {
                None
            } else {
                Some(n.action_url.clone())
            },
            availability: n.availability.clone(),
            is_ongoing: Some(n.is_ongoing),
        },
        Node::Need(n) => Event::NeedDiscovered {
            id: n.meta.id,
            title: n.meta.title.clone(),
            summary: n.meta.summary.clone(),
            sensitivity: n.meta.sensitivity,
            confidence: n.meta.confidence,
            source_url: n.meta.source_url.clone(),
            extracted_at: n.meta.extracted_at,
            content_date: n.meta.content_date,
            location: meta_to_location(&n.meta),
            from_location: meta_to_from_location(&n.meta),
            implied_queries: n.meta.implied_queries.clone(),
            mentioned_actors: n.meta.mentioned_actors.clone(),
            author_actor: n.meta.author_actor.clone(),
            urgency: Some(n.urgency),
            what_needed: n.what_needed.clone(),
            goal: n.goal.clone(),
        },
        Node::Notice(n) => Event::NoticeDiscovered {
            id: n.meta.id,
            title: n.meta.title.clone(),
            summary: n.meta.summary.clone(),
            sensitivity: n.meta.sensitivity,
            confidence: n.meta.confidence,
            source_url: n.meta.source_url.clone(),
            extracted_at: n.meta.extracted_at,
            content_date: n.meta.content_date,
            location: meta_to_location(&n.meta),
            from_location: meta_to_from_location(&n.meta),
            implied_queries: n.meta.implied_queries.clone(),
            mentioned_actors: n.meta.mentioned_actors.clone(),
            author_actor: n.meta.author_actor.clone(),
            severity: Some(n.severity),
            category: n.category.clone(),
            effective_date: n.effective_date,
            source_authority: n.source_authority.clone(),
        },
        Node::Tension(n) => Event::TensionDiscovered {
            id: n.meta.id,
            title: n.meta.title.clone(),
            summary: n.meta.summary.clone(),
            sensitivity: n.meta.sensitivity,
            confidence: n.meta.confidence,
            source_url: n.meta.source_url.clone(),
            extracted_at: n.meta.extracted_at,
            content_date: n.meta.content_date,
            location: meta_to_location(&n.meta),
            from_location: meta_to_from_location(&n.meta),
            implied_queries: n.meta.implied_queries.clone(),
            mentioned_actors: n.meta.mentioned_actors.clone(),
            author_actor: n.meta.author_actor.clone(),
            severity: Some(n.severity),
            what_would_help: n.what_would_help.clone(),
        },
        Node::Evidence(_) => unreachable!("Evidence nodes use create_evidence, not create_node"),
    }
}

fn schedule_from_gathering(n: &rootsignal_common::types::GatheringNode) -> Option<Schedule> {
    if n.starts_at.is_none() && n.ends_at.is_none() && !n.is_recurring {
        return None;
    }
    Some(Schedule {
        starts_at: n.starts_at,
        ends_at: n.ends_at,
        all_day: false,
        rrule: None,
        timezone: None,
    })
}

fn evidence_to_event(evidence: &EvidenceNode, signal_id: Uuid) -> Event {
    Event::CitationRecorded {
        citation_id: evidence.id,
        entity_id: signal_id,
        url: evidence.source_url.clone(),
        content_hash: evidence.content_hash.clone(),
        snippet: evidence.snippet.clone(),
        relevance: evidence.relevance.clone(),
        channel_type: evidence.channel_type,
        evidence_confidence: evidence.evidence_confidence,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

impl EventSourcedStore {
    /// Read the current corroboration count from the graph for idempotent projection.
    ///
    /// NOTE: This is a read-then-write pattern. Two concurrent corroborations of the
    /// same signal could both read count=N and write count=N+1, losing an increment.
    /// Acceptable for a single-writer system; revisit if we move to concurrent writers.
    async fn read_corroboration_count(&self, id: Uuid, node_type: NodeType) -> Result<u32> {
        let label = match node_type {
            NodeType::Gathering => "Gathering",
            NodeType::Aid => "Aid",
            NodeType::Need => "Need",
            NodeType::Notice => "Notice",
            NodeType::Tension => "Tension",
            NodeType::Evidence => "Evidence",
        };
        let q = rootsignal_graph::query(&format!(
            "MATCH (n:{label} {{id: $id}}) RETURN n.corroboration_count AS count"
        ))
        .param("id", id.to_string());

        let graph = self.writer.client().inner();
        let mut stream = match graph.execute(q).await {
            Ok(s) => s,
            Err(_) => return Ok(0),
        };
        if let Some(row) = stream.next().await? {
            let count: i64 = row.get("count").unwrap_or(0);
            Ok(count as u32)
        } else {
            Ok(0)
        }
    }

    /// Read current scrape_count and consecutive_empty_runs from the graph.
    ///
    /// Same read-then-write pattern as `read_corroboration_count`.
    async fn read_source_scrape_stats(&self, canonical_key: &str) -> Result<(u32, u32)> {
        let q = rootsignal_graph::query(
            "MATCH (s:Source {canonical_key: $key}) RETURN s.scrape_count AS sc, s.consecutive_empty_runs AS cer"
        )
        .param("key", canonical_key);

        let graph = self.writer.client().inner();
        let mut stream = match graph.execute(q).await {
            Ok(s) => s,
            Err(_) => return Ok((0, 0)),
        };
        if let Some(row) = stream.next().await? {
            let sc: i64 = row.get("sc").unwrap_or(0);
            let cer: i64 = row.get("cer").unwrap_or(0);
            Ok((sc as u32, cer as u32))
        } else {
            Ok((0, 0))
        }
    }

    /// Append an event and project it to the graph.
    async fn append_and_project(&self, event: &Event, actor: Option<&str>) -> Result<()> {
        let mut append = AppendEvent::new(event.event_type(), event.to_payload())
            .with_run_id(&self.run_id);
        if let Some(a) = actor {
            append = append.with_actor(a);
        }
        let stored = self.event_store.append_and_read(append).await?;
        self.projector.project(&stored).await?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// SignalStore implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl SignalStore for EventSourcedStore {
    // --- URL/content guards (read-only, delegate to writer) ---

    async fn blocked_urls(&self, urls: &[String]) -> Result<HashSet<String>> {
        Ok(self.writer.blocked_urls(urls).await?)
    }

    async fn content_already_processed(&self, hash: &str, url: &str) -> Result<bool> {
        Ok(self.writer.content_already_processed(hash, url).await?)
    }

    // --- Signal lifecycle (append event → project to graph) ---

    async fn create_node(
        &self,
        node: &Node,
        _embedding: &[f32],
        created_by: &str,
        run_id: &str,
    ) -> Result<Uuid> {
        let event = node_to_event(node);
        let append = AppendEvent::new(event.event_type(), event.to_payload())
            .with_run_id(run_id)
            .with_actor(created_by);
        let stored = self.event_store.append_and_read(append).await?;
        self.projector.project(&stored).await?;
        Ok(node.id())
    }

    async fn create_evidence(&self, evidence: &EvidenceNode, signal_id: Uuid) -> Result<()> {
        let event = evidence_to_event(evidence, signal_id);
        self.append_and_project(&event, None).await
    }

    async fn refresh_signal(
        &self,
        id: Uuid,
        node_type: NodeType,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let event = Event::FreshnessConfirmed {
            entity_ids: vec![id],
            node_type,
            confirmed_at: now,
        };
        self.append_and_project(&event, None).await
    }

    async fn refresh_url_signals(&self, url: &str, now: DateTime<Utc>) -> Result<u64> {
        // Query matching signal IDs grouped by type, emit FreshnessConfirmed events.
        let mut total = 0u64;
        for (label, node_type) in &[
            ("Gathering", NodeType::Gathering),
            ("Aid", NodeType::Aid),
            ("Need", NodeType::Need),
            ("Notice", NodeType::Notice),
            ("Tension", NodeType::Tension),
        ] {
            let q = rootsignal_graph::query(&format!(
                "MATCH (n:{label}) WHERE n.source_url = $url RETURN n.id AS id"
            ))
            .param("url", url);

            let graph = self.writer.client().inner();
            let mut ids = Vec::new();
            let mut stream = graph.execute(q).await?;
            while let Some(row) = stream.next().await? {
                let id_str: String = row.get("id").unwrap_or_default();
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    ids.push(id);
                }
            }

            if !ids.is_empty() {
                total += ids.len() as u64;
                let event = Event::FreshnessConfirmed {
                    entity_ids: ids,
                    node_type: *node_type,
                    confirmed_at: now,
                };
                self.append_and_project(&event, None).await?;
            }
        }
        Ok(total)
    }

    async fn corroborate(
        &self,
        id: Uuid,
        node_type: NodeType,
        _now: DateTime<Utc>,
        _entity_mappings: &[EntityMappingOwned],
        source_url: &str,
        similarity: f64,
    ) -> Result<()> {
        let current_count = self.read_corroboration_count(id, node_type).await?;
        let event = Event::ObservationCorroborated {
            entity_id: id,
            node_type,
            new_source_url: source_url.to_string(),
            similarity,
            new_corroboration_count: current_count + 1,
            summary: None,
        };
        self.append_and_project(&event, None).await
    }

    // --- Dedup queries (read-only, delegate to writer) ---

    async fn existing_titles_for_url(&self, url: &str) -> Result<Vec<String>> {
        Ok(self.writer.existing_titles_for_url(url).await?)
    }

    async fn find_by_titles_and_types(
        &self,
        pairs: &[(String, NodeType)],
    ) -> Result<HashMap<(String, NodeType), (Uuid, String)>> {
        Ok(self.writer.find_by_titles_and_types(pairs).await?)
    }

    async fn find_duplicate(
        &self,
        embedding: &[f32],
        primary_type: NodeType,
        threshold: f64,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Option<DuplicateMatch>> {
        Ok(self
            .writer
            .find_duplicate(embedding, primary_type, threshold, min_lat, max_lat, min_lng, max_lng)
            .await?)
    }

    // --- Actor graph (append event → project to graph) ---

    async fn find_actor_by_name(&self, name: &str) -> Result<Option<Uuid>> {
        Ok(self.writer.find_actor_by_name(name).await?)
    }

    async fn upsert_actor(&self, actor: &ActorNode) -> Result<()> {
        let event = Event::ActorIdentified {
            actor_id: actor.id,
            name: actor.name.clone(),
            actor_type: actor.actor_type,
            entity_id: actor.entity_id.clone(),
            domains: actor.domains.clone(),
            social_urls: actor.social_urls.clone(),
            description: actor.description.clone(),
            bio: actor.bio.clone(),
            location_lat: actor.location_lat,
            location_lng: actor.location_lng,
            location_name: actor.location_name.clone(),
            discovery_depth: Some(actor.discovery_depth),
        };
        self.append_and_project(&event, None).await
    }

    async fn link_actor_to_signal(
        &self,
        actor_id: Uuid,
        signal_id: Uuid,
        role: &str,
    ) -> Result<()> {
        let event = Event::ActorLinkedToEntity {
            actor_id,
            entity_id: signal_id,
            role: role.to_string(),
        };
        self.append_and_project(&event, None).await
    }

    async fn link_actor_to_source(&self, actor_id: Uuid, source_id: Uuid) -> Result<()> {
        let event = Event::ActorLinkedToSource {
            actor_id,
            source_id,
        };
        self.append_and_project(&event, None).await
    }

    async fn link_signal_to_source(&self, signal_id: Uuid, source_id: Uuid) -> Result<()> {
        let event = Event::SignalLinkedToSource { signal_id, source_id };
        self.append_and_project(&event, None).await
    }

    async fn find_actor_by_entity_id(&self, entity_id: &str) -> Result<Option<Uuid>> {
        Ok(self.writer.find_actor_by_entity_id(entity_id).await?)
    }

    // --- Resource graph (pass through, Phase 2 scope) ---

    async fn find_or_create_resource(
        &self,
        name: &str,
        slug: &str,
        description: &str,
        embedding: &[f32],
    ) -> Result<Uuid> {
        Ok(self
            .writer
            .find_or_create_resource(name, slug, description, embedding)
            .await?)
    }

    async fn create_requires_edge(
        &self,
        signal_id: Uuid,
        resource_id: Uuid,
        confidence: f32,
        quantity: Option<&str>,
        notes: Option<&str>,
    ) -> Result<()> {
        Ok(self
            .writer
            .create_requires_edge(signal_id, resource_id, confidence, quantity, notes)
            .await?)
    }

    async fn create_prefers_edge(
        &self,
        signal_id: Uuid,
        resource_id: Uuid,
        confidence: f32,
    ) -> Result<()> {
        Ok(self
            .writer
            .create_prefers_edge(signal_id, resource_id, confidence)
            .await?)
    }

    async fn create_offers_edge(
        &self,
        signal_id: Uuid,
        resource_id: Uuid,
        confidence: f32,
        capacity: Option<&str>,
    ) -> Result<()> {
        Ok(self
            .writer
            .create_offers_edge(signal_id, resource_id, confidence, capacity)
            .await?)
    }

    // --- Source management (append event → project to graph) ---

    async fn get_active_sources(&self) -> Result<Vec<SourceNode>> {
        Ok(self.writer.get_active_sources().await?)
    }

    async fn upsert_source(&self, source: &SourceNode) -> Result<()> {
        let event = Event::SourceRegistered {
            source_id: source.id,
            canonical_key: source.canonical_key.clone(),
            canonical_value: source.canonical_value.clone(),
            url: source.url.clone(),
            discovery_method: source.discovery_method,
            weight: source.weight,
            source_role: source.source_role,
            gap_context: source.gap_context.clone(),
        };
        self.append_and_project(&event, None).await
    }

    async fn record_source_scrape(
        &self,
        canonical_key: &str,
        signals_produced: u32,
        _now: DateTime<Utc>,
    ) -> Result<()> {
        let (scrape_count, consecutive_empty_runs) =
            self.read_source_scrape_stats(canonical_key).await?;
        let new_scrape_count = scrape_count + 1;
        let new_consecutive_empty_runs = if signals_produced > 0 {
            0
        } else {
            consecutive_empty_runs + 1
        };
        let event = Event::SourceScrapeRecorded {
            canonical_key: canonical_key.to_string(),
            entities_produced: signals_produced,
            scrape_count: new_scrape_count,
            consecutive_empty_runs: new_consecutive_empty_runs,
        };
        self.append_and_project(&event, None).await
    }

    async fn delete_pins(&self, pin_ids: &[Uuid]) -> Result<()> {
        if pin_ids.is_empty() {
            return Ok(());
        }
        let event = Event::PinsRemoved {
            pin_ids: pin_ids.to_vec(),
        };
        self.append_and_project(&event, None).await
    }

    async fn reap_expired(&self) -> Result<ReapStats> {
        let mut stats = ReapStats::default();
        let graph = self.writer.client().inner();

        // 1. Past non-recurring gatherings
        let q = rootsignal_graph::query(&format!(
            "MATCH (n:Gathering)
             WHERE n.is_recurring = false
               AND n.starts_at IS NOT NULL AND n.starts_at <> ''
               AND CASE
                   WHEN n.ends_at IS NOT NULL AND n.ends_at <> ''
                   THEN datetime(n.ends_at) < datetime() - duration('PT{}H')
                   ELSE datetime(n.starts_at) < datetime() - duration('PT{}H')
               END
             RETURN n.id AS id",
            GATHERING_PAST_GRACE_HOURS, GATHERING_PAST_GRACE_HOURS
        ));
        let mut ids = Vec::new();
        let mut stream = graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                ids.push(id);
            }
        }
        for id in &ids {
            let event = Event::EntityExpired {
                entity_id: *id,
                node_type: NodeType::Gathering,
                reason: "past_event".to_string(),
            };
            self.append_and_project(&event, None).await?;
        }
        stats.gatherings = ids.len() as u64;

        // 2. Expired needs
        let q = rootsignal_graph::query(&format!(
            "MATCH (n:Need)
             WHERE datetime(n.extracted_at) < datetime() - duration('P{}D')
             RETURN n.id AS id",
            NEED_EXPIRE_DAYS
        ));
        let mut ids = Vec::new();
        let mut stream = graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                ids.push(id);
            }
        }
        for id in &ids {
            let event = Event::EntityExpired {
                entity_id: *id,
                node_type: NodeType::Need,
                reason: "need_expired".to_string(),
            };
            self.append_and_project(&event, None).await?;
        }
        stats.needs = ids.len() as u64;

        // 3. Expired notices
        let q = rootsignal_graph::query(&format!(
            "MATCH (n:Notice)
             WHERE datetime(n.extracted_at) < datetime() - duration('P{}D')
             RETURN n.id AS id",
            NOTICE_EXPIRE_DAYS
        ));
        let mut ids = Vec::new();
        let mut stream = graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                ids.push(id);
            }
        }
        for id in &ids {
            let event = Event::EntityExpired {
                entity_id: *id,
                node_type: NodeType::Notice,
                reason: "notice_expired".to_string(),
            };
            self.append_and_project(&event, None).await?;
        }
        stats.stale += ids.len() as u64;

        // 4. Stale unconfirmed signals (Aid, Tension)
        for (label, node_type) in &[("Aid", NodeType::Aid), ("Tension", NodeType::Tension)] {
            let q = rootsignal_graph::query(&format!(
                "MATCH (n:{label})
                 WHERE datetime(n.last_confirmed_active) < datetime() - duration('P{days}D')
                 RETURN n.id AS id",
                label = label,
                days = FRESHNESS_MAX_DAYS,
            ));
            let mut ids = Vec::new();
            let mut stream = graph.execute(q).await?;
            while let Some(row) = stream.next().await? {
                let id_str: String = row.get("id").unwrap_or_default();
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    ids.push(id);
                }
            }
            for id in &ids {
                let event = Event::EntityExpired {
                    entity_id: *id,
                    node_type: *node_type,
                    reason: "stale_unconfirmed".to_string(),
                };
                self.append_and_project(&event, None).await?;
            }
            stats.stale += ids.len() as u64;
        }

        Ok(stats)
    }

    async fn batch_tag_signals(&self, signal_id: Uuid, tag_slugs: &[String]) -> Result<()> {
        Ok(self.writer.batch_tag_signals(signal_id, tag_slugs).await?)
    }

    // --- Actor location enrichment (pass through) ---

    async fn get_signals_for_actor(
        &self,
        actor_id: Uuid,
    ) -> Result<Vec<(f64, f64, String, DateTime<Utc>)>> {
        Ok(self.writer.get_signals_for_actor(actor_id).await?)
    }

    async fn update_actor_location(
        &self,
        actor_id: Uuid,
        lat: f64,
        lng: f64,
        name: &str,
    ) -> Result<()> {
        let event = Event::ActorLocationIdentified {
            actor_id,
            location_lat: lat,
            location_lng: lng,
            location_name: if name.is_empty() { None } else { Some(name.to_string()) },
        };
        self.append_and_project(&event, None).await
    }

    async fn list_all_actors(&self) -> Result<Vec<(ActorNode, Vec<SourceNode>)>> {
        Ok(self.writer.list_all_actors().await?)
    }
}

// ---------------------------------------------------------------------------
// Tests — verify Node → Event mapping
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rootsignal_common::events::Event;
    use rootsignal_common::safety::SensitivityLevel;
    use rootsignal_common::types::*;

    fn test_meta(title: &str) -> NodeMeta {
        NodeMeta {
            id: Uuid::new_v4(),
            title: title.to_string(),
            summary: "test summary".to_string(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.85,
            freshness_score: 0.0,
            corroboration_count: 0,
            about_location: Some(GeoPoint {
                lat: 44.9778,
                lng: -93.265,
                precision: GeoPrecision::Neighborhood,
            }),
            about_location_name: Some("Minneapolis".to_string()),
            from_location: None,
            source_url: "https://example.com".to_string(),
            extracted_at: Utc::now(),
            content_date: None,
            last_confirmed_active: Utc::now(),
            source_diversity: 0,
            external_ratio: 0.0,
            cause_heat: 0.0,
            implied_queries: vec!["test query".to_string()],
            channel_diversity: 1,
            mentioned_actors: vec!["Test Org".to_string()],
            author_actor: None,
        }
    }

    #[test]
    fn gathering_node_maps_to_gathering_discovered_event() {
        let meta = test_meta("Community Dinner");
        let id = meta.id;
        let node = Node::Gathering(GatheringNode {
            meta,
            starts_at: Some(Utc::now()),
            ends_at: None,
            action_url: "https://example.com/signup".to_string(),
            organizer: Some("Lake Street Council".to_string()),
            is_recurring: false,
        });

        let event = node_to_event(&node);
        match event {
            Event::GatheringDiscovered { id: eid, title, organizer, action_url, schedule, .. } => {
                assert_eq!(eid, id);
                assert_eq!(title, "Community Dinner");
                assert_eq!(organizer, Some("Lake Street Council".to_string()));
                assert_eq!(action_url, Some("https://example.com/signup".to_string()));
                assert!(schedule.is_some());
            }
            _ => panic!("Expected GatheringDiscovered"),
        }
    }

    #[test]
    fn aid_node_maps_to_aid_discovered_event() {
        let meta = test_meta("Food Shelf");
        let id = meta.id;
        let node = Node::Aid(AidNode {
            meta,
            action_url: "https://example.com/food".to_string(),
            availability: Some("Mon-Fri".to_string()),
            is_ongoing: true,
        });

        let event = node_to_event(&node);
        match event {
            Event::AidDiscovered { id: eid, title, availability, is_ongoing, .. } => {
                assert_eq!(eid, id);
                assert_eq!(title, "Food Shelf");
                assert_eq!(availability, Some("Mon-Fri".to_string()));
                assert_eq!(is_ongoing, Some(true));
            }
            _ => panic!("Expected AidDiscovered"),
        }
    }

    #[test]
    fn need_node_maps_to_need_discovered_event() {
        let meta = test_meta("Volunteers Needed");
        let id = meta.id;
        let node = Node::Need(NeedNode {
            meta,
            urgency: Urgency::High,
            what_needed: Some("20 volunteers".to_string()),
            action_url: None,
            goal: Some("clean up after storm".to_string()),
        });

        let event = node_to_event(&node);
        match event {
            Event::NeedDiscovered { id: eid, title, urgency, what_needed, goal, .. } => {
                assert_eq!(eid, id);
                assert_eq!(title, "Volunteers Needed");
                assert_eq!(urgency, Some(Urgency::High));
                assert_eq!(what_needed, Some("20 volunteers".to_string()));
                assert_eq!(goal, Some("clean up after storm".to_string()));
            }
            _ => panic!("Expected NeedDiscovered"),
        }
    }

    #[test]
    fn notice_node_maps_to_notice_discovered_event() {
        let meta = test_meta("Water Main Break");
        let id = meta.id;
        let node = Node::Notice(NoticeNode {
            meta,
            severity: Severity::High,
            category: Some("infrastructure".to_string()),
            effective_date: None,
            source_authority: Some("City of Minneapolis".to_string()),
        });

        let event = node_to_event(&node);
        match event {
            Event::NoticeDiscovered { id: eid, title, severity, category, source_authority, .. } => {
                assert_eq!(eid, id);
                assert_eq!(title, "Water Main Break");
                assert_eq!(severity, Some(Severity::High));
                assert_eq!(category, Some("infrastructure".to_string()));
                assert_eq!(source_authority, Some("City of Minneapolis".to_string()));
            }
            _ => panic!("Expected NoticeDiscovered"),
        }
    }

    #[test]
    fn tension_node_maps_to_tension_discovered_event() {
        let meta = test_meta("Housing Shortage");
        let id = meta.id;
        let node = Node::Tension(TensionNode {
            meta,
            severity: Severity::Critical,
            category: Some("housing".to_string()),
            what_would_help: Some("More affordable units".to_string()),
        });

        let event = node_to_event(&node);
        match event {
            Event::TensionDiscovered { id: eid, title, severity, what_would_help, .. } => {
                assert_eq!(eid, id);
                assert_eq!(title, "Housing Shortage");
                assert_eq!(severity, Some(Severity::Critical));
                assert_eq!(what_would_help, Some("More affordable units".to_string()));
            }
            _ => panic!("Expected TensionDiscovered"),
        }
    }

    #[test]
    fn evidence_node_maps_to_citation_recorded_event() {
        let signal_id = Uuid::new_v4();
        let evidence = EvidenceNode {
            id: Uuid::new_v4(),
            source_url: "https://source.com/article".to_string(),
            retrieved_at: Utc::now(),
            content_hash: "abc123".to_string(),
            snippet: Some("relevant text".to_string()),
            relevance: Some("high".to_string()),
            evidence_confidence: Some(0.9),
            channel_type: Some(ChannelType::Press),
        };

        let event = evidence_to_event(&evidence, signal_id);
        match event {
            Event::CitationRecorded { citation_id, entity_id, url, content_hash, snippet, .. } => {
                assert_eq!(citation_id, evidence.id);
                assert_eq!(entity_id, signal_id);
                assert_eq!(url, "https://source.com/article");
                assert_eq!(content_hash, "abc123");
                assert_eq!(snippet, Some("relevant text".to_string()));
            }
            _ => panic!("Expected CitationRecorded"),
        }
    }

    #[test]
    fn event_roundtrips_through_json_payload() {
        let meta = test_meta("Roundtrip Test");
        let node = Node::Gathering(GatheringNode {
            meta,
            starts_at: None,
            ends_at: None,
            action_url: String::new(),
            organizer: None,
            is_recurring: false,
        });

        let event = node_to_event(&node);
        let payload = event.to_payload();
        let roundtripped = Event::from_payload(&payload).unwrap();

        match roundtripped {
            Event::GatheringDiscovered { title, .. } => {
                assert_eq!(title, "Roundtrip Test");
            }
            _ => panic!("Expected GatheringDiscovered after roundtrip"),
        }
    }

    #[test]
    fn location_maps_from_node_meta_geo_point() {
        let meta = test_meta("Location Test");
        let loc = meta_to_location(&meta);
        assert!(loc.is_some());
        let loc = loc.unwrap();
        assert!(loc.point.is_some());
        let point = loc.point.unwrap();
        assert!((point.lat - 44.9778).abs() < f64::EPSILON);
        assert!((point.lng - (-93.265)).abs() < f64::EPSILON);
        assert_eq!(loc.name, Some("Minneapolis".to_string()));
    }

    #[test]
    fn empty_action_url_maps_to_none() {
        let meta = test_meta("No Action URL");
        let node = Node::Gathering(GatheringNode {
            meta,
            starts_at: None,
            ends_at: None,
            action_url: String::new(),
            organizer: None,
            is_recurring: false,
        });

        let event = node_to_event(&node);
        match event {
            Event::GatheringDiscovered { action_url, schedule, .. } => {
                assert!(action_url.is_none());
                assert!(schedule.is_none());
            }
            _ => panic!("Expected GatheringDiscovered"),
        }
    }
}
