use chrono::{DateTime, NaiveDateTime, Utc};
use futures::future::join_all;
use neo4rs::query;
use uuid::Uuid;

use rootsignal_common::{
    fuzz_location, ConditionNode, Location, ResourceOfferNode, CitationNode, GatheringNode, GeoPoint,
    GeoPrecision, HelpRequestNode, Node, NodeMeta, NodeType, AnnouncementNode, ScheduleNode,
    SensitivityLevel, Severity, ConcernNode,
    ConcernResponse, Urgency, CONFIDENCE_DISPLAY_LIMITED, FRESHNESS_MAX_DAYS,
    GATHERING_PAST_GRACE_HOURS, NEED_EXPIRE_DAYS, NOTICE_EXPIRE_DAYS,
};
use crate::GraphClient;
use crate::writer::{row_to_source_node, row_datetime_opt_pub, SignalBrief};
use rootsignal_common::SourceNode;

/// Pipe-separated location edge types for Cypher MATCH patterns.
const LOC_EDGES: &str = "HELD_AT|AVAILABLE_AT|NEEDED_AT|RELEVANT_TO|AFFECTS|OBSERVED_AT|REFERENCES_LOCATION";

/// Returns a Cypher EXISTS subquery for bounding-box filtering through Location edges.
/// Assumes `$min_lat`, `$max_lat`, `$min_lng`, `$max_lng` params are bound.
fn bbox_exists(node_var: &str) -> String {
    format!(
        "EXISTS {{
           MATCH ({node_var})-[:{LOC_EDGES}]->(l:Location)
           WHERE l.lat >= $min_lat AND l.lat <= $max_lat
             AND l.lng >= $min_lng AND l.lng <= $max_lng
         }}"
    )
}

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
                NodeType::Gathering,
                NodeType::Resource,
                NodeType::HelpRequest,
                NodeType::Announcement,
                NodeType::Concern,
            ]
        });

        // Use bounding box on plain lat/lng properties.
        // ~1 degree lat ≈ 111km, 1 degree lng ≈ 111km * cos(lat)
        let lat_delta = radius_km / 111.0;
        let lng_delta = radius_km / (111.0 * lat.to_radians().cos());

        let branches: Vec<String> = types
            .iter()
            .map(|nt| {
                let label = node_type_label(*nt);
                format!(
                    "MATCH (n:{label})
                     WHERE n.review_status = 'accepted'
                       AND n.confidence >= $min_confidence
                       {expiry}
                       AND {bbox}
                     OPTIONAL MATCH (n)<-[:ACTED_IN {{role: 'authored'}}]-(author:Actor)
                     RETURN n, labels(n)[0] AS node_label,
                            n.cause_heat AS _sort_heat,
                            n.confidence AS _sort_conf,
                            n.last_confirmed_active AS _sort_time,
                            author.location_lat AS author_lat,
                            author.location_lng AS author_lng
                     ORDER BY _sort_heat DESC, _sort_conf DESC, _sort_time DESC
                     LIMIT 200",
                    expiry = expiry_clause(*nt),
                    bbox = bbox_exists("n"),
                )
            })
            .collect();

        let cypher = branches.join("\nUNION ALL\n");

        let q = query(&cypher)
            .param("min_lat", lat - lat_delta)
            .param("max_lat", lat + lat_delta)
            .param("min_lng", lng - lng_delta)
            .param("max_lng", lng + lng_delta)
            .param("min_confidence", CONFIDENCE_DISPLAY_LIMITED as f64);

        let mut results = Vec::new();
        let mut stream = self.client.execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Some(node) = row_to_node_by_label(&row) {
                if passes_display_filter(&node) {
                    results.push(fuzz_node(node));
                }
            }
        }

        Ok(results)
    }

    /// Get a single node by ID with its evidence links. Returns fuzzed coordinates.
    pub async fn get_node_detail(
        &self,
        id: Uuid,
    ) -> Result<Option<(Node, Vec<CitationNode>)>, neo4rs::Error> {
        let id_str = id.to_string();

        // Search across all signal types
        for nt in &[
            NodeType::Gathering,
            NodeType::Resource,
            NodeType::HelpRequest,
            NodeType::Announcement,
            NodeType::Concern,
        ] {
            let label = node_type_label(*nt);
            let cypher = format!(
                "MATCH (n:{label} {{id: $id}})
                 OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Citation)
                 OPTIONAL MATCH (n)<-[:ACTED_IN {{role: 'authored'}}]-(author:Actor)
                 RETURN n, collect(ev) AS evidence,
                        author.location_lat AS author_lat,
                        author.location_lng AS author_lng"
            );

            let q = query(&cypher).param("id", id_str.as_str());
            let mut stream = self.client.execute(q).await?;

            if let Some(row) = stream.next().await? {
                if let Some(mut node) = row_to_node(&row, *nt) {
                    if !passes_display_filter(&node) {
                        return Ok(None);
                    }

                    // Derive from_location at query time from authored actor's location
                    if let (Ok(lat), Ok(lng)) =
                        (row.get::<f64>("author_lat"), row.get::<f64>("author_lng"))
                    {
                        if let Some(meta) = node.meta_mut() {
                            meta.locations.push(Location {
                                point: Some(GeoPoint {
                                    lat,
                                    lng,
                                    precision: GeoPrecision::Approximate,
                                }),
                                name: None,
                                address: None,
                                role: Some("origin".to_string()),
                                timezone: None,
                            });
                        }
                    }

                    let evidence = extract_citation(&row);
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
                NodeType::Gathering,
                NodeType::Resource,
                NodeType::HelpRequest,
                NodeType::Announcement,
                NodeType::Concern,
            ]
        });

        let branches: Vec<String> = types
            .iter()
            .map(|nt| {
                let label = node_type_label(*nt);
                format!(
                    "MATCH (n:{label})
                     WHERE n.review_status = 'accepted'
                       AND n.confidence >= $min_confidence
                       {expiry}
                     RETURN n, labels(n)[0] AS node_label
                     ORDER BY n.cause_heat DESC, n.last_confirmed_active DESC
                     LIMIT $limit",
                    expiry = expiry_clause(*nt),
                )
            })
            .collect();

        let cypher = branches.join("\nUNION ALL\n");

        let q = query(&cypher)
            .param("min_confidence", CONFIDENCE_DISPLAY_LIMITED as f64)
            .param("limit", limit as i64);

        let mut ranked: Vec<Node> = Vec::new();
        let mut stream = self.client.execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Some(node) = row_to_node_by_label(&row) {
                if passes_display_filter(&node) {
                    ranked.push(fuzz_node(node));
                }
            }
        }

        // Sort: cause_heat first, then recency
        ranked.sort_by(|a, b| {
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

        ranked.truncate(limit as usize);
        Ok(ranked)
    }

    /// List recent signals scoped to a geographic bounding box.
    pub async fn list_recent_in_bbox(
        &self,
        lat: f64,
        lng: f64,
        radius_km: f64,
        limit: u32,
    ) -> Result<Vec<Node>, neo4rs::Error> {
        let lat_delta = radius_km / 111.0;
        let lng_delta = radius_km / (111.0 * lat.to_radians().cos());

        let all_types = [
            NodeType::Gathering,
            NodeType::Resource,
            NodeType::HelpRequest,
            NodeType::Announcement,
            NodeType::Concern,
        ];

        let branches: Vec<String> = all_types
            .iter()
            .map(|nt| {
                let label = node_type_label(*nt);
                format!(
                    "MATCH (n:{label})
                     WHERE {bbox}
                     RETURN n, labels(n)[0] AS node_label
                     ORDER BY coalesce(n.cause_heat, 0) DESC, n.last_confirmed_active DESC
                     LIMIT $limit",
                    bbox = bbox_exists("n"),
                )
            })
            .collect();

        let cypher = branches.join("\nUNION ALL\n");

        let q = query(&cypher)
            .param("min_lat", lat - lat_delta)
            .param("max_lat", lat + lat_delta)
            .param("min_lng", lng - lng_delta)
            .param("max_lng", lng + lng_delta)
            .param("limit", limit as i64);

        let mut all: Vec<Node> = Vec::new();
        let mut stream = self.client.execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Some(node) = row_to_node_by_label(&row) {
                all.push(node);
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

    /// Fetch evidence nodes for a signal by ID.
    pub async fn get_signal_evidence(
        &self,
        signal_id: Uuid,
    ) -> Result<Vec<CitationNode>, neo4rs::Error> {
        let id_str = signal_id.to_string();

        for nt in &[
            NodeType::Gathering,
            NodeType::Resource,
            NodeType::HelpRequest,
            NodeType::Announcement,
            NodeType::Concern,
        ] {
            let label = node_type_label(*nt);
            let cypher = format!(
                "MATCH (n:{label} {{id: $id}})-[:SOURCED_FROM]->(ev:Citation)
                 RETURN collect(ev) AS evidence"
            );

            let q = query(&cypher).param("id", id_str.as_str());
            let mut stream = self.client.execute(q).await?;

            if let Some(row) = stream.next().await? {
                let evidence = extract_citation(&row);
                if !evidence.is_empty() {
                    return Ok(evidence);
                }
            }
        }

        Ok(Vec::new())
    }

    /// Get a single signal by ID.
    pub async fn get_signal_by_id(&self, id: Uuid) -> Result<Option<Node>, neo4rs::Error> {
        match self.get_node_detail(id).await? {
            Some((node, _)) => Ok(Some(node)),
            None => Ok(None),
        }
    }

    /// Get a signal by ID without display filters or coordinate fuzzing.
    /// Used by admin endpoints that need to see all signals regardless of
    /// confidence or staleness.
    pub async fn get_signal_by_id_unfiltered(&self, id: Uuid) -> Result<Option<Node>, neo4rs::Error> {
        let id_str = id.to_string();

        for nt in &[
            NodeType::Gathering,
            NodeType::Resource,
            NodeType::HelpRequest,
            NodeType::Announcement,
            NodeType::Concern,
            NodeType::Condition,
        ] {
            let label = node_type_label(*nt);
            let cypher = format!(
                "MATCH (n:{label} {{id: $id}})
                 RETURN n, labels(n)[0] AS node_label"
            );

            let q = query(&cypher).param("id", id_str.as_str());
            let mut stream = self.client.execute(q).await?;

            if let Some(row) = stream.next().await? {
                if let Some(node) = row_to_node(&row, *nt) {
                    return Ok(Some(node));
                }
            }
        }

        Ok(None)
    }

    // --- Actor queries ---

    /// Get a single actor by ID with recent signals.
    pub async fn actor_detail(
        &self,
        actor_id: Uuid,
    ) -> Result<Option<rootsignal_common::ActorNode>, neo4rs::Error> {
        let q = query("MATCH (a:Actor {id: $id}) RETURN a").param("id", actor_id.to_string());

        let mut stream = self.client.execute(q).await?;
        if let Some(row) = stream.next().await? {
            return Ok(row_to_actor(&row));
        }
        Ok(None)
    }

    // --- Tension response queries ---

    /// Get Aid/Gathering/Need signals that respond to a tension, with edge metadata.
    pub async fn concern_responses(
        &self,
        concern_id: Uuid,
    ) -> Result<Vec<ConcernResponse>, neo4rs::Error> {
        let response_types = [NodeType::Resource, NodeType::Gathering, NodeType::HelpRequest];

        let branches: Vec<String> = response_types
            .iter()
            .map(|nt| {
                let label = node_type_label(*nt);
                format!(
                    "MATCH (t:Concern {{id: $id}})<-[rel:RESPONDS_TO|DRAWN_TO|EVIDENCE_OF]-(n:{label})
                     RETURN n, labels(n)[0] AS node_label, rel.match_strength AS match_strength, rel.explanation AS explanation
                     ORDER BY n.confidence DESC"
                )
            })
            .collect();

        let cypher = branches.join("\nUNION ALL\n");
        let q = query(&cypher).param("id", concern_id.to_string());

        let mut results = Vec::new();
        let mut stream = self.client.execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Some(node) = row_to_node_by_label(&row) {
                if passes_display_filter(&node) {
                    let match_strength: f64 = row.get("match_strength").unwrap_or(0.0);
                    let explanation: String = row.get("explanation").unwrap_or_default();
                    results.push(ConcernResponse {
                        node: fuzz_node(node),
                        match_strength,
                        explanation,
                    });
                }
            }
        }

        Ok(results)
    }

    /// Count EVIDENCE_OF edges on a Tension — a structural measure of how well-grounded it is.
    pub async fn evidence_of_count(&self, concern_id: Uuid) -> Result<u32, neo4rs::Error> {
        let q = query(
            "MATCH (sig)-[:EVIDENCE_OF]->(t:Concern {id: $id})
             RETURN count(sig) AS cnt",
        )
        .param("id", concern_id.to_string());

        let mut stream = self.client.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let cnt: i64 = row.get("cnt").unwrap_or(0);
            Ok(cnt as u32)
        } else {
            Ok(0)
        }
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
        let all_types = [
            NodeType::Gathering,
            NodeType::Resource,
            NodeType::HelpRequest,
            NodeType::Announcement,
            NodeType::Concern,
        ];

        let branches: Vec<String> = all_types
            .iter()
            .map(|nt| {
                let label = node_type_label(*nt);
                format!(
                    "MATCH (n:{label})
                     WHERE n.confidence >= $min_confidence
                       {expiry}
                       AND {bbox}
                     RETURN n, labels(n)[0] AS node_label
                     ORDER BY coalesce(n.cause_heat, 0) DESC, n.confidence DESC
                     LIMIT $limit",
                    expiry = expiry_clause(*nt),
                    bbox = bbox_exists("n"),
                )
            })
            .collect();

        let cypher = branches.join("\nUNION ALL\n");

        let q = query(&cypher)
            .param("min_lat", min_lat)
            .param("max_lat", max_lat)
            .param("min_lng", min_lng)
            .param("max_lng", max_lng)
            .param("min_confidence", CONFIDENCE_DISPLAY_LIMITED as f64)
            .param("limit", limit as i64);

        let mut all: Vec<Node> = Vec::new();
        let mut stream = self.client.execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Some(node) = row_to_node_by_label(&row) {
                if passes_display_filter(&node) {
                    all.push(fuzz_node(node));
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

    /// Find tensions with < 2 respondents within a bounding box.
    /// Sorted by cause_heat DESC. Used to surface unresponded community needs.
    pub async fn unresponded_tensions_in_bounds(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
        limit: u32,
    ) -> Result<Vec<Node>, neo4rs::Error> {
        let bbox = bbox_exists("t");
        let q = query(&format!(
            "MATCH (t:Concern)
             WHERE t.review_status = 'accepted'
               AND {bbox}
             OPTIONAL MATCH (t)<-[:RESPONDS_TO|DRAWN_TO|EVIDENCE_OF]-(r)
             WITH t, count(r) AS resp_count
             WHERE resp_count < 2
             RETURN t AS n
             ORDER BY coalesce(t.cause_heat, 0.0) DESC
             LIMIT $limit",
        ))
        .param("min_lat", min_lat)
        .param("max_lat", max_lat)
        .param("min_lng", min_lng)
        .param("max_lng", max_lng)
        .param("limit", limit as i64);

        let mut results = Vec::new();
        let mut stream = self.client.execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Some(node) = row_to_node(&row, NodeType::Concern) {
                results.push(fuzz_node(node));
            }
        }
        Ok(results)
    }

    /// Semantic search for signals within a bounding box using vector KNN.
    /// Over-fetches from the vector index (K per type), then post-filters by bbox.
    /// Returns (node, blended_score) pairs sorted by blended score.
    /// All 5 vector index queries run concurrently via join_all.
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

        let index_names = [
            ("Gathering", "gathering_embedding", NodeType::Gathering),
            ("Resource", "aid_embedding", NodeType::Resource),
            ("HelpRequest", "need_embedding", NodeType::HelpRequest),
            ("Announcement", "notice_embedding", NodeType::Announcement),
            ("Concern", "tension_embedding", NodeType::Concern),
        ];

        let futures: Vec<_> = index_names
            .iter()
            .map(|(_label, index_name, nt)| {
                let nt = *nt;
                let embedding_vec = embedding_vec.clone();
                let graph = &self.client;
                async move {
                    let bbox = bbox_exists("node");
                    let cypher = format!("CALL db.index.vector.queryNodes($index_name, $k, $embedding)
                         YIELD node, score
                         WHERE score >= $min_score
                           AND node.review_status = 'accepted'
                           AND node.confidence >= $min_confidence
                           AND {bbox}
                         RETURN node AS n, score");

                    let q = query(&cypher)
                        .param("index_name", *index_name)
                        .param("k", k_per_type)
                        .param("embedding", embedding_vec)
                        .param("min_score", min_score)
                        .param("min_lat", min_lat)
                        .param("max_lat", max_lat)
                        .param("min_lng", min_lng)
                        .param("max_lng", max_lng)
                        .param("min_confidence", CONFIDENCE_DISPLAY_LIMITED as f64);

                    let mut results: Vec<(Node, f64)> = Vec::new();
                    let mut stream = graph.execute(q).await?;
                    while let Some(row) = stream.next().await? {
                        let similarity: f64 = row.get("score").unwrap_or(0.0);
                        if let Some(node) = row_to_node(&row, nt) {
                            if passes_display_filter(&node) {
                                let heat = node.meta().map(|m| m.cause_heat).unwrap_or(0.0);
                                let blended = similarity * 0.6 + heat * 0.4;
                                results.push((fuzz_node(node), blended));
                            }
                        }
                    }
                    Ok::<_, neo4rs::Error>(results)
                }
            })
            .collect();

        let all_results = join_all(futures).await;

        let mut scored: Vec<(Node, f64)> = Vec::new();
        for result in all_results {
            scored.extend(result?);
        }

        scored.sort_by(|(_, a), (_, b)| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit as usize);
        Ok(scored)
    }

    // --- Admin/Quality queries (not public-facing, but through reader for safety) ---

    /// Get total signal count by type (for quality dashboard).
    pub async fn count_by_type(&self) -> Result<Vec<(NodeType, u64)>, neo4rs::Error> {
        let mut counts = Vec::new();
        for nt in &[
            NodeType::Gathering,
            NodeType::Resource,
            NodeType::HelpRequest,
            NodeType::Announcement,
            NodeType::Concern,
        ] {
            let label = node_type_label(*nt);
            let q = query(&format!("MATCH (n:{label}) RETURN count(n) AS cnt"));
            let mut stream = self.client.execute(q).await?;
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
            WHERE n:Gathering OR n:Resource OR n:HelpRequest OR n:Announcement OR n:Concern OR n:Condition
            WITH CASE
                WHEN n.confidence >= 0.8 THEN 'high (0.8+)'
                WHEN n.confidence >= 0.6 THEN 'good (0.6-0.8)'
                WHEN n.confidence >= 0.4 THEN 'limited (0.4-0.6)'
                ELSE 'low (<0.4)'
            END AS bucket
            RETURN bucket, count(*) AS cnt
            ORDER BY bucket",
        );

        let mut stream = self.client.execute(q).await?;
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
            WHERE n:Gathering OR n:Resource OR n:HelpRequest OR n:Announcement OR n:Concern OR n:Condition
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

        let mut stream = self.client.execute(q).await?;
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
    /// Returns Vec<(date_string, gathering, aid, need, notice, tension)>.
    pub async fn signal_volume_by_day(
        &self,
    ) -> Result<Vec<(String, u64, u64, u64, u64, u64)>, neo4rs::Error> {
        let q = query(
            "WITH date(datetime() - duration('P30D')) AS cutoff
             UNWIND range(0, 29) AS offset
             WITH date(datetime() - duration('P' + toString(offset) + 'D')) AS day
             OPTIONAL MATCH (e:Gathering) WHERE date(e.extracted_at) = day
             WITH day, count(e) AS events
             OPTIONAL MATCH (g:Resource) WHERE date(g.extracted_at) = day
             WITH day, events, count(g) AS gives
             OPTIONAL MATCH (a:HelpRequest) WHERE date(a.extracted_at) = day
             WITH day, events, gives, count(a) AS needs
             OPTIONAL MATCH (n:Announcement) WHERE date(n.extracted_at) = day
             WITH day, events, gives, needs, count(n) AS notices
             OPTIONAL MATCH (t:Concern) WHERE date(t.extracted_at) = day
             RETURN toString(day) AS day, events, gives, needs, notices, count(t) AS tensions
             ORDER BY day",
        );

        let mut results = Vec::new();
        let mut stream = self.client.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let day: String = row.get("day").unwrap_or_default();
            let events: i64 = row.get("events").unwrap_or(0);
            let gives: i64 = row.get("gives").unwrap_or(0);
            let needs: i64 = row.get("needs").unwrap_or(0);
            let notices: i64 = row.get("notices").unwrap_or(0);
            let tensions: i64 = row.get("tensions").unwrap_or(0);
            results.push((
                day,
                events as u64,
                gives as u64,
                needs as u64,
                notices as u64,
                tensions as u64,
            ));
        }
        Ok(results)
    }

    /// Total actor count.
    pub async fn actor_count(&self) -> Result<u64, neo4rs::Error> {
        let q = query("MATCH (a:Actor) RETURN count(a) AS cnt");
        let mut stream = self.client.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let cnt: i64 = row.get("cnt").unwrap_or(0);
            return Ok(cnt as u64);
        }
        Ok(0)
    }

    // --- Batch queries for DataLoaders ---

    /// Batch-fetch evidence for multiple signal IDs. Returns map of signal_id -> Vec<CitationNode>.
    pub async fn batch_citation_by_signal_ids(
        &self,
        ids: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, Vec<CitationNode>>, neo4rs::Error> {
        let mut map: std::collections::HashMap<Uuid, Vec<CitationNode>> =
            std::collections::HashMap::new();

        if ids.is_empty() {
            return Ok(map);
        }

        let id_strs: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
        let cypher = "MATCH (n)-[:SOURCED_FROM]->(ev:Citation)
             WHERE n.id IN $ids AND (n:Gathering OR n:Resource OR n:HelpRequest OR n:Announcement OR n:Concern OR n:Condition)
             RETURN n.id AS signal_id, collect(ev) AS evidence";

        let q = query(cypher).param("ids", id_strs);
        let mut stream = self.client.execute(q).await?;

        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("signal_id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                let evidence = extract_citation(&row);
                map.insert(id, evidence);
            }
        }

        Ok(map)
    }

    /// Batch-fetch schedules for multiple signal IDs. Returns map of signal_id -> ScheduleNode.
    pub async fn batch_schedules_by_signal_ids(
        &self,
        ids: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, ScheduleNode>, neo4rs::Error> {
        let mut map: std::collections::HashMap<Uuid, ScheduleNode> =
            std::collections::HashMap::new();

        if ids.is_empty() {
            return Ok(map);
        }

        let id_strs: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
        let cypher = "MATCH (n)-[:HAS_SCHEDULE]->(s:Schedule)
             WHERE n.id IN $ids AND (n:Gathering OR n:Resource OR n:HelpRequest OR n:Announcement OR n:Concern OR n:Condition)
             RETURN n.id AS signal_id, s";

        let q = query(cypher).param("ids", id_strs);
        let mut stream = self.client.execute(q).await?;

        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("signal_id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                let s: neo4rs::Node = match row.get("s") {
                    Ok(n) => n,
                    Err(_) => continue,
                };
                let schedule_id_str: String = s.get("id").unwrap_or_default();
                let schedule_id = match Uuid::parse_str(&schedule_id_str) {
                    Ok(id) => id,
                    Err(_) => continue,
                };
                let rrule: String = s.get("rrule").unwrap_or_default();
                let timezone: String = s.get("timezone").unwrap_or_default();
                let schedule_text: String = s.get("schedule_text").unwrap_or_default();

                let dtstart = parse_optional_datetime_prop(&s, "dtstart");
                let dtend = parse_optional_datetime_prop(&s, "dtend");
                let extracted_at = parse_datetime_prop(&s, "extracted_at");

                // Parse rdates/exdates arrays of datetime strings
                let rdates_raw: Vec<String> = s.get("rdates").unwrap_or_default();
                let exdates_raw: Vec<String> = s.get("exdates").unwrap_or_default();
                let rdates: Vec<DateTime<Utc>> = rdates_raw
                    .iter()
                    .filter_map(|d| {
                        DateTime::parse_from_rfc3339(d)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                            .or_else(|| {
                                NaiveDateTime::parse_from_str(d, "%Y-%m-%dT%H:%M:%S%.f")
                                    .ok()
                                    .map(|n| n.and_utc())
                            })
                    })
                    .collect();
                let exdates: Vec<DateTime<Utc>> = exdates_raw
                    .iter()
                    .filter_map(|d| {
                        DateTime::parse_from_rfc3339(d)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                            .or_else(|| {
                                NaiveDateTime::parse_from_str(d, "%Y-%m-%dT%H:%M:%S%.f")
                                    .ok()
                                    .map(|n| n.and_utc())
                            })
                    })
                    .collect();

                let schedule = ScheduleNode {
                    id: schedule_id,
                    rrule: if rrule.is_empty() { None } else { Some(rrule) },
                    rdates,
                    exdates,
                    dtstart,
                    dtend,
                    timezone: if timezone.is_empty() {
                        None
                    } else {
                        Some(timezone)
                    },
                    schedule_text: if schedule_text.is_empty() {
                        None
                    } else {
                        Some(schedule_text)
                    },
                    extracted_at,
                };
                map.insert(id, schedule);
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
        let mut stream = self.client.execute(q).await?;

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

    pub async fn batch_entities_by_signal_ids(
        &self,
        ids: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, Vec<rootsignal_common::types::Entity>>, neo4rs::Error>
    {
        use rootsignal_common::types::EntityType;

        let mut map: std::collections::HashMap<Uuid, Vec<rootsignal_common::types::Entity>> =
            std::collections::HashMap::new();

        if ids.is_empty() {
            return Ok(map);
        }

        let id_strs: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
        let q = query(
            "MATCH (e:Entity)-[r:MENTIONED_IN]->(n)
             WHERE n.id IN $ids
             RETURN n.id AS signal_id, e.name AS name, e.entity_type AS entity_type, r.role AS role",
        )
        .param("ids", id_strs);
        let mut stream = self.client.execute(q).await?;

        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("signal_id").unwrap_or_default();
            let Ok(id) = Uuid::parse_str(&id_str) else {
                continue;
            };
            let name: String = row.get("name").unwrap_or_default();
            let et_str: String = row.get("entity_type").unwrap_or_default();
            let role: String = row.get("role").unwrap_or_default();

            let entity_type = match et_str.as_str() {
                "Group" => EntityType::Group,
                "GovernmentBody" => EntityType::GovernmentBody,
                "Thing" => EntityType::Thing,
                _ => EntityType::Organization,
            };

            map.entry(id).or_default().push(rootsignal_common::types::Entity {
                name,
                entity_type,
                role: if role.is_empty() { None } else { Some(role) },
            });
        }

        Ok(map)
    }

    // ─── Resource Capability Matching ────────────────────────────────

    /// Find Need/Gathering nodes that REQUIRE a specific resource.
    /// Returns matches scored by resource completeness, sorted by score descending.
    pub async fn find_needs_by_resource(
        &self,
        slug: &str,
        lat: f64,
        lng: f64,
        radius_km: f64,
        limit: u32,
    ) -> Result<Vec<ResourceMatch>, neo4rs::Error> {
        self.find_needs_by_resources(&[slug.to_string()], lat, lng, radius_km, limit)
            .await
    }

    /// Find Need/Gathering nodes matching ANY of the provided resource slugs.
    /// Scores by match completeness: each matched Requires = 1/total_requires, +0.2 per matched Prefers.
    pub async fn find_needs_by_resources(
        &self,
        slugs: &[String],
        lat: f64,
        lng: f64,
        radius_km: f64,
        limit: u32,
    ) -> Result<Vec<ResourceMatch>, neo4rs::Error> {
        let lat_delta = radius_km / 111.0;
        let lng_delta = radius_km / (111.0 * lat.to_radians().cos());

        let bbox = bbox_exists("n");
        let cypher = format!("MATCH (r:Resource)<-[e:REQUIRES|PREFERS]-(n)
             WHERE r.slug IN $slugs
               AND (n:HelpRequest OR n:Gathering)
               AND n.confidence >= $min_confidence
               AND ({bbox} OR NOT EXISTS {{ MATCH (n)-[:{LOC_EDGES}]->(:Location) }})
             WITH n, collect({{slug: r.slug, type: type(e)}}) AS matched_resources
             OPTIONAL MATCH (n)-[:REQUIRES]->(all_req:Resource)
             OPTIONAL MATCH (n)-[:PREFERS]->(all_pref:Resource)
             RETURN n,
                    matched_resources,
                    collect(DISTINCT all_req.slug) AS all_requires,
                    collect(DISTINCT all_pref.slug) AS all_prefers");

        let slug_strings: Vec<String> = slugs.iter().map(|s| s.to_string()).collect();
        let q = query(&cypher)
            .param("slugs", slug_strings)
            .param("min_lat", lat - lat_delta)
            .param("max_lat", lat + lat_delta)
            .param("min_lng", lng - lng_delta)
            .param("max_lng", lng + lng_delta)
            .param("min_confidence", CONFIDENCE_DISPLAY_LIMITED as f64);

        let slug_set: std::collections::HashSet<&str> = slugs.iter().map(|s| s.as_str()).collect();
        let mut matches = Vec::new();

        let mut stream = self.client.execute(q).await?;
        while let Some(row) = stream.next().await? {
            // Try to parse as Need or Gathering
            let node = row_to_node(&row, NodeType::HelpRequest)
                .or_else(|| row_to_node(&row, NodeType::Gathering));

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

    /// Find Aid nodes that OFFER a specific resource.
    pub async fn find_aids_by_resource(
        &self,
        slug: &str,
        lat: f64,
        lng: f64,
        radius_km: f64,
        limit: u32,
    ) -> Result<Vec<ResourceMatch>, neo4rs::Error> {
        let lat_delta = radius_km / 111.0;
        let lng_delta = radius_km / (111.0 * lat.to_radians().cos());

        let bbox = bbox_exists("n");
        let cypher = format!("MATCH (r:Resource {{slug: $slug}})<-[:OFFERS]-(n:Resource)
             WHERE n.confidence >= $min_confidence
               AND ({bbox} OR NOT EXISTS {{ MATCH (n)-[:{LOC_EDGES}]->(:Location) }})
             RETURN n
             ORDER BY n.cause_heat DESC, n.confidence DESC
             LIMIT $limit");

        let q = query(&cypher)
            .param("slug", slug)
            .param("min_lat", lat - lat_delta)
            .param("max_lat", lat + lat_delta)
            .param("min_lng", lng - lng_delta)
            .param("max_lng", lng + lng_delta)
            .param("min_confidence", CONFIDENCE_DISPLAY_LIMITED as f64)
            .param("limit", limit as i64);

        let mut results = Vec::new();
        let mut stream = self.client.execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Some(node) = row_to_node(&row, NodeType::Resource) {
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
        let mut stream = self.client.execute(q).await?;
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
             OPTIONAL MATCH (r)<-[:REQUIRES]-(rq)
             WITH r, count(rq) AS req_count
             OPTIONAL MATCH (r)<-[:OFFERS]-(of)
             RETURN r.slug AS slug, r.name AS name,
                    req_count AS requires_count, count(of) AS offers_count
             ORDER BY (toInteger(req_count) - count(of)) DESC",
        );

        let mut gaps = Vec::new();
        let mut stream = self.client.execute(q).await?;
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

    // ========== Supervisor / Validation Issues ==========

    /// List ValidationIssue nodes for a region, with optional status filter and limit.
    pub async fn list_validation_issues(
        &self,
        region: &str,
        status_filter: Option<&str>,
        limit: i64,
    ) -> Result<Vec<ValidationIssueRow>, neo4rs::Error> {
        let cypher = if status_filter.is_some() {
            "MATCH (v:ValidationIssue)
             WHERE v.region = $region AND v.status = $status
             RETURN v
             ORDER BY v.created_at DESC
             LIMIT $limit"
        } else {
            "MATCH (v:ValidationIssue)
             WHERE v.region = $region
             RETURN v
             ORDER BY v.created_at DESC
             LIMIT $limit"
        };

        let mut q = neo4rs::query(cypher)
            .param("region", region.to_string())
            .param("limit", limit);

        if let Some(status) = status_filter {
            q = q.param("status", status.to_string());
        }

        let mut stream = self.client.execute(q).await?;
        let mut results = Vec::new();

        while let Some(row) = stream.next().await? {
            if let Ok(n) = row.get::<neo4rs::Node>("v") {
                results.push(ValidationIssueRow::from_neo4j_node(&n));
            }
        }

        Ok(results)
    }

    /// List validation issues that target a specific node (signal/source) by its UUID.
    pub async fn list_validation_issues_for_target(
        &self,
        target_id: &str,
        limit: i64,
    ) -> Result<Vec<ValidationIssueRow>, neo4rs::Error> {
        let q = neo4rs::query(
            "MATCH (v:ValidationIssue)
             WHERE v.target_id = $target_id
             RETURN v
             ORDER BY v.created_at DESC
             LIMIT $limit",
        )
        .param("target_id", target_id.to_string())
        .param("limit", limit);

        let mut stream = self.client.execute(q).await?;
        let mut results = Vec::new();

        while let Some(row) = stream.next().await? {
            if let Ok(n) = row.get::<neo4rs::Node>("v") {
                results.push(ValidationIssueRow::from_neo4j_node(&n));
            }
        }

        Ok(results)
    }

    /// Aggregate counts of ValidationIssues by type, severity, and status for a region.
    pub async fn validation_issue_summary(
        &self,
        region: &str,
    ) -> Result<ValidationIssueSummary, neo4rs::Error> {
        let q = neo4rs::query(
            "MATCH (v:ValidationIssue)
             WHERE v.region = $region
             RETURN
               sum(CASE WHEN v.status = 'open' THEN 1 ELSE 0 END) AS total_open,
               sum(CASE WHEN v.status = 'resolved' THEN 1 ELSE 0 END) AS total_resolved,
               sum(CASE WHEN v.status = 'dismissed' THEN 1 ELSE 0 END) AS total_dismissed,
               v.issue_type AS issue_type,
               v.severity AS severity,
               v.status AS status,
               count(v) AS cnt",
        )
        .param("region", region.to_string());

        let mut stream = self.client.execute(q).await?;

        let mut total_open = 0i64;
        let mut total_resolved = 0i64;
        let mut total_dismissed = 0i64;
        let mut count_by_type: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();
        let mut count_by_severity: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();

        while let Some(row) = stream.next().await? {
            total_open = row.get::<i64>("total_open").unwrap_or(0);
            total_resolved = row.get::<i64>("total_resolved").unwrap_or(0);
            total_dismissed = row.get::<i64>("total_dismissed").unwrap_or(0);
            let issue_type: String = row.get("issue_type").unwrap_or_default();
            let severity: String = row.get("severity").unwrap_or_default();
            let cnt: i64 = row.get("cnt").unwrap_or(0);

            if !issue_type.is_empty() {
                *count_by_type.entry(issue_type).or_insert(0) += cnt;
            }
            if !severity.is_empty() {
                *count_by_severity.entry(severity).or_insert(0) += cnt;
            }
        }

        Ok(ValidationIssueSummary {
            total_open,
            total_resolved,
            total_dismissed,
            count_by_type: count_by_type.into_iter().collect(),
            count_by_severity: count_by_severity.into_iter().collect(),
        })
    }

    /// Fetch a single source by UUID.
    pub async fn source_by_id(&self, id: &Uuid) -> Result<Option<SourceNode>, neo4rs::Error> {
        let cypher = "MATCH (s:Source {id: $id})
            RETURN s.id AS id, s.canonical_key AS canonical_key,
                   s.canonical_value AS canonical_value, s.url AS url,
                   s.discovery_method AS discovery_method,
                   s.created_at AS created_at, s.last_scraped AS last_scraped,
                   s.last_produced_signal AS last_produced_signal,
                   s.signals_produced AS signals_produced,
                   s.signals_corroborated AS signals_corroborated,
                   s.consecutive_empty_runs AS consecutive_empty_runs,
                   s.active AS active, s.gap_context AS gap_context,
                   s.weight AS weight, s.cadence_hours AS cadence_hours,
                   s.avg_signals_per_scrape AS avg_signals_per_scrape,
                   s.quality_penalty AS quality_penalty,
                   s.source_role AS source_role,
                   s.scrape_count AS scrape_count,
                   s.cw_page AS cw_page, s.cw_feed AS cw_feed,
                   s.cw_media AS cw_media, s.cw_discussion AS cw_discussion,
                   s.cw_events AS cw_events";

        let q = query(cypher).param("id", id.to_string());
        let mut stream = self.client.execute(q).await?;
        if let Some(row) = stream.next().await? {
            Ok(row_to_source_node(&row))
        } else {
            Ok(None)
        }
    }

    /// Fetch recent signals linked to an actor via ACTED_IN.
    pub async fn signals_for_actor(&self, actor_id: &Uuid) -> Result<Vec<SignalBrief>, neo4rs::Error> {
        let cypher = "MATCH (a:Actor {id: $id})-[rel:ACTED_IN]->(n)-[:PRODUCED_BY]->(s:Source)
            RETURN n.id AS id, n.title AS title, labels(n)[0] AS signal_type,
                   n.confidence AS confidence, n.extracted_at AS extracted_at,
                   s.url AS source_url_from_source, n.review_status AS review_status,
                   n.location_name AS location_name, n.content_date AS content_date,
                   rel.role AS role
            ORDER BY n.extracted_at DESC
            LIMIT 50";

        let q = query(cypher).param("id", actor_id.to_string());
        let mut signals = Vec::new();
        let mut stream = self.client.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let id = match Uuid::parse_str(&id_str) {
                Ok(u) => u,
                Err(_) => continue,
            };
            signals.push(SignalBrief {
                id,
                title: row.get("title").unwrap_or_default(),
                signal_type: row.get("signal_type").unwrap_or_default(),
                confidence: row.get::<f64>("confidence").unwrap_or(0.0) as f32,
                extracted_at: {
                    let raw: Option<String> = row.get("extracted_at").ok();
                    raw.and_then(|s| {
                        chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S%.f")
                            .ok()
                            .map(|n| n.and_utc())
                    })
                },
                url: row.get("source_url_from_source").unwrap_or_default(),
                review_status: row.get("review_status").unwrap_or_else(|_| "staged".to_string()),
                location_name: row.get("location_name").ok(),
                content_date: row_datetime_opt_pub(&row, "content_date"),
            });
        }
        Ok(signals)
    }

    /// Fetch distinct actors linked to signals from a source, with per-source signal counts.
    pub async fn actors_for_source(&self, source_id: &Uuid) -> Result<Vec<(rootsignal_common::ActorNode, u32)>, neo4rs::Error> {
        let cypher = "MATCH (a:Actor)-[:ACTED_IN]->(n)-[:PRODUCED_BY]->(s:Source {id: $id})
            RETURN a, count(DISTINCT n) AS source_signal_count
            ORDER BY source_signal_count DESC
            LIMIT 50";

        let q = query(cypher).param("id", source_id.to_string());
        let mut results = Vec::new();
        let mut stream = self.client.execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Some(actor) = row_to_actor(&row) {
                let count: i64 = row.get("source_signal_count").unwrap_or(0);
                results.push((actor, count as u32));
            }
        }
        Ok(results)
    }

    /// Fetch recent signals produced by a source.
    pub async fn signals_for_source(&self, source_id: &Uuid) -> Result<Vec<SignalBrief>, neo4rs::Error> {
        let cypher = "MATCH (n)-[:PRODUCED_BY]->(s:Source {id: $id})
            WHERE n.review_status IN ['staged', 'accepted']
            RETURN n.id AS id, n.title AS title, labels(n)[0] AS signal_type,
                   n.confidence AS confidence, n.extracted_at AS extracted_at,
                   s.url AS source_url_from_source, n.review_status AS review_status,
                   n.location_name AS location_name, n.content_date AS content_date
            ORDER BY n.extracted_at DESC
            LIMIT 50";

        let q = query(cypher).param("id", source_id.to_string());
        let mut signals = Vec::new();
        let mut stream = self.client.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let id = match Uuid::parse_str(&id_str) {
                Ok(u) => u,
                Err(_) => continue,
            };
            signals.push(SignalBrief {
                id,
                title: row.get("title").unwrap_or_default(),
                signal_type: row.get("signal_type").unwrap_or_default(),
                confidence: row.get::<f64>("confidence").unwrap_or(0.0) as f32,
                extracted_at: row_datetime_opt_pub(&row, "extracted_at"),
                url: row.get("source_url_from_source").unwrap_or_default(),
                review_status: row.get("review_status").unwrap_or_default(),
                location_name: row.get("location_name").ok(),
                content_date: row_datetime_opt_pub(&row, "content_date"),
            });
        }
        Ok(signals)
    }

    /// Fetch signals created by a specific scout run.
    pub async fn signals_for_run(&self, run_id: &str, limit: i64) -> Result<(Vec<SignalBrief>, i64), neo4rs::Error> {
        let count_cypher = "MATCH (n) WHERE n.scout_run_id = $run_id
            AND (n:Gathering OR n:Resource OR n:HelpRequest OR n:Announcement OR n:Concern OR n:Condition)
            RETURN count(n) AS cnt";
        let cq = query(count_cypher).param("run_id", run_id);
        let mut cs = self.client.execute(cq).await?;
        let total = if let Some(row) = cs.next().await? {
            row.get::<i64>("cnt").unwrap_or(0)
        } else {
            0
        };

        let cypher = "MATCH (n) WHERE n.scout_run_id = $run_id
            AND (n:Gathering OR n:Resource OR n:HelpRequest OR n:Announcement OR n:Concern OR n:Condition)
            RETURN n.id AS id, n.title AS title, labels(n)[0] AS signal_type,
                   n.confidence AS confidence, n.extracted_at AS extracted_at,
                   n.url AS url, n.review_status AS review_status,
                   n.location_name AS location_name, n.content_date AS content_date
            ORDER BY n.confidence DESC
            LIMIT $limit";
        let q = query(cypher).param("run_id", run_id).param("limit", limit);
        let mut signals = Vec::new();
        let mut stream = self.client.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let id = match Uuid::parse_str(&id_str) {
                Ok(u) => u,
                Err(_) => continue,
            };
            signals.push(SignalBrief {
                id,
                title: row.get("title").unwrap_or_default(),
                signal_type: row.get("signal_type").unwrap_or_default(),
                confidence: row.get::<f64>("confidence").unwrap_or(0.0) as f32,
                extracted_at: row_datetime_opt_pub(&row, "extracted_at"),
                url: row.get("url").unwrap_or_default(),
                review_status: row.get("review_status").unwrap_or_default(),
                location_name: row.get("location_name").ok(),
                content_date: row_datetime_opt_pub(&row, "content_date"),
            });
        }
        Ok((signals, total))
    }

    pub async fn signal_count_for_source(&self, source_id: &Uuid) -> Result<i64, neo4rs::Error> {
        let cypher = "MATCH (n)-[:PRODUCED_BY]->(s:Source {id: $id})
            WHERE n.review_status IN ['staged', 'accepted']
            RETURN count(n) AS cnt";

        let q = query(cypher).param("id", source_id.to_string());
        let mut stream = self.client.execute(q).await?;
        if let Some(row) = stream.next().await? {
            Ok(row.get::<i64>("cnt").unwrap_or(0))
        } else {
            Ok(0)
        }
    }

    /// Fetch the LINKED_FROM neighbourhood around a source for the discovery tree.
    /// Returns (nodes, edges) where edges are (child_id, parent_id) pairs.
    pub async fn discovery_tree(
        &self,
        source_id: &Uuid,
    ) -> Result<(Vec<DiscoveryTreeRow>, Vec<(String, String)>), neo4rs::Error> {
        let cypher = "
            MATCH (root:Source {id: $id})
            OPTIONAL MATCH (root)-[:LINKED_FROM*1..3]->(ancestor:Source)
            OPTIONAL MATCH (child:Source)-[:LINKED_FROM]->(root)
            WITH root,
                 COLLECT(DISTINCT ancestor) AS ancestors,
                 COLLECT(DISTINCT child) AS children
            UNWIND (ancestors + [root] + children) AS node
            WITH DISTINCT node
            OPTIONAL MATCH (node)-[:LINKED_FROM]->(parent:Source)
            RETURN node.id AS id, node.canonical_value AS canonical_value,
                   node.discovery_method AS discovery_method, node.active AS active,
                   node.signals_produced AS signals_produced,
                   parent.id AS parent_id";

        let q = query(cypher).param("id", source_id.to_string());
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut stream = self.client.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id: String = row.get("id").unwrap_or_default();
            if id.is_empty() || !seen.insert(id.clone()) {
                continue;
            }
            nodes.push(DiscoveryTreeRow {
                id: id.clone(),
                canonical_value: row.get("canonical_value").unwrap_or_default(),
                discovery_method: row.get("discovery_method").unwrap_or_default(),
                active: row.get("active").unwrap_or(true),
                signals_produced: row.get::<i64>("signals_produced").unwrap_or(0) as u32,
            });
            let parent_id: String = row.get("parent_id").unwrap_or_default();
            if !parent_id.is_empty() {
                edges.push((id, parent_id));
            }
        }
        Ok((nodes, edges))
    }
}

/// Row from the discovery tree query.
#[derive(Debug, Clone)]
pub struct DiscoveryTreeRow {
    pub id: String,
    pub canonical_value: String,
    pub discovery_method: String,
    pub active: bool,
    pub signals_produced: u32,
}

/// A row from the ValidationIssue query.
#[derive(Debug, Clone)]
pub struct ValidationIssueRow {
    pub id: String,
    pub issue_type: String,
    pub severity: String,
    pub target_id: String,
    pub target_label: String,
    pub description: String,
    pub suggested_action: String,
    pub status: String,
    pub created_at: Option<DateTime<Utc>>,
    pub resolved_at: Option<DateTime<Utc>>,
}

impl ValidationIssueRow {
    fn from_neo4j_node(n: &neo4rs::Node) -> Self {
        Self {
            id: n.get("id").unwrap_or_default(),
            issue_type: n.get("issue_type").unwrap_or_default(),
            severity: n.get("severity").unwrap_or_default(),
            target_id: n.get("target_id").unwrap_or_default(),
            target_label: n.get("target_label").unwrap_or_default(),
            description: n.get("description").unwrap_or_default(),
            suggested_action: n.get("suggested_action").unwrap_or_default(),
            status: n.get("status").unwrap_or_default(),
            created_at: n.get::<String>("created_at").ok().and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|d| d.with_timezone(&Utc))
                    .or_else(|| {
                        NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S%.f")
                            .ok()
                            .map(|d| d.and_utc())
                    })
            }),
            resolved_at: n.get::<String>("resolved_at").ok().and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|d| d.with_timezone(&Utc))
                    .or_else(|| {
                        NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S%.f")
                            .ok()
                            .map(|d| d.and_utc())
                    })
            }),
        }
    }
}

