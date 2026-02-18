use chrono::{DateTime, Utc};
use neo4rs::query;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{
    ActorNode, AskNode, CityNode, ClusterSnapshot, DiscoveryMethod, EditionNode, EvidenceNode,
    EventNode, GiveNode, Node, NodeMeta, NodeType, NoticeNode, SensitivityLevel, SourceNode,
    SourceType, StoryNode, TensionNode, ASK_EXPIRE_DAYS, EVENT_PAST_GRACE_HOURS,
    FRESHNESS_MAX_DAYS, NOTICE_EXPIRE_DAYS,
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
            Node::Notice(n) => self.create_notice(n, embedding).await,
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
                freshness_score: $freshness_score,
                corroboration_count: $corroboration_count,
                source_diversity: $source_diversity,
                external_ratio: $external_ratio,
                source_url: $source_url,
                extracted_at: datetime($extracted_at),
                last_confirmed_active: datetime($last_confirmed_active),

                location_name: $location_name,
                starts_at: CASE WHEN $starts_at = '' THEN null ELSE datetime($starts_at) END,
                ends_at: CASE WHEN $ends_at = '' THEN null ELSE datetime($ends_at) END,
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
        .param("freshness_score", n.meta.freshness_score as f64)
        .param("corroboration_count", n.meta.corroboration_count as i64)
        .param("source_diversity", n.meta.source_diversity as i64)
        .param("external_ratio", n.meta.external_ratio as f64)
        .param("source_url", n.meta.source_url.as_str())
        .param("extracted_at", memgraph_datetime(&n.meta.extracted_at))
        .param(
            "last_confirmed_active",
            memgraph_datetime(&n.meta.last_confirmed_active),
        )

        .param("location_name", n.meta.location_name.as_deref().unwrap_or(""))
        .param(
            "starts_at",
            n.starts_at
                .map(|dt| memgraph_datetime(&dt))
                .unwrap_or_default(),
        )
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
                freshness_score: $freshness_score,
                corroboration_count: $corroboration_count,
                source_diversity: $source_diversity,
                external_ratio: $external_ratio,
                source_url: $source_url,
                extracted_at: datetime($extracted_at),
                last_confirmed_active: datetime($last_confirmed_active),

                location_name: $location_name,
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
        .param("freshness_score", n.meta.freshness_score as f64)
        .param("corroboration_count", n.meta.corroboration_count as i64)
        .param("source_diversity", n.meta.source_diversity as i64)
        .param("external_ratio", n.meta.external_ratio as f64)
        .param("source_url", n.meta.source_url.as_str())
        .param("extracted_at", memgraph_datetime(&n.meta.extracted_at))
        .param(
            "last_confirmed_active",
            memgraph_datetime(&n.meta.last_confirmed_active),
        )

        .param("location_name", n.meta.location_name.as_deref().unwrap_or(""))
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
                freshness_score: $freshness_score,
                corroboration_count: $corroboration_count,
                source_diversity: $source_diversity,
                external_ratio: $external_ratio,
                source_url: $source_url,
                extracted_at: datetime($extracted_at),
                last_confirmed_active: datetime($last_confirmed_active),

                location_name: $location_name,
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
        .param("freshness_score", n.meta.freshness_score as f64)
        .param("corroboration_count", n.meta.corroboration_count as i64)
        .param("source_diversity", n.meta.source_diversity as i64)
        .param("external_ratio", n.meta.external_ratio as f64)
        .param("source_url", n.meta.source_url.as_str())
        .param("extracted_at", memgraph_datetime(&n.meta.extracted_at))
        .param(
            "last_confirmed_active",
            memgraph_datetime(&n.meta.last_confirmed_active),
        )

        .param("location_name", n.meta.location_name.as_deref().unwrap_or(""))
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

    async fn create_notice(
        &self,
        n: &NoticeNode,
        embedding: &[f32],
    ) -> Result<Uuid, neo4rs::Error> {
        let q = query(
            "CREATE (nc:Notice {
                id: $id,
                title: $title,
                summary: $summary,
                sensitivity: $sensitivity,
                confidence: $confidence,
                freshness_score: $freshness_score,
                corroboration_count: $corroboration_count,
                source_diversity: $source_diversity,
                external_ratio: $external_ratio,
                source_url: $source_url,
                extracted_at: datetime($extracted_at),
                last_confirmed_active: datetime($last_confirmed_active),

                location_name: $location_name,
                severity: $severity,
                category: $category,
                effective_date: $effective_date,
                source_authority: $source_authority,
                lat: $lat,
                lng: $lng,
                embedding: $embedding
            }) RETURN nc.id AS id",
        )
        .param("id", n.meta.id.to_string())
        .param("title", n.meta.title.as_str())
        .param("summary", n.meta.summary.as_str())
        .param("sensitivity", sensitivity_str(n.meta.sensitivity))
        .param("confidence", n.meta.confidence as f64)
        .param("freshness_score", n.meta.freshness_score as f64)
        .param("corroboration_count", n.meta.corroboration_count as i64)
        .param("source_diversity", n.meta.source_diversity as i64)
        .param("external_ratio", n.meta.external_ratio as f64)
        .param("source_url", n.meta.source_url.as_str())
        .param("extracted_at", memgraph_datetime(&n.meta.extracted_at))
        .param(
            "last_confirmed_active",
            memgraph_datetime(&n.meta.last_confirmed_active),
        )

        .param("location_name", n.meta.location_name.as_deref().unwrap_or(""))
        .param("severity", severity_str(n.severity))
        .param("category", n.category.clone().unwrap_or_default())
        .param(
            "effective_date",
            n.effective_date
                .map(|dt| memgraph_datetime(&dt))
                .unwrap_or_default(),
        )
        .param("source_authority", n.source_authority.clone().unwrap_or_default())
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
                freshness_score: $freshness_score,
                corroboration_count: $corroboration_count,
                source_diversity: $source_diversity,
                external_ratio: $external_ratio,
                source_url: $source_url,
                extracted_at: datetime($extracted_at),
                last_confirmed_active: datetime($last_confirmed_active),

                location_name: $location_name,
                severity: $severity,
                category: $category,
                what_would_help: $what_would_help,
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
        .param("freshness_score", n.meta.freshness_score as f64)
        .param("corroboration_count", n.meta.corroboration_count as i64)
        .param("source_diversity", n.meta.source_diversity as i64)
        .param("external_ratio", n.meta.external_ratio as f64)
        .param("source_url", n.meta.source_url.as_str())
        .param("extracted_at", memgraph_datetime(&n.meta.extracted_at))
        .param(
            "last_confirmed_active",
            memgraph_datetime(&n.meta.last_confirmed_active),
        )

        .param("location_name", n.meta.location_name.as_deref().unwrap_or(""))
        .param(
            "severity",
            severity_str(n.severity),
        )
        .param("category", n.category.as_deref().unwrap_or(""))
        .param("what_would_help", n.what_would_help.as_deref().unwrap_or(""))
        .param("embedding", embedding_to_f64(embedding));

        let q = add_location_params(q, &n.meta);
        let mut stream = self.client.graph.execute(q).await?;
        while stream.next().await?.is_some() {}

        Ok(n.meta.id)
    }

    /// Create an Evidence node and link it to a signal node via SOURCED_FROM edge.
    ///
    /// **Idempotent:** Uses MERGE on (signal)-[:SOURCED_FROM]->(Evidence {source_url}).
    /// If evidence from this source_url already exists for this signal, updates the
    /// content_hash and retrieved_at instead of creating a duplicate. This is the
    /// safety net that prevents evidence pile-up from dynamic pages.
    pub async fn create_evidence(
        &self,
        evidence: &EvidenceNode,
        signal_node_id: Uuid,
    ) -> Result<(), neo4rs::Error> {
        // Find the target signal across all labels, then MERGE evidence by source_url.
        // ON CREATE: set all fields on the new Evidence node.
        // ON MATCH: update hash + timestamp (page content changed but same source).
        let q = query(
            "OPTIONAL MATCH (e:Event {id: $signal_id})
            OPTIONAL MATCH (g:Give {id: $signal_id})
            OPTIONAL MATCH (a:Ask {id: $signal_id})
            OPTIONAL MATCH (nc:Notice {id: $signal_id})
            OPTIONAL MATCH (t:Tension {id: $signal_id})
            WITH coalesce(e, g, a, nc, t) AS n
            WHERE n IS NOT NULL
            MERGE (n)-[:SOURCED_FROM]->(ev:Evidence {source_url: $source_url})
            ON CREATE SET
                ev.id = $ev_id,
                ev.retrieved_at = datetime($retrieved_at),
                ev.content_hash = $content_hash,
                ev.snippet = $snippet,
                ev.relevance = $relevance,
                ev.evidence_confidence = $evidence_confidence
            ON MATCH SET
                ev.retrieved_at = datetime($retrieved_at),
                ev.content_hash = $content_hash",
        )
        .param("ev_id", evidence.id.to_string())
        .param("source_url", evidence.source_url.as_str())
        .param("retrieved_at", memgraph_datetime(&evidence.retrieved_at))
        .param("content_hash", evidence.content_hash.as_str())
        .param("snippet", evidence.snippet.clone().unwrap_or_default())
        .param("relevance", evidence.relevance.clone().unwrap_or_default())
        .param("evidence_confidence", evidence.evidence_confidence.unwrap_or(0.0) as f64)
        .param("signal_id", signal_node_id.to_string());

        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Refresh a signal's `last_confirmed_active` timestamp without incrementing
    /// corroboration metrics. Used for same-source re-scrapes where the signal
    /// is confirmed still active but no new independent source was found.
    pub async fn refresh_signal(
        &self,
        signal_id: Uuid,
        node_type: NodeType,
        now: DateTime<Utc>,
    ) -> Result<(), neo4rs::Error> {
        let label = match node_type {
            NodeType::Event => "Event",
            NodeType::Give => "Give",
            NodeType::Ask => "Ask",
            NodeType::Notice => "Notice",
            NodeType::Tension => "Tension",
            NodeType::Evidence => return Ok(()),
        };

        let q = query(&format!(
            "MATCH (n:{} {{id: $id}})
             SET n.last_confirmed_active = datetime($now)",
            label
        ))
        .param("id", signal_id.to_string())
        .param("now", memgraph_datetime(&now));

        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Find a duplicate signal by vector similarity across all signal types.
    /// Returns the best match (highest similarity) above threshold.
    pub async fn find_duplicate(
        &self,
        embedding: &[f32],
        _primary_type: NodeType,
        threshold: f64,
    ) -> Result<Option<DuplicateMatch>, neo4rs::Error> {
        let mut best: Option<DuplicateMatch> = None;

        for nt in &[NodeType::Event, NodeType::Give, NodeType::Ask, NodeType::Notice, NodeType::Tension] {
            if let Some(m) = self.vector_search(*nt, embedding, threshold).await? {
                if best.as_ref().map_or(true, |b| m.similarity > b.similarity) {
                    best = Some(m);
                }
            }
        }

        Ok(best)
    }

    async fn vector_search(
        &self,
        node_type: NodeType,
        embedding: &[f32],
        threshold: f64,
    ) -> Result<Option<DuplicateMatch>, neo4rs::Error> {
        let index_name = match node_type {
            NodeType::Event => "event_embedding",
            NodeType::Give => "give_embedding",
            NodeType::Ask => "ask_embedding",
            NodeType::Notice => "notice_embedding",
            NodeType::Tension => "tension_embedding",
            NodeType::Evidence => return Ok(None),
        };

        let q = query(&format!(
            "CALL vector_search.search('{}', 1, $embedding)
             YIELD node, similarity
             RETURN node.id AS id, node.source_url AS source_url, similarity",
            index_name
        ))
        .param("embedding", embedding_to_f64(embedding));

        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let similarity: f64 = row.get("similarity").unwrap_or(0.0);
            let source_url: String = row.get("source_url").unwrap_or_default();
            if similarity >= threshold {
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    return Ok(Some(DuplicateMatch {
                        id,
                        node_type,
                        source_url,
                        similarity,
                    }));
                }
            }
        }

        Ok(None)
    }

    /// Check if content with this hash has already been processed for this specific URL.
    /// Scoped to (hash, source_url) so cross-source corroboration isn't suppressed.
    pub async fn content_already_processed(
        &self,
        content_hash: &str,
        source_url: &str,
    ) -> Result<bool, neo4rs::Error> {
        let q = query(
            "MATCH (ev:Evidence {content_hash: $hash, source_url: $url})
             RETURN ev LIMIT 1",
        )
        .param("hash", content_hash)
        .param("url", source_url);

        let mut stream = self.client.graph.execute(q).await?;
        Ok(stream.next().await?.is_some())
    }

    /// Bump `last_confirmed_active` on all signals from a source URL.
    /// Used when content hasn't changed — keeps signals fresh without re-extracting.
    pub async fn refresh_url_signals(
        &self,
        source_url: &str,
        now: DateTime<Utc>,
    ) -> Result<u64, neo4rs::Error> {
        let q = query(
            "MATCH (n)
             WHERE (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension)
               AND n.source_url = $url
             SET n.last_confirmed_active = datetime($now)
             RETURN count(n) AS refreshed",
        )
        .param("url", source_url)
        .param("now", memgraph_datetime(&now));

        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            Ok(row.get::<i64>("refreshed").unwrap_or(0) as u64)
        } else {
            Ok(0)
        }
    }

    /// Return titles of existing signals from a given source URL.
    /// Used for cheap pre-filtering before expensive embedding-based dedup.
    pub async fn existing_titles_for_url(
        &self,
        source_url: &str,
    ) -> Result<Vec<String>, neo4rs::Error> {
        let q = query(
            "MATCH (n)
             WHERE (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension)
               AND n.source_url = $url
             RETURN n.title AS title",
        )
        .param("url", source_url);

        let mut titles = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Ok(title) = row.get::<String>("title") {
                titles.push(title);
            }
        }
        Ok(titles)
    }

    /// Batch-find existing signals by exact title+type (case-insensitive).
    /// Returns a map of lowercase title → (node_id, node_type, source_url).
    /// Single Cypher query regardless of input size.
    pub async fn find_by_titles_and_types(
        &self,
        titles_and_types: &[(String, NodeType)],
    ) -> Result<std::collections::HashMap<(String, NodeType), (Uuid, String)>, neo4rs::Error> {
        let mut results = std::collections::HashMap::new();
        if titles_and_types.is_empty() {
            return Ok(results);
        }

        // Query each label once with all titles for that type
        for nt in &[NodeType::Event, NodeType::Give, NodeType::Ask, NodeType::Notice] {
            let label = match nt {
                NodeType::Event => "Event",
                NodeType::Give => "Give",
                NodeType::Ask => "Ask",
                NodeType::Notice => "Notice",
                _ => continue,
            };

            let titles_for_type: Vec<String> = titles_and_types
                .iter()
                .filter(|(_, t)| t == nt)
                .map(|(title, _)| title.to_lowercase())
                .collect();

            if titles_for_type.is_empty() {
                continue;
            }

            let q = query(&format!(
                "MATCH (n:{label})
                 WHERE toLower(n.title) IN $titles
                 RETURN toLower(n.title) AS title, n.id AS id, n.source_url AS source_url"
            ))
            .param("titles", titles_for_type);

            let mut stream = self.client.graph.execute(q).await?;
            while let Some(row) = stream.next().await? {
                let title: String = row.get("title").unwrap_or_default();
                let id_str: String = row.get("id").unwrap_or_default();
                let source_url: String = row.get("source_url").unwrap_or_default();
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    results.insert((title, *nt), (id, source_url));
                }
            }
        }

        Ok(results)
    }

    /// Increment corroboration count, update freshness, and recompute source diversity.
    pub async fn corroborate(
        &self,
        node_id: Uuid,
        node_type: NodeType,
        now: DateTime<Utc>,
        entity_mappings: &[rootsignal_common::EntityMappingOwned],
    ) -> Result<(), neo4rs::Error> {
        let label = match node_type {
            NodeType::Event => "Event",
            NodeType::Give => "Give",
            NodeType::Ask => "Ask",
            NodeType::Notice => "Notice",
            NodeType::Tension => "Tension",
            NodeType::Evidence => return Ok(()),
        };

        // Increment corroboration count
        let q = query(&format!(
            "MATCH (n:{} {{id: $id}})
             SET n.corroboration_count = n.corroboration_count + 1,
                 n.last_confirmed_active = datetime($now)",
            label
        ))
        .param("id", node_id.to_string())
        .param("now", memgraph_datetime(&now));

        self.client.graph.run(q).await?;

        // Recompute source diversity from all evidence nodes
        let (diversity, external_ratio) = self.compute_source_diversity(node_id, node_type, entity_mappings).await?;

        let q = query(&format!(
            "MATCH (n:{} {{id: $id}})
             SET n.source_diversity = $diversity, n.external_ratio = $ratio",
            label
        ))
        .param("id", node_id.to_string())
        .param("diversity", diversity as i64)
        .param("ratio", external_ratio as f64);

        self.client.graph.run(q).await?;

        info!(%node_id, %label, diversity, external_ratio, "Corroborated existing signal");
        Ok(())
    }

    /// Compute source diversity and external ratio for a signal from its evidence nodes.
    pub async fn compute_source_diversity(
        &self,
        node_id: Uuid,
        node_type: NodeType,
        entity_mappings: &[rootsignal_common::EntityMappingOwned],
    ) -> Result<(u32, f32), neo4rs::Error> {
        let label = match node_type {
            NodeType::Event => "Event",
            NodeType::Give => "Give",
            NodeType::Ask => "Ask",
            NodeType::Notice => "Notice",
            NodeType::Tension => "Tension",
            NodeType::Evidence => return Ok((1, 0.0)),
        };

        // Get the signal's own source_url and all evidence source_urls
        let q = query(&format!(
            "MATCH (n:{label} {{id: $id}})
             OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
             RETURN n.source_url AS self_url, collect(ev.source_url) AS evidence_urls"
        ))
        .param("id", node_id.to_string());

        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let self_url: String = row.get("self_url").unwrap_or_default();
            let evidence_urls: Vec<String> = row.get("evidence_urls").unwrap_or_default();

            let self_entity = rootsignal_common::resolve_entity(&self_url, entity_mappings);

            let mut entities = std::collections::HashSet::new();
            let mut external_count = 0u32;
            let total = evidence_urls.len() as u32;

            for url in &evidence_urls {
                let entity = rootsignal_common::resolve_entity(url, entity_mappings);
                entities.insert(entity.clone());
                if entity != self_entity {
                    external_count += 1;
                }
            }

            let diversity = entities.len().max(1) as u32;
            let external_ratio = if total > 0 {
                external_count as f32 / total as f32
            } else {
                0.0
            };

            Ok((diversity, external_ratio))
        } else {
            Ok((1, 0.0))
        }
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

        // 1. Past non-recurring events (only those with a known start date)
        let q = query(&format!(
            "MATCH (n:Event)
             WHERE n.is_recurring = false
               AND n.starts_at IS NOT NULL AND n.starts_at <> ''
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

        // 3. Expired notices
        let q = query(&format!(
            "MATCH (n:Notice)
             WHERE datetime(n.extracted_at) < datetime() - duration('P{}D')
             OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
             DETACH DELETE n, ev
             RETURN count(DISTINCT n) AS deleted",
            NOTICE_EXPIRE_DAYS
        ));
        if let Some(row) = self.client.graph.execute(q).await?.next().await? {
            stats.stale += row.get::<i64>("deleted").unwrap_or(0) as u64;
        }

        // 4. Stale unconfirmed signals (all signals must be re-confirmed within FRESHNESS_MAX_DAYS)
        for label in &["Give", "Tension"] {
            let q = query(&format!(
                "MATCH (n:{label})
                 WHERE datetime(n.last_confirmed_active) < datetime() - duration('P{days}D')
                 OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
                 DETACH DELETE n, ev
                 RETURN count(DISTINCT n) AS deleted",
                label = label,
                days = FRESHNESS_MAX_DAYS,
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

    // --- Story operations ---

    /// Create a new Story node in the graph.
    pub async fn create_story(&self, story: &StoryNode) -> Result<(), neo4rs::Error> {
        let q = query(
            "CREATE (s:Story {
                id: $id,
                headline: $headline,
                summary: $summary,
                signal_count: $signal_count,
                first_seen: datetime($first_seen),
                last_updated: datetime($last_updated),
                velocity: $velocity,
                energy: $energy,
                centroid_lat: $centroid_lat,
                centroid_lng: $centroid_lng,
                dominant_type: $dominant_type,

                sensitivity: $sensitivity,
                source_count: $source_count,
                entity_count: $entity_count,
                type_diversity: $type_diversity,
                source_domains: $source_domains,
                corroboration_depth: $corroboration_depth,
                status: $status
            })"
        )
        .param("id", story.id.to_string())
        .param("headline", story.headline.as_str())
        .param("summary", story.summary.as_str())
        .param("signal_count", story.signal_count as i64)
        .param("first_seen", memgraph_datetime(&story.first_seen))
        .param("last_updated", memgraph_datetime(&story.last_updated))
        .param("velocity", story.velocity)
        .param("energy", story.energy)
        .param("dominant_type", story.dominant_type.as_str())

        .param("sensitivity", story.sensitivity.as_str())
        .param("source_count", story.source_count as i64)
        .param("entity_count", story.entity_count as i64)
        .param("type_diversity", story.type_diversity as i64)
        .param("source_domains", story.source_domains.clone())
        .param("corroboration_depth", story.corroboration_depth as i64)
        .param("status", story.status.as_str());

        let q = match (story.centroid_lat, story.centroid_lng) {
            (Some(lat), Some(lng)) => q.param("centroid_lat", lat).param("centroid_lng", lng),
            _ => q.param::<Option<f64>>("centroid_lat", None).param::<Option<f64>>("centroid_lng", None),
        };

        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Update an existing Story node.
    pub async fn update_story(&self, story: &StoryNode) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (s:Story {id: $id})
             SET s.headline = $headline,
                 s.summary = $summary,
                 s.signal_count = $signal_count,
                 s.last_updated = datetime($last_updated),
                 s.velocity = $velocity,
                 s.energy = $energy,
                 s.centroid_lat = $centroid_lat,
                 s.centroid_lng = $centroid_lng,
                 s.dominant_type = $dominant_type,

                 s.sensitivity = $sensitivity,
                 s.source_count = $source_count,
                 s.entity_count = $entity_count,
                 s.type_diversity = $type_diversity,
                 s.source_domains = $source_domains,
                 s.corroboration_depth = $corroboration_depth,
                 s.status = $status"
        )
        .param("id", story.id.to_string())
        .param("headline", story.headline.as_str())
        .param("summary", story.summary.as_str())
        .param("signal_count", story.signal_count as i64)
        .param("last_updated", memgraph_datetime(&story.last_updated))
        .param("velocity", story.velocity)
        .param("energy", story.energy)
        .param("dominant_type", story.dominant_type.as_str())

        .param("sensitivity", story.sensitivity.as_str())
        .param("source_count", story.source_count as i64)
        .param("entity_count", story.entity_count as i64)
        .param("type_diversity", story.type_diversity as i64)
        .param("source_domains", story.source_domains.clone())
        .param("corroboration_depth", story.corroboration_depth as i64)
        .param("status", story.status.as_str());

        let q = match (story.centroid_lat, story.centroid_lng) {
            (Some(lat), Some(lng)) => q.param("centroid_lat", lat).param("centroid_lng", lng),
            _ => q.param::<Option<f64>>("centroid_lat", None).param::<Option<f64>>("centroid_lng", None),
        };

        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Link a signal to a story via CONTAINS relationship.
    pub async fn link_signal_to_story(
        &self,
        story_id: Uuid,
        signal_id: Uuid,
    ) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (s:Story {id: $story_id})
             MATCH (n) WHERE n.id = $signal_id AND (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension)
             MERGE (s)-[:CONTAINS]->(n)"
        )
        .param("story_id", story_id.to_string())
        .param("signal_id", signal_id.to_string());

        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Clear all CONTAINS relationships for a story (before rebuilding).
    pub async fn clear_story_signals(&self, story_id: Uuid) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (s:Story {id: $story_id})-[r:CONTAINS]->()
             DELETE r"
        )
        .param("story_id", story_id.to_string());

        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Create an EVOLVED_FROM relationship between stories.
    pub async fn link_story_evolution(
        &self,
        new_story_id: Uuid,
        old_story_id: Uuid,
    ) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (new:Story {id: $new_id})
             MATCH (old:Story {id: $old_id})
             MERGE (new)-[:EVOLVED_FROM]->(old)"
        )
        .param("new_id", new_story_id.to_string())
        .param("old_id", old_story_id.to_string());

        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Create a cluster snapshot for velocity tracking.
    pub async fn create_cluster_snapshot(&self, snapshot: &ClusterSnapshot) -> Result<(), neo4rs::Error> {
        let q = query(
            "CREATE (cs:ClusterSnapshot {
                id: $id,
                story_id: $story_id,
                signal_count: $signal_count,
                entity_count: $entity_count,
                run_at: datetime($run_at)
            })"
        )
        .param("id", snapshot.id.to_string())
        .param("story_id", snapshot.story_id.to_string())
        .param("signal_count", snapshot.signal_count as i64)
        .param("entity_count", snapshot.entity_count as i64)
        .param("run_at", memgraph_datetime(&snapshot.run_at));

        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Get existing stories with their constituent signal IDs.
    /// Used for story reconciliation (asymmetric containment).
    pub async fn get_existing_stories(&self) -> Result<Vec<(Uuid, Vec<String>)>, neo4rs::Error> {
        let q = query(
            "MATCH (s:Story)
             OPTIONAL MATCH (s)-[:CONTAINS]->(n)
             RETURN s.id AS story_id, collect(n.id) AS signal_ids"
        );

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("story_id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                let signal_ids: Vec<String> = row.get("signal_ids").unwrap_or_default();
                results.push((id, signal_ids));
            }
        }

        Ok(results)
    }

    /// Update story synthesis fields (lede, narrative, arc, category, action_guidance).
    pub async fn update_story_synthesis(
        &self,
        story_id: Uuid,
        headline: &str,
        lede: &str,
        narrative: &str,
        arc: &str,
        category: &str,
        action_guidance_json: &str,
    ) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (s:Story {id: $id})
             SET s.headline = $headline,
                 s.lede = $lede,
                 s.narrative = $narrative,
                 s.arc = $arc,
                 s.category = $category,
                 s.action_guidance = $action_guidance"
        )
        .param("id", story_id.to_string())
        .param("headline", headline)
        .param("lede", lede)
        .param("narrative", narrative)
        .param("arc", arc)
        .param("category", category)
        .param("action_guidance", action_guidance_json);

        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Archive (delete) a story and its relationships.
    pub async fn archive_story(&self, story_id: Uuid) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (s:Story {id: $id})
             DETACH DELETE s"
        )
        .param("id", story_id.to_string());

        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Get the snapshot signal count from 7 days ago for velocity calculation.
    pub async fn get_snapshot_count_7d_ago(&self, story_id: Uuid) -> Result<Option<u32>, neo4rs::Error> {
        let q = query(
            "MATCH (cs:ClusterSnapshot {story_id: $story_id})
             WHERE datetime(cs.run_at) >= datetime() - duration('P8D')
               AND datetime(cs.run_at) <= datetime() - duration('P6D')
             RETURN cs.signal_count AS cnt
             ORDER BY cs.run_at ASC
             LIMIT 1"
        )
        .param("story_id", story_id.to_string());

        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let cnt: i64 = row.get("cnt").unwrap_or(0);
            return Ok(Some(cnt as u32));
        }

        Ok(None)
    }

    // --- City operations ---

    /// Create or update a City node. MERGE on slug for idempotency.
    pub async fn upsert_city(&self, city: &CityNode) -> Result<(), neo4rs::Error> {
        let q = query(
            "MERGE (c:City {slug: $slug})
             ON CREATE SET
                c.id = $id,
                c.name = $name,
                c.center_lat = $center_lat,
                c.center_lng = $center_lng,
                c.radius_km = $radius_km,
                c.geo_terms = $geo_terms,
                c.active = $active,
                c.created_at = datetime($created_at)
             ON MATCH SET
                c.name = $name,
                c.center_lat = $center_lat,
                c.center_lng = $center_lng,
                c.radius_km = $radius_km,
                c.geo_terms = $geo_terms,
                c.active = $active"
        )
        .param("id", city.id.to_string())
        .param("slug", city.slug.as_str())
        .param("name", city.name.as_str())
        .param("center_lat", city.center_lat)
        .param("center_lng", city.center_lng)
        .param("radius_km", city.radius_km)
        .param("geo_terms", city.geo_terms.clone())
        .param("active", city.active)
        .param("created_at", memgraph_datetime(&city.created_at));

        self.client.graph.run(q).await?;
        info!(slug = city.slug.as_str(), name = city.name.as_str(), "City node upserted");
        Ok(())
    }

    /// Get a City node by slug. Returns None if not found.
    pub async fn get_city(&self, slug: &str) -> Result<Option<CityNode>, neo4rs::Error> {
        let q = query(
            "MATCH (c:City {slug: $slug})
             RETURN c.id AS id, c.name AS name, c.slug AS slug,
                    c.center_lat AS center_lat, c.center_lng AS center_lng,
                    c.radius_km AS radius_km, c.geo_terms AS geo_terms,
                    c.active AS active, c.created_at AS created_at"
        )
        .param("slug", slug);

        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let id = match Uuid::parse_str(&id_str) {
                Ok(id) => id,
                Err(_) => return Ok(None),
            };

            let created_at_str: String = row.get("created_at").unwrap_or_default();
            let created_at = chrono::NaiveDateTime::parse_from_str(&created_at_str, "%Y-%m-%dT%H:%M:%S%.f")
                .map(|ndt| ndt.and_utc())
                .unwrap_or_else(|_| Utc::now());

            Ok(Some(CityNode {
                id,
                name: row.get("name").unwrap_or_default(),
                slug: row.get("slug").unwrap_or_default(),
                center_lat: row.get("center_lat").unwrap_or(0.0),
                center_lng: row.get("center_lng").unwrap_or(0.0),
                radius_km: row.get("radius_km").unwrap_or(0.0),
                geo_terms: row.get("geo_terms").unwrap_or_default(),
                active: row.get("active").unwrap_or(true),
                created_at,
            }))
        } else {
            Ok(None)
        }
    }

    // --- Source operations (emergent source discovery) ---

    /// Create or update a Source node in the graph.
    /// Uses MERGE on canonical_key to be idempotent (safe for seeding curated sources every run).
    pub async fn upsert_source(&self, source: &SourceNode) -> Result<(), neo4rs::Error> {
        let q = query(
            "MERGE (s:Source {canonical_key: $canonical_key})
             ON CREATE SET
                s.id = $id,
                s.canonical_value = $canonical_value,
                s.url = $url,
                s.source_type = $source_type,
                s.discovery_method = $discovery_method,
                s.city = $city,
                s.created_at = datetime($created_at),
                s.signals_produced = $signals_produced,
                s.signals_corroborated = $signals_corroborated,
                s.consecutive_empty_runs = $consecutive_empty_runs,
                s.active = $active,
                s.gap_context = $gap_context,
                s.weight = $weight,
                s.avg_signals_per_scrape = $avg_signals_per_scrape,
                s.total_cost_cents = $total_cost_cents,
                s.last_cost_cents = $last_cost_cents
             ON MATCH SET
                s.active = CASE WHEN s.active = false AND $discovery_method = 'curated' THEN true ELSE s.active END,
                s.url = CASE WHEN $url <> '' THEN $url ELSE s.url END"
        )
        .param("id", source.id.to_string())
        .param("canonical_key", source.canonical_key.as_str())
        .param("canonical_value", source.canonical_value.as_str())
        .param("url", source.url.clone().unwrap_or_default())
        .param("source_type", source.source_type.to_string())
        .param("discovery_method", source.discovery_method.to_string())
        .param("city", source.city.as_str())
        .param("created_at", memgraph_datetime(&source.created_at))
        .param("signals_produced", source.signals_produced as i64)
        .param("signals_corroborated", source.signals_corroborated as i64)
        .param("consecutive_empty_runs", source.consecutive_empty_runs as i64)
        .param("active", source.active)
        .param("gap_context", source.gap_context.clone().unwrap_or_default())
        .param("weight", source.weight)
        .param("avg_signals_per_scrape", source.avg_signals_per_scrape)
        .param("total_cost_cents", source.total_cost_cents as i64)
        .param("last_cost_cents", source.last_cost_cents as i64);

        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Create a Submission node and link it to its associated Source.
    pub async fn upsert_submission(
        &self,
        submission: &rootsignal_common::SubmissionNode,
        source_canonical_key: &str,
    ) -> Result<(), neo4rs::Error> {
        let q = query(
            "CREATE (sub:Submission {
                id: $id,
                url: $url,
                reason: $reason,
                city: $city,
                submitted_at: datetime($submitted_at)
            })
            WITH sub
            MATCH (s:Source {canonical_key: $canonical_key})
            MERGE (sub)-[:SUBMITTED_FOR]->(s)"
        )
        .param("id", submission.id.to_string())
        .param("url", submission.url.as_str())
        .param("reason", submission.reason.clone().unwrap_or_default())
        .param("city", submission.city.as_str())
        .param("submitted_at", memgraph_datetime(&submission.submitted_at))
        .param("canonical_key", source_canonical_key);

        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Get all active sources for a city (by slug).
    pub async fn get_active_sources(&self, city: &str) -> Result<Vec<SourceNode>, neo4rs::Error> {
        let q = query(
            "MATCH (s:Source {city: $city, active: true})
             RETURN s.id AS id, s.canonical_key AS canonical_key,
                    s.canonical_value AS canonical_value, s.url AS url,
                    s.source_type AS source_type,
                    s.discovery_method AS discovery_method, s.city AS city,
                    s.created_at AS created_at, s.last_scraped AS last_scraped,
                    s.last_produced_signal AS last_produced_signal,
                    s.signals_produced AS signals_produced,
                    s.signals_corroborated AS signals_corroborated,
                    s.consecutive_empty_runs AS consecutive_empty_runs,
                    s.active AS active, s.gap_context AS gap_context,
                    s.weight AS weight, s.cadence_hours AS cadence_hours,
                    s.avg_signals_per_scrape AS avg_signals_per_scrape,
                    s.total_cost_cents AS total_cost_cents,
                    s.last_cost_cents AS last_cost_cents,
                    s.taxonomy_stats AS taxonomy_stats"
        )
        .param("city", city);

        let mut sources = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let id = match Uuid::parse_str(&id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };

            let source_type_str: String = row.get("source_type").unwrap_or_default();
            let source_type = SourceType::from_str_loose(&source_type_str);

            let discovery_str: String = row.get("discovery_method").unwrap_or_default();
            let discovery_method = match discovery_str.as_str() {
                "gap_analysis" => DiscoveryMethod::GapAnalysis,
                "signal_reference" => DiscoveryMethod::SignalReference,
                "hashtag_discovery" => DiscoveryMethod::HashtagDiscovery,
                "cold_start" => DiscoveryMethod::ColdStart,
                "tension_seed" => DiscoveryMethod::TensionSeed,
                "human_submission" => DiscoveryMethod::HumanSubmission,
                _ => DiscoveryMethod::Curated,
            };

            let created_at = parse_memgraph_datetime_opt(&row.get::<String>("created_at").unwrap_or_default())
                .unwrap_or_else(Utc::now);

            let last_scraped = row.get::<String>("last_scraped").ok()
                .and_then(|s| parse_memgraph_datetime_opt(&s));
            let last_produced_signal = row.get::<String>("last_produced_signal").ok()
                .and_then(|s| parse_memgraph_datetime_opt(&s));

            let gap_context: String = row.get("gap_context").unwrap_or_default();
            let url: String = row.get("url").unwrap_or_default();
            let taxonomy_stats: String = row.get("taxonomy_stats").unwrap_or_default();
            let cadence: i64 = row.get::<i64>("cadence_hours").unwrap_or(0);

            sources.push(SourceNode {
                id,
                canonical_key: row.get("canonical_key").unwrap_or_default(),
                canonical_value: row.get("canonical_value").unwrap_or_default(),
                url: if url.is_empty() { None } else { Some(url) },
                source_type,
                discovery_method,
                city: row.get("city").unwrap_or_default(),
                created_at,
                last_scraped,
                last_produced_signal,
                signals_produced: row.get::<i64>("signals_produced").unwrap_or(0) as u32,
                signals_corroborated: row.get::<i64>("signals_corroborated").unwrap_or(0) as u32,
                consecutive_empty_runs: row.get::<i64>("consecutive_empty_runs").unwrap_or(0) as u32,
                active: row.get("active").unwrap_or(true),
                gap_context: if gap_context.is_empty() { None } else { Some(gap_context) },
                weight: row.get("weight").unwrap_or(0.5),
                cadence_hours: if cadence > 0 { Some(cadence as u32) } else { None },
                avg_signals_per_scrape: row.get("avg_signals_per_scrape").unwrap_or(0.0),
                total_cost_cents: row.get::<i64>("total_cost_cents").unwrap_or(0) as u64,
                last_cost_cents: row.get::<i64>("last_cost_cents").unwrap_or(0) as u64,
                taxonomy_stats: if taxonomy_stats.is_empty() { None } else { Some(taxonomy_stats) },
            });
        }

        Ok(sources)
    }

    /// Record that a source produced signals this run.
    /// Updates last_scraped, signals_produced, consecutive_empty_runs.
    pub async fn record_source_scrape(
        &self,
        canonical_key: &str,
        signals_produced: u32,
        now: DateTime<Utc>,
    ) -> Result<(), neo4rs::Error> {
        if signals_produced > 0 {
            let q = query(
                "MATCH (s:Source {canonical_key: $key})
                 SET s.last_scraped = datetime($now),
                     s.last_produced_signal = datetime($now),
                     s.signals_produced = s.signals_produced + $count,
                     s.consecutive_empty_runs = 0"
            )
            .param("key", canonical_key)
            .param("now", memgraph_datetime(&now))
            .param("count", signals_produced as i64);
            self.client.graph.run(q).await?;
        } else {
            let q = query(
                "MATCH (s:Source {canonical_key: $key})
                 SET s.last_scraped = datetime($now),
                     s.consecutive_empty_runs = s.consecutive_empty_runs + 1"
            )
            .param("key", canonical_key)
            .param("now", memgraph_datetime(&now));
            self.client.graph.run(q).await?;
        }
        Ok(())
    }

    /// Update weight and cadence for a source based on computed metrics.
    pub async fn update_source_weight(
        &self,
        canonical_key: &str,
        weight: f64,
        cadence_hours: u32,
    ) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (s:Source {canonical_key: $key})
             SET s.weight = $weight, s.cadence_hours = $cadence"
        )
        .param("key", canonical_key)
        .param("weight", weight)
        .param("cadence", cadence_hours as i64);
        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Count tension signals produced by a specific source.
    pub async fn count_source_tensions(
        &self,
        canonical_key: &str,
    ) -> Result<u32, neo4rs::Error> {
        // Look up URL from canonical_key, then count Tension nodes with matching source_url
        let q = query(
            "MATCH (s:Source {canonical_key: $key})
             WITH s.url AS url, s.canonical_value AS cv
             OPTIONAL MATCH (t:Tension)
             WHERE t.source_url = url OR t.source_url CONTAINS cv
             RETURN count(t) AS cnt"
        )
        .param("key", canonical_key);

        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            Ok(row.get::<i64>("cnt").unwrap_or(0) as u32)
        } else {
            Ok(0)
        }
    }

    /// Deactivate sources that have had too many consecutive empty runs.
    pub async fn deactivate_dead_sources(&self, max_empty_runs: u32) -> Result<u32, neo4rs::Error> {
        let q = query(
            "MATCH (s:Source {active: true})
             WHERE s.consecutive_empty_runs >= $max
               AND s.discovery_method <> 'curated'
             SET s.active = false
             RETURN count(s) AS deactivated"
        )
        .param("max", max_empty_runs as i64);

        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            Ok(row.get::<i64>("deactivated").unwrap_or(0) as u32)
        } else {
            Ok(0)
        }
    }

    /// Check if a URL matches a blocked source pattern.
    pub async fn is_blocked(&self, url: &str) -> Result<bool, neo4rs::Error> {
        let q = query(
            "MATCH (b:BlockedSource)
             WHERE $url CONTAINS b.url_pattern OR b.url_pattern = $url
             RETURN b LIMIT 1"
        )
        .param("url", url);

        let mut stream = self.client.graph.execute(q).await?;
        Ok(stream.next().await?.is_some())
    }

    /// Get source-level stats for reporting.
    pub async fn get_source_stats(&self, city: &str) -> Result<SourceStats, neo4rs::Error> {
        let q = query(
            "MATCH (s:Source {city: $city})
             RETURN count(s) AS total,
                    count(CASE WHEN s.active THEN 1 END) AS active,
                    count(CASE WHEN s.discovery_method = 'curated' THEN 1 END) AS curated,
                    count(CASE WHEN s.discovery_method <> 'curated' THEN 1 END) AS discovered"
        )
        .param("city", city);

        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            Ok(SourceStats {
                total: row.get::<i64>("total").unwrap_or(0) as u32,
                active: row.get::<i64>("active").unwrap_or(0) as u32,
                curated: row.get::<i64>("curated").unwrap_or(0) as u32,
                discovered: row.get::<i64>("discovered").unwrap_or(0) as u32,
            })
        } else {
            Ok(SourceStats::default())
        }
    }

    // --- Actor operations ---

    /// Create or update an Actor node. MERGE on entity_id for idempotency.
    pub async fn upsert_actor(&self, actor: &ActorNode) -> Result<(), neo4rs::Error> {
        let q = query(
            "MERGE (a:Actor {entity_id: $entity_id})
             ON CREATE SET
                a.id = $id,
                a.name = $name,
                a.actor_type = $actor_type,
                a.domains = $domains,
                a.social_urls = $social_urls,
                a.city = $city,
                a.description = $description,
                a.signal_count = $signal_count,
                a.first_seen = datetime($first_seen),
                a.last_active = datetime($last_active),
                a.typical_roles = $typical_roles
             ON MATCH SET
                a.name = $name,
                a.last_active = datetime($last_active),
                a.signal_count = a.signal_count + 1"
        )
        .param("id", actor.id.to_string())
        .param("entity_id", actor.entity_id.as_str())
        .param("name", actor.name.as_str())
        .param("actor_type", actor.actor_type.to_string())
        .param("domains", actor.domains.clone())
        .param("social_urls", actor.social_urls.clone())
        .param("city", actor.city.as_str())
        .param("description", actor.description.as_str())
        .param("signal_count", actor.signal_count as i64)
        .param("first_seen", memgraph_datetime(&actor.first_seen))
        .param("last_active", memgraph_datetime(&actor.last_active))
        .param("typical_roles", actor.typical_roles.clone());

        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Link an actor to a signal with a role.
    pub async fn link_actor_to_signal(
        &self,
        actor_id: Uuid,
        signal_id: Uuid,
        role: &str,
    ) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (a:Actor {id: $actor_id})
             MATCH (n) WHERE n.id = $signal_id AND (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension)
             MERGE (a)-[:ACTED_IN {role: $role}]->(n)"
        )
        .param("actor_id", actor_id.to_string())
        .param("signal_id", signal_id.to_string())
        .param("role", role);

        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Find an actor by name (case-insensitive).
    pub async fn find_actor_by_name(&self, name: &str) -> Result<Option<Uuid>, neo4rs::Error> {
        let q = query(
            "MATCH (a:Actor) WHERE toLower(a.name) = toLower($name)
             RETURN a.id AS id LIMIT 1"
        )
        .param("name", name);

        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    /// Find an actor by entity_id.
    pub async fn find_actor_by_entity_id(&self, entity_id: &str) -> Result<Option<Uuid>, neo4rs::Error> {
        let q = query(
            "MATCH (a:Actor {entity_id: $entity_id})
             RETURN a.id AS id LIMIT 1"
        )
        .param("entity_id", entity_id);

        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    /// Find an actor by domain match.
    pub async fn find_actor_by_domain(&self, domain: &str) -> Result<Option<Uuid>, neo4rs::Error> {
        let q = query(
            "MATCH (a:Actor)
             WHERE any(d IN a.domains WHERE $domain CONTAINS d)
             RETURN a.id AS id LIMIT 1"
        )
        .param("domain", domain);

        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    /// Update actor signal count and last_active.
    pub async fn update_actor_stats(
        &self,
        actor_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (a:Actor {id: $id})
             SET a.signal_count = a.signal_count + 1,
                 a.last_active = datetime($now)"
        )
        .param("id", actor_id.to_string())
        .param("now", memgraph_datetime(&now));

        self.client.graph.run(q).await?;
        Ok(())
    }

    // --- Response mapping operations ---

    /// Create a RESPONDS_TO edge between a Give/Event signal and a Tension.
    pub async fn create_response_edge(
        &self,
        responder_id: Uuid,
        tension_id: Uuid,
        match_strength: f64,
        explanation: &str,
    ) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (resp) WHERE resp.id = $resp_id AND (resp:Give OR resp:Event)
             MATCH (t:Tension {id: $tension_id})
             MERGE (resp)-[:RESPONDS_TO {match_strength: $strength, explanation: $explanation}]->(t)"
        )
        .param("resp_id", responder_id.to_string())
        .param("tension_id", tension_id.to_string())
        .param("strength", match_strength)
        .param("explanation", explanation);

        self.client.graph.run(q).await?;
        Ok(())
    }

    // --- Edition operations ---

    /// Create an Edition node.
    pub async fn create_edition(&self, edition: &EditionNode) -> Result<(), neo4rs::Error> {
        let q = query(
            "CREATE (e:Edition {
                id: $id,
                city: $city,
                period: $period,
                period_start: datetime($period_start),
                period_end: datetime($period_end),
                generated_at: datetime($generated_at),
                story_count: $story_count,
                new_signal_count: $new_signal_count,
                editorial_summary: $editorial_summary
            })"
        )
        .param("id", edition.id.to_string())
        .param("city", edition.city.as_str())
        .param("period", edition.period.as_str())
        .param("period_start", memgraph_datetime(&edition.period_start))
        .param("period_end", memgraph_datetime(&edition.period_end))
        .param("generated_at", memgraph_datetime(&edition.generated_at))
        .param("story_count", edition.story_count as i64)
        .param("new_signal_count", edition.new_signal_count as i64)
        .param("editorial_summary", edition.editorial_summary.as_str());

        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Link an edition to a featured story.
    pub async fn link_edition_to_story(
        &self,
        edition_id: Uuid,
        story_id: Uuid,
    ) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (e:Edition {id: $edition_id})
             MATCH (s:Story {id: $story_id})
             MERGE (e)-[:FEATURES]->(s)"
        )
        .param("edition_id", edition_id.to_string())
        .param("story_id", story_id.to_string());

        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Get recent tension titles and what_would_help for discovery queries.
    pub async fn get_recent_tensions(&self, limit: u32) -> Result<Vec<(String, Option<String>)>, neo4rs::Error> {
        let q = query(
            "MATCH (t:Tension)
             RETURN t.title AS title, t.what_would_help AS help
             ORDER BY t.extracted_at DESC
             LIMIT $limit"
        )
        .param("limit", limit as i64);

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let title: String = row.get("title").unwrap_or_default();
            let help: String = row.get("help").unwrap_or_default();
            if !title.is_empty() {
                results.push((title, if help.is_empty() { None } else { Some(help) }));
            }
        }
        Ok(results)
    }

    /// Get actors with their domains and social URLs for source discovery.
    pub async fn get_actors_with_domains(
        &self,
        city: &str,
    ) -> Result<Vec<(String, Vec<String>, Vec<String>)>, neo4rs::Error> {
        let q = query(
            "MATCH (a:Actor {city: $city})
             WHERE size(a.domains) > 0 OR size(a.social_urls) > 0
             RETURN a.name AS name, a.domains AS domains, a.social_urls AS social_urls"
        )
        .param("city", city);

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let name: String = row.get("name").unwrap_or_default();
            let domains: Vec<String> = row.get("domains").unwrap_or_default();
            let social_urls: Vec<String> = row.get("social_urls").unwrap_or_default();
            if !name.is_empty() && (!domains.is_empty() || !social_urls.is_empty()) {
                results.push((name, domains, social_urls));
            }
        }
        Ok(results)
    }

    /// Get active tensions for response mapping.
    pub async fn get_active_tensions(&self) -> Result<Vec<(Uuid, Vec<f64>)>, neo4rs::Error> {
        let q = query(
            "MATCH (t:Tension)
             WHERE datetime(t.last_confirmed_active) >= datetime() - duration('P30D')
             RETURN t.id AS id, t.embedding AS embedding"
        );

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                let embedding: Vec<f64> = row.get("embedding").unwrap_or_default();
                if !embedding.is_empty() {
                    results.push((id, embedding));
                }
            }
        }
        Ok(results)
    }

    /// Get stories active in a time period (for edition generation).
    pub async fn get_stories_in_period(
        &self,
        start: &DateTime<Utc>,
        end: &DateTime<Utc>,
    ) -> Result<Vec<(Uuid, String, String, f64)>, neo4rs::Error> {
        let q = query(
            "MATCH (s:Story)
             WHERE datetime(s.last_updated) >= datetime($start)
               AND datetime(s.last_updated) <= datetime($end)
             RETURN s.id AS id, s.headline AS headline,
                    s.category AS category, s.energy AS energy
             ORDER BY s.energy DESC"
        )
        .param("start", memgraph_datetime(start))
        .param("end", memgraph_datetime(end));

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                let headline: String = row.get("headline").unwrap_or_default();
                let category: String = row.get("category").unwrap_or_default();
                let energy: f64 = row.get("energy").unwrap_or(0.0);
                results.push((id, headline, category, energy));
            }
        }
        Ok(results)
    }

    // --- Investigation operations ---

    /// Find signals that warrant investigation. Returns candidates across 3 priority
    /// categories with per-source-domain dedup (max 1 per domain to prevent budget exhaustion).
    pub async fn find_investigation_targets(&self) -> Result<Vec<InvestigationTarget>, neo4rs::Error> {
        let mut targets = Vec::new();
        let mut seen_domains = std::collections::HashSet::new();

        // Priority 1: New tensions (last 24h, < 2 evidence nodes, not investigated in 7d)
        let q = query(
            "MATCH (t:Tension)
             WHERE datetime(t.extracted_at) > datetime() - duration('P1D')
               AND (t.investigated_at IS NULL OR datetime(t.investigated_at) < datetime() - duration('P7D'))
             OPTIONAL MATCH (t)-[:SOURCED_FROM]->(ev:Evidence)
             WITH t, count(ev) AS ev_count
             WHERE ev_count < 2
             RETURN t.id AS id, 'Tension' AS label, t.title AS title, t.summary AS summary,
                    t.source_url AS source_url, t.sensitivity AS sensitivity
             LIMIT 10"
        );
        self.collect_investigation_targets(&mut targets, &mut seen_domains, q).await?;

        // Priority 2: High-urgency asks (urgency high/critical, < 2 evidence nodes)
        let q = query(
            "MATCH (a:Ask)
             WHERE a.urgency IN ['high', 'critical']
               AND (a.investigated_at IS NULL OR datetime(a.investigated_at) < datetime() - duration('P7D'))
             OPTIONAL MATCH (a)-[:SOURCED_FROM]->(ev:Evidence)
             WITH a, count(ev) AS ev_count
             WHERE ev_count < 2
             RETURN a.id AS id, 'Ask' AS label, a.title AS title, a.summary AS summary,
                    a.source_url AS source_url, a.sensitivity AS sensitivity
             LIMIT 10"
        );
        self.collect_investigation_targets(&mut targets, &mut seen_domains, q).await?;

        // Priority 3: Thin-story signals (from emerging stories, < 2 evidence nodes)
        let q = query(
            "MATCH (s:Story {status: 'emerging'})-[:CONTAINS]->(n)
             WHERE (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension)
               AND (n.investigated_at IS NULL OR datetime(n.investigated_at) < datetime() - duration('P7D'))
             OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Evidence)
             WITH n, count(ev) AS ev_count,
                  CASE WHEN n:Event THEN 'Event'
                       WHEN n:Give THEN 'Give'
                       WHEN n:Ask THEN 'Ask'
                       WHEN n:Notice THEN 'Notice'
                       WHEN n:Tension THEN 'Tension'
                  END AS label
             WHERE ev_count < 2
             RETURN n.id AS id, label, n.title AS title, n.summary AS summary,
                    n.source_url AS source_url, n.sensitivity AS sensitivity
             LIMIT 10"
        );
        self.collect_investigation_targets(&mut targets, &mut seen_domains, q).await?;

        Ok(targets)
    }

    /// Helper to collect targets from a Cypher query, enforcing per-domain dedup.
    async fn collect_investigation_targets(
        &self,
        targets: &mut Vec<InvestigationTarget>,
        seen_domains: &mut std::collections::HashSet<String>,
        q: neo4rs::Query,
    ) -> Result<(), neo4rs::Error> {
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let id = match Uuid::parse_str(&id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };

            let label: String = row.get("label").unwrap_or_default();
            let node_type = match label.as_str() {
                "Event" => NodeType::Event,
                "Give" => NodeType::Give,
                "Ask" => NodeType::Ask,
                "Notice" => NodeType::Notice,
                "Tension" => NodeType::Tension,
                _ => continue,
            };

            let source_url: String = row.get("source_url").unwrap_or_default();
            let domain = url::Url::parse(&source_url)
                .ok()
                .and_then(|u| u.host_str().map(|h| h.to_lowercase()))
                .unwrap_or_default();

            // Per-domain dedup: max 1 target per source domain
            if !domain.is_empty() && !seen_domains.insert(domain) {
                continue;
            }

            let sensitivity: String = row.get("sensitivity").unwrap_or_default();
            let is_sensitive = sensitivity == "sensitive" || sensitivity == "elevated";

            targets.push(InvestigationTarget {
                signal_id: id,
                node_type,
                title: row.get("title").unwrap_or_default(),
                summary: row.get("summary").unwrap_or_default(),
                source_url,
                is_sensitive,
            });
        }
        Ok(())
    }

    /// Mark a signal as investigated (sets investigated_at, 7-day cooldown).
    pub async fn mark_investigated(&self, signal_id: Uuid, node_type: NodeType) -> Result<(), neo4rs::Error> {
        let label = match node_type {
            NodeType::Event => "Event",
            NodeType::Give => "Give",
            NodeType::Ask => "Ask",
            NodeType::Notice => "Notice",
            NodeType::Tension => "Tension",
            NodeType::Evidence => return Ok(()),
        };

        let q = query(&format!(
            "MATCH (n:{} {{id: $id}})
             SET n.investigated_at = datetime($now)",
            label
        ))
        .param("id", signal_id.to_string())
        .param("now", memgraph_datetime(&Utc::now()));

        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Get the snapshot entity count from 7 days ago for velocity calculation.
    /// Velocity is driven by entity diversity growth — a flood from one source doesn't move the needle.
    pub async fn get_snapshot_entity_count_7d_ago(&self, story_id: Uuid) -> Result<Option<u32>, neo4rs::Error> {
        let q = query(
            "MATCH (cs:ClusterSnapshot {story_id: $story_id})
             WHERE datetime(cs.run_at) >= datetime() - duration('P8D')
               AND datetime(cs.run_at) <= datetime() - duration('P6D')
             RETURN cs.entity_count AS cnt
             ORDER BY cs.run_at ASC
             LIMIT 1"
        )
        .param("story_id", story_id.to_string());

        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let cnt: i64 = row.get("cnt").unwrap_or(0);
            return Ok(Some(cnt as u32));
        }

        Ok(None)
    }
}

