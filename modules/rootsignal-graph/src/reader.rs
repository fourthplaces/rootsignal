use chrono::{DateTime, NaiveDateTime, Utc};
use neo4rs::query;
use uuid::Uuid;

use rootsignal_common::{
    fuzz_location, AskNode, EventNode, EvidenceNode, GeoPoint, GeoPrecision, GiveNode, Node,
    NodeMeta, NodeType, NoticeNode, SensitivityLevel, Severity, StoryNode, TensionNode,
    TensionResponse, Urgency, ASK_EXPIRE_DAYS, CONFIDENCE_DISPLAY_LIMITED, EVENT_PAST_GRACE_HOURS,
    FRESHNESS_MAX_DAYS, NOTICE_EXPIRE_DAYS,
};

use crate::GraphClient;

/// Read-only wrapper for the graph. Used by the web server.
/// Enforces sensitivity-based coordinate fuzzing, confidence thresholds,
/// freshness filtering, and corroboration requirements for sensitive signals.
///
/// Does NOT expose: raw Cypher, actor traversals, temporal queries, or graph topology.
pub struct PublicGraphReader {
    client: GraphClient,
}

impl PublicGraphReader {
    pub fn new(client: GraphClient) -> Self {
        Self { client }
    }

    /// Find signal nodes near a geographic point. Returns fuzzed coordinates.
    /// Filters: confidence >= 0.4, not expired, freshness within threshold.
    pub async fn find_nodes_near(
        &self,
        lat: f64,
        lng: f64,
        radius_km: f64,
        node_types: Option<&[NodeType]>,
    ) -> Result<Vec<Node>, neo4rs::Error> {
        let types = node_types.map(|t| t.to_vec()).unwrap_or_else(|| {
            vec![
                NodeType::Event,
                NodeType::Give,
                NodeType::Ask,
                NodeType::Notice,
                NodeType::Tension,
            ]
        });

        let mut results = Vec::new();

        for nt in &types {
            let label = node_type_label(*nt);
            // Use bounding box on plain lat/lng properties.
            // ~1 degree lat ≈ 111km, 1 degree lng ≈ 111km * cos(lat)
            let lat_delta = radius_km / 111.0;
            let lng_delta = radius_km / (111.0 * lat.to_radians().cos());

            let cypher = format!(
                "MATCH (n:{label})
                 OPTIONAL MATCH (n)<-[:CONTAINS]-(s:Story)
                 WHERE n.lat <> 0.0
                   AND n.lat >= $min_lat AND n.lat <= $max_lat
                   AND n.lng >= $min_lng AND n.lng <= $max_lng
                   AND n.confidence >= $min_confidence
                   {expiry}
                 RETURN n
                 ORDER BY coalesce(s.type_diversity, 0) DESC, n.cause_heat DESC, n.confidence DESC, n.last_confirmed_active DESC
                 LIMIT 200",
                expiry = expiry_clause(*nt),
            );

            let q = query(&cypher)
                .param("min_lat", lat - lat_delta)
                .param("max_lat", lat + lat_delta)
                .param("min_lng", lng - lng_delta)
                .param("max_lng", lng + lng_delta)
                .param("min_confidence", CONFIDENCE_DISPLAY_LIMITED as f64);

            let mut stream = self.client.graph.execute(q).await?;
            while let Some(row) = stream.next().await? {
                if let Some(node) = row_to_node(&row, *nt) {
                    if passes_display_filter(&node) {
                        results.push(fuzz_node(node));
                    }
                }
            }
        }

        Ok(results)
    }

    /// Get a single node by ID with its evidence links. Returns fuzzed coordinates.
    pub async fn get_node_detail(
        &self,
        id: Uuid,
    ) -> Result<Option<(Node, Vec<EvidenceNode>)>, neo4rs::Error> {
        let id_str = id.to_string();

        // Search across all signal types
        for nt in &[
            NodeType::Event,
            NodeType::Give,
            NodeType::Ask,
            NodeType::Notice,
            NodeType::Tension,
        ] {
            let label = node_type_label(*nt);
            let cypher = format!(
                "MATCH (n:{label} {{id: $id}})
                 OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
                 RETURN n, collect(ev) AS evidence"
            );

            let q = query(&cypher).param("id", id_str.as_str());
            let mut stream = self.client.graph.execute(q).await?;

            if let Some(row) = stream.next().await? {
                if let Some(node) = row_to_node(&row, *nt) {
                    if !passes_display_filter(&node) {
                        return Ok(None);
                    }

                    let evidence = extract_evidence(&row);
                    return Ok(Some((fuzz_node(node), evidence)));
                }
            }
        }

        Ok(None)
    }

    /// List recent signals, ordered by triangulation then cause_heat.
    /// Signals in well-triangulated stories surface first. Returns fuzzed coordinates.
    pub async fn list_recent(
        &self,
        limit: u32,
        node_types: Option<&[NodeType]>,
    ) -> Result<Vec<Node>, neo4rs::Error> {
        let types = node_types.map(|t| t.to_vec()).unwrap_or_else(|| {
            vec![
                NodeType::Event,
                NodeType::Give,
                NodeType::Ask,
                NodeType::Notice,
                NodeType::Tension,
            ]
        });

        // Carry (node, story_type_diversity) for cross-type sorting
        let mut ranked: Vec<(Node, i64)> = Vec::new();

        for nt in &types {
            let label = node_type_label(*nt);
            let cypher = format!(
                "MATCH (n:{label})
                 OPTIONAL MATCH (n)<-[:CONTAINS]-(s:Story)
                 WHERE n.confidence >= $min_confidence
                   {expiry}
                 RETURN n, coalesce(s.type_diversity, 0) AS story_triangulation
                 ORDER BY story_triangulation DESC, n.cause_heat DESC, n.last_confirmed_active DESC
                 LIMIT $limit",
                expiry = expiry_clause(*nt),
            );

            let q = query(&cypher)
                .param("min_confidence", CONFIDENCE_DISPLAY_LIMITED as f64)
                .param("limit", limit as i64);

            let mut stream = self.client.graph.execute(q).await?;
            while let Some(row) = stream.next().await? {
                let tri: i64 = row.get("story_triangulation").unwrap_or(0);
                if let Some(node) = row_to_node(&row, *nt) {
                    if passes_display_filter(&node) {
                        ranked.push((fuzz_node(node), tri));
                    }
                }
            }
        }

        // Sort: triangulation (story type diversity) first, then cause_heat, then recency
        ranked.sort_by(|(a, a_tri), (b, b_tri)| {
            b_tri
                .cmp(a_tri)
                .then_with(|| {
                    let a_heat = a.meta().map(|m| m.cause_heat).unwrap_or(0.0);
                    let b_heat = b.meta().map(|m| m.cause_heat).unwrap_or(0.0);
                    b_heat
                        .partial_cmp(&a_heat)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| {
                    let a_time = a.meta().map(|m| m.last_confirmed_active);
                    let b_time = b.meta().map(|m| m.last_confirmed_active);
                    b_time.cmp(&a_time)
                })
        });

        ranked.truncate(limit as usize);
        Ok(ranked.into_iter().map(|(node, _)| node).collect())
    }

    // --- Story queries ---

    /// Get top stories ordered by energy, with optional status filter.
    pub async fn top_stories_by_energy(
        &self,
        limit: u32,
        status_filter: Option<&str>,
    ) -> Result<Vec<StoryNode>, neo4rs::Error> {
        let cypher = match status_filter {
            Some(_) => "MATCH (s:Story) WHERE s.status = $status RETURN s ORDER BY s.energy DESC LIMIT $limit",
            None => "MATCH (s:Story) RETURN s ORDER BY s.energy DESC LIMIT $limit",
        };

        let mut q = query(cypher).param("limit", limit as i64);
        if let Some(status) = status_filter {
            q = q.param("status", status);
        }

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Some(story) = row_to_story(&row) {
                results.push(story);
            }
        }

        Ok(results)
    }

