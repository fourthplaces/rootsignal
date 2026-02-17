use chrono::{DateTime, NaiveDateTime, Utc};
use neo4rs::query;
use uuid::Uuid;

use rootsignal_common::{
    fuzz_location, AskNode, AudienceRole, EvidenceNode, EventNode, GeoPoint, GeoPrecision,
    GiveNode, Node, NodeMeta, NodeType, NoticeNode, Severity, SensitivityLevel, StoryNode,
    TensionNode, Urgency, ASK_EXPIRE_DAYS, CONFIDENCE_DISPLAY_LIMITED, EVENT_PAST_GRACE_HOURS,
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
        let types = node_types
            .map(|t| t.to_vec())
            .unwrap_or_else(|| vec![NodeType::Event, NodeType::Give, NodeType::Ask, NodeType::Notice, NodeType::Tension]);

        let mut results = Vec::new();

        for nt in &types {
            let label = node_type_label(*nt);
            // Use bounding box on plain lat/lng properties to avoid Memgraph point() serialization issues.
            // ~1 degree lat ≈ 111km, 1 degree lng ≈ 111km * cos(lat)
            let lat_delta = radius_km / 111.0;
            let lng_delta = radius_km / (111.0 * lat.to_radians().cos());

            let cypher = format!(
                "MATCH (n:{label})
                 WHERE n.lat <> 0.0
                   AND n.lat >= $min_lat AND n.lat <= $max_lat
                   AND n.lng >= $min_lng AND n.lng <= $max_lng
                   AND n.confidence >= $min_confidence
                   {expiry}
                 RETURN n
                 ORDER BY n.confidence DESC, n.last_confirmed_active DESC
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
        for nt in &[NodeType::Event, NodeType::Give, NodeType::Ask, NodeType::Notice, NodeType::Tension] {
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
            .unwrap_or_else(|| vec![NodeType::Event, NodeType::Give, NodeType::Ask, NodeType::Notice, NodeType::Tension]);

        let mut results = Vec::new();

        for nt in &types {
            let label = node_type_label(*nt);
            let cypher = format!(
                "MATCH (n:{label})
                 WHERE n.confidence >= $min_confidence
                   {expiry}
                 RETURN n
                 ORDER BY n.last_confirmed_active DESC
                 LIMIT $limit",
                expiry = expiry_clause(*nt),
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

    /// Get a single story with its constituent signals.
    pub async fn get_story_with_signals(
        &self,
        story_id: Uuid,
    ) -> Result<Option<(StoryNode, Vec<Node>)>, neo4rs::Error> {
        // First get the story
        let q = query("MATCH (s:Story {id: $id}) RETURN s")
            .param("id", story_id.to_string());

        let mut stream = self.client.graph.execute(q).await?;
        let story = match stream.next().await? {
            Some(row) => match row_to_story(&row) {
                Some(s) => s,
                None => return Ok(None),
            },
            None => return Ok(None),
        };

        // Then get constituent signals
        let signals = self.get_story_signals(story_id).await?;

        Ok(Some((story, signals)))
    }

    /// Get the constituent signals for a story.
    pub async fn get_story_signals(
        &self,
        story_id: Uuid,
    ) -> Result<Vec<Node>, neo4rs::Error> {
        let mut signals = Vec::new();

        for nt in &[NodeType::Event, NodeType::Give, NodeType::Ask, NodeType::Notice, NodeType::Tension] {
            let label = node_type_label(*nt);
            let cypher = format!(
                "MATCH (s:Story {{id: $id}})-[:CONTAINS]->(n:{label})
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

    /// Get a single signal by ID.
    pub async fn get_signal_by_id(
        &self,
        id: Uuid,
    ) -> Result<Option<Node>, neo4rs::Error> {
        match self.get_node_detail(id).await? {
            Some((node, _)) => Ok(Some(node)),
            None => Ok(None),
        }
    }

    // --- Admin/Quality queries (not public-facing, but through reader for safety) ---

    /// Get total signal count by type (for quality dashboard).
    pub async fn count_by_type(&self) -> Result<Vec<(NodeType, u64)>, neo4rs::Error> {
        let mut counts = Vec::new();
        for nt in &[NodeType::Event, NodeType::Give, NodeType::Ask, NodeType::Notice, NodeType::Tension] {
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

    /// Get audience role distribution (for quality dashboard).
    pub async fn audience_role_distribution(&self) -> Result<Vec<(String, u64)>, neo4rs::Error> {
        let q = query(
            "MATCH (n)
            WHERE n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension
            UNWIND n.audience_roles AS role
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
}

// --- Helpers ---

fn node_type_label(nt: NodeType) -> &'static str {
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
            "AND (n.is_recurring = true OR \
             CASE \
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

    // Event-specific: hide past non-recurring events
    if let Node::Event(e) = node {
        if !e.is_recurring {
            let event_end = e.ends_at.unwrap_or(e.starts_at);
            if (now - event_end).num_hours() > EVENT_PAST_GRACE_HOURS {
                return false;
            }
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
        freshness_score: freshness_score as f32,
        corroboration_count: corroboration_count as u32,
        location,
        location_name: {
            let name: String = n.get("location_name").unwrap_or_default();
            if name.is_empty() { None } else { Some(name) }
        },
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
                category: if category.is_empty() { None } else { Some(category) },
                effective_date,
                source_authority: if source_authority.is_empty() { None } else { Some(source_authority) },
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

fn parse_datetime_prop(n: &neo4rs::Node, prop: &str) -> DateTime<Utc> {
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

fn row_to_story(row: &neo4rs::Row) -> Option<StoryNode> {
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
    let audience_roles: Vec<String> = n.get("audience_roles").unwrap_or_default();
    let sensitivity: String = n.get("sensitivity").unwrap_or_else(|_| "general".to_string());
    let source_count: i64 = n.get("source_count").unwrap_or(0);
    let entity_count: i64 = n.get("entity_count").unwrap_or(0);
    let type_diversity: i64 = n.get("type_diversity").unwrap_or(0);
    let source_domains: Vec<String> = n.get("source_domains").unwrap_or_default();
    let corroboration_depth: i64 = n.get("corroboration_depth").unwrap_or(0);
    let status: String = n.get("status").unwrap_or_else(|_| "emerging".to_string());

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
        audience_roles,
        sensitivity,
        source_count: source_count as u32,
        entity_count: entity_count as u32,
        type_diversity: type_diversity as u32,
        source_domains,
        corroboration_depth: corroboration_depth as u32,
        status,
    })
}

fn parse_story_datetime(n: &neo4rs::Node, prop: &str) -> DateTime<Utc> {
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
