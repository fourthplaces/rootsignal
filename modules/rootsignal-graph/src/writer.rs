use chrono::{DateTime, Utc};
use neo4rs::query;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{
    AskNode, EvidenceNode, EventNode, GiveNode, Node, NodeMeta, NodeType, SensitivityLevel,
    TensionNode, ASK_EXPIRE_DAYS, EVENT_PAST_GRACE_HOURS, FRESHNESS_MAX_DAYS,
};

use crate::GraphClient;

/// Write-side wrapper for the graph. Used by scout only.
pub struct GraphWriter {
    client: GraphClient,
}

impl GraphWriter {
    pub fn new(client: GraphClient) -> Self {
        Self { client }
    }

    /// Create a typed node in the graph. Returns the node's UUID.
    pub async fn create_node(
        &self,
        node: &Node,
        embedding: &[f32],
    ) -> Result<Uuid, neo4rs::Error> {
        match node {
            Node::Event(n) => self.create_event(n, embedding).await,
            Node::Give(n) => self.create_give(n, embedding).await,
            Node::Ask(n) => self.create_ask(n, embedding).await,
            Node::Tension(n) => self.create_tension(n, embedding).await,
            Node::Evidence(_) => {
                return Err(neo4rs::Error::UnsupportedVersion(
                    "Evidence nodes should use create_evidence() directly".to_string(),
                ));
            }
        }
    }