/// Summary counts for validation issues.
#[derive(Debug, Clone)]
pub struct ValidationIssueSummary {
    pub total_open: i64,
    pub total_resolved: i64,
    pub total_dismissed: i64,
    pub count_by_type: Vec<(String, i64)>,
    pub count_by_severity: Vec<(String, i64)>,
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
        NodeType::Gathering => "Gathering",
        NodeType::Resource => "Resource",
        NodeType::HelpRequest => "HelpRequest",
        NodeType::Announcement => "Announcement",
        NodeType::Concern => "Concern",
        NodeType::Condition => "Condition",
        NodeType::Citation => "Citation",
    }
}

/// Inverse of `node_type_label`: map a Neo4j label string back to a NodeType.
fn label_to_node_type(label: &str) -> Option<NodeType> {
    match label {
        "Gathering" => Some(NodeType::Gathering),
        "Resource" => Some(NodeType::Resource),
        "HelpRequest" => Some(NodeType::HelpRequest),
        "Announcement" => Some(NodeType::Announcement),
        "Concern" => Some(NodeType::Concern),
        "Condition" => Some(NodeType::Condition),
        "Citation" => Some(NodeType::Citation),
        _ => None,
    }
}

/// Parse a row that includes a `node_label` column (from UNION queries) and dispatch
/// to `row_to_node` with the correct NodeType.
pub(crate) fn row_to_node_by_label(row: &neo4rs::Row) -> Option<Node> {
    let label: String = row.get("node_label").ok()?;
    let nt = label_to_node_type(&label)?;
    let mut node = row_to_node(row, nt)?;
    // Derive from_location at query time from authored actor's location
    if let (Ok(lat), Ok(lng)) = (row.get::<f64>("author_lat"), row.get::<f64>("author_lng")) {
        if let Some(meta) = node.meta_mut() {
            meta.locations.push(Location {
                point: Some(GeoPoint {
                    lat,
                    lng,
                    precision: GeoPrecision::Approximate,
                }),
                name: None,
                address: None,
                role: Some("origin".to_string()),
                timezone: None,
            });
        }
    }
    Some(node)
}