    /// List recent signals scoped to a city's geographic bounding box.
    pub async fn list_recent_for_city(
        &self,
        lat: f64,
        lng: f64,
        radius_km: f64,
        limit: u32,
    ) -> Result<Vec<Node>, neo4rs::Error> {
        let lat_delta = radius_km / 111.0;
        let lng_delta = radius_km / (111.0 * lat.to_radians().cos());

        let mut all: Vec<Node> = Vec::new();
        for nt in &[
            NodeType::Event,
            NodeType::Give,
            NodeType::Ask,
            NodeType::Notice,
            NodeType::Tension,
        ] {
            let label = node_type_label(*nt);
            let cypher = format!(
                "MATCH (n:{label})
                 WHERE n.lat <> 0.0
                   AND n.lat >= $min_lat AND n.lat <= $max_lat
                   AND n.lng >= $min_lng AND n.lng <= $max_lng
                 RETURN n
                 ORDER BY coalesce(n.cause_heat, 0) DESC, n.last_confirmed_active DESC
                 LIMIT $limit"
            );
            let q = query(&cypher)
                .param("min_lat", lat - lat_delta)
                .param("max_lat", lat + lat_delta)
                .param("min_lng", lng - lng_delta)
                .param("max_lng", lng + lng_delta)
                .param("limit", limit as i64);
            let mut stream = self.client.graph.execute(q).await?;
            while let Some(row) = stream.next().await? {
                if let Some(node) = row_to_node(&row, *nt) {
                    all.push(node);
                }
            }
        }
        // Sort by cause_heat (tension-connected signals first), then recency
        all.sort_by(|a, b| {
            let a_heat = a.meta().map(|m| m.cause_heat).unwrap_or(0.0);
            let b_heat = b.meta().map(|m| m.cause_heat).unwrap_or(0.0);
            b_heat
                .partial_cmp(&a_heat)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    let a_time = a.meta().map(|m| m.last_confirmed_active);
                    let b_time = b.meta().map(|m| m.last_confirmed_active);
                    b_time.cmp(&a_time)
                })
        });
        all.truncate(limit as usize);
        Ok(all)
    }

    /// Top stories by energy, scoped to a city's geographic bounding box (via story centroid).
    pub async fn top_stories_for_city(
        &self,
        lat: f64,
        lng: f64,
        radius_km: f64,
        limit: u32,
    ) -> Result<Vec<StoryNode>, neo4rs::Error> {
        let lat_delta = radius_km / 111.0;
        let lng_delta = radius_km / (111.0 * lat.to_radians().cos());

        let q = query(
            "MATCH (s:Story)
             WHERE s.centroid_lat IS NOT NULL
               AND s.centroid_lat >= $min_lat AND s.centroid_lat <= $max_lat
               AND s.centroid_lng >= $min_lng AND s.centroid_lng <= $max_lng
             RETURN s
             ORDER BY s.energy DESC
             LIMIT $limit",
        )
        .param("min_lat", lat - lat_delta)
        .param("max_lat", lat + lat_delta)
        .param("min_lng", lng - lng_delta)
        .param("max_lng", lng + lng_delta)
        .param("limit", limit as i64);

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Some(story) = row_to_story(&row) {
                results.push(story);
            }
        }
        Ok(results)
    }

    /// Get a single story with its constituent signals.
    pub async fn get_story_with_signals(
        &self,
        story_id: Uuid,
    ) -> Result<Option<(StoryNode, Vec<Node>)>, neo4rs::Error> {
        // First get the story
        let q = query("MATCH (s:Story {id: $id}) RETURN s").param("id", story_id.to_string());

        let mut stream = self.client.graph.execute(q).await?;
        let story = match stream.next().await? {
            Some(row) => match row_to_story(&row) {
                Some(s) => s,
                None => return Ok(None),
            },
            None => return Ok(None),
        };

        // Then get constituent signals
        let signals = self.get_story_signals(story_id, false).await?;

        Ok(Some((story, signals)))
    }

    /// Get the constituent signals for a story.
    /// When `upcoming_only` is true, filters Events to those with starts_at >= now
    /// and Asks to those with last_confirmed_active within 7 days.
    pub async fn get_story_signals(
        &self,
        story_id: Uuid,
        upcoming_only: bool,
    ) -> Result<Vec<Node>, neo4rs::Error> {
        let mut signals = Vec::new();

        for nt in &[
            NodeType::Event,
            NodeType::Give,
            NodeType::Ask,
            NodeType::Notice,
            NodeType::Tension,
        ] {
            let label = node_type_label(*nt);
            let where_clause = if upcoming_only {
                match nt {
                    NodeType::Event => " AND (n.starts_at IS NULL OR n.starts_at >= datetime())",
                    NodeType::Ask => " AND (n.last_confirmed_active IS NULL OR datetime(n.last_confirmed_active) >= datetime() - duration('P7D'))",
                    _ => "",
                }
            } else {
                ""
            };
            let cypher = format!(
                "MATCH (s:Story {{id: $id}})-[:CONTAINS]->(n:{label})
                 WHERE true{where_clause}
                 RETURN n
                 ORDER BY n.confidence DESC"
            );

            let q = query(&cypher).param("id", story_id.to_string());
            let mut stream = self.client.graph.execute(q).await?;
            while let Some(row) = stream.next().await? {
                if let Some(node) = row_to_node(&row, *nt) {
                    if passes_display_filter(&node) {
                        signals.push(fuzz_node(node));
                    }
                }
            }
        }

        Ok(signals)
    }

    /// Fetch evidence nodes for a signal by ID.
    pub async fn get_signal_evidence(
        &self,
        signal_id: Uuid,
    ) -> Result<Vec<EvidenceNode>, neo4rs::Error> {
        let id_str = signal_id.to_string();

        for nt in &[
            NodeType::Event,
            NodeType::Give,
            NodeType::Ask,
            NodeType::Notice,
            NodeType::Tension,
        ] {
            let label = node_type_label(*nt);
            let cypher = format!(
                "MATCH (n:{label} {{id: $id}})-[:SOURCED_FROM]->(ev:Evidence)
                 RETURN collect(ev) AS evidence"
            );

            let q = query(&cypher).param("id", id_str.as_str());
            let mut stream = self.client.graph.execute(q).await?;

            if let Some(row) = stream.next().await? {
                let evidence = extract_evidence(&row);
                if !evidence.is_empty() {
                    return Ok(evidence);
                }
            }
        }

        Ok(Vec::new())
    }

    /// Batch query for evidence counts per story.
    pub async fn story_evidence_counts(
        &self,
        story_ids: &[Uuid],
    ) -> Result<Vec<(Uuid, u32)>, neo4rs::Error> {
        if story_ids.is_empty() {
            return Ok(Vec::new());
        }

        let ids: Vec<String> = story_ids.iter().map(|id| id.to_string()).collect();
        let cypher = "MATCH (s:Story)-[:CONTAINS]->(n)-[:SOURCED_FROM]->(ev:Evidence)
                      WHERE s.id IN $ids
                      RETURN s.id AS story_id, count(DISTINCT ev) AS evidence_count";

        let q = query(cypher).param("ids", ids);
        let mut stream = self.client.graph.execute(q).await?;
        let mut results = Vec::new();

        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("story_id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                let cnt: i64 = row.get("evidence_count").unwrap_or(0);
                results.push((id, cnt as u32));
            }
        }

        Ok(results)
    }

    /// Batch-fetch evidence for all signals in a story. Returns (signal_id, Vec<EvidenceNode>) pairs.
    pub async fn get_story_signal_evidence(
        &self,
        story_id: Uuid,
    ) -> Result<Vec<(Uuid, Vec<EvidenceNode>)>, neo4rs::Error> {
        let cypher = "MATCH (s:Story {id: $id})-[:CONTAINS]->(n)-[:SOURCED_FROM]->(ev:Evidence)
             WHERE n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension
             RETURN n.id AS signal_id, collect(ev) AS evidence";

        let q = query(cypher).param("id", story_id.to_string());
        let mut stream = self.client.graph.execute(q).await?;
        let mut results = Vec::new();

        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("signal_id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                let evidence = extract_evidence(&row);
                results.push((id, evidence));
            }
        }

        Ok(results)
    }

    /// Batch-fetch response signals for all tensions in a story.
    /// Returns (tension_id, Vec<response_summary>) pairs.
    pub async fn get_story_tension_responses(
        &self,
        story_id: Uuid,
    ) -> Result<Vec<(Uuid, Vec<serde_json::Value>)>, neo4rs::Error> {
        let cypher =
            "MATCH (s:Story {id: $id})-[:CONTAINS]->(t:Tension)<-[rel:RESPONDS_TO|DRAWN_TO]-(resp)
             WHERE resp:Give OR resp:Event OR resp:Ask
             RETURN t.id AS tension_id, resp.id AS resp_id, resp.title AS resp_title,
                    resp.summary AS resp_summary,
                    labels(resp) AS resp_labels,
                    rel.match_strength AS match_strength,
                    rel.explanation AS explanation,
                    type(rel) AS edge_type,
                    rel.gathering_type AS gathering_type";

        let q = query(cypher).param("id", story_id.to_string());
        let mut stream = self.client.graph.execute(q).await?;

        let mut map: std::collections::HashMap<Uuid, Vec<serde_json::Value>> =
            std::collections::HashMap::new();

        while let Some(row) = stream.next().await? {
            let tid_str: String = row.get("tension_id").unwrap_or_default();
            let Ok(tid) = Uuid::parse_str(&tid_str) else {
                continue;
            };
            let rid_str: String = row.get("resp_id").unwrap_or_default();
            let title: String = row.get("resp_title").unwrap_or_default();
            let summary: String = row.get("resp_summary").unwrap_or_default();
            let labels: Vec<String> = row.get("resp_labels").unwrap_or_default();
            let node_type = labels
                .iter()
                .find(|l| *l != "Node")
                .cloned()
                .unwrap_or_default();
            let match_strength: f64 = row.get("match_strength").unwrap_or(0.0);
            let explanation: String = row.get("explanation").unwrap_or_default();
            let edge_type: String = row.get("edge_type").unwrap_or_default();
            let gathering_type: Option<String> = row.get::<String>("gathering_type").ok();

            map.entry(tid).or_default().push(serde_json::json!({
                "id": rid_str,
                "title": title,
                "summary": summary,
                "node_type": node_type,
                "match_strength": match_strength,
                "explanation": explanation,
                "edge_type": edge_type,
                "gathering_type": gathering_type,
            }));
        }

        Ok(map.into_iter().collect())
    }

    /// Get a single signal by ID.
    pub async fn get_signal_by_id(&self, id: Uuid) -> Result<Option<Node>, neo4rs::Error> {
        match self.get_node_detail(id).await? {
            Some((node, _)) => Ok(Some(node)),
            None => Ok(None),
        }
    }

    // --- Story filter queries ---

    /// Get stories filtered by category.
    pub async fn stories_by_category(
        &self,
        category: &str,
        limit: u32,
    ) -> Result<Vec<StoryNode>, neo4rs::Error> {
        let q = query(
            "MATCH (s:Story) WHERE s.category = $category
             RETURN s ORDER BY s.energy DESC LIMIT $limit",
        )
        .param("category", category)
        .param("limit", limit as i64);

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Some(story) = row_to_story(&row) {
                results.push(story);
            }
        }
        Ok(results)
    }

    /// Get stories filtered by arc phase.
    pub async fn stories_by_arc(
        &self,
        arc: &str,
        limit: u32,
    ) -> Result<Vec<StoryNode>, neo4rs::Error> {
        let q = query(
            "MATCH (s:Story) WHERE s.arc = $arc
             RETURN s ORDER BY s.energy DESC LIMIT $limit",
        )
        .param("arc", arc)
        .param("limit", limit as i64);

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Some(story) = row_to_story(&row) {
                results.push(story);
            }
        }
        Ok(results)
    }

    // --- Actor queries ---

    /// List actors active in a city.
    pub async fn actors_active_in_area(
        &self,
        city: &str,
        limit: u32,
    ) -> Result<Vec<rootsignal_common::ActorNode>, neo4rs::Error> {
        let q = query(
            "MATCH (a:Actor)
             WHERE a.city = $city
             RETURN a
             ORDER BY a.last_active DESC
             LIMIT $limit",
        )
        .param("city", city)
        .param("limit", limit as i64);

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Some(actor) = row_to_actor(&row) {
                results.push(actor);
            }
        }
        Ok(results)
    }

    /// Get a single actor by ID with recent signals.
    pub async fn actor_detail(
        &self,
        actor_id: Uuid,
    ) -> Result<Option<rootsignal_common::ActorNode>, neo4rs::Error> {
        let q = query("MATCH (a:Actor {id: $id}) RETURN a").param("id", actor_id.to_string());

        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            return Ok(row_to_actor(&row));
        }
        Ok(None)
    }

    /// Get stories involving an actor (via ACTED_IN -> signals -> CONTAINS <- stories).
    pub async fn actor_stories(
        &self,
        actor_id: Uuid,
        limit: u32,
    ) -> Result<Vec<StoryNode>, neo4rs::Error> {
        let q = query(
            "MATCH (a:Actor {id: $id})-[:ACTED_IN]->(n)<-[:CONTAINS]-(s:Story)
             RETURN DISTINCT s
             ORDER BY s.energy DESC
             LIMIT $limit",
        )
        .param("id", actor_id.to_string())
        .param("limit", limit as i64);

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Some(story) = row_to_story(&row) {
                results.push(story);
            }
        }
        Ok(results)
    }

    /// Get actors involved in a story.
    pub async fn actors_for_story(
        &self,
        story_id: Uuid,
    ) -> Result<Vec<rootsignal_common::ActorNode>, neo4rs::Error> {
        let q = query(
            "MATCH (s:Story {id: $id})-[:CONTAINS]->(n)<-[:ACTED_IN]-(a:Actor)
             RETURN DISTINCT a",
        )
        .param("id", story_id.to_string());

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Some(actor) = row_to_actor(&row) {
                results.push(actor);
            }
        }
        Ok(results)
    }

    // --- Tension response queries ---

    /// Get Give/Event/Ask signals that respond to a tension, with edge metadata.
    pub async fn tension_responses(
        &self,
        tension_id: Uuid,
    ) -> Result<Vec<TensionResponse>, neo4rs::Error> {
        let mut results = Vec::new();

        for nt in &[NodeType::Give, NodeType::Event, NodeType::Ask] {
            let label = node_type_label(*nt);
            let cypher = format!(
                "MATCH (t:Tension {{id: $id}})<-[rel:RESPONDS_TO|DRAWN_TO]-(n:{label})
                 RETURN n, rel.match_strength AS match_strength, rel.explanation AS explanation
                 ORDER BY n.confidence DESC"
            );

            let q = query(&cypher).param("id", tension_id.to_string());
            let mut stream = self.client.graph.execute(q).await?;
            while let Some(row) = stream.next().await? {
                if let Some(node) = row_to_node(&row, *nt) {
                    if passes_display_filter(&node) {
                        let match_strength: f64 = row.get("match_strength").unwrap_or(0.0);
                        let explanation: String = row.get("explanation").unwrap_or_default();
                        results.push(TensionResponse {
                            node: fuzz_node(node),
                            match_strength,
                            explanation,
                        });
                    }
                }
            }
        }

        Ok(results)
    }

    // --- Bounding-box & semantic search queries (for search app) ---

    /// Find signals within a bounding box, sorted by cause_heat.
    /// Used by the search app when no text query is active.
    pub async fn signals_in_bounds(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
        limit: u32,
    ) -> Result<Vec<Node>, neo4rs::Error> {
        let mut all: Vec<Node> = Vec::new();
        for nt in &[
            NodeType::Event,
            NodeType::Give,
            NodeType::Ask,
            NodeType::Notice,
            NodeType::Tension,
        ] {
            let label = node_type_label(*nt);
            let cypher = format!(
                "MATCH (n:{label})
                 WHERE n.lat <> 0.0
                   AND n.lat >= $min_lat AND n.lat <= $max_lat
                   AND n.lng >= $min_lng AND n.lng <= $max_lng
                   AND n.confidence >= $min_confidence
                   {expiry}
                 RETURN n
                 ORDER BY coalesce(n.cause_heat, 0) DESC, n.confidence DESC
                 LIMIT $limit",
                expiry = expiry_clause(*nt),
            );
            let q = query(&cypher)
                .param("min_lat", min_lat)
                .param("max_lat", max_lat)
                .param("min_lng", min_lng)
                .param("max_lng", max_lng)
                .param("min_confidence", CONFIDENCE_DISPLAY_LIMITED as f64)
                .param("limit", limit as i64);
            let mut stream = self.client.graph.execute(q).await?;
            while let Some(row) = stream.next().await? {
                if let Some(node) = row_to_node(&row, *nt) {
                    if passes_display_filter(&node) {
                        all.push(fuzz_node(node));
                    }
                }
            }
        }
        all.sort_by(|a, b| {
            let a_heat = a.meta().map(|m| m.cause_heat).unwrap_or(0.0);
            let b_heat = b.meta().map(|m| m.cause_heat).unwrap_or(0.0);
            b_heat
                .partial_cmp(&a_heat)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        all.truncate(limit as usize);
        Ok(all)
    }

    /// Find stories within a bounding box (by centroid), sorted by energy.
    /// Excludes archived stories. Used by the search app when no text query is active.
    pub async fn stories_in_bounds(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
        limit: u32,
    ) -> Result<Vec<StoryNode>, neo4rs::Error> {
        let q = query(
            "MATCH (s:Story)
             WHERE s.centroid_lat IS NOT NULL
               AND s.centroid_lat >= $min_lat AND s.centroid_lat <= $max_lat
               AND s.centroid_lng >= $min_lng AND s.centroid_lng <= $max_lng
               AND (s.arc IS NULL OR s.arc <> 'archived')
             RETURN s
             ORDER BY s.energy DESC
             LIMIT $limit",
        )
        .param("min_lat", min_lat)
        .param("max_lat", max_lat)
        .param("min_lng", min_lng)
        .param("max_lng", max_lng)
        .param("limit", limit as i64);

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Some(story) = row_to_story(&row) {
                results.push(story);
            }
        }
        Ok(results)
    }

    /// Find tensions with < 2 respondents that aren't in any story, within a bounding box.
    /// Sorted by cause_heat DESC. Used to surface unresponded community needs.
    pub async fn unresponded_tensions_in_bounds(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
        limit: u32,
    ) -> Result<Vec<Node>, neo4rs::Error> {
        let q = query(
            "MATCH (t:Tension)
             WHERE t.lat IS NOT NULL
               AND t.lat >= $min_lat AND t.lat <= $max_lat
               AND t.lng >= $min_lng AND t.lng <= $max_lng
               AND NOT (t)<-[:CONTAINS]-(:Story)
             OPTIONAL MATCH (t)<-[:RESPONDS_TO|DRAWN_TO]-(r)
             WITH t, count(r) AS resp_count
             WHERE resp_count < 2
             RETURN t AS n
             ORDER BY coalesce(t.cause_heat, 0.0) DESC
             LIMIT $limit",
        )
        .param("min_lat", min_lat)
        .param("max_lat", max_lat)
        .param("min_lng", min_lng)
        .param("max_lng", max_lng)
        .param("limit", limit as i64);

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Some(node) = row_to_node(&row, NodeType::Tension) {
                results.push(fuzz_node(node));
            }
        }
        Ok(results)
    }

    /// Semantic search for signals within a bounding box using vector KNN.
    /// Over-fetches from the vector index (K per type), then post-filters by bbox.
    /// Returns (node, blended_score) pairs sorted by blended score.
    pub async fn semantic_search_signals_in_bounds(
        &self,
        embedding: &[f32],
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
        limit: u32,
    ) -> Result<Vec<(Node, f64)>, neo4rs::Error> {
        let embedding_vec: Vec<f64> = embedding.iter().map(|&v| v as f64).collect();
        let k_per_type = 100_i64;
        let min_score = 0.3_f64;

        let mut scored: Vec<(Node, f64)> = Vec::new();

        let index_names = [
            ("Event", "event_embedding"),
            ("Give", "give_embedding"),
            ("Ask", "ask_embedding"),
            ("Notice", "notice_embedding"),
            ("Tension", "tension_embedding"),
        ];
        let type_map = [
            ("Event", NodeType::Event),
            ("Give", NodeType::Give),
            ("Ask", NodeType::Ask),
            ("Notice", NodeType::Notice),
            ("Tension", NodeType::Tension),
        ];
        let type_lookup: std::collections::HashMap<&str, NodeType> =
            type_map.iter().cloned().collect();

        for (label, index_name) in &index_names {
            let nt = type_lookup[label];
            let cypher = format!(
                "CALL db.index.vector.queryNodes($index_name, $k, $embedding)
                 YIELD node, score
                 WHERE score >= $min_score
                   AND node.lat <> 0.0
                   AND node.lat >= $min_lat AND node.lat <= $max_lat
                   AND node.lng >= $min_lng AND node.lng <= $max_lng
                   AND node.confidence >= $min_confidence
                 RETURN node AS n, score"
            );

            let q = query(&cypher)
                .param("index_name", *index_name)
                .param("k", k_per_type)
                .param("embedding", embedding_vec.clone())
                .param("min_score", min_score)
                .param("min_lat", min_lat)
                .param("max_lat", max_lat)
                .param("min_lng", min_lng)
                .param("max_lng", max_lng)
                .param("min_confidence", CONFIDENCE_DISPLAY_LIMITED as f64);

            let mut stream = self.client.graph.execute(q).await?;
            while let Some(row) = stream.next().await? {
                let similarity: f64 = row.get("score").unwrap_or(0.0);
                if let Some(node) = row_to_node(&row, nt) {
                    if passes_display_filter(&node) {
                        let heat = node.meta().map(|m| m.cause_heat).unwrap_or(0.0);
                        let blended = similarity * 0.6 + heat * 0.4;
                        scored.push((fuzz_node(node), blended));
                    }
                }
            }
        }

        scored.sort_by(|(_, a), (_, b)| {
            b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(limit as usize);
        Ok(scored)
    }

    /// Semantic search for stories within a bounding box.
    /// Stories lack embeddings, so we search signals via KNN and aggregate to parent stories.
    /// Returns (story, best_signal_score, best_signal_title) tuples sorted by blended score.
    pub async fn semantic_search_stories_in_bounds(
        &self,
        embedding: &[f32],
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
        limit: u32,
    ) -> Result<Vec<(StoryNode, f64, String)>, neo4rs::Error> {
        let embedding_vec: Vec<f64> = embedding.iter().map(|&v| v as f64).collect();
        let k_per_type = 100_i64;
        let min_score = 0.3_f64;

        // Collect (story_id -> (best_similarity, best_signal_title)) from signal search
        let mut story_scores: std::collections::HashMap<Uuid, (f64, String)> =
            std::collections::HashMap::new();

        let index_names = [
            ("Event", "event_embedding"),
            ("Give", "give_embedding"),
            ("Ask", "ask_embedding"),
            ("Notice", "notice_embedding"),
            ("Tension", "tension_embedding"),
        ];

        for (_label, index_name) in &index_names {
            let cypher =
                "CALL db.index.vector.queryNodes($index_name, $k, $embedding)
                 YIELD node, score
                 WHERE score >= $min_score
                   AND node.lat <> 0.0
                   AND node.lat >= $min_lat AND node.lat <= $max_lat
                   AND node.lng >= $min_lng AND node.lng <= $max_lng
                 WITH node, score
                 MATCH (s:Story)-[:CONTAINS]->(node)
                 RETURN s.id AS story_id, score, node.title AS signal_title";

            let q = query(cypher)
                .param("index_name", *index_name)
                .param("k", k_per_type)
                .param("embedding", embedding_vec.clone())
                .param("min_score", min_score)
                .param("min_lat", min_lat)
                .param("max_lat", max_lat)
                .param("min_lng", min_lng)
                .param("max_lng", max_lng);

            let mut stream = self.client.graph.execute(q).await?;
            while let Some(row) = stream.next().await? {
                let sid_str: String = row.get("story_id").unwrap_or_default();
                let Ok(sid) = Uuid::parse_str(&sid_str) else {
                    continue;
                };
                let score: f64 = row.get("score").unwrap_or(0.0);
                let title: String = row.get("signal_title").unwrap_or_default();

                let entry = story_scores.entry(sid).or_insert((0.0, String::new()));
                if score > entry.0 {
                    *entry = (score, title);
                }
            }
        }

        if story_scores.is_empty() {
            return Ok(Vec::new());
        }

        // Fetch full story nodes for the matched IDs
        let story_ids: Vec<String> = story_scores.keys().map(|id| id.to_string()).collect();
        let q = query(
            "MATCH (s:Story)
             WHERE s.id IN $ids
             RETURN s",
        )
        .param("ids", story_ids);

        let mut results: Vec<(StoryNode, f64, String)> = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Some(story) = row_to_story(&row) {
                if let Some((best_sim, best_title)) = story_scores.get(&story.id) {
                    let blended = best_sim * 0.6 + story.energy * 0.4;
                    results.push((story, blended, best_title.clone()));
                }
            }
        }

        results.sort_by(|(_, a, _), (_, b, _)| {
            b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit as usize);
        Ok(results)
    }

    // --- Admin/Quality queries (not public-facing, but through reader for safety) ---

    /// Get total signal count by type (for quality dashboard).
    pub async fn count_by_type(&self) -> Result<Vec<(NodeType, u64)>, neo4rs::Error> {
        let mut counts = Vec::new();
        for nt in &[
            NodeType::Event,
            NodeType::Give,
            NodeType::Ask,
            NodeType::Notice,
            NodeType::Tension,
        ] {
            let label = node_type_label(*nt);
            let q = query(&format!("MATCH (n:{label}) RETURN count(n) AS cnt"));
            let mut stream = self.client.graph.execute(q).await?;
            if let Some(row) = stream.next().await? {
                let cnt: i64 = row.get("cnt").unwrap_or(0);
                counts.push((*nt, cnt as u64));
            }
        }
        Ok(counts)
    }

    /// Get confidence distribution (for quality dashboard).
    pub async fn confidence_distribution(&self) -> Result<Vec<(String, u64)>, neo4rs::Error> {
        let q = query(
            "MATCH (n)
            WHERE n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension
            WITH CASE
                WHEN n.confidence >= 0.8 THEN 'high (0.8+)'
                WHEN n.confidence >= 0.6 THEN 'good (0.6-0.8)'
                WHEN n.confidence >= 0.4 THEN 'limited (0.4-0.6)'
                ELSE 'low (<0.4)'
            END AS bucket
            RETURN bucket, count(*) AS cnt
            ORDER BY bucket",
        );

        let mut stream = self.client.graph.execute(q).await?;
        let mut results = Vec::new();
        while let Some(row) = stream.next().await? {
            let bucket: String = row.get("bucket").unwrap_or_default();
            let cnt: i64 = row.get("cnt").unwrap_or(0);
            results.push((bucket, cnt as u64));
        }
        Ok(results)
    }

    /// Get freshness distribution (for quality dashboard).
    pub async fn freshness_distribution(&self) -> Result<Vec<(String, u64)>, neo4rs::Error> {
        let q = query(
            "MATCH (n)
            WHERE n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension
            WITH (datetime() - n.last_confirmed_active).day AS age_days
            WITH CASE
                WHEN age_days <= 7 THEN '< 7 days'
                WHEN age_days <= 30 THEN '7-30 days'
                WHEN age_days <= 90 THEN '30-90 days'
                ELSE '> 90 days'
            END AS bucket
            RETURN bucket, count(*) AS cnt
            ORDER BY bucket",
        );

        let mut stream = self.client.graph.execute(q).await?;
        let mut results = Vec::new();
        while let Some(row) = stream.next().await? {
            let bucket: String = row.get("bucket").unwrap_or_default();
            let cnt: i64 = row.get("cnt").unwrap_or(0);
            results.push((bucket, cnt as u64));
        }
        Ok(results)
    }

    /// Total signal count across all types (for quality dashboard).
    pub async fn total_count(&self) -> Result<u64, neo4rs::Error> {
        let counts = self.count_by_type().await?;
        Ok(counts.iter().map(|(_, c)| c).sum())
    }

    /// Signal volume by day for last 30 days, grouped by type.
    /// Returns Vec<(date_string, event, give, ask, notice, tension)>.
    pub async fn signal_volume_by_day(
        &self,
    ) -> Result<Vec<(String, u64, u64, u64, u64, u64)>, neo4rs::Error> {
        let q = query(
            "WITH date(datetime() - duration('P30D')) AS cutoff
             UNWIND range(0, 29) AS offset
             WITH date(datetime() - duration('P' + toString(offset) + 'D')) AS day
             OPTIONAL MATCH (e:Event) WHERE date(e.extracted_at) = day
             WITH day, count(e) AS events
             OPTIONAL MATCH (g:Give) WHERE date(g.extracted_at) = day
             WITH day, events, count(g) AS gives
             OPTIONAL MATCH (a:Ask) WHERE date(a.extracted_at) = day
             WITH day, events, gives, count(a) AS asks
             OPTIONAL MATCH (n:Notice) WHERE date(n.extracted_at) = day
             WITH day, events, gives, asks, count(n) AS notices
             OPTIONAL MATCH (t:Tension) WHERE date(t.extracted_at) = day
             RETURN toString(day) AS day, events, gives, asks, notices, count(t) AS tensions
             ORDER BY day",
        );

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let day: String = row.get("day").unwrap_or_default();
            let events: i64 = row.get("events").unwrap_or(0);
            let gives: i64 = row.get("gives").unwrap_or(0);
            let asks: i64 = row.get("asks").unwrap_or(0);
            let notices: i64 = row.get("notices").unwrap_or(0);
            let tensions: i64 = row.get("tensions").unwrap_or(0);
            results.push((
                day,
                events as u64,
                gives as u64,
                asks as u64,
                notices as u64,
                tensions as u64,
            ));
        }
        Ok(results)
    }

    /// Story count grouped by arc (30-day window).
    pub async fn story_count_by_arc(&self) -> Result<Vec<(String, u64)>, neo4rs::Error> {
        let q = query(
            "MATCH (s:Story)
             WHERE s.last_updated >= datetime() - duration('P30D')
             RETURN coalesce(s.arc, 'unknown') AS arc, count(s) AS cnt
             ORDER BY cnt DESC",
        );

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let arc: String = row.get("arc").unwrap_or_default();
            let cnt: i64 = row.get("cnt").unwrap_or(0);
            results.push((arc, cnt as u64));
        }
        Ok(results)
    }

    /// Story count grouped by category (30-day window).
    pub async fn story_count_by_category(&self) -> Result<Vec<(String, u64)>, neo4rs::Error> {
        let q = query(
            "MATCH (s:Story)
             WHERE s.last_updated >= datetime() - duration('P30D')
             RETURN coalesce(s.category, 'uncategorized') AS category, count(s) AS cnt
             ORDER BY cnt DESC",
        );

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let category: String = row.get("category").unwrap_or_default();
            let cnt: i64 = row.get("cnt").unwrap_or(0);
            results.push((category, cnt as u64));
        }
        Ok(results)
    }

    /// Total story count.
    pub async fn story_count(&self) -> Result<u64, neo4rs::Error> {
        let q = query("MATCH (s:Story) RETURN count(s) AS cnt");
        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let cnt: i64 = row.get("cnt").unwrap_or(0);
            return Ok(cnt as u64);
        }
        Ok(0)
    }

    /// Total actor count.
    pub async fn actor_count(&self) -> Result<u64, neo4rs::Error> {
        let q = query("MATCH (a:Actor) RETURN count(a) AS cnt");
        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let cnt: i64 = row.get("cnt").unwrap_or(0);
            return Ok(cnt as u64);
        }
        Ok(0)
    }

    // --- Batch queries for DataLoaders ---

    /// Get a single story by ID (without signals).
    pub async fn get_story_by_id(&self, id: Uuid) -> Result<Option<StoryNode>, neo4rs::Error> {
        let q = query("MATCH (s:Story {id: $id}) RETURN s").param("id", id.to_string());
        let mut stream = self.client.graph.execute(q).await?;
        match stream.next().await? {
            Some(row) => Ok(row_to_story(&row)),
            None => Ok(None),
        }
    }

    /// Batch-fetch evidence for multiple signal IDs. Returns map of signal_id -> Vec<EvidenceNode>.
    pub async fn batch_evidence_by_signal_ids(
        &self,
        ids: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, Vec<EvidenceNode>>, neo4rs::Error> {
        let mut map: std::collections::HashMap<Uuid, Vec<EvidenceNode>> =
            std::collections::HashMap::new();

        if ids.is_empty() {
            return Ok(map);
        }

        let id_strs: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
        let cypher = "MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
             WHERE n.id IN $ids AND (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension)
             RETURN n.id AS signal_id, collect(ev) AS evidence";

        let q = query(cypher).param("ids", id_strs);
        let mut stream = self.client.graph.execute(q).await?;

        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("signal_id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                let evidence = extract_evidence(&row);
                map.insert(id, evidence);
            }
        }

        Ok(map)
    }

    /// Batch-fetch actors for multiple signal IDs. Returns map of signal_id -> Vec<ActorNode>.
    pub async fn batch_actors_by_signal_ids(
        &self,
        ids: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, Vec<rootsignal_common::ActorNode>>, neo4rs::Error>
    {
        let mut map: std::collections::HashMap<Uuid, Vec<rootsignal_common::ActorNode>> =
            std::collections::HashMap::new();

        if ids.is_empty() {
            return Ok(map);
        }

        let id_strs: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
        let cypher = "MATCH (a:Actor)-[:ACTED_IN]->(n)
             WHERE n.id IN $ids
             RETURN n.id AS signal_id, a";

        let q = query(cypher).param("ids", id_strs);
        let mut stream = self.client.graph.execute(q).await?;

        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("signal_id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                if let Some(actor) = row_to_actor(&row) {
                    map.entry(id).or_default().push(actor);
                }
            }
        }

        Ok(map)
    }

    /// Batch-fetch the parent story for multiple signal IDs. Returns map of signal_id -> StoryNode.
    pub async fn batch_story_by_signal_ids(
        &self,
        ids: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, StoryNode>, neo4rs::Error> {
        let mut map: std::collections::HashMap<Uuid, StoryNode> = std::collections::HashMap::new();

        if ids.is_empty() {
            return Ok(map);
        }

        let id_strs: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
        let cypher = "MATCH (s:Story)-[:CONTAINS]->(n)
             WHERE n.id IN $ids
             RETURN n.id AS signal_id, s";

        let q = query(cypher).param("ids", id_strs);
        let mut stream = self.client.graph.execute(q).await?;

        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("signal_id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                if let Some(story) = row_to_story(&row) {
                    map.insert(id, story);
                }
            }
        }

        Ok(map)
    }

    // ─── Resource Capability Matching ────────────────────────────────

    /// Find Ask/Event nodes that REQUIRE a specific resource.
    /// Returns matches scored by resource completeness, sorted by score descending.
    pub async fn find_asks_by_resource(
        &self,
        slug: &str,
        lat: f64,
        lng: f64,
        radius_km: f64,
        limit: u32,
    ) -> Result<Vec<ResourceMatch>, neo4rs::Error> {
        self.find_asks_by_resources(&[slug.to_string()], lat, lng, radius_km, limit)
            .await
    }

    /// Find Ask/Event nodes matching ANY of the provided resource slugs.
    /// Scores by match completeness: each matched Requires = 1/total_requires, +0.2 per matched Prefers.
    pub async fn find_asks_by_resources(
        &self,
        slugs: &[String],
        lat: f64,
        lng: f64,
        radius_km: f64,
        limit: u32,
    ) -> Result<Vec<ResourceMatch>, neo4rs::Error> {
        let lat_delta = radius_km / 111.0;
        let lng_delta = radius_km / (111.0 * lat.to_radians().cos());

        // Find all Ask/Event nodes linked to ANY of the requested resources
        let cypher = "MATCH (r:Resource)<-[e:REQUIRES|PREFERS]-(s)
             WHERE r.slug IN $slugs
               AND (s:Ask OR s:Event)
               AND s.confidence >= $min_confidence
               AND (
                   (s.lat IS NOT NULL AND s.lat >= $min_lat AND s.lat <= $max_lat
                    AND s.lng >= $min_lng AND s.lng <= $max_lng)
                   OR s.lat IS NULL
               )
             WITH s, collect({slug: r.slug, type: type(e)}) AS matched_resources
             OPTIONAL MATCH (s)-[:REQUIRES]->(all_req:Resource)
             OPTIONAL MATCH (s)-[:PREFERS]->(all_pref:Resource)
             RETURN s,
                    matched_resources,
                    collect(DISTINCT all_req.slug) AS all_requires,
                    collect(DISTINCT all_pref.slug) AS all_prefers";

        let slug_strings: Vec<String> = slugs.iter().map(|s| s.to_string()).collect();
        let q = query(cypher)
            .param("slugs", slug_strings)
            .param("min_lat", lat - lat_delta)
            .param("max_lat", lat + lat_delta)
            .param("min_lng", lng - lng_delta)
            .param("max_lng", lng + lng_delta)
            .param("min_confidence", CONFIDENCE_DISPLAY_LIMITED as f64);

        let slug_set: std::collections::HashSet<&str> = slugs.iter().map(|s| s.as_str()).collect();
        let mut matches = Vec::new();

        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            // Try to parse as Ask or Event
            let node =
                row_to_node(&row, NodeType::Ask).or_else(|| row_to_node(&row, NodeType::Event));

            let Some(node) = node else { continue };
            if !passes_display_filter(&node) {
                continue;
            }

            let all_requires: Vec<String> = row.get("all_requires").unwrap_or_default();
            let all_prefers: Vec<String> = row.get("all_prefers").unwrap_or_default();

            let matched_requires: Vec<String> = all_requires
                .iter()
                .filter(|r| slug_set.contains(r.as_str()))
                .cloned()
                .collect();
            let matched_prefers: Vec<String> = all_prefers
                .iter()
                .filter(|p| slug_set.contains(p.as_str()))
                .cloned()
                .collect();
            let unmatched_requires: Vec<String> = all_requires
                .iter()
                .filter(|r| !slug_set.contains(r.as_str()))
                .cloned()
                .collect();

            let total_requires = all_requires.len().max(1) as f64;
            let score = (matched_requires.len() as f64 / total_requires)
                + (matched_prefers.len() as f64 * 0.2);

            if score <= 0.0 {
                continue;
            }

            matches.push(ResourceMatch {
                node: fuzz_node(node),
                score,
                normalized_score: score.min(1.0),
                matched_requires,
                matched_prefers,
                unmatched_requires,
            });
        }

        matches.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        matches.truncate(limit as usize);
        Ok(matches)
    }

    /// Find Give nodes that OFFER a specific resource.
    pub async fn find_gives_by_resource(
        &self,
        slug: &str,
        lat: f64,
        lng: f64,
        radius_km: f64,
        limit: u32,
    ) -> Result<Vec<ResourceMatch>, neo4rs::Error> {
        let lat_delta = radius_km / 111.0;
        let lng_delta = radius_km / (111.0 * lat.to_radians().cos());

        let cypher = "MATCH (r:Resource {slug: $slug})<-[:OFFERS]-(s:Give)
             WHERE s.confidence >= $min_confidence
               AND (
                   (s.lat IS NOT NULL AND s.lat >= $min_lat AND s.lat <= $max_lat
                    AND s.lng >= $min_lng AND s.lng <= $max_lng)
                   OR s.lat IS NULL
               )
             RETURN s
             ORDER BY s.cause_heat DESC, s.confidence DESC
             LIMIT $limit";

        let q = query(cypher)
            .param("slug", slug)
            .param("min_lat", lat - lat_delta)
            .param("max_lat", lat + lat_delta)
            .param("min_lng", lng - lng_delta)
            .param("max_lng", lng + lng_delta)
            .param("min_confidence", CONFIDENCE_DISPLAY_LIMITED as f64)
            .param("limit", limit as i64);

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Some(node) = row_to_node(&row, NodeType::Give) {
                if passes_display_filter(&node) {
                    results.push(ResourceMatch {
                        node: fuzz_node(node),
                        score: 1.0,
                        normalized_score: 1.0,
                        matched_requires: vec![],
                        matched_prefers: vec![],
                        unmatched_requires: vec![],
                    });
                }
            }
        }
        Ok(results)
    }

    /// List all resources sorted by signal_count descending.
    pub async fn list_resources(
        &self,
        limit: u32,
    ) -> Result<Vec<rootsignal_common::ResourceNode>, neo4rs::Error> {
        let q = query(
            "MATCH (r:Resource)
             RETURN r.id AS id, r.name AS name, r.slug AS slug,
                    r.description AS description, r.signal_count AS signal_count,
                    r.created_at AS created_at, r.last_seen AS last_seen
             ORDER BY r.signal_count DESC
             LIMIT $limit",
        )
        .param("limit", limit as i64);

        let mut resources = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let id = match Uuid::parse_str(&id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };
            resources.push(rootsignal_common::ResourceNode {
                id,
                name: row.get("name").unwrap_or_default(),
                slug: row.get("slug").unwrap_or_default(),
                description: row.get("description").unwrap_or_default(),
                signal_count: row.get::<i64>("signal_count").unwrap_or(0) as u32,
                created_at: parse_row_datetime(&row, "created_at"),
                last_seen: parse_row_datetime(&row, "last_seen"),
            });
        }
        Ok(resources)
    }

    /// Aggregate resource gap analysis: which resources are most needed but least offered.
    pub async fn resource_gap_analysis(&self) -> Result<Vec<ResourceGap>, neo4rs::Error> {
        let q = query(
            "MATCH (r:Resource)
             OPTIONAL MATCH (r)<-[:REQUIRES]-()
             WITH r, count(*) AS req_count
             OPTIONAL MATCH (r)<-[:OFFERS]-()
             RETURN r.slug AS slug, r.name AS name,
                    req_count AS requires_count, count(*) AS offers_count
             ORDER BY (toInteger(req_count) - count(*)) DESC",
        );

        let mut gaps = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let requires_count = row.get::<i64>("requires_count").unwrap_or(0) as u32;
            let offers_count = row.get::<i64>("offers_count").unwrap_or(0) as u32;
            gaps.push(ResourceGap {
                resource_slug: row.get("slug").unwrap_or_default(),
                resource_name: row.get("name").unwrap_or_default(),
                requires_count,
                offers_count,
                gap: requires_count as i32 - offers_count as i32,
            });
        }

        // Sort by gap descending (worst unmet needs first)
        gaps.sort_by(|a, b| b.gap.cmp(&a.gap));
        Ok(gaps)
    }
}

