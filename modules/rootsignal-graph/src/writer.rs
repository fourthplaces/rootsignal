use chrono::{DateTime, Utc};
use neo4rs::query;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{
    AskNode, EvidenceNode, EventNode, GiveNode, Node, NodeType, SensitivityLevel, TensionNode,
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
                panic!("Use create_evidence() for Evidence nodes")
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
        .param("extracted_at", n.meta.extracted_at.to_rfc3339())
        .param(
            "last_confirmed_active",
            n.meta.last_confirmed_active.to_rfc3339(),
        )
        .param("audience_roles", roles_to_strings(&n.meta.audience_roles))
        .param("starts_at", n.starts_at.to_rfc3339())
        .param(
            "ends_at",
            n.ends_at
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default(),
        )
        .param("action_url", n.action_url.as_str())
        .param("organizer", n.organizer.clone().unwrap_or_default())
        .param("is_recurring", n.is_recurring)
        .param("embedding", embedding_to_f64(embedding));

        if let Some(loc) = &n.meta.location {
            let q = q.param("lat", loc.lat).param("lng", loc.lng);
            let q_with_loc = query(
                "MATCH (e:Event {id: $id}) SET e.location = point({latitude: $lat, longitude: $lng})",
            )
            .param("id", n.meta.id.to_string())
            .param("lat", loc.lat)
            .param("lng", loc.lng);

            let mut stream = self.client.graph.execute(q).await?;
            while stream.next().await?.is_some() {}
            self.client.graph.run(q_with_loc).await?;
        } else {
            let mut stream = self.client.graph.execute(q).await?;
            while stream.next().await?.is_some() {}
        }

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
        .param("extracted_at", n.meta.extracted_at.to_rfc3339())
        .param(
            "last_confirmed_active",
            n.meta.last_confirmed_active.to_rfc3339(),
        )
        .param("audience_roles", roles_to_strings(&n.meta.audience_roles))
        .param("action_url", n.action_url.as_str())
        .param("availability", n.availability.as_str())
        .param("is_ongoing", n.is_ongoing)
        .param("embedding", embedding_to_f64(embedding));

        let mut stream = self.client.graph.execute(q).await?;
        while stream.next().await?.is_some() {}

        if let Some(loc) = &n.meta.location {
            let q_loc = query(
                "MATCH (g:Give {id: $id}) SET g.location = point({latitude: $lat, longitude: $lng})",
            )
            .param("id", n.meta.id.to_string())
            .param("lat", loc.lat)
            .param("lng", loc.lng);
            self.client.graph.run(q_loc).await?;
        }

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
        .param("extracted_at", n.meta.extracted_at.to_rfc3339())
        .param(
            "last_confirmed_active",
            n.meta.last_confirmed_active.to_rfc3339(),
        )
        .param("audience_roles", roles_to_strings(&n.meta.audience_roles))
        .param(
            "urgency",
            format!("{:?}", n.urgency).to_lowercase(),
        )
        .param("what_needed", n.what_needed.as_str())
        .param(
            "action_url",
            n.action_url.clone().unwrap_or_default(),
        )
        .param("goal", n.goal.clone().unwrap_or_default())
        .param("embedding", embedding_to_f64(embedding));

        let mut stream = self.client.graph.execute(q).await?;
        while stream.next().await?.is_some() {}

        if let Some(loc) = &n.meta.location {
            let q_loc = query(
                "MATCH (a:Ask {id: $id}) SET a.location = point({latitude: $lat, longitude: $lng})",
            )
            .param("id", n.meta.id.to_string())
            .param("lat", loc.lat)
            .param("lng", loc.lng);
            self.client.graph.run(q_loc).await?;
        }

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
        .param("extracted_at", n.meta.extracted_at.to_rfc3339())
        .param(
            "last_confirmed_active",
            n.meta.last_confirmed_active.to_rfc3339(),
        )
        .param("audience_roles", roles_to_strings(&n.meta.audience_roles))
        .param(
            "severity",
            format!("{:?}", n.severity).to_lowercase(),
        )
        .param("embedding", embedding_to_f64(embedding));

        let mut stream = self.client.graph.execute(q).await?;
        while stream.next().await?.is_some() {}

        if let Some(loc) = &n.meta.location {
            let q_loc = query(
                "MATCH (t:Tension {id: $id}) SET t.location = point({latitude: $lat, longitude: $lng})",
            )
            .param("id", n.meta.id.to_string())
            .param("lat", loc.lat)
            .param("lng", loc.lng);
            self.client.graph.run(q_loc).await?;
        }

        Ok(n.meta.id)
    }

    /// Create an Evidence node and link it to a signal node via SOURCED_FROM edge.
    pub async fn create_evidence(
        &self,
        evidence: &EvidenceNode,
        signal_node_id: Uuid,
    ) -> Result<(), neo4rs::Error> {
        // Create evidence node and link to the signal node
        // We search across all signal labels to find the target
        let q = query(
            "CREATE (ev:Evidence {
                id: $ev_id,
                source_url: $source_url,
                retrieved_at: datetime($retrieved_at),
                content_hash: $content_hash,
                snippet: $snippet
            })
            WITH ev
            CALL {
                WITH ev
                OPTIONAL MATCH (n:Event {id: $signal_id}) RETURN n
                UNION
                WITH ev
                OPTIONAL MATCH (n:Give {id: $signal_id}) RETURN n
                UNION
                WITH ev
                OPTIONAL MATCH (n:Ask {id: $signal_id}) RETURN n
                UNION
                WITH ev
                OPTIONAL MATCH (n:Tension {id: $signal_id}) RETURN n
            }
            WITH ev, n WHERE n IS NOT NULL LIMIT 1
            CREATE (n)-[:SOURCED_FROM]->(ev)",
        )
        .param("ev_id", evidence.id.to_string())
        .param("source_url", evidence.source_url.as_str())
        .param("retrieved_at", evidence.retrieved_at.to_rfc3339())
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
                "CALL db.index.vector.queryNodes('{}', 1, $embedding)
                 YIELD node, score
                 WHERE score >= $threshold
                 RETURN node.id AS id, score",
                index_name
            ),
        )
        .param("embedding", embedding_to_f64(embedding))
        .param("threshold", threshold);

        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let score: f64 = row.get("score").unwrap_or(0.0);
            if let Ok(id) = Uuid::parse_str(&id_str) {
                return Ok(Some((id, score)));
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
        .param("now", now.to_rfc3339());

        self.client.graph.run(q).await?;
        info!(%node_id, %label, "Corroborated existing signal");
        Ok(())
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
    pub async fn acquire_scout_lock(&self) -> Result<bool, neo4rs::Error> {
        // Clean stale locks older than 1 hour
        self.client
            .graph
            .run(query(
                "MATCH (lock:ScoutLock)
                 WHERE lock.started_at < datetime() - duration('PT1H')
                 DELETE lock",
            ))
            .await?;

        // Check for existing lock
        let mut stream = self
            .client
            .graph
            .execute(query("MATCH (lock:ScoutLock) RETURN lock.started_at AS started_at"))
            .await?;

        if stream.next().await?.is_some() {
            return Ok(false);
        }

        // Create lock
        self.client
            .graph
            .run(query("CREATE (:ScoutLock {started_at: datetime()})"))
            .await?;

        Ok(true)
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

fn sensitivity_str(s: SensitivityLevel) -> &'static str {
    match s {
        SensitivityLevel::General => "general",
        SensitivityLevel::Elevated => "elevated",
        SensitivityLevel::Sensitive => "sensitive",
    }
}

fn roles_to_strings(roles: &[rootsignal_common::AudienceRole]) -> Vec<String> {
    roles.iter().map(|r| format!("{:?}", r).to_lowercase()).collect()
}

fn embedding_to_f64(embedding: &[f32]) -> Vec<f64> {
    embedding.iter().map(|&v| v as f64).collect()
}
