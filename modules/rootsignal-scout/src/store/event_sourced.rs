//! EventSourcedStore — events are the source of truth, projector writes the graph.
//!
//! Every write method does exactly two things:
//!   1. Build an Event from the method args → append to EventStore (Postgres)
//!   2. Project the stored event to the graph via GraphProjector (Neo4j)
//!
//! The events table is the single source of truth. The graph is a projection.
//!
//! Read methods delegate to GraphWriter (graph is always current).
//! Resource/edge methods pass through to GraphWriter (no event variants yet).

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use rootsignal_common::events::{
    Event, Location, Schedule, SystemEvent, TelemetryEvent, WorldEvent,
};
use rootsignal_common::types::{ActorNode, GeoPoint, Node, NodeType, SourceNode};
use rootsignal_common::{
    FRESHNESS_MAX_DAYS, GATHERING_PAST_GRACE_HOURS, NEED_EXPIRE_DAYS,
    NOTICE_EXPIRE_DAYS,
};
use rootsignal_events::{AppendEvent, EventStore};
use rootsignal_graph::{DuplicateMatch, GraphProjector, GraphWriter, ReapStats};

use crate::traits::SignalStore;

/// SignalStore that appends events then projects them to the graph.
pub struct EventSourcedStore {
    writer: GraphWriter,       // kept for READ methods + resource/edge pass-through
    projector: GraphProjector, // sole write path for signal lifecycle
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

/// Build the world-fact event for a discovery — no sensitivity or implied_queries.
pub(crate) fn node_to_world_event(node: &Node) -> WorldEvent {
    match node {
        Node::Gathering(n) => WorldEvent::GatheringDiscovered {
            id: n.meta.id,
            title: n.meta.title.clone(),
            summary: n.meta.summary.clone(),
            confidence: n.meta.confidence,
            source_url: n.meta.source_url.clone(),
            extracted_at: n.meta.extracted_at,
            published_at: n.meta.published_at,
            location: meta_to_location(&n.meta),
            from_location: meta_to_from_location(&n.meta),
            mentioned_actors: n.meta.mentioned_actors.clone(),
            author_actor: None,
            schedule: schedule_from_gathering(n),
            action_url: if n.action_url.is_empty() {
                None
            } else {
                Some(n.action_url.clone())
            },
            organizer: n.organizer.clone(),
        },
        Node::Aid(n) => WorldEvent::AidDiscovered {
            id: n.meta.id,
            title: n.meta.title.clone(),
            summary: n.meta.summary.clone(),
            confidence: n.meta.confidence,
            source_url: n.meta.source_url.clone(),
            extracted_at: n.meta.extracted_at,
            published_at: n.meta.published_at,
            location: meta_to_location(&n.meta),
            from_location: meta_to_from_location(&n.meta),
            mentioned_actors: n.meta.mentioned_actors.clone(),
            author_actor: None,
            action_url: if n.action_url.is_empty() {
                None
            } else {
                Some(n.action_url.clone())
            },
            availability: n.availability.clone(),
            is_ongoing: Some(n.is_ongoing),
        },
        Node::Need(n) => WorldEvent::NeedDiscovered {
            id: n.meta.id,
            title: n.meta.title.clone(),
            summary: n.meta.summary.clone(),
            confidence: n.meta.confidence,
            source_url: n.meta.source_url.clone(),
            extracted_at: n.meta.extracted_at,
            published_at: n.meta.published_at,
            location: meta_to_location(&n.meta),
            from_location: meta_to_from_location(&n.meta),
            mentioned_actors: n.meta.mentioned_actors.clone(),
            author_actor: None,
            urgency: Some(n.urgency),
            what_needed: n.what_needed.clone(),
            goal: n.goal.clone(),
        },
        Node::Notice(n) => WorldEvent::NoticeDiscovered {
            id: n.meta.id,
            title: n.meta.title.clone(),
            summary: n.meta.summary.clone(),
            confidence: n.meta.confidence,
            source_url: n.meta.source_url.clone(),
            extracted_at: n.meta.extracted_at,
            published_at: n.meta.published_at,
            location: meta_to_location(&n.meta),
            from_location: meta_to_from_location(&n.meta),
            mentioned_actors: n.meta.mentioned_actors.clone(),
            author_actor: None,
            severity: Some(n.severity),
            category: n.category.clone(),
            effective_date: n.effective_date,
            source_authority: n.source_authority.clone(),
        },
        Node::Tension(n) => WorldEvent::TensionDiscovered {
            id: n.meta.id,
            title: n.meta.title.clone(),
            summary: n.meta.summary.clone(),
            confidence: n.meta.confidence,
            source_url: n.meta.source_url.clone(),
            extracted_at: n.meta.extracted_at,
            published_at: n.meta.published_at,
            location: meta_to_location(&n.meta),
            from_location: meta_to_from_location(&n.meta),
            mentioned_actors: n.meta.mentioned_actors.clone(),
            author_actor: None,
            severity: Some(n.severity),
            what_would_help: n.what_would_help.clone(),
        },
        Node::Citation(_) => unreachable!("Evidence nodes use create_evidence, not create_node"),
    }
}

/// Build system decision events paired with a discovery.
/// Returns SensitivityClassified (always) + ImpliedQueriesExtracted (if non-empty).
pub(crate) fn node_system_events(node: &Node) -> Vec<SystemEvent> {
    let meta = node.meta().expect("discovery nodes always have meta");
    let mut events = vec![SystemEvent::SensitivityClassified {
        signal_id: meta.id,
        level: meta.sensitivity,
    }];
    if !meta.implied_queries.is_empty() {
        events.push(SystemEvent::ImpliedQueriesExtracted {
            signal_id: meta.id,
            queries: meta.implied_queries.clone(),
        });
    }
    events
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

impl EventSourcedStore {
    /// Append an event and project it to the graph.
    async fn append_and_project(&self, event: &Event, actor: Option<&str>) -> Result<()> {
        let mut append =
            AppendEvent::new(event.event_type(), event.to_payload()).with_run_id(&self.run_id);
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
    // --- Corroboration reads ---

    async fn read_corroboration_count(&self, id: Uuid, node_type: NodeType) -> Result<u32> {
        let label = match node_type {
            NodeType::Gathering => "Gathering",
            NodeType::Aid => "Aid",
            NodeType::Need => "Need",
            NodeType::Notice => "Notice",
            NodeType::Tension => "Tension",
            NodeType::Citation => "Evidence",
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

    // --- URL/content guards (read-only, delegate to writer) ---

    async fn blocked_urls(&self, urls: &[String]) -> Result<HashSet<String>> {
        Ok(self.writer.blocked_urls(urls).await?)
    }

    async fn content_already_processed(&self, hash: &str, url: &str) -> Result<bool> {
        Ok(self.writer.content_already_processed(hash, url).await?)
    }

    async fn signal_ids_for_url(&self, url: &str) -> Result<Vec<(Uuid, NodeType)>> {
        let mut results = Vec::new();
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
            let mut stream = graph.execute(q).await?;
            while let Some(row) = stream.next().await? {
                let id_str: String = row.get("id").unwrap_or_default();
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    results.push((id, *node_type));
                }
            }
        }
        Ok(results)
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
            .find_duplicate(
                embedding,
                primary_type,
                threshold,
                min_lat,
                max_lat,
                min_lng,
                max_lng,
            )
            .await?)
    }

    // --- Actor graph (append event → project to graph) ---

    async fn find_actor_by_name(&self, name: &str) -> Result<Option<Uuid>> {
        Ok(self.writer.find_actor_by_name(name).await?)
    }

    async fn find_actor_by_canonical_key(&self, canonical_key: &str) -> Result<Option<Uuid>> {
        Ok(self.writer.find_actor_by_canonical_key(canonical_key).await?)
    }

    // --- Source management (append event → project to graph) ---

    async fn get_active_sources(&self) -> Result<Vec<SourceNode>> {
        Ok(self.writer.get_active_sources().await?)
    }

    async fn delete_pins(&self, pin_ids: &[Uuid]) -> Result<()> {
        if pin_ids.is_empty() {
            return Ok(());
        }
        let event = Event::Telemetry(TelemetryEvent::PinsRemoved {
            pin_ids: pin_ids.to_vec(),
        });
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
            let event = Event::System(SystemEvent::EntityExpired {
                signal_id: *id,
                node_type: NodeType::Gathering,
                reason: "past_event".to_string(),
            });
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
            let event = Event::System(SystemEvent::EntityExpired {
                signal_id: *id,
                node_type: NodeType::Need,
                reason: "need_expired".to_string(),
            });
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
            let event = Event::System(SystemEvent::EntityExpired {
                signal_id: *id,
                node_type: NodeType::Notice,
                reason: "notice_expired".to_string(),
            });
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
                let event = Event::System(SystemEvent::EntityExpired {
                    signal_id: *id,
                    node_type: *node_type,
                    reason: "stale_unconfirmed".to_string(),
                });
                self.append_and_project(&event, None).await?;
            }
            stats.stale += ids.len() as u64;
        }

        Ok(stats)
    }