/// A signal matched to a resource query with scoring.
#[derive(Debug, Clone)]
pub struct ResourceMatch {
    pub node: Node,
    /// Raw match score (can exceed 1.0 when Prefers bonuses add up)
    pub score: f64,
    /// Normalized to 0.0–1.0 for display
    pub normalized_score: f64,
    /// Resource slugs matched from REQUIRES edges
    pub matched_requires: Vec<String>,
    /// Resource slugs matched from PREFERS edges
    pub matched_prefers: Vec<String>,
    /// Resource slugs NOT matched from REQUIRES edges
    pub unmatched_requires: Vec<String>,
}

/// Gap between required and offered resources.
#[derive(Debug, Clone)]
pub struct ResourceGap {
    pub resource_slug: String,
    pub resource_name: String,
    pub requires_count: u32,
    pub offers_count: u32,
    /// Positive = more requires than offers (unmet need)
    pub gap: i32,
}

// --- Helpers ---

/// Parse a datetime from a neo4rs Row, falling back to Utc::now() if missing or unparseable.
fn parse_row_datetime(row: &neo4rs::Row, key: &str) -> DateTime<Utc> {
    if let Ok(dt) = row.get::<chrono::DateTime<chrono::FixedOffset>>(key) {
        return dt.with_timezone(&Utc);
    }
    if let Ok(ndt) = row.get::<NaiveDateTime>(key) {
        return ndt.and_utc();
    }
    if let Ok(s) = row.get::<String>(key) {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S%.f") {
            return ndt.and_utc();
        }
    }
    Utc::now()
}

