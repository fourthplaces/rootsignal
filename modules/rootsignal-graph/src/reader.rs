use chrono::{DateTime, Utc};
use neo4rs::query;
use uuid::Uuid;

use rootsignal_common::{
    fuzz_location, AskNode, AudienceRole, EvidenceNode, EventNode, GeoPoint, GeoPrecision,
    GiveNode, Node, NodeMeta, NodeType, Severity, SensitivityLevel, TensionNode, Urgency,
    CONFIDENCE_DISPLAY_LIMITED, FRESHNESS_MAX_DAYS, SENSITIVE_CORROBORATION_MIN,
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
        let types = node_types
            .map(|t| t.to_vec())
            .unwrap_or_else(|| vec![NodeType::Event, NodeType::Give, NodeType::Ask, NodeType::Tension]);

        let mut results = Vec::new();

        for nt in &types {
            let label = node_type_label(*nt);
            let cypher = format!(
                "MATCH (n:{label})
                 WHERE n.location IS NOT NULL
                   AND point.distance(n.location, point({{latitude: $lat, longitude: $lng}})) <= $radius_m
                   AND n.confidence >= $min_confidence
                 RETURN n
                 ORDER BY n.confidence DESC, n.last_confirmed_active DESC
                 LIMIT 200"
            );

            let q = query(&cypher)
                .param("lat", lat)
                .param("lng", lng)
                .param("radius_m", radius_km * 1000.0)
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
        for nt in &[NodeType::Event, NodeType::Give, NodeType::Ask, NodeType::Tension] {
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

    /// List recent signals, ordered by freshness. Returns fuzzed coordinates.
    pub async fn list_recent(
        &self,
        limit: u32,
        node_types: Option<&[NodeType]>,
    ) -> Result<Vec<Node>, neo4rs::Error> {
        let types = node_types
            .map(|t| t.to_vec())
            .unwrap_or_else(|| vec![NodeType::Event, NodeType::Give, NodeType::Ask, NodeType::Tension]);

        let mut results = Vec::new();

        for nt in &types {
            let label = node_type_label(*nt);
            let cypher = format!(
                "MATCH (n:{label})
                 WHERE n.confidence >= $min_confidence
                 RETURN n
                 ORDER BY n.last_confirmed_active DESC
                 LIMIT $limit"
            );

            let q = query(&cypher)
                .param("min_confidence", CONFIDENCE_DISPLAY_LIMITED as f64)
                .param("limit", limit as i64);

            let mut stream = self.client.graph.execute(q).await?;
            while let Some(row) = stream.next().await? {
                if let Some(node) = row_to_node(&row, *nt) {
                    if passes_display_filter(&node) {
                        results.push(fuzz_node(node));
                    }
                }
            }
        }

        // Sort all results by last_confirmed_active descending
        results.sort_by(|a, b| {
            let a_time = a.meta().map(|m| m.last_confirmed_active);
            let b_time = b.meta().map(|m| m.last_confirmed_active);
            b_time.cmp(&a_time)
        });

        results.truncate(limit as usize);
        Ok(results)
    }

    // --- Admin/Quality queries (not public-facing, but through reader for safety) ---

    /// Get total signal count by type (for quality dashboard).
    pub async fn count_by_type(&self) -> Result<Vec<(NodeType, u64)>, neo4rs::Error> {
        let mut counts = Vec::new();
        for nt in &[NodeType::Event, NodeType::Give, NodeType::Ask, NodeType::Tension] {
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
            "CALL {
                MATCH (n:Event) RETURN n.confidence AS conf
                UNION ALL
                MATCH (n:Give) RETURN n.confidence AS conf
                UNION ALL
                MATCH (n:Ask) RETURN n.confidence AS conf
                UNION ALL
                MATCH (n:Tension) RETURN n.confidence AS conf
            }
            WITH CASE
                WHEN conf >= 0.8 THEN 'high (0.8+)'
                WHEN conf >= 0.6 THEN 'good (0.6-0.8)'
                WHEN conf >= 0.4 THEN 'limited (0.4-0.6)'
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

    /// Get audience role distribution (for quality dashboard).
    pub async fn audience_role_distribution(&self) -> Result<Vec<(String, u64)>, neo4rs::Error> {
        let q = query(
            "CALL {
                MATCH (n:Event) UNWIND n.audience_roles AS role RETURN role
                UNION ALL
                MATCH (n:Give) UNWIND n.audience_roles AS role RETURN role
                UNION ALL
                MATCH (n:Ask) UNWIND n.audience_roles AS role RETURN role
                UNION ALL
                MATCH (n:Tension) UNWIND n.audience_roles AS role RETURN role
            }
            RETURN role, count(*) AS cnt
            ORDER BY cnt DESC",
        );

        let mut stream = self.client.graph.execute(q).await?;
        let mut results = Vec::new();
        while let Some(row) = stream.next().await? {
            let role: String = row.get("role").unwrap_or_default();
            let cnt: i64 = row.get("cnt").unwrap_or(0);
            results.push((role, cnt as u64));
        }
        Ok(results)
    }

    /// Get freshness distribution (for quality dashboard).
    pub async fn freshness_distribution(&self) -> Result<Vec<(String, u64)>, neo4rs::Error> {
        let q = query(
            "CALL {
                MATCH (n:Event) RETURN n.last_confirmed_active AS ts
                UNION ALL
                MATCH (n:Give) RETURN n.last_confirmed_active AS ts
                UNION ALL
                MATCH (n:Ask) RETURN n.last_confirmed_active AS ts
                UNION ALL
                MATCH (n:Tension) RETURN n.last_confirmed_active AS ts
            }
            WITH duration.between(ts, datetime()).days AS age_days
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
}

// --- Helpers ---

fn node_type_label(nt: NodeType) -> &'static str {
    match nt {
        NodeType::Event => "Event",
        NodeType::Give => "Give",
        NodeType::Ask => "Ask",
        NodeType::Tension => "Tension",
        NodeType::Evidence => "Evidence",
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
        Node::Tension(n) => Some(&mut n.meta),
        Node::Evidence(_) => None,
    }
}

/// Check if a node passes the display filter:
/// - Sensitive nodes need corroboration_count >= 2
/// - Freshness within threshold (unless ongoing)
fn passes_display_filter(node: &Node) -> bool {
    let Some(meta) = node.meta() else {
        return true;
    };

    // Sensitive signals need corroboration
    if meta.sensitivity == SensitivityLevel::Sensitive
        && meta.corroboration_count < SENSITIVE_CORROBORATION_MIN
    {
        return false;
    }

    // Freshness check
    let age_days = (Utc::now() - meta.last_confirmed_active).num_days();
    if age_days > FRESHNESS_MAX_DAYS {
        // For events/asks that aren't ongoing, hide stale signals
        match node {
            Node::Give(g) if g.is_ongoing => {}    // ongoing gives are ok
            Node::Event(e) if e.is_recurring => {}  // recurring events are ok
            _ => return false,
        }
    }

    true
}

/// Parse a neo4rs Row into a typed Node.
fn row_to_node(row: &neo4rs::Row, node_type: NodeType) -> Option<Node> {
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
    let source_trust: f64 = n.get("source_trust").unwrap_or(0.5);
    let freshness_score: f64 = n.get("freshness_score").unwrap_or(0.5);
    let corroboration_count: i64 = n.get("corroboration_count").unwrap_or(0);
    let source_url: String = n.get("source_url").unwrap_or_default();
    let audience_roles_raw: Vec<String> = n.get("audience_roles").unwrap_or_default();

    // Parse location from point
    let location = parse_location(&n);

    // Parse timestamps
    let extracted_at = parse_datetime_prop(&n, "extracted_at");
    let last_confirmed_active = parse_datetime_prop(&n, "last_confirmed_active");

    let audience_roles = audience_roles_raw
        .iter()
        .filter_map(|s| parse_audience_role(s))
        .collect();

    let meta = NodeMeta {
        id,
        title,
        summary,
        sensitivity,
        confidence: confidence as f32,
        source_trust: source_trust as f32,
        freshness_score: freshness_score as f32,
        corroboration_count: corroboration_count as u32,
        location,
        source_url,
        extracted_at,
        last_confirmed_active,
        audience_roles,
    };

    match node_type {
        NodeType::Event => {
            let starts_at = parse_datetime_prop(&n, "starts_at");
            let ends_at_str: String = n.get("ends_at").unwrap_or_default();
            let ends_at = if ends_at_str.is_empty() {
                None
            } else {
                DateTime::parse_from_rfc3339(&ends_at_str).ok().map(|dt| dt.with_timezone(&Utc))
            };
            let action_url: String = n.get("action_url").unwrap_or_default();
            let organizer: String = n.get("organizer").unwrap_or_default();
            let is_recurring: bool = n.get("is_recurring").unwrap_or(false);

            Some(Node::Event(EventNode {
                meta,
                starts_at,
                ends_at,
                action_url,
                organizer: if organizer.is_empty() { None } else { Some(organizer) },
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
                availability,
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
                what_needed,
                action_url: if action_url.is_empty() { None } else { Some(action_url) },
                goal: if goal.is_empty() { None } else { Some(goal) },
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

            Some(Node::Tension(TensionNode { meta, severity }))
        }
        NodeType::Evidence => None,
    }
}

fn parse_location(n: &neo4rs::Node) -> Option<GeoPoint> {
    // neo4rs returns BoltPoint2D for point() values
    // Try to extract lat/lng from the point
    let point: neo4rs::BoltPoint2D = n.get("location").ok()?;
    Some(GeoPoint {
        lat: point.y.value,
        lng: point.x.value,
        precision: GeoPrecision::Exact,
    })
}

fn parse_datetime_prop(n: &neo4rs::Node, prop: &str) -> DateTime<Utc> {
    // Neo4j datetime comes back as a string when stored via datetime($str)
    if let Ok(s) = n.get::<String>(prop) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(&s) {
            return dt.with_timezone(&Utc);
        }
    }
    Utc::now()
}

fn parse_audience_role(s: &str) -> Option<AudienceRole> {
    match s {
        "volunteer" => Some(AudienceRole::Volunteer),
        "donor" => Some(AudienceRole::Donor),
        "neighbor" => Some(AudienceRole::Neighbor),
        "parent" => Some(AudienceRole::Parent),
        "youth" => Some(AudienceRole::Youth),
        "senior" => Some(AudienceRole::Senior),
        "immigrant" => Some(AudienceRole::Immigrant),
        "steward" => Some(AudienceRole::Steward),
        "civicparticipant" | "civic_participant" => Some(AudienceRole::CivicParticipant),
        "skillprovider" | "skill_provider" => Some(AudienceRole::SkillProvider),
        _ => None,
    }
}

fn extract_evidence(row: &neo4rs::Row) -> Vec<EvidenceNode> {
    // Evidence nodes come as a collected list
    let nodes: Vec<neo4rs::Node> = row.get("evidence").unwrap_or_default();
    nodes
        .into_iter()
        .filter_map(|n| {
            let id_str: String = n.get("id").ok()?;
            let id = Uuid::parse_str(&id_str).ok()?;
            let source_url: String = n.get("source_url").unwrap_or_default();
            let retrieved_at = parse_evidence_datetime(&n, "retrieved_at");
            let content_hash: String = n.get("content_hash").unwrap_or_default();
            let snippet: String = n.get("snippet").unwrap_or_default();

            Some(EvidenceNode {
                id,
                source_url,
                retrieved_at,
                content_hash,
                snippet: if snippet.is_empty() { None } else { Some(snippet) },
            })
        })
        .collect()
}

fn parse_evidence_datetime(n: &neo4rs::Node, prop: &str) -> DateTime<Utc> {
    if let Ok(s) = n.get::<String>(prop) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(&s) {
            return dt.with_timezone(&Utc);
        }
    }
    Utc::now()
}