    async fn create_event(
        &self,
        n: &EventNode,
        embedding: &[f32],
    ) -> Result<Uuid, neo4rs::Error> {
        let q = query(
            "CREATE (e:Event {
                id: $id,
                title: $title,
                summary: $summary,
                sensitivity: $sensitivity,
                confidence: $confidence,
                source_trust: $source_trust,
                freshness_score: $freshness_score,
                corroboration_count: $corroboration_count,
                source_url: $source_url,
                extracted_at: datetime($extracted_at),
                last_confirmed_active: datetime($last_confirmed_active),
                audience_roles: $audience_roles,
                starts_at: datetime($starts_at),
                ends_at: $ends_at,
                action_url: $action_url,
                organizer: $organizer,
                is_recurring: $is_recurring,
                lat: $lat,
                lng: $lng,
                embedding: $embedding
            }) RETURN e.id AS id",
        )
        .param("id", n.meta.id.to_string())
        .param("title", n.meta.title.as_str())
        .param("summary", n.meta.summary.as_str())
        .param("sensitivity", sensitivity_str(n.meta.sensitivity))
        .param("confidence", n.meta.confidence as f64)
        .param("source_trust", n.meta.source_trust as f64)
        .param("freshness_score", n.meta.freshness_score as f64)
        .param("corroboration_count", n.meta.corroboration_count as i64)
        .param("source_url", n.meta.source_url.as_str())
        .param("extracted_at", memgraph_datetime(&n.meta.extracted_at))
        .param(
            "last_confirmed_active",
            memgraph_datetime(&n.meta.last_confirmed_active),
        )
        .param("audience_roles", roles_to_strings(&n.meta.audience_roles))
        .param("starts_at", memgraph_datetime(&n.starts_at))
        .param(
            "ends_at",
            n.ends_at
                .map(|dt| memgraph_datetime(&dt))
                .unwrap_or_default(),
        )
        .param("action_url", n.action_url.as_str())
        .param("organizer", n.organizer.clone().unwrap_or_default())
        .param("is_recurring", n.is_recurring)
        .param("embedding", embedding_to_f64(embedding));

        let q = add_location_params(q, &n.meta);
        let mut stream = self.client.graph.execute(q).await?;
        while stream.next().await?.is_some() {}

        Ok(n.meta.id)
    }

    async fn create_give(
        &self,
        n: &GiveNode,
        embedding: &[f32],
    ) -> Result<Uuid, neo4rs::Error> {
        let q = query(
            "CREATE (g:Give {
                id: $id,
                title: $title,
                summary: $summary,
                sensitivity: $sensitivity,
                confidence: $confidence,
                source_trust: $source_trust,
                freshness_score: $freshness_score,
                corroboration_count: $corroboration_count,
                source_url: $source_url,
                extracted_at: datetime($extracted_at),
                last_confirmed_active: datetime($last_confirmed_active),
                audience_roles: $audience_roles,
                action_url: $action_url,
                availability: $availability,
                is_ongoing: $is_ongoing,
                lat: $lat,
                lng: $lng,
                embedding: $embedding
            }) RETURN g.id AS id",
        )
        .param("id", n.meta.id.to_string())
        .param("title", n.meta.title.as_str())
        .param("summary", n.meta.summary.as_str())
        .param("sensitivity", sensitivity_str(n.meta.sensitivity))
        .param("confidence", n.meta.confidence as f64)
        .param("source_trust", n.meta.source_trust as f64)
        .param("freshness_score", n.meta.freshness_score as f64)
        .param("corroboration_count", n.meta.corroboration_count as i64)
        .param("source_url", n.meta.source_url.as_str())
        .param("extracted_at", memgraph_datetime(&n.meta.extracted_at))
        .param(
            "last_confirmed_active",
            memgraph_datetime(&n.meta.last_confirmed_active),
        )
        .param("audience_roles", roles_to_strings(&n.meta.audience_roles))
        .param("action_url", n.action_url.as_str())
        .param("availability", n.availability.as_str())
        .param("is_ongoing", n.is_ongoing)
        .param("embedding", embedding_to_f64(embedding));

        let q = add_location_params(q, &n.meta);
        let mut stream = self.client.graph.execute(q).await?;
        while stream.next().await?.is_some() {}

        Ok(n.meta.id)
    }

    async fn create_ask(
        &self,
        n: &AskNode,
        embedding: &[f32],
    ) -> Result<Uuid, neo4rs::Error> {
        let q = query(
            "CREATE (a:Ask {
                id: $id,
                title: $title,
                summary: $summary,
                sensitivity: $sensitivity,
                confidence: $confidence,
                source_trust: $source_trust,
                freshness_score: $freshness_score,
                corroboration_count: $corroboration_count,
                source_url: $source_url,
                extracted_at: datetime($extracted_at),
                last_confirmed_active: datetime($last_confirmed_active),
                audience_roles: $audience_roles,
                urgency: $urgency,
                what_needed: $what_needed,
                action_url: $action_url,
                goal: $goal,
                lat: $lat,
                lng: $lng,
                embedding: $embedding
            }) RETURN a.id AS id",
        )
        .param("id", n.meta.id.to_string())
        .param("title", n.meta.title.as_str())
        .param("summary", n.meta.summary.as_str())
        .param("sensitivity", sensitivity_str(n.meta.sensitivity))
        .param("confidence", n.meta.confidence as f64)
        .param("source_trust", n.meta.source_trust as f64)
        .param("freshness_score", n.meta.freshness_score as f64)
        .param("corroboration_count", n.meta.corroboration_count as i64)
        .param("source_url", n.meta.source_url.as_str())
        .param("extracted_at", memgraph_datetime(&n.meta.extracted_at))
        .param(
            "last_confirmed_active",
            memgraph_datetime(&n.meta.last_confirmed_active),
        )
        .param("audience_roles", roles_to_strings(&n.meta.audience_roles))
        .param(
            "urgency",
            urgency_str(n.urgency),
        )
        .param("what_needed", n.what_needed.as_str())
        .param(
            "action_url",
            n.action_url.clone().unwrap_or_default(),
        )
        .param("goal", n.goal.clone().unwrap_or_default())
        .param("embedding", embedding_to_f64(embedding));

        let q = add_location_params(q, &n.meta);
        let mut stream = self.client.graph.execute(q).await?;
        while stream.next().await?.is_some() {}

        Ok(n.meta.id)
    }

    async fn create_tension(
        &self,
        n: &TensionNode,
        embedding: &[f32],
    ) -> Result<Uuid, neo4rs::Error> {
        let q = query(
            "CREATE (t:Tension {
                id: $id,
                title: $title,
                summary: $summary,
                sensitivity: $sensitivity,
                confidence: $confidence,
                source_trust: $source_trust,
                freshness_score: $freshness_score,
                corroboration_count: $corroboration_count,
                source_url: $source_url,
                extracted_at: datetime($extracted_at),
                last_confirmed_active: datetime($last_confirmed_active),
                audience_roles: $audience_roles,
                severity: $severity,
                lat: $lat,
                lng: $lng,
                embedding: $embedding
            }) RETURN t.id AS id",
        )
        .param("id", n.meta.id.to_string())
        .param("title", n.meta.title.as_str())
        .param("summary", n.meta.summary.as_str())
        .param("sensitivity", sensitivity_str(n.meta.sensitivity))
        .param("confidence", n.meta.confidence as f64)
        .param("source_trust", n.meta.source_trust as f64)
        .param("freshness_score", n.meta.freshness_score as f64)
        .param("corroboration_count", n.meta.corroboration_count as i64)
        .param("source_url", n.meta.source_url.as_str())
        .param("extracted_at", memgraph_datetime(&n.meta.extracted_at))
        .param(
            "last_confirmed_active",
            memgraph_datetime(&n.meta.last_confirmed_active),
        )
        .param("audience_roles", roles_to_strings(&n.meta.audience_roles))
        .param(
            "severity",
            severity_str(n.severity),
        )
        .param("embedding", embedding_to_f64(embedding));

        let q = add_location_params(q, &n.meta);
        let mut stream = self.client.graph.execute(q).await?;
        while stream.next().await?.is_some() {}

        Ok(n.meta.id)
    }

    /// Create an Evidence node and link it to a signal node via SOURCED_FROM edge.
    pub async fn create_evidence(
        &self,
        evidence: &EvidenceNode,
        signal_node_id: Uuid,
    ) -> Result<(), neo4rs::Error> {
        // Create evidence node and link to the signal node.
        // Use multiple OPTIONAL MATCHes + COALESCE to find the target across labels.
        let q = query(
            "OPTIONAL MATCH (e:Event {id: $signal_id})
            OPTIONAL MATCH (g:Give {id: $signal_id})
            OPTIONAL MATCH (a:Ask {id: $signal_id})
            OPTIONAL MATCH (t:Tension {id: $signal_id})
            WITH coalesce(e, g, a, t) AS n
            WHERE n IS NOT NULL
            CREATE (ev:Evidence {
                id: $ev_id,
                source_url: $source_url,
                retrieved_at: datetime($retrieved_at),
                content_hash: $content_hash,
                snippet: $snippet
            })
            CREATE (n)-[:SOURCED_FROM]->(ev)",
        )
        .param("ev_id", evidence.id.to_string())
        .param("source_url", evidence.source_url.as_str())
        .param("retrieved_at", memgraph_datetime(&evidence.retrieved_at))
        .param("content_hash", evidence.content_hash.as_str())
        .param("snippet", evidence.snippet.clone().unwrap_or_default())
        .param("signal_id", signal_node_id.to_string());

        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Find a duplicate signal by vector similarity within the same node type.
    /// Returns (node_id, similarity_score) if a match above threshold is found.
    pub async fn find_duplicate(
        &self,
        embedding: &[f32],
        node_type: NodeType,
        threshold: f64,
    ) -> Result<Option<(Uuid, f64)>, neo4rs::Error> {
        let index_name = match node_type {
            NodeType::Event => "event_embedding",
            NodeType::Give => "give_embedding",
            NodeType::Ask => "ask_embedding",
            NodeType::Tension => "tension_embedding",
            NodeType::Evidence => return Ok(None),
        };

        let q = query(
            &format!(
                "CALL vector_search.search('{}', 1, $embedding)
                 YIELD node, similarity
                 RETURN node.id AS id, similarity",
                index_name
            ),
        )
        .param("embedding", embedding_to_f64(embedding));

        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let similarity: f64 = row.get("similarity").unwrap_or(0.0);
            if similarity >= threshold {
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    return Ok(Some((id, similarity)));
                }
            }
        }

        Ok(None)
    }

    /// Increment corroboration count and update freshness on an existing node.
    pub async fn corroborate(
        &self,
        node_id: Uuid,
        node_type: NodeType,
        now: DateTime<Utc>,
    ) -> Result<(), neo4rs::Error> {
        let label = match node_type {
            NodeType::Event => "Event",
            NodeType::Give => "Give",
            NodeType::Ask => "Ask",
            NodeType::Tension => "Tension",
            NodeType::Evidence => return Ok(()),
        };

        let q = query(&format!(
            "MATCH (n:{} {{id: $id}})
             SET n.corroboration_count = n.corroboration_count + 1,
                 n.last_confirmed_active = datetime($now)",
            label
        ))
        .param("id", node_id.to_string())
        .param("now", memgraph_datetime(&now));

        self.client.graph.run(q).await?;
        info!(%node_id, %label, "Corroborated existing signal");
        Ok(())
    }

    /// Reap expired signals from the graph. Runs at the start of each scout cycle.
    ///
    /// Deletes:
    /// - Non-recurring events whose end (or start) is past the grace period
    /// - Ask signals older than ASK_EXPIRE_DAYS
    /// - Any signal not confirmed within FRESHNESS_MAX_DAYS (except ongoing gives, recurring events)
    ///
    /// Also detaches and deletes orphaned Evidence nodes.
    pub async fn reap_expired(&self) -> Result<ReapStats, neo4rs::Error> {
        let mut stats = ReapStats::default();

        // 1. Past non-recurring events
        let q = query(&format!(
            "MATCH (n:Event)
             WHERE n.is_recurring = false
               AND CASE
                   WHEN n.ends_at IS NOT NULL AND n.ends_at <> ''
                   THEN datetime(n.ends_at) < datetime() - duration('PT{}H')
                   ELSE datetime(n.starts_at) < datetime() - duration('PT{}H')
               END
             OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
             DETACH DELETE n, ev
             RETURN count(DISTINCT n) AS deleted",
            EVENT_PAST_GRACE_HOURS, EVENT_PAST_GRACE_HOURS
        ));
        if let Some(row) = self.client.graph.execute(q).await?.next().await? {
            stats.events = row.get::<i64>("deleted").unwrap_or(0) as u64;
        }

        // 2. Expired asks
        let q = query(&format!(
            "MATCH (n:Ask)
             WHERE datetime(n.extracted_at) < datetime() - duration('P{}D')
             OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
             DETACH DELETE n, ev
             RETURN count(DISTINCT n) AS deleted",
            ASK_EXPIRE_DAYS
        ));
        if let Some(row) = self.client.graph.execute(q).await?.next().await? {
            stats.asks = row.get::<i64>("deleted").unwrap_or(0) as u64;
        }

        // 3. Stale unconfirmed signals (excluding ongoing gives and recurring events)
        for label in &["Give", "Tension"] {
            let extra = if *label == "Give" {
                "AND n.is_ongoing = false"
            } else {
                ""
            };
            let q = query(&format!(
                "MATCH (n:{label})
                 WHERE datetime(n.last_confirmed_active) < datetime() - duration('P{days}D')
                 {extra}
                 OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
                 DETACH DELETE n, ev
                 RETURN count(DISTINCT n) AS deleted",
                label = label,
                days = FRESHNESS_MAX_DAYS,
                extra = extra,
            ));
            if let Some(row) = self.client.graph.execute(q).await?.next().await? {
                stats.stale += row.get::<i64>("deleted").unwrap_or(0) as u64;
            }
        }

        let total = stats.events + stats.asks + stats.stale;
        if total > 0 {
            info!(
                events = stats.events,
                asks = stats.asks,
                stale = stats.stale,
                "Reaped expired signals"
            );
        }

        Ok(stats)
    }

    /// Delete all nodes sourced from a given URL (opt-out support).
    pub async fn delete_by_source_url(&self, url: &str) -> Result<u64, neo4rs::Error> {
        // Delete evidence nodes linked to signals from this URL, then the signals themselves
        let q = query(
            "MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
             WHERE n.source_url = $url
             DETACH DELETE n, ev
             RETURN count(*) AS deleted",
        )
        .param("url", url);

        let mut stream = self.client.graph.execute(q).await?;
        let deleted = if let Some(row) = stream.next().await? {
            row.get::<i64>("deleted").unwrap_or(0) as u64
        } else {
            0
        };

        warn!(%url, deleted, "Deleted nodes by source URL (opt-out)");
        Ok(deleted)
    }

    /// Acquire a scout lock. Returns false if another scout is running.
    /// Cleans up stale locks (>30 min) from killed containers.
    /// Uses a single atomic query to avoid TOCTOU race between check and create.
    pub async fn acquire_scout_lock(&self) -> Result<bool, neo4rs::Error> {
        // Delete stale locks older than 30 minutes
        self.client
            .graph
            .run(query(
                "MATCH (lock:ScoutLock) WHERE lock.started_at < datetime() - duration('PT30M') DELETE lock"
            ))
            .await?;

        // Atomic check-and-create: only creates if no lock exists
        let q = query(
            "OPTIONAL MATCH (existing:ScoutLock)
             WITH existing WHERE existing IS NULL
             CREATE (lock:ScoutLock {started_at: datetime()})
             RETURN lock IS NOT NULL AS acquired"
        );

        let mut result = self.client.graph.execute(q).await?;
        if let Some(row) = result.next().await? {
            let acquired: bool = row.get("acquired").unwrap_or(false);
            return Ok(acquired);
        }

        // No row returned means the WHERE filtered it out (lock exists)
        Ok(false)
    }

    /// Release the scout lock.
    pub async fn release_scout_lock(&self) -> Result<(), neo4rs::Error> {
        self.client
            .graph
            .run(query("MATCH (lock:ScoutLock) DELETE lock"))
            .await?;
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct ReapStats {
    pub events: u64,
    pub asks: u64,
    pub stale: u64,
}

/// Add lat/lng params to a query from node metadata.
/// Uses null for nodes without a location.
fn add_location_params(q: neo4rs::Query, meta: &NodeMeta) -> neo4rs::Query {
    match &meta.location {
        Some(loc) => q.param("lat", loc.lat).param("lng", loc.lng),
        None => q.param::<Option<f64>>("lat", None).param::<Option<f64>>("lng", None),
    }
}

fn urgency_str(u: rootsignal_common::Urgency) -> &'static str {
    use rootsignal_common::Urgency;
    match u {
        Urgency::Low => "low",
        Urgency::Medium => "medium",
        Urgency::High => "high",
        Urgency::Critical => "critical",
    }
}