pub fn node_type_label(nt: NodeType) -> &'static str {
    match nt {
        NodeType::Event => "Event",
        NodeType::Give => "Give",
        NodeType::Ask => "Ask",
        NodeType::Notice => "Notice",
        NodeType::Tension => "Tension",
        NodeType::Evidence => "Evidence",
    }
}

/// Per-type Cypher WHERE clause fragment for expiration.
/// Returns an AND clause (or empty string) to inject into existing WHERE blocks.
fn expiry_clause(nt: NodeType) -> String {
    match nt {
        NodeType::Event => format!(
            "AND (n.is_recurring = true \
             OR n.starts_at IS NULL OR n.starts_at = '' \
             OR CASE \
               WHEN n.ends_at IS NOT NULL AND n.ends_at <> '' \
               THEN datetime(n.ends_at) >= datetime() - duration('PT{grace}H') \
               ELSE datetime(n.starts_at) >= datetime() - duration('PT{grace}H') \
             END)",
            grace = EVENT_PAST_GRACE_HOURS,
        ),
        NodeType::Ask => format!(
            "AND datetime(n.extracted_at) >= datetime() - duration('P{days}D')",
            days = ASK_EXPIRE_DAYS,
        ),
        NodeType::Give => format!(
            "AND datetime(n.last_confirmed_active) >= datetime() - duration('P{days}D')",
            days = FRESHNESS_MAX_DAYS,
        ),
        NodeType::Notice => format!(
            "AND datetime(n.extracted_at) >= datetime() - duration('P{days}D')",
            days = NOTICE_EXPIRE_DAYS,
        ),
        NodeType::Tension => format!(
            "AND datetime(n.last_confirmed_active) >= datetime() - duration('P{days}D')",
            days = FRESHNESS_MAX_DAYS,
        ),
        NodeType::Evidence => String::new(),
    }
}