/// Per-type Cypher WHERE clause fragment for expiration.
/// Returns an AND clause (or empty string) to inject into existing WHERE blocks.
pub(crate) fn expiry_clause(nt: NodeType) -> String {
    match nt {
        NodeType::Gathering => format!(
            "AND (n.is_recurring = true \
             OR n.starts_at IS NULL OR n.starts_at = '' \
             OR CASE \
               WHEN n.ends_at IS NOT NULL AND n.ends_at <> '' \
               THEN datetime(n.ends_at) >= datetime() - duration('PT{grace}H') \
               ELSE datetime(n.starts_at) >= datetime() - duration('PT{grace}H') \
             END)",
            grace = GATHERING_PAST_GRACE_HOURS,
        ),
        NodeType::HelpRequest => format!(
            "AND datetime(n.extracted_at) >= datetime() - duration('P{days}D')",
            days = NEED_EXPIRE_DAYS,
        ),
        NodeType::Resource => format!(
            "AND datetime(n.last_confirmed_active) >= datetime() - duration('P{days}D')",
            days = FRESHNESS_MAX_DAYS,
        ),
        NodeType::Announcement => format!(
            "AND datetime(n.last_confirmed_active) >= datetime() - duration('P{days}D')",
            days = NOTICE_EXPIRE_DAYS,
        ),
        NodeType::Concern => format!(
            "AND datetime(n.last_confirmed_active) >= datetime() - duration('P{days}D')",
            days = FRESHNESS_MAX_DAYS,
        ),
        NodeType::Condition => format!(
            "AND datetime(n.last_confirmed_active) >= datetime() - duration('P{days}D')",
            days = FRESHNESS_MAX_DAYS,
        ),
        NodeType::Citation => String::new(),
    }
}