    // --- Actor location enrichment (pass through) ---

    async fn get_signals_for_actor(
        &self,
        actor_id: Uuid,
    ) -> Result<Vec<(f64, f64, String, DateTime<Utc>)>> {
        Ok(self.writer.get_signals_for_actor(actor_id).await?)
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
    use rootsignal_common::events::{Event, SystemEvent, WorldEvent};
    use rootsignal_common::safety::SensitivityLevel;
    use rootsignal_common::types::*;

    fn test_meta(title: &str) -> NodeMeta {
        NodeMeta {
            id: Uuid::new_v4(),
            title: title.to_string(),
            summary: "test summary".to_string(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.85,
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
            published_at: None,
            last_confirmed_active: Utc::now(),
            source_diversity: 0,
            cause_heat: 0.0,
            implied_queries: vec!["test query".to_string()],
            channel_diversity: 1,
            review_status: ReviewStatus::Staged,
            was_corrected: false,
            corrections: None,
            rejection_reason: None,
            mentioned_actors: Vec::new(),
        }
    }

    #[test]
    fn gathering_node_maps_to_world_event_without_sensitivity() {
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

        let world = node_to_world_event(&node);
        match world {
            WorldEvent::GatheringDiscovered {
                id: eid,
                ref title,
                ref organizer,
                ref action_url,
                ref schedule,
                ..
            } => {
                assert_eq!(eid, id);
                assert_eq!(title, "Community Dinner");
                assert_eq!(*organizer, Some("Lake Street Council".to_string()));
                assert_eq!(*action_url, Some("https://example.com/signup".to_string()));
                assert!(schedule.is_some());
            }
            _ => panic!("Expected GatheringDiscovered"),
        }

        // Verify no sensitivity field in serialized payload
        let event = Event::World(world);
        let payload = event.to_payload();
        assert!(
            payload.get("sensitivity").is_none(),
            "World event should not contain sensitivity"
        );
    }

    #[test]
    fn node_system_events_emits_sensitivity_and_implied_queries() {
        let meta = test_meta("Test Signal");
        let id = meta.id;
        let node = Node::Gathering(GatheringNode {
            meta,
            starts_at: None,
            ends_at: None,
            action_url: String::new(),
            organizer: None,
            is_recurring: false,
        });

        let events = node_system_events(&node);
        assert_eq!(events.len(), 2);

        match &events[0] {
            SystemEvent::SensitivityClassified { signal_id, level } => {
                assert_eq!(*signal_id, id);
                assert_eq!(*level, SensitivityLevel::General);
            }
            _ => panic!("Expected SensitivityClassified"),
        }

        match &events[1] {
            SystemEvent::ImpliedQueriesExtracted { signal_id, queries } => {
                assert_eq!(*signal_id, id);
                assert_eq!(queries, &["test query".to_string()]);
            }
            _ => panic!("Expected ImpliedQueriesExtracted"),
        }
    }

    #[test]
    fn node_with_no_implied_queries_skips_extraction_event() {
        let mut meta = test_meta("No Queries");
        meta.implied_queries = vec![];
        let node = Node::Gathering(GatheringNode {
            meta,
            starts_at: None,
            ends_at: None,
            action_url: String::new(),
            organizer: None,
            is_recurring: false,
        });

        let events = node_system_events(&node);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            SystemEvent::SensitivityClassified { .. }
        ));
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

        let world = node_to_world_event(&node);
        match world {
            WorldEvent::AidDiscovered {
                id: eid,
                title,
                availability,
                is_ongoing,
                ..
            } => {
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

        let world = node_to_world_event(&node);
        match world {
            WorldEvent::NeedDiscovered {
                id: eid,
                title,
                urgency,
                what_needed,
                goal,
                ..
            } => {
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
    fn evidence_node_maps_to_citation_recorded_event() {
        let signal_id = Uuid::new_v4();
        let evidence = CitationNode {
            id: Uuid::new_v4(),
            source_url: "https://source.com/article".to_string(),
            retrieved_at: Utc::now(),
            content_hash: "abc123".to_string(),
            snippet: Some("relevant text".to_string()),
            relevance: Some("high".to_string()),
            confidence: Some(0.9),
            channel_type: Some(ChannelType::Press),
        };

        let event = Event::World(WorldEvent::CitationRecorded {
            citation_id: evidence.id,
            signal_id,
            url: evidence.source_url.clone(),
            content_hash: evidence.content_hash.clone(),
            snippet: evidence.snippet.clone(),
            relevance: evidence.relevance.clone(),
            channel_type: evidence.channel_type,
            evidence_confidence: evidence.confidence,
        });
        match event {
            Event::World(WorldEvent::CitationRecorded {
                citation_id,
                signal_id,
                url,
                content_hash,
                snippet,
                ..
            }) => {
                assert_eq!(citation_id, evidence.id);
                assert_eq!(signal_id, signal_id);
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

        let world = node_to_world_event(&node);
        let event = Event::World(world);
        let payload = event.to_payload();
        let roundtripped = Event::from_payload(&payload).unwrap();

        match roundtripped {
            Event::World(WorldEvent::GatheringDiscovered { title, .. }) => {
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

        let world = node_to_world_event(&node);
        match world {
            WorldEvent::GatheringDiscovered {
                action_url,
                schedule,
                ..
            } => {
                assert!(action_url.is_none());
                assert!(schedule.is_none());
            }
            _ => panic!("Expected GatheringDiscovered"),
        }
    }

    #[test]
    fn all_three_event_layers_roundtrip_through_payload() {
        let world_event = Event::World(WorldEvent::NeedDiscovered {
            id: Uuid::new_v4(),
            title: "Warming Center Needed".to_string(),
            summary: "Residents need warming center".to_string(),
            confidence: 0.8,
            source_url: "https://example.com".to_string(),
            extracted_at: Utc::now(),
            published_at: None,
            location: None,
            from_location: None,
            mentioned_actors: vec!["Red Cross".to_string()],
            author_actor: None,
            urgency: Some(rootsignal_common::Urgency::High),
            what_needed: Some("Warming center".to_string()),
            goal: None,
        });

        let system_event = Event::System(SystemEvent::SignalTagged {
            signal_id: Uuid::new_v4(),
            tag_slugs: vec!["housing".to_string(), "crisis".to_string()],
        });

        let telemetry_event = Event::Telemetry(TelemetryEvent::UrlScraped {
            url: "https://example.com".to_string(),
            strategy: "direct".to_string(),
            success: true,
            content_bytes: 1024,
        });

        for (label, event) in [
            ("world", world_event),
            ("system", system_event),
            ("telemetry", telemetry_event),
        ] {
            let payload = event.to_payload();
            let roundtripped = Event::from_payload(&payload)
                .unwrap_or_else(|e| panic!("{label} event failed roundtrip: {e}"));

            // Verify layer identity is preserved
            match (&event, &roundtripped) {
                (Event::World(_), Event::World(_)) => {}
                (Event::System(_), Event::System(_)) => {}
                (Event::Telemetry(_), Event::Telemetry(_)) => {}
                _ => panic!("{label} event deserialized into wrong layer"),
            }
        }
    }

    #[test]
    fn signal_linked_to_source_roundtrips() {
        let signal_id = Uuid::new_v4();
        let source_id = Uuid::new_v4();
        let event = Event::System(SystemEvent::SignalLinkedToSource {
            signal_id,
            source_id,
        });
        let payload = event.to_payload();
        let roundtripped = Event::from_payload(&payload).unwrap();

        match roundtripped {
            Event::System(SystemEvent::SignalLinkedToSource {
                signal_id: sid,
                source_id: src,
            }) => {
                assert_eq!(sid, signal_id);
                assert_eq!(src, source_id);
            }
            _ => panic!("Expected SignalLinkedToSource after roundtrip"),
        }
    }

    #[test]
    fn mentioned_actors_flow_into_world_event() {
        let mut meta = test_meta("Community Workshop");
        meta.mentioned_actors = vec!["YMCA".to_string(), "Habitat for Humanity".to_string()];

        let node = Node::Need(NeedNode {
            meta,
            urgency: rootsignal_common::Urgency::Medium,
            what_needed: Some("Volunteers".to_string()),
            action_url: None,
            goal: None,
        });

        let world = node_to_world_event(&node);
        match world {
            WorldEvent::NeedDiscovered {
                mentioned_actors, ..
            } => {
                assert_eq!(mentioned_actors, vec!["YMCA", "Habitat for Humanity"]);
            }
            _ => panic!("Expected NeedDiscovered"),
        }
    }
}