/// Apply sensitivity-based coordinate fuzzing to a node.
fn fuzz_node(mut node: Node) -> Node {
    if let Some(meta) = node_meta_mut(&mut node) {
        if let Some(ref mut loc) = meta.location {
            *loc = fuzz_location(*loc, meta.sensitivity);
        }
    }
    node
}

fn node_meta_mut(node: &mut Node) -> Option<&mut NodeMeta> {
    match node {
        Node::Event(n) => Some(&mut n.meta),
        Node::Give(n) => Some(&mut n.meta),
        Node::Ask(n) => Some(&mut n.meta),
        Node::Notice(n) => Some(&mut n.meta),
        Node::Tension(n) => Some(&mut n.meta),
        Node::Evidence(_) => None,
    }
}

/// Safety-net display filter. Primary filtering happens in Cypher queries via `expiry_clause()`;
/// this catches anything that slips through (e.g. direct ID lookups).
fn passes_display_filter(node: &Node) -> bool {
    let Some(meta) = node.meta() else {
        return true;
    };

    let now = Utc::now();

    // Event-specific: hide past non-recurring events (only if date is known)
    if let Node::Event(e) = node {
        if !e.is_recurring {
            if let Some(starts_at) = e.starts_at {
                let event_end = e.ends_at.unwrap_or(starts_at);
                if (now - event_end).num_hours() > EVENT_PAST_GRACE_HOURS {
                    return false;
                }
            }
            // Events with no starts_at: fall through to general freshness check
        }
    }

    // Ask-specific: expire after ASK_EXPIRE_DAYS
    if matches!(node, Node::Ask(_)) {
        if (now - meta.extracted_at).num_days() > ASK_EXPIRE_DAYS {
            return false;
        }
    }

    // Notice-specific: expire after NOTICE_EXPIRE_DAYS
    if matches!(node, Node::Notice(_)) {
        if (now - meta.extracted_at).num_days() > NOTICE_EXPIRE_DAYS {
            return false;
        }
    }

    // General freshness check (recurring events still exempt — they persist between occurrences)
    let age_days = (now - meta.last_confirmed_active).num_days();
    if age_days > FRESHNESS_MAX_DAYS {
        match node {
            Node::Event(e) if e.is_recurring => {}
            _ => return false,
        }
    }

    true
}