#[derive(Debug, Default)]
pub struct ReapStats {
    pub events: u64,
    pub asks: u64,
    pub stale: u64,
}

#[derive(Debug, Default)]
pub struct SourceStats {
    pub total: u32,
    pub active: u32,
    pub curated: u32,
    pub discovered: u32,
}

#[derive(Debug)]
pub struct DuplicateMatch {
    pub id: Uuid,
    pub node_type: NodeType,
    pub source_url: String,
    pub similarity: f64,
}

/// A signal that warrants investigation.
#[derive(Debug)]
pub struct InvestigationTarget {
    pub signal_id: Uuid,
    pub node_type: NodeType,
    pub title: String,
    pub summary: String,
    pub source_url: String,
    pub is_sensitive: bool,
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

fn embedding_to_f64(embedding: &[f32]) -> Vec<f64> {
    embedding.iter().map(|&v| v as f64).collect()
}

/// Format a DateTime<Utc> as a local datetime string without timezone offset.
/// Memgraph's datetime() requires "YYYY-MM-DDThh:mm:ss" format (no +00:00 suffix).
fn memgraph_datetime(dt: &DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
}

/// Public version of memgraph_datetime for use by other modules (e.g. cluster.rs).
pub fn memgraph_datetime_pub(dt: &DateTime<Utc>) -> String {
    memgraph_datetime(dt)
}

/// Parse a Memgraph datetime string back into a DateTime<Utc>.
/// Returns None for empty strings or parse failures.
fn parse_memgraph_datetime_opt(s: &str) -> Option<DateTime<Utc>> {
    if s.is_empty() {
        return None;
    }
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
        .map(|ndt| ndt.and_utc())
        .ok()
}