fn severity_str(s: rootsignal_common::Severity) -> &'static str {
    use rootsignal_common::Severity;
    match s {
        Severity::Low => "low",
        Severity::Medium => "medium",
        Severity::High => "high",
        Severity::Critical => "critical",
    }
}

fn sensitivity_str(s: SensitivityLevel) -> &'static str {
    match s {
        SensitivityLevel::General => "general",
        SensitivityLevel::Elevated => "elevated",
        SensitivityLevel::Sensitive => "sensitive",
    }
}

fn roles_to_strings(roles: &[rootsignal_common::AudienceRole]) -> Vec<String> {
    roles.iter().map(|r| audience_role_str(r).to_string()).collect()
}

fn audience_role_str(role: &rootsignal_common::AudienceRole) -> &'static str {
    use rootsignal_common::AudienceRole;
    match role {
        AudienceRole::Volunteer => "volunteer",
        AudienceRole::Donor => "donor",
        AudienceRole::Neighbor => "neighbor",
        AudienceRole::Parent => "parent",
        AudienceRole::Youth => "youth",
        AudienceRole::Senior => "senior",
        AudienceRole::Immigrant => "immigrant",
        AudienceRole::Steward => "steward",
        AudienceRole::CivicParticipant => "civic_participant",
        AudienceRole::SkillProvider => "skill_provider",
    }
}

fn embedding_to_f64(embedding: &[f32]) -> Vec<f64> {
    embedding.iter().map(|&v| v as f64).collect()
}

/// Format a DateTime<Utc> as a local datetime string without timezone offset.
/// Memgraph's datetime() requires "YYYY-MM-DDThh:mm:ss" format (no +00:00 suffix).
fn memgraph_datetime(dt: &DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
}