/// Parse a neo4rs Row into a typed Node.
pub fn row_to_node(row: &neo4rs::Row, node_type: NodeType) -> Option<Node> {
    let n: neo4rs::Node = row.get("n").ok()?;

    let id_str: String = n.get("id").ok()?;
    let id = Uuid::parse_str(&id_str).ok()?;

    let title: String = n.get("title").unwrap_or_default();
    let summary: String = n.get("summary").unwrap_or_default();
    let sensitivity_str: String = n.get("sensitivity").unwrap_or_default();
    let sensitivity = match sensitivity_str.as_str() {
        "elevated" => SensitivityLevel::Elevated,
        "sensitive" => SensitivityLevel::Sensitive,
        _ => SensitivityLevel::General,
    };
    let confidence: f64 = n.get("confidence").unwrap_or(0.5);
    let freshness_score: f64 = n.get("freshness_score").unwrap_or(0.5);
    let corroboration_count: i64 = n.get("corroboration_count").unwrap_or(0);
    let source_url: String = n.get("source_url").unwrap_or_default();
    // Parse location from point
    let location = parse_location(&n);

    // Parse timestamps
    let extracted_at = parse_datetime_prop(&n, "extracted_at");
    let last_confirmed_active = parse_datetime_prop(&n, "last_confirmed_active");

    let source_diversity: i64 = n.get("source_diversity").unwrap_or(1);
    let external_ratio: f64 = n.get("external_ratio").unwrap_or(0.0);
    let cause_heat: f64 = n.get("cause_heat").unwrap_or(0.0);

    let meta = NodeMeta {
        id,
        title,
        summary,
        sensitivity,
        confidence: confidence as f32,
        freshness_score: freshness_score as f32,
        corroboration_count: corroboration_count as u32,
        location,
        location_name: {
            let name: String = n.get("location_name").unwrap_or_default();
            if name.is_empty() {
                None
            } else {
                Some(name)
            }
        },
        source_url,
        extracted_at,
        last_confirmed_active,
        source_diversity: source_diversity as u32,
        external_ratio: external_ratio as f32,
        cause_heat,
        mentioned_actors: Vec::new(),
        implied_queries: Vec::new(),
    };

    match node_type {
        NodeType::Event => {
            let starts_at = parse_optional_datetime_prop(&n, "starts_at");
            let ends_at_str: String = n.get("ends_at").unwrap_or_default();
            let ends_at = if ends_at_str.is_empty() {
                None
            } else {
                DateTime::parse_from_rfc3339(&ends_at_str)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
                    .or_else(|| {
                        NaiveDateTime::parse_from_str(&ends_at_str, "%Y-%m-%dT%H:%M:%S%.f")
                            .ok()
                            .map(|n| n.and_utc())
                    })
            };
            let action_url: String = n.get("action_url").unwrap_or_default();
            let organizer: String = n.get("organizer").unwrap_or_default();
            let is_recurring: bool = n.get("is_recurring").unwrap_or(false);

            Some(Node::Event(EventNode {
                meta,
                starts_at,
                ends_at,
                action_url,
                organizer: if organizer.is_empty() {
                    None
                } else {
                    Some(organizer)
                },
                is_recurring,
            }))
        }
        NodeType::Give => {
            let action_url: String = n.get("action_url").unwrap_or_default();
            let availability: String = n.get("availability").unwrap_or_default();
            let is_ongoing: bool = n.get("is_ongoing").unwrap_or(false);

            Some(Node::Give(GiveNode {
                meta,
                action_url,
                availability: if availability.is_empty() {
                    None
                } else {
                    Some(availability)
                },
                is_ongoing,
            }))
        }
        NodeType::Ask => {
            let urgency_str: String = n.get("urgency").unwrap_or_default();
            let urgency = match urgency_str.as_str() {
                "high" => Urgency::High,
                "critical" => Urgency::Critical,
                "low" => Urgency::Low,
                _ => Urgency::Medium,
            };
            let what_needed: String = n.get("what_needed").unwrap_or_default();
            let action_url: String = n.get("action_url").unwrap_or_default();
            let goal: String = n.get("goal").unwrap_or_default();

            Some(Node::Ask(AskNode {
                meta,
                urgency,
                what_needed: if what_needed.is_empty() {
                    None
                } else {
                    Some(what_needed)
                },
                action_url: if action_url.is_empty() {
                    None
                } else {
                    Some(action_url)
                },
                goal: if goal.is_empty() { None } else { Some(goal) },
            }))
        }
        NodeType::Notice => {
            let severity_str: String = n.get("severity").unwrap_or_default();
            let severity = match severity_str.as_str() {
                "high" => Severity::High,
                "critical" => Severity::Critical,
                "low" => Severity::Low,
                _ => Severity::Medium,
            };
            let category: String = n.get("category").unwrap_or_default();
            let effective_date_str: String = n.get("effective_date").unwrap_or_default();
            let effective_date = if effective_date_str.is_empty() {
                None
            } else {
                DateTime::parse_from_rfc3339(&effective_date_str)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
                    .or_else(|| {
                        NaiveDateTime::parse_from_str(&effective_date_str, "%Y-%m-%dT%H:%M:%S%.f")
                            .ok()
                            .map(|naive| naive.and_utc())
                    })
            };
            let source_authority: String = n.get("source_authority").unwrap_or_default();

            Some(Node::Notice(NoticeNode {
                meta,
                severity,
                category: if category.is_empty() {
                    None
                } else {
                    Some(category)
                },
                effective_date,
                source_authority: if source_authority.is_empty() {
                    None
                } else {
                    Some(source_authority)
                },
            }))
        }
        NodeType::Tension => {
            let severity_str: String = n.get("severity").unwrap_or_default();
            let severity = match severity_str.as_str() {
                "high" => Severity::High,
                "critical" => Severity::Critical,
                "low" => Severity::Low,
                _ => Severity::Medium,
            };
            let category: String = n.get("category").unwrap_or_default();
            let what_would_help: String = n.get("what_would_help").unwrap_or_default();

            Some(Node::Tension(TensionNode {
                meta,
                severity,
                category: if category.is_empty() {
                    None
                } else {
                    Some(category)
                },
                what_would_help: if what_would_help.is_empty() {
                    None
                } else {
                    Some(what_would_help)
                },
            }))
        }
        NodeType::Evidence => None,
    }
}