/// Apply sensitivity-based coordinate fuzzing to a node.
pub(crate) fn fuzz_node(mut node: Node) -> Node {
    if let Some(meta) = node_meta_mut(&mut node) {
        if let Some(first) = meta.locations.first_mut() {
            if let Some(ref mut pt) = first.point {
                *pt = fuzz_location(*pt, meta.sensitivity);
            }
        }
    }
    node
}

fn node_meta_mut(node: &mut Node) -> Option<&mut NodeMeta> {
    match node {
        Node::Gathering(n) => Some(&mut n.meta),
        Node::Resource(n) => Some(&mut n.meta),
        Node::HelpRequest(n) => Some(&mut n.meta),
        Node::Announcement(n) => Some(&mut n.meta),
        Node::Concern(n) => Some(&mut n.meta),
        Node::Condition(n) => Some(&mut n.meta),
        Node::Citation(_) => None,
    }
}

/// Safety-net display filter. Primary filtering happens in Cypher queries via `expiry_clause()`;
/// this catches anything that slips through (e.g. direct ID lookups).
pub(crate) fn passes_display_filter(node: &Node) -> bool {
    let Some(meta) = node.meta() else {
        return true;
    };

    let now = Utc::now();

    // Gathering-specific: hide past non-recurring events (only if date is known)
    if let Node::Gathering(e) = node {
        if !e.is_recurring {
            if let Some(starts_at) = e.starts_at {
                let event_end = e.ends_at.unwrap_or(starts_at);
                if (now - event_end).num_hours() > GATHERING_PAST_GRACE_HOURS {
                    return false;
                }
            }
            // Gatherings with no starts_at: fall through to general freshness check
        }
    }

    // Need-specific: expire after NEED_EXPIRE_DAYS
    if matches!(node, Node::HelpRequest(_)) {
        if (now - meta.extracted_at).num_days() > NEED_EXPIRE_DAYS {
            return false;
        }
    }

    // Notice-specific: expire after NOTICE_EXPIRE_DAYS (based on last_confirmed_active)
    if matches!(node, Node::Announcement(_)) {
        if (now - meta.last_confirmed_active).num_days() > NOTICE_EXPIRE_DAYS {
            return false;
        }
    }

    // General freshness check (recurring events still exempt — they persist between occurrences)
    let age_days = (now - meta.last_confirmed_active).num_days();
    if age_days > FRESHNESS_MAX_DAYS {
        match node {
            Node::Gathering(e) if e.is_recurring => {}
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
    let corroboration_count: i64 = n.get("corroboration_count").unwrap_or(0);
    let url: String = n.get("url").unwrap_or_default();

    // Parse timestamps
    let extracted_at = parse_datetime_prop(&n, "extracted_at");
    let published_at = parse_optional_datetime_prop(&n, "published_at");
    let last_confirmed_active = parse_datetime_prop(&n, "last_confirmed_active");

    let source_diversity: i64 = n.get("source_diversity").unwrap_or(1);
    let cause_heat: f64 = n.get("cause_heat").unwrap_or(0.0);
    let channel_diversity: i64 = n.get("channel_diversity").unwrap_or(1);

    let category: Option<String> = n.get("category").ok().filter(|s: &String| !s.is_empty());

    let meta = NodeMeta {
        id,
        title,
        summary,
        sensitivity,
        confidence: confidence as f32,
        corroboration_count: corroboration_count as u32,
        locations: {
            let json_str: String = n.get("locations_json").unwrap_or_default();
            if json_str.is_empty() {
                Vec::new()
            } else {
                serde_json::from_str::<Vec<Location>>(&json_str).unwrap_or_default()
            }
        },
        url,
        extracted_at,
        published_at,
        last_confirmed_active,
        source_diversity: source_diversity as u32,
        cause_heat,
        channel_diversity: channel_diversity as u32,
        implied_queries: Vec::new(),
        review_status: {
            let s: String = n.get("review_status").unwrap_or_default();
            match s.as_str() {
                "accepted" => rootsignal_common::ReviewStatus::Accepted,
                "rejected" => rootsignal_common::ReviewStatus::Rejected,
                "corrected" => rootsignal_common::ReviewStatus::Corrected,
                _ => rootsignal_common::ReviewStatus::Staged,
            }
        },
        was_corrected: n.get("was_corrected").unwrap_or(false),
        corrections: {
            let c: String = n.get("corrections").unwrap_or_default();
            if c.is_empty() {
                None
            } else {
                Some(c)
            }
        },
        rejection_reason: {
            let r: String = n.get("rejection_reason").unwrap_or_default();
            if r.is_empty() {
                None
            } else {
                Some(r)
            }
        },
        mentioned_entities: vec![],
        category,
    };

    match node_type {
        NodeType::Gathering => {
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

            Some(Node::Gathering(GatheringNode {
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
        NodeType::Resource => {
            let action_url: String = n.get("action_url").unwrap_or_default();
            let availability: String = n.get("availability").unwrap_or_default();
            let eligibility: String = n.get("eligibility").unwrap_or_default();
            let is_ongoing: bool = n.get("is_ongoing").unwrap_or(false);

            Some(Node::Resource(ResourceOfferNode {
                meta,
                action_url,
                availability: if availability.is_empty() {
                    None
                } else {
                    Some(availability)
                },
                eligibility: if eligibility.is_empty() {
                    None
                } else {
                    Some(eligibility)
                },
                is_ongoing,
            }))
        }
        NodeType::HelpRequest => {
            let urgency_str: String = n.get("urgency").unwrap_or_default();
            let urgency = match urgency_str.as_str() {
                "high" => Urgency::High,
                "critical" => Urgency::Critical,
                "low" => Urgency::Low,
                _ => Urgency::Medium,
            };
            let what_needed: String = n.get("what_needed").unwrap_or_default();
            let action_url: String = n.get("action_url").unwrap_or_default();
            let stated_goal: String = n.get("stated_goal").unwrap_or_default();

            Some(Node::HelpRequest(HelpRequestNode {
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
                stated_goal: if stated_goal.is_empty() { None } else { Some(stated_goal) },
            }))
        }
        NodeType::Announcement => {
            let severity_str: String = n.get("severity").unwrap_or_default();
            let severity = match severity_str.as_str() {
                "high" => Severity::High,
                "critical" => Severity::Critical,
                "low" => Severity::Low,
                _ => Severity::Medium,
            };
            let subject: String = n.get("subject").unwrap_or_default();
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
            // Legacy property from pre-refactor correction events — no active write path,
            // but historical data may still carry it. Gracefully defaults to empty.
            let source_authority: String = n.get("source_authority").unwrap_or_default();

            Some(Node::Announcement(AnnouncementNode {
                meta,
                severity,
                subject: if subject.is_empty() {
                    None
                } else {
                    Some(subject)
                },
                effective_date,
                source_authority: if source_authority.is_empty() {
                    None
                } else {
                    Some(source_authority)
                },
            }))
        }
        NodeType::Concern => {
            let severity_str: String = n.get("severity").unwrap_or_default();
            let severity = match severity_str.as_str() {
                "high" => Severity::High,
                "critical" => Severity::Critical,
                "low" => Severity::Low,
                _ => Severity::Medium,
            };
            let subject: String = n.get("subject").unwrap_or_default();
            let opposing: String = n.get("opposing").unwrap_or_default();

            Some(Node::Concern(ConcernNode {
                meta,
                severity,
                subject: if subject.is_empty() {
                    None
                } else {
                    Some(subject)
                },
                opposing: if opposing.is_empty() {
                    None
                } else {
                    Some(opposing)
                },
            }))
        }
        NodeType::Condition => {
            let severity_str: String = n.get("severity").unwrap_or_default();
            let severity = match severity_str.as_str() {
                "high" => Severity::High,
                "critical" => Severity::Critical,
                "low" => Severity::Low,
                _ => Severity::Medium,
            };
            let subject: String = n.get("subject").unwrap_or_default();
            let observed_by: String = n.get("observed_by").unwrap_or_default();
            let measurement: String = n.get("measurement").unwrap_or_default();
            let affected_scope: String = n.get("affected_scope").unwrap_or_default();

            Some(Node::Condition(ConditionNode {
                meta,
                severity,
                subject: if subject.is_empty() { None } else { Some(subject) },
                observed_by: if observed_by.is_empty() { None } else { Some(observed_by) },
                measurement: if measurement.is_empty() { None } else { Some(measurement) },
                affected_scope: if affected_scope.is_empty() { None } else { Some(affected_scope) },
            }))
        }
        NodeType::Citation => None,
    }
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

pub(crate) fn extract_citation(row: &neo4rs::Row) -> Vec<CitationNode> {
    // Evidence nodes come as a collected list, sorted by confidence descending
    let nodes: Vec<neo4rs::Node> = row.get("evidence").unwrap_or_default();
    let mut evidence: Vec<CitationNode> = nodes
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

            let channel_type_str: String = n.get("channel_type").unwrap_or_default();
            let channel_type = match channel_type_str.as_str() {
                "social" => Some(rootsignal_common::ChannelType::Social),
                "direct_action" => Some(rootsignal_common::ChannelType::DirectAction),
                "community_media" => Some(rootsignal_common::ChannelType::CommunityMedia),
                "press" => Some(rootsignal_common::ChannelType::Press),
                _ => None,
            };

            Some(CitationNode {
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
                confidence: if ev_conf > 0.0 {
                    Some(ev_conf as f32)
                } else {
                    None
                },
                channel_type,
            })
        })
        .collect();
    evidence.sort_by(|a, b| {
        let ca = a.confidence.unwrap_or(0.0);
        let cb = b.confidence.unwrap_or(0.0);
        cb.partial_cmp(&ca).unwrap_or(std::cmp::Ordering::Equal)
    });
    evidence
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

pub(crate) fn row_to_actor(row: &neo4rs::Row) -> Option<rootsignal_common::ActorNode> {
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
    let canonical_key: String = n.get("canonical_key").unwrap_or_default();
    let domains: Vec<String> = n.get("domains").unwrap_or_default();
    let social_urls: Vec<String> = n.get("social_urls").unwrap_or_default();
    let description: String = n.get("description").unwrap_or_default();
    let signal_count: i64 = n.get("signal_count").unwrap_or(0);
    let first_seen = parse_datetime_prop(&n, "first_seen");
    let last_active = parse_datetime_prop(&n, "last_active");
    let typical_roles: Vec<String> = n.get("typical_roles").unwrap_or_default();

    let bio: Option<String> = n.get("bio").ok();
    let external_url: Option<String> = n.get("external_url").ok().filter(|u: &String| !u.is_empty());
    let location_lat: Option<f64> = n.get("location_lat").ok();
    let location_lng: Option<f64> = n.get("location_lng").ok();
    let location_name_entity: Option<String> = n.get("location_name").ok();

    Some(rootsignal_common::ActorNode {
        id,
        name,
        actor_type,
        canonical_key,
        domains,
        social_urls,
        description,
        signal_count: signal_count as u32,
        first_seen,
        last_active,
        typical_roles,
        bio,
        external_url,
        location_lat,
        location_lng,
        location_name: location_name_entity,
        discovery_depth: n.get::<i64>("discovery_depth").unwrap_or(0) as u32,
    })
}

// --- Situation reader methods ---

impl PublicGraphReader {
    /// Fetch a single situation by ID.
    pub async fn situation_by_id(
        &self,
        id: &Uuid,
    ) -> Result<Option<rootsignal_common::SituationNode>, neo4rs::Error> {
        let g = &self.client;

        let q = query(
            "MATCH (s:Situation {id: $id})
             RETURN s",
        )
        .param("id", id.to_string());

        let mut stream = g.execute(q).await?;
        match stream.next().await? {
            Some(row) => Ok(row_to_situation(&row, "s")),
            None => Ok(None),
        }
    }

    /// Fetch situations within a geographic bounding box, ordered by temperature descending.
    pub async fn situations_in_bounds(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
        limit: u32,
        arc_filter: Option<&str>,
    ) -> Result<Vec<rootsignal_common::SituationNode>, neo4rs::Error> {
        let g = &self.client;

        let (arc_clause, arc_param): (&str, Option<&str>) = match arc_filter {
            Some(arc) => ("AND s.arc = $arc", Some(arc)),
            None => ("", None),
        };

        let mut q = query(&format!(
            "MATCH (s:Situation)
             WHERE s.centroid_lat >= $min_lat AND s.centroid_lat <= $max_lat
               AND s.centroid_lng >= $min_lng AND s.centroid_lng <= $max_lng
               {arc_clause}
             RETURN s
             ORDER BY s.temperature DESC
             LIMIT $limit"
        ))
        .param("min_lat", min_lat)
        .param("max_lat", max_lat)
        .param("min_lng", min_lng)
        .param("max_lng", max_lng)
        .param("limit", limit as i64);

        if let Some(arc) = arc_param {
            q = q.param("arc", arc);
        }

        let mut stream = g.execute(q).await?;
        let mut results = Vec::new();
        while let Some(row) = stream.next().await? {
            if let Some(sit) = row_to_situation(&row, "s") {
                results.push(sit);
            }
        }
        Ok(results)
    }

    /// Fetch situations filtered by arc, ordered by temperature descending.
    pub async fn situations_by_arc(
        &self,
        arc: &str,
        limit: u32,
    ) -> Result<Vec<rootsignal_common::SituationNode>, neo4rs::Error> {
        let g = &self.client;

        let q = query(
            "MATCH (s:Situation {arc: $arc})
             RETURN s
             ORDER BY s.temperature DESC
             LIMIT $limit",
        )
        .param("arc", arc)
        .param("limit", limit as i64);

        let mut stream = g.execute(q).await?;
        let mut results = Vec::new();
        while let Some(row) = stream.next().await? {
            if let Some(sit) = row_to_situation(&row, "s") {
                results.push(sit);
            }
        }
        Ok(results)
    }

    /// Fetch top situations ordered by temperature descending.
    pub async fn situations(
        &self,
        limit: u32,
    ) -> Result<Vec<rootsignal_common::SituationNode>, neo4rs::Error> {
        let g = &self.client;

        let q = query(
            "MATCH (s:Situation)
             RETURN s
             ORDER BY s.temperature DESC
             LIMIT $limit",
        )
        .param("limit", limit as i64);

        let mut stream = g.execute(q).await?;
        let mut results = Vec::new();
        while let Some(row) = stream.next().await? {
            if let Some(sit) = row_to_situation(&row, "s") {
                results.push(sit);
            }
        }
        Ok(results)
    }

    /// Fetch dispatches for a situation, ordered by creation time.
    pub async fn dispatches_for_situation(
        &self,
        situation_id: &Uuid,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<rootsignal_common::DispatchNode>, neo4rs::Error> {
        let g = &self.client;

        let q = query(
            "MATCH (s:Situation {id: $id})-[:HAS_DISPATCH]->(d:Dispatch)
             RETURN d
             ORDER BY d.created_at ASC
             SKIP $offset
             LIMIT $limit",
        )
        .param("id", situation_id.to_string())
        .param("offset", offset as i64)
        .param("limit", limit as i64);

        let mut stream = g.execute(q).await?;
        let mut results = Vec::new();
        while let Some(row) = stream.next().await? {
            if let Some(dispatch) = row_to_dispatch(&row, "d") {
                results.push(dispatch);
            }
        }
        Ok(results)
    }

    /// Total situation count.
    pub async fn situation_count(&self) -> Result<u64, neo4rs::Error> {
        let g = &self.client;
        let q = query("MATCH (s:Situation) RETURN count(s) AS cnt");
        let mut stream = g.execute(q).await?;
        match stream.next().await? {
            Some(row) => {
                let cnt: i64 = row.get("cnt").unwrap_or(0);
                Ok(cnt as u64)
            }
            None => Ok(0),
        }
    }

    /// Situation count grouped by arc.
    pub async fn situation_count_by_arc(&self) -> Result<Vec<(String, u64)>, neo4rs::Error> {
        let g = &self.client;
        let q = query(
            "MATCH (s:Situation)
             RETURN s.arc AS arc, count(s) AS cnt
             ORDER BY cnt DESC",
        );
        let mut stream = g.execute(q).await?;
        let mut results = Vec::new();
        while let Some(row) = stream.next().await? {
            let arc: String = row.get("arc").unwrap_or_default();
            let cnt: i64 = row.get("cnt").unwrap_or(0);
            results.push((arc, cnt as u64));
        }
        Ok(results)
    }

    /// Situation count grouped by category.
    pub async fn situation_count_by_category(&self) -> Result<Vec<(String, u64)>, neo4rs::Error> {
        let g = &self.client;
        let q = query(
            "MATCH (s:Situation)
             WHERE s.category IS NOT NULL
             RETURN s.category AS cat, count(s) AS cnt
             ORDER BY cnt DESC",
        );
        let mut stream = g.execute(q).await?;
        let mut results = Vec::new();
        while let Some(row) = stream.next().await? {
            let cat: String = row.get("cat").unwrap_or_default();
            let cnt: i64 = row.get("cnt").unwrap_or(0);
            results.push((cat, cnt as u64));
        }
        Ok(results)
    }

    /// Fetch situations that a signal evidences (many-to-many via PART_OF).
    pub async fn situations_for_signal(
        &self,
        signal_id: &Uuid,
    ) -> Result<Vec<rootsignal_common::SituationNode>, neo4rs::Error> {
        let g = &self.client;

        let q = query(
            "MATCH (sig)-[:PART_OF]->(s:Situation)
             WHERE sig.id = $signal_id
             RETURN s
             ORDER BY s.temperature DESC",
        )
        .param("signal_id", signal_id.to_string());

        let mut stream = g.execute(q).await?;
        let mut results = Vec::new();
        while let Some(row) = stream.next().await? {
            if let Some(sit) = row_to_situation(&row, "s") {
                results.push(sit);
            }
        }
        Ok(results)
    }
}

/// Parse a Situation node from a neo4rs Row.
fn row_to_situation(row: &neo4rs::Row, key: &str) -> Option<rootsignal_common::SituationNode> {
    let n: neo4rs::Node = row.get(key).ok()?;
    let id_str: String = n.get("id").ok()?;
    let id = Uuid::parse_str(&id_str).ok()?;
    let headline: String = n.get("headline").unwrap_or_default();
    let lede: String = n.get("lede").unwrap_or_default();
    let arc_str: String = n.get("arc").unwrap_or_default();
    let arc: rootsignal_common::SituationArc = arc_str
        .parse()
        .unwrap_or(rootsignal_common::SituationArc::Emerging);

    let temperature: f64 = n.get("temperature").unwrap_or(0.0);
    let tension_heat: f64 = n.get("tension_heat").unwrap_or(0.0);
    let entity_velocity: f64 = n.get("entity_velocity").unwrap_or(0.0);
    let amplification: f64 = n.get("amplification").unwrap_or(0.0);
    let response_coverage: f64 = n.get("response_coverage").unwrap_or(0.0);
    let clarity_need: f64 = n.get("clarity_need").unwrap_or(0.0);

    let clarity_str: String = n.get("clarity").unwrap_or_default();
    let clarity: rootsignal_common::Clarity = clarity_str
        .parse()
        .unwrap_or(rootsignal_common::Clarity::Fuzzy);

    let centroid_lat: Option<f64> = n.get("centroid_lat").ok();
    let centroid_lng: Option<f64> = n.get("centroid_lng").ok();
    let location_name: Option<String> = n
        .get("location_name")
        .ok()
        .filter(|s: &String| !s.is_empty());

    let structured_state: String = n.get("structured_state").unwrap_or_default();

    let signal_count: i64 = n.get("signal_count").unwrap_or(0);
    let tension_count: i64 = n.get("tension_count").unwrap_or(0);
    let dispatch_count: i64 = n.get("dispatch_count").unwrap_or(0);
    let first_seen = parse_datetime_prop(&n, "first_seen");
    let last_updated = parse_datetime_prop(&n, "last_updated");

    let sensitivity_str: String = n.get("sensitivity").unwrap_or_default();
    let sensitivity = match sensitivity_str.as_str() {
        "elevated" => SensitivityLevel::Elevated,
        "sensitive" => SensitivityLevel::Sensitive,
        _ => SensitivityLevel::General,
    };

    let category: Option<String> = n.get("category").ok().filter(|s: &String| !s.is_empty());

    Some(rootsignal_common::SituationNode {
        id,
        headline,
        lede,
        arc,
        temperature,
        tension_heat,
        entity_velocity,
        amplification,
        response_coverage,
        clarity_need,
        clarity,
        centroid_lat,
        centroid_lng,
        location_name,
        structured_state,
        signal_count: signal_count as u32,
        tension_count: tension_count as u32,
        dispatch_count: dispatch_count as u32,
        first_seen,
        last_updated,
        sensitivity,
        category,
    })
}

/// Parse a Dispatch node from a neo4rs Row.
fn row_to_dispatch(row: &neo4rs::Row, key: &str) -> Option<rootsignal_common::DispatchNode> {
    let n: neo4rs::Node = row.get(key).ok()?;
    let id_str: String = n.get("id").ok()?;
    let id = Uuid::parse_str(&id_str).ok()?;
    let situation_id_str: String = n.get("situation_id").unwrap_or_default();
    let situation_id = Uuid::parse_str(&situation_id_str).ok()?;
    let body: String = n.get("body").unwrap_or_default();

    let signal_ids_raw: Vec<String> = n.get("signal_ids").unwrap_or_default();
    let signal_ids: Vec<Uuid> = signal_ids_raw
        .iter()
        .filter_map(|s| Uuid::parse_str(s).ok())
        .collect();

    let created_at = parse_datetime_prop(&n, "created_at");
    let dispatch_type_str: String = n.get("dispatch_type").unwrap_or_default();
    let dispatch_type: rootsignal_common::DispatchType = dispatch_type_str
        .parse()
        .unwrap_or(rootsignal_common::DispatchType::Update);

    let supersedes_str: Option<String> =
        n.get("supersedes").ok().filter(|s: &String| !s.is_empty());
    let supersedes = supersedes_str.and_then(|s| Uuid::parse_str(&s).ok());

    let flagged_for_review: bool = n.get("flagged_for_review").unwrap_or(false);
    let flag_reason: Option<String> = n.get("flag_reason").ok().filter(|s: &String| !s.is_empty());
    let fidelity_score: Option<f64> = n.get("fidelity_score").ok().filter(|v: &f64| *v >= 0.0);

    Some(rootsignal_common::DispatchNode {
        id,
        situation_id,
        body,
        signal_ids,
        created_at,
        dispatch_type,
        supersedes,
        flagged_for_review,
        flag_reason,
        fidelity_score,
    })
}