pub fn parse_location(n: &neo4rs::Node) -> Option<GeoPoint> {
    let lat: f64 = n.get("lat").ok()?;
    let lng: f64 = n.get("lng").ok()?;
    if lat == 0.0 && lng == 0.0 {
        return None;
    }
    Some(GeoPoint {
        lat,
        lng,
        precision: GeoPrecision::Exact,
    })
}

fn parse_optional_datetime_prop(n: &neo4rs::Node, prop: &str) -> Option<DateTime<Utc>> {
    if let Ok(s) = n.get::<String>(prop) {
        if s.is_empty() {
            return None;
        }
        if let Ok(dt) = DateTime::parse_from_rfc3339(&s) {
            return Some(dt.with_timezone(&Utc));
        }
        if let Ok(naive) = NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S%.f") {
            return Some(naive.and_utc());
        }
    }
    None
}

pub fn parse_datetime_prop(n: &neo4rs::Node, prop: &str) -> DateTime<Utc> {
    // Writer stores as "%Y-%m-%dT%H:%M:%S%.6f" (no timezone, implicitly UTC)
    if let Ok(s) = n.get::<String>(prop) {
        // Try RFC3339 first (has timezone), then naive datetime (writer format)
        if let Ok(dt) = DateTime::parse_from_rfc3339(&s) {
            return dt.with_timezone(&Utc);
        }
        if let Ok(naive) = NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S%.f") {
            return naive.and_utc();
        }
    }
    Utc::now()
}

fn extract_evidence(row: &neo4rs::Row) -> Vec<EvidenceNode> {
    // Evidence nodes come as a collected list, sorted by confidence descending
    let nodes: Vec<neo4rs::Node> = row.get("evidence").unwrap_or_default();
    let mut evidence: Vec<EvidenceNode> = nodes
        .into_iter()
        .filter_map(|n| {
            let id_str: String = n.get("id").ok()?;
            let id = Uuid::parse_str(&id_str).ok()?;
            let source_url: String = n.get("source_url").unwrap_or_default();
            let retrieved_at = parse_evidence_datetime(&n, "retrieved_at");
            let content_hash: String = n.get("content_hash").unwrap_or_default();
            let snippet: String = n.get("snippet").unwrap_or_default();
            let relevance: String = n.get("relevance").unwrap_or_default();
            let ev_conf: f64 = n.get("evidence_confidence").unwrap_or(0.0);

            Some(EvidenceNode {
                id,
                source_url,
                retrieved_at,
                content_hash,
                snippet: if snippet.is_empty() {
                    None
                } else {
                    Some(snippet)
                },
                relevance: if relevance.is_empty() {
                    None
                } else {
                    Some(relevance)
                },
                evidence_confidence: if ev_conf > 0.0 {
                    Some(ev_conf as f32)
                } else {
                    None
                },
            })
        })
        .collect();
    evidence.sort_by(|a, b| {
        let ca = a.evidence_confidence.unwrap_or(0.0);
        let cb = b.evidence_confidence.unwrap_or(0.0);
        cb.partial_cmp(&ca).unwrap_or(std::cmp::Ordering::Equal)
    });
    evidence
}

pub fn row_to_story(row: &neo4rs::Row) -> Option<StoryNode> {
    let n: neo4rs::Node = row.get("s").ok()?;

    let id_str: String = n.get("id").ok()?;
    let id = Uuid::parse_str(&id_str).ok()?;

    let headline: String = n.get("headline").unwrap_or_default();
    let summary: String = n.get("summary").unwrap_or_default();
    let signal_count: i64 = n.get("signal_count").unwrap_or(0);
    let first_seen = parse_story_datetime(&n, "first_seen");
    let last_updated = parse_story_datetime(&n, "last_updated");
    let velocity: f64 = n.get("velocity").unwrap_or(0.0);
    let energy: f64 = n.get("energy").unwrap_or(0.0);

    let centroid_lat: Option<f64> = n.get("centroid_lat").ok();
    let centroid_lng: Option<f64> = n.get("centroid_lng").ok();

    let dominant_type: String = n.get("dominant_type").unwrap_or_default();
    let sensitivity: String = n
        .get("sensitivity")
        .unwrap_or_else(|_| "general".to_string());
    let source_count: i64 = n.get("source_count").unwrap_or(0);
    let entity_count: i64 = n.get("entity_count").unwrap_or(0);
    let type_diversity: i64 = n.get("type_diversity").unwrap_or(0);
    let source_domains: Vec<String> = n.get("source_domains").unwrap_or_default();
    let corroboration_depth: i64 = n.get("corroboration_depth").unwrap_or(0);
    let status: String = n.get("status").unwrap_or_else(|_| "emerging".to_string());

    let arc: Option<String> = n
        .get("arc")
        .ok()
        .and_then(|s: String| if s.is_empty() { None } else { Some(s) });
    let category: Option<String> =
        n.get("category")
            .ok()
            .and_then(|s: String| if s.is_empty() { None } else { Some(s) });
    let lede: Option<String> =
        n.get("lede")
            .ok()
            .and_then(|s: String| if s.is_empty() { None } else { Some(s) });
    let narrative: Option<String> =
        n.get("narrative")
            .ok()
            .and_then(|s: String| if s.is_empty() { None } else { Some(s) });
    let action_guidance: Option<String> =
        n.get("action_guidance")
            .ok()
            .and_then(|s: String| if s.is_empty() { None } else { Some(s) });

    let cause_heat: f64 = n.get("cause_heat").unwrap_or(0.0);
    let ask_count: i64 = n.get("ask_count").unwrap_or(0);
    let give_count: i64 = n.get("give_count").unwrap_or(0);
    let event_count: i64 = n.get("event_count").unwrap_or(0);
    let drawn_to_count: i64 = n.get("drawn_to_count").unwrap_or(0);
    let gap_score: i64 = n.get("gap_score").unwrap_or(0);
    let gap_velocity: f64 = n.get("gap_velocity").unwrap_or(0.0);

    // Centroid fuzzing for sensitive stories
    let (centroid_lat, centroid_lng) = match (centroid_lat, centroid_lng) {
        (Some(lat), Some(lng)) if sensitivity == "sensitive" || sensitivity == "elevated" => {
            let radius = if sensitivity == "sensitive" {
                0.05 // ~5km
            } else {
                0.005 // ~500m
            };
            let fuzzed_lat = (lat / radius).round() * radius;
            let fuzzed_lng = (lng / radius).round() * radius;
            (Some(fuzzed_lat), Some(fuzzed_lng))
        }
        other => other,
    };

    Some(StoryNode {
        id,
        headline,
        summary,
        signal_count: signal_count as u32,
        first_seen,
        last_updated,
        velocity,
        energy,
        centroid_lat,
        centroid_lng,
        dominant_type,
        sensitivity,
        source_count: source_count as u32,
        entity_count: entity_count as u32,
        type_diversity: type_diversity as u32,
        source_domains,
        corroboration_depth: corroboration_depth as u32,
        status,
        arc,
        category,
        lede,
        narrative,
        action_guidance,
        cause_heat,
        ask_count: ask_count as u32,
        give_count: give_count as u32,
        event_count: event_count as u32,
        drawn_to_count: drawn_to_count as u32,
        gap_score: gap_score as i32,
        gap_velocity,
    })
}

pub fn parse_story_datetime(n: &neo4rs::Node, prop: &str) -> DateTime<Utc> {
    if let Ok(s) = n.get::<String>(prop) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(&s) {
            return dt.with_timezone(&Utc);
        }
        if let Ok(naive) = NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S%.f") {
            return naive.and_utc();
        }
    }
    Utc::now()
}

fn parse_evidence_datetime(n: &neo4rs::Node, prop: &str) -> DateTime<Utc> {
    if let Ok(s) = n.get::<String>(prop) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(&s) {
            return dt.with_timezone(&Utc);
        }
        if let Ok(naive) = NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S%.f") {
            return naive.and_utc();
        }
    }
    Utc::now()
}

fn row_to_actor(row: &neo4rs::Row) -> Option<rootsignal_common::ActorNode> {
    let n: neo4rs::Node = row.get("a").ok()?;

    let id_str: String = n.get("id").ok()?;
    let id = Uuid::parse_str(&id_str).ok()?;

    let name: String = n.get("name").unwrap_or_default();
    let actor_type_str: String = n.get("actor_type").unwrap_or_default();
    let actor_type = match actor_type_str.as_str() {
        "individual" => rootsignal_common::ActorType::Individual,
        "government_body" => rootsignal_common::ActorType::GovernmentBody,
        "coalition" => rootsignal_common::ActorType::Coalition,
        _ => rootsignal_common::ActorType::Organization,
    };
    let entity_id: String = n.get("entity_id").unwrap_or_default();
    let domains: Vec<String> = n.get("domains").unwrap_or_default();
    let social_urls: Vec<String> = n.get("social_urls").unwrap_or_default();
    let city: String = n.get("city").unwrap_or_default();
    let description: String = n.get("description").unwrap_or_default();
    let signal_count: i64 = n.get("signal_count").unwrap_or(0);
    let first_seen = parse_story_datetime(&n, "first_seen");
    let last_active = parse_story_datetime(&n, "last_active");
    let typical_roles: Vec<String> = n.get("typical_roles").unwrap_or_default();

    Some(rootsignal_common::ActorNode {
        id,
        name,
        actor_type,
        entity_id,
        domains,
        social_urls,
        city,
        description,
        signal_count: signal_count as u32,
        first_seen,
        last_active,
        typical_roles,
    })
}
