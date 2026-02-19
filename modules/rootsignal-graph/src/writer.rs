use chrono::{DateTime, Utc};
use neo4rs::query;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{
    ActorNode, AskNode, CityNode, ClusterSnapshot, DiscoveryMethod, EditionNode, EvidenceNode,
    EventNode, GiveNode, Node, NodeMeta, NodeType, NoticeNode, SensitivityLevel, SourceNode,
    SourceRole, SourceType, StoryNode, TensionNode, ASK_EXPIRE_DAYS, EVENT_PAST_GRACE_HOURS,
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
                implied_queries: CASE WHEN size($implied_queries) > 0 THEN $implied_queries ELSE null END,
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
        .param("extracted_at", format_datetime(&n.meta.extracted_at))
        .param(
            "last_confirmed_active",
            format_datetime(&n.meta.last_confirmed_active),
        )

        .param("location_name", n.meta.location_name.as_deref().unwrap_or(""))
        .param(
            "starts_at",
            n.starts_at
                .map(|dt| format_datetime(&dt))
                .unwrap_or_default(),
        )
        .param(
            "ends_at",
            n.ends_at
                .map(|dt| format_datetime(&dt))
                .unwrap_or_default(),
        )
        .param("action_url", n.action_url.as_str())
        .param("organizer", n.organizer.clone().unwrap_or_default())
        .param("is_recurring", n.is_recurring)
        .param("implied_queries", n.meta.implied_queries.clone())
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
                implied_queries: CASE WHEN size($implied_queries) > 0 THEN $implied_queries ELSE null END,
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
        .param("extracted_at", format_datetime(&n.meta.extracted_at))
        .param(
            "last_confirmed_active",
            format_datetime(&n.meta.last_confirmed_active),
        )

        .param("location_name", n.meta.location_name.as_deref().unwrap_or(""))
        .param("action_url", n.action_url.as_str())
        .param("availability", n.availability.as_deref().unwrap_or(""))
        .param("is_ongoing", n.is_ongoing)
        .param("implied_queries", n.meta.implied_queries.clone())
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
        .param("extracted_at", format_datetime(&n.meta.extracted_at))
        .param(
            "last_confirmed_active",
            format_datetime(&n.meta.last_confirmed_active),
        )

        .param("location_name", n.meta.location_name.as_deref().unwrap_or(""))
        .param(
            "urgency",
            urgency_str(n.urgency),
        )
        .param("what_needed", n.what_needed.as_deref().unwrap_or(""))
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
        .param("extracted_at", format_datetime(&n.meta.extracted_at))
        .param(
            "last_confirmed_active",
            format_datetime(&n.meta.last_confirmed_active),
        )

        .param("location_name", n.meta.location_name.as_deref().unwrap_or(""))
        .param("severity", severity_str(n.severity))
        .param("category", n.category.clone().unwrap_or_default())
        .param(
            "effective_date",
            n.effective_date
                .map(|dt| format_datetime(&dt))
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
        .param("extracted_at", format_datetime(&n.meta.extracted_at))
        .param(
            "last_confirmed_active",
            format_datetime(&n.meta.last_confirmed_active),
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
        .param("retrieved_at", format_datetime(&evidence.retrieved_at))
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
        .param("now", format_datetime(&now));

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
            "CALL db.index.vector.queryNodes('{}', 1, $embedding)
             YIELD node, score AS similarity
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
        .param("now", format_datetime(&now));

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
        .param("now", format_datetime(&now));

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

    /// Acquire a per-city scout lock. Returns false if a scout is already running for this city.
    /// Cleans up stale locks (>30 min) from killed containers.
    /// Uses a single atomic query to avoid TOCTOU race between check and create.
    pub async fn acquire_scout_lock(&self, city: &str) -> Result<bool, neo4rs::Error> {
        // Delete stale locks older than 30 minutes for this city
        self.client
            .graph
            .run(query(
                "MATCH (lock:ScoutLock {city: $city}) WHERE lock.started_at < datetime() - duration('PT30M') DELETE lock"
            ).param("city", city))
            .await?;

        // Atomic check-and-create: only creates if no lock exists for this city
        let q = query(
            "OPTIONAL MATCH (existing:ScoutLock {city: $city})
             WITH existing WHERE existing IS NULL
             CREATE (lock:ScoutLock {city: $city, started_at: datetime()})
             RETURN lock IS NOT NULL AS acquired"
        ).param("city", city);

        let mut result = self.client.graph.execute(q).await?;
        if let Some(row) = result.next().await? {
            let acquired: bool = row.get("acquired").unwrap_or(false);
            return Ok(acquired);
        }

        // No row returned means the WHERE filtered it out (lock exists)
        Ok(false)
    }

    /// Release the per-city scout lock.
    pub async fn release_scout_lock(&self, city: &str) -> Result<(), neo4rs::Error> {
        self.client
            .graph
            .run(query("MATCH (lock:ScoutLock {city: $city}) DELETE lock").param("city", city))
            .await?;
        Ok(())
    }

    /// Check if a scout is currently running for a city (read-only, no acquire/release dance).
    pub async fn is_scout_running(&self, city: &str) -> Result<bool, neo4rs::Error> {
        let q = query(
            "OPTIONAL MATCH (lock:ScoutLock {city: $city}) WHERE lock.started_at >= datetime() - duration('PT30M') RETURN lock IS NOT NULL AS running"
        ).param("city", city);

        let mut result = self.client.graph.execute(q).await?;
        if let Some(row) = result.next().await? {
            let running: bool = row.get("running").unwrap_or(false);
            return Ok(running);
        }
        Ok(false)
    }

    /// Stamp the city's last_scout_completed_at to now.
    pub async fn set_city_scout_completed(&self, slug: &str) -> Result<(), neo4rs::Error> {
        self.client
            .graph
            .run(query(
                "MATCH (c:City {slug: $slug}) SET c.last_scout_completed_at = datetime()"
            ).param("slug", slug))
            .await?;
        Ok(())
    }

    /// Count sources that are overdue for scraping.
    pub async fn count_due_sources(&self, city: &str) -> Result<u32, neo4rs::Error> {
        let q = query(
            "MATCH (s:Source {city: $city, active: true})
             WHERE s.last_scraped IS NULL
                OR datetime(s.last_scraped) + duration('PT' + toString(coalesce(s.cadence_hours, 24)) + 'H') < datetime()
             RETURN count(s) AS due"
        ).param("city", city);

        let mut result = self.client.graph.execute(q).await?;
        if let Some(row) = result.next().await? {
            let due: i64 = row.get("due").unwrap_or(0);
            return Ok(due as u32);
        }
        Ok(0)
    }

    /// Get the earliest time a source becomes due for scraping.
    pub async fn next_source_due(&self, city: &str) -> Result<Option<chrono::DateTime<Utc>>, neo4rs::Error> {
        let q = query(
            "MATCH (s:Source {city: $city, active: true})
             WHERE s.last_scraped IS NOT NULL
             RETURN min(datetime(s.last_scraped) + duration('PT' + toString(coalesce(s.cadence_hours, 24)) + 'H')) AS next_due"
        ).param("city", city);

        let mut result = self.client.graph.execute(q).await?;
        if let Some(row) = result.next().await? {
            let next_due_str: String = row.get("next_due").unwrap_or_default();
            if !next_due_str.is_empty() {
                if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(&next_due_str, "%Y-%m-%dT%H:%M:%S%.f") {
                    return Ok(Some(ndt.and_utc()));
                }
            }
        }
        Ok(None)
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
        .param("first_seen", format_datetime(&story.first_seen))
        .param("last_updated", format_datetime(&story.last_updated))
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
        .param("last_updated", format_datetime(&story.last_updated))
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
        .param("run_at", format_datetime(&snapshot.run_at));

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
        .param("created_at", format_datetime(&city.created_at));

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
                    c.active AS active, c.created_at AS created_at,
                    c.last_scout_completed_at AS last_scout_completed_at"
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

            let last_scout_completed_at = {
                let s: String = row.get("last_scout_completed_at").unwrap_or_default();
                if s.is_empty() {
                    None
                } else {
                    chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S%.f")
                        .map(|ndt| ndt.and_utc())
                        .ok()
                }
            };

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
                last_scout_completed_at,
            }))
        } else {
            Ok(None)
        }
    }

    /// List all cities, ordered by name.
    pub async fn list_cities(&self) -> Result<Vec<CityNode>, neo4rs::Error> {
        let q = query(
            "MATCH (c:City)
             RETURN c.id AS id, c.name AS name, c.slug AS slug,
                    c.center_lat AS center_lat, c.center_lng AS center_lng,
                    c.radius_km AS radius_km, c.geo_terms AS geo_terms,
                    c.active AS active, c.created_at AS created_at,
                    c.last_scout_completed_at AS last_scout_completed_at
             ORDER BY c.name"
        );

        let mut cities = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let id = match Uuid::parse_str(&id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };

            let created_at_str: String = row.get("created_at").unwrap_or_default();
            let created_at = chrono::NaiveDateTime::parse_from_str(&created_at_str, "%Y-%m-%dT%H:%M:%S%.f")
                .map(|ndt| ndt.and_utc())
                .unwrap_or_else(|_| Utc::now());

            let last_scout_completed_at = {
                let s: String = row.get("last_scout_completed_at").unwrap_or_default();
                if s.is_empty() {
                    None
                } else {
                    chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S%.f")
                        .map(|ndt| ndt.and_utc())
                        .ok()
                }
            };

            cities.push(CityNode {
                id,
                name: row.get("name").unwrap_or_default(),
                slug: row.get("slug").unwrap_or_default(),
                center_lat: row.get("center_lat").unwrap_or(0.0),
                center_lng: row.get("center_lng").unwrap_or(0.0),
                radius_km: row.get("radius_km").unwrap_or(0.0),
                geo_terms: row.get("geo_terms").unwrap_or_default(),
                active: row.get("active").unwrap_or(true),
                created_at,
                last_scout_completed_at,
            });
        }

        Ok(cities)
    }

    /// Batch count of sources and signals per city.
    /// Accepts city tuples of (slug, center_lat, center_lng, radius_km).
    /// Signal counts use geographic bounding box on signal lat/lng.
    /// Returns Vec<(slug, source_count, signal_count)>.
    pub async fn get_city_counts(&self, cities: &[(String, f64, f64, f64)]) -> Result<Vec<(String, u32, u32)>, neo4rs::Error> {
        let mut results = Vec::new();
        for (slug, lat, lng, radius_km) in cities {
            // Source count by slug
            let sq = query(
                "MATCH (src:Source {city: $city, active: true})
                 RETURN count(src) AS cnt"
            ).param("city", slug.as_str());
            let mut stream = self.client.graph.execute(sq).await?;
            let source_count: i64 = match stream.next().await? {
                Some(row) => row.get("cnt").unwrap_or(0),
                None => 0,
            };

            // Signal count by bounding box
            let lat_delta = radius_km / 111.0;
            let lng_delta = radius_km / (111.0 * lat.to_radians().cos());
            let nq = query(
                "MATCH (n)
                 WHERE (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension)
                   AND n.lat <> 0.0
                   AND n.lat >= $min_lat AND n.lat <= $max_lat
                   AND n.lng >= $min_lng AND n.lng <= $max_lng
                 RETURN count(n) AS cnt"
            )
            .param("min_lat", lat - lat_delta)
            .param("max_lat", lat + lat_delta)
            .param("min_lng", lng - lng_delta)
            .param("max_lng", lng + lng_delta);
            let mut stream = self.client.graph.execute(nq).await?;
            let signal_count: i64 = match stream.next().await? {
                Some(row) => row.get("cnt").unwrap_or(0),
                None => 0,
            };

            results.push((slug.clone(), source_count as u32, signal_count as u32));
        }
        Ok(results)
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
                s.quality_penalty = $quality_penalty,
                s.source_role = $source_role,
                s.scrape_count = $scrape_count
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
        .param("created_at", format_datetime(&source.created_at))
        .param("signals_produced", source.signals_produced as i64)
        .param("signals_corroborated", source.signals_corroborated as i64)
        .param("consecutive_empty_runs", source.consecutive_empty_runs as i64)
        .param("active", source.active)
        .param("gap_context", source.gap_context.clone().unwrap_or_default())
        .param("weight", source.weight)
        .param("avg_signals_per_scrape", source.avg_signals_per_scrape)
        .param("quality_penalty", source.quality_penalty)
        .param("source_role", source.source_role.to_string())
        .param("scrape_count", source.scrape_count as i64);

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
        .param("submitted_at", format_datetime(&submission.submitted_at))
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
                    s.quality_penalty AS quality_penalty,
                    s.source_role AS source_role,
                    s.scrape_count AS scrape_count"
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
                "signal_expansion" => DiscoveryMethod::SignalExpansion,
                _ => DiscoveryMethod::Curated,
            };

            let created_at = row_datetime_opt(&row, "created_at")
                .unwrap_or_else(Utc::now);

            let last_scraped = row_datetime_opt(&row, "last_scraped");
            let last_produced_signal = row_datetime_opt(&row, "last_produced_signal");

            let gap_context: String = row.get("gap_context").unwrap_or_default();
            let url: String = row.get("url").unwrap_or_default();
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
                quality_penalty: row.get("quality_penalty").unwrap_or(1.0),
                source_role: SourceRole::from_str_loose(
                    &row.get::<String>("source_role").unwrap_or_default(),
                ),
                scrape_count: row.get::<i64>("scrape_count").unwrap_or(0) as u32,
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
                     s.consecutive_empty_runs = 0,
                     s.scrape_count = coalesce(s.scrape_count, 0) + 1"
            )
            .param("key", canonical_key)
            .param("now", format_datetime(&now))
            .param("count", signals_produced as i64);
            self.client.graph.run(q).await?;
        } else {
            let q = query(
                "MATCH (s:Source {canonical_key: $key})
                 SET s.last_scraped = datetime($now),
                     s.consecutive_empty_runs = s.consecutive_empty_runs + 1,
                     s.scrape_count = coalesce(s.scrape_count, 0) + 1"
            )
            .param("key", canonical_key)
            .param("now", format_datetime(&now));
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
    /// Protects curated and human-submitted sources. Scoped to a single city.
    pub async fn deactivate_dead_sources(&self, city: &str, max_empty_runs: u32) -> Result<u32, neo4rs::Error> {
        let q = query(
            "MATCH (s:Source {active: true, city: $city})
             WHERE s.consecutive_empty_runs >= $max
               AND s.discovery_method <> 'curated'
               AND s.discovery_method <> 'human_submission'
             SET s.active = false
             RETURN count(s) AS deactivated"
        )
        .param("city", city)
        .param("max", max_empty_runs as i64);

        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            Ok(row.get::<i64>("deactivated").unwrap_or(0) as u32)
        } else {
            Ok(0)
        }
    }

    /// Get all active TavilyQuery canonical_values for a city (used for expansion dedup).
    pub async fn get_active_tavily_queries(&self, city: &str) -> Result<Vec<String>, neo4rs::Error> {
        let q = query(
            "MATCH (s:Source {city: $city, active: true, source_type: 'tavily_query'})
             RETURN s.canonical_value AS query"
        )
        .param("city", city);

        let mut queries = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let query_str: String = row.get("query").unwrap_or_default();
            if !query_str.is_empty() {
                queries.push(query_str);
            }
        }
        Ok(queries)
    }

    /// Get implied queries from Give/Event signals recently linked to heated tensions.
    /// These signals were extracted with implied_queries but deferred expansion until
    /// response mapping connected them to a tension. Clears queries after collection
    /// to prevent replay on subsequent runs.
    pub async fn get_recently_linked_signals_with_queries(
        &self,
        _city: &str,
    ) -> Result<Vec<String>, neo4rs::Error> {
        // Find Give/Event signals with implied_queries that are linked to heated tensions
        let q = query(
            "MATCH (s)-[:RESPONDS_TO|DRAWN_TO]->(t:Tension)
             WHERE (s:Give OR s:Event)
               AND s.implied_queries IS NOT NULL
               AND size(s.implied_queries) > 0
               AND coalesce(t.cause_heat, 0.0) >= 0.1
             WITH DISTINCT s
             RETURN s.implied_queries AS queries, s.id AS id"
        );

        let mut all_queries = Vec::new();
        let mut signal_ids = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            // neo4rs returns List<String> as Vec<String>
            let queries: Vec<String> = row.get("queries").unwrap_or_default();
            all_queries.extend(queries);
            let id: String = row.get("id").unwrap_or_default();
            if !id.is_empty() {
                signal_ids.push(id);
            }
        }

        // Clear implied_queries on processed signals to prevent replay
        if !signal_ids.is_empty() {
            for id in &signal_ids {
                let clear_q = query(
                    "MATCH (s {id: $id})
                     WHERE s:Give OR s:Event
                     SET s.implied_queries = null"
                )
                .param("id", id.as_str());
                if let Err(e) = self.client.graph.run(clear_q).await {
                    warn!(id = id.as_str(), error = %e, "Failed to clear implied_queries");
                }
            }
        }

        Ok(all_queries)
    }

    /// Get tension response shape analysis for discovery briefing.
    pub async fn get_tension_response_shape(
        &self,
        limit: u32,
    ) -> Result<Vec<TensionResponseShape>, neo4rs::Error> {
        let q = query(
            "MATCH (t:Tension)
             WHERE t.confidence >= 0.5
               AND coalesce(t.cause_heat, 0.0) >= 0.1
             WITH t
             ORDER BY coalesce(t.cause_heat, 0.0) DESC
             LIMIT $limit
             OPTIONAL MATCH (r)-[:RESPONDS_TO]->(t)
             WHERE r:Give OR r:Event OR r:Ask
             WITH t,
                  count(CASE WHEN r:Give THEN 1 END) AS give_count,
                  count(CASE WHEN r:Event THEN 1 END) AS event_count,
                  count(CASE WHEN r:Ask THEN 1 END) AS ask_count,
                  collect(DISTINCT r.title)[..5] AS sample_titles
             WHERE give_count + event_count + ask_count > 0
             RETURN t.title AS title,
                    t.what_would_help AS what_would_help,
                    coalesce(t.cause_heat, 0.0) AS cause_heat,
                    give_count, event_count, ask_count,
                    sample_titles"
        )
        .param("limit", limit as i64);

        let mut shapes = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let title: String = row.get("title").unwrap_or_default();
            let what_would_help: Option<String> = row.get("what_would_help").ok();
            let cause_heat: f64 = row.get("cause_heat").unwrap_or(0.0);
            let give_count: i64 = row.get("give_count").unwrap_or(0);
            let event_count: i64 = row.get("event_count").unwrap_or(0);
            let ask_count: i64 = row.get("ask_count").unwrap_or(0);
            let sample_titles: Vec<String> = row.get("sample_titles").unwrap_or_default();

            shapes.push(TensionResponseShape {
                title,
                what_would_help,
                cause_heat,
                give_count: give_count as u32,
                event_count: event_count as u32,
                ask_count: ask_count as u32,
                sample_titles,
            });
        }
        Ok(shapes)
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
        .param("first_seen", format_datetime(&actor.first_seen))
        .param("last_active", format_datetime(&actor.last_active))
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
        .param("now", format_datetime(&now));

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
            "MATCH (resp) WHERE resp.id = $resp_id AND (resp:Give OR resp:Event OR resp:Ask)
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
        .param("period_start", format_datetime(&edition.period_start))
        .param("period_end", format_datetime(&edition.period_end))
        .param("generated_at", format_datetime(&edition.generated_at))
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

    /// Get actors with their domains, social URLs, and dominant signal role for source discovery.
    pub async fn get_actors_with_domains(
        &self,
        city: &str,
    ) -> Result<Vec<(String, Vec<String>, Vec<String>, String)>, neo4rs::Error> {
        let q = query(
            "MATCH (a:Actor {city: $city})
             WHERE size(a.domains) > 0 OR size(a.social_urls) > 0
             OPTIONAL MATCH (a)-[:ACTED_IN]->(n)
             WITH a,
                  count(CASE WHEN n:Give OR n:Event THEN 1 END) AS response_signals,
                  count(CASE WHEN n:Tension THEN 1 END) AS tension_signals
             RETURN a.name AS name, a.domains AS domains, a.social_urls AS social_urls,
                    CASE
                      WHEN response_signals > tension_signals THEN 'response'
                      WHEN tension_signals > response_signals THEN 'tension'
                      ELSE 'mixed'
                    END AS dominant_role"
        )
        .param("city", city);

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let name: String = row.get("name").unwrap_or_default();
            let domains: Vec<String> = row.get("domains").unwrap_or_default();
            let social_urls: Vec<String> = row.get("social_urls").unwrap_or_default();
            let dominant_role: String = row.get("dominant_role").unwrap_or_default();
            if !name.is_empty() && (!domains.is_empty() || !social_urls.is_empty()) {
                results.push((name, domains, social_urls, dominant_role));
            }
        }
        Ok(results)
    }

    // --- Discovery briefing queries ---

    /// Get tensions ordered by: unmet first, then by severity. Includes response coverage.
    pub async fn get_unmet_tensions(&self, limit: u32) -> Result<Vec<UnmetTension>, neo4rs::Error> {
        let q = query(
            "MATCH (t:Tension)
             WHERE datetime(t.last_confirmed_active) >= datetime() - duration('P30D')
             OPTIONAL MATCH (resp)-[:RESPONDS_TO]->(t)
             WITH t, count(resp) AS response_count
             RETURN t.title AS title, t.severity AS severity,
                    t.what_would_help AS what_would_help, t.category AS category,
                    response_count = 0 AS unmet,
                    COALESCE(t.corroboration_count, 0) AS corroboration_count,
                    COALESCE(t.source_diversity, 0) AS source_diversity,
                    COALESCE(t.cause_heat, 0.0) AS cause_heat
             ORDER BY response_count ASC,
                      (COALESCE(t.corroboration_count, 0) + COALESCE(t.source_diversity, 0)) DESC,
                      t.cause_heat DESC,
                      t.severity DESC
             LIMIT $limit"
        )
        .param("limit", limit as i64);

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let title: String = row.get("title").unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            results.push(UnmetTension {
                title,
                severity: row.get("severity").unwrap_or_default(),
                what_would_help: {
                    let h: String = row.get("what_would_help").unwrap_or_default();
                    if h.is_empty() { None } else { Some(h) }
                },
                category: {
                    let c: String = row.get("category").unwrap_or_default();
                    if c.is_empty() { None } else { Some(c) }
                },
                unmet: row.get("unmet").unwrap_or(true),
                corroboration_count: row.get::<i64>("corroboration_count").unwrap_or(0) as u32,
                source_diversity: row.get::<i64>("source_diversity").unwrap_or(0) as u32,
                cause_heat: row.get("cause_heat").unwrap_or(0.0),
            });
        }
        Ok(results)
    }

    /// Recent stories by energy — gives the LLM a sense of what narratives are forming.
    pub async fn get_story_landscape(&self, limit: u32) -> Result<Vec<StoryBrief>, neo4rs::Error> {
        let q = query(
            "MATCH (s:Story)
             WHERE datetime(s.last_updated) >= datetime() - duration('P14D')
             RETURN s.headline AS headline, s.arc AS arc, s.energy AS energy,
                    s.signal_count AS signal_count, s.type_diversity AS type_diversity,
                    s.dominant_type AS dominant_type, s.source_count AS source_count
             ORDER BY s.energy DESC LIMIT $limit"
        )
        .param("limit", limit as i64);

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            results.push(StoryBrief {
                headline: row.get("headline").unwrap_or_default(),
                arc: {
                    let a: String = row.get("arc").unwrap_or_default();
                    if a.is_empty() { None } else { Some(a) }
                },
                energy: row.get("energy").unwrap_or(0.0),
                signal_count: row.get::<i64>("signal_count").unwrap_or(0) as u32,
                type_diversity: row.get::<i64>("type_diversity").unwrap_or(0) as u32,
                dominant_type: row.get("dominant_type").unwrap_or_default(),
                source_count: row.get::<i64>("source_count").unwrap_or(0) as u32,
            });
        }
        Ok(results)
    }

    /// Aggregate counts of each active signal type. Reveals systemic imbalances.
    pub async fn get_signal_type_counts(&self, _city: &str) -> Result<SignalTypeCounts, neo4rs::Error> {
        let mut counts = SignalTypeCounts::default();

        for (label, field) in &[
            ("Event", "events"),
            ("Give", "gives"),
            ("Ask", "asks"),
            ("Notice", "notices"),
            ("Tension", "tensions"),
        ] {
            let q = query(&format!(
                "MATCH (n:{label})
                 WHERE datetime(n.last_confirmed_active) >= datetime() - duration('P30D')
                 RETURN count(n) AS cnt"
            ));
            let mut stream = self.client.graph.execute(q).await?;
            if let Some(row) = stream.next().await? {
                let cnt = row.get::<i64>("cnt").unwrap_or(0) as u32;
                match *field {
                    "events" => counts.events = cnt,
                    "gives" => counts.gives = cnt,
                    "asks" => counts.asks = cnt,
                    "notices" => counts.notices = cnt,
                    "tensions" => counts.tensions = cnt,
                    _ => {}
                }
            }
        }

        Ok(counts)
    }

    /// Top successful and bottom failed LLM-discovered sources.
    /// Returns (successes, failures) filtered to gap_analysis/tension_seed discovery methods.
    pub async fn get_discovery_performance(
        &self,
        city: &str,
    ) -> Result<(Vec<SourceBrief>, Vec<SourceBrief>), neo4rs::Error> {
        // Top 5 successful: active, signals_produced > 0, ordered by weight DESC
        let q = query(
            "MATCH (s:Source {city: $city, active: true})
             WHERE s.discovery_method IN ['gap_analysis', 'tension_seed']
               AND s.signals_produced > 0
             RETURN s.canonical_value AS cv, s.signals_produced AS sp,
                    s.weight AS weight, s.consecutive_empty_runs AS cer,
                    s.gap_context AS gc, s.active AS active
             ORDER BY s.weight DESC
             LIMIT 5"
        )
        .param("city", city);

        let mut successes = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            successes.push(SourceBrief {
                canonical_value: row.get("cv").unwrap_or_default(),
                signals_produced: row.get::<i64>("sp").unwrap_or(0) as u32,
                weight: row.get("weight").unwrap_or(0.0),
                consecutive_empty_runs: row.get::<i64>("cer").unwrap_or(0) as u32,
                gap_context: {
                    let gc: String = row.get("gc").unwrap_or_default();
                    if gc.is_empty() { None } else { Some(gc) }
                },
                active: row.get("active").unwrap_or(true),
            });
        }

        // Bottom 5 failures: deactivated or 3+ consecutive empty runs
        let q = query(
            "MATCH (s:Source {city: $city})
             WHERE s.discovery_method IN ['gap_analysis', 'tension_seed']
               AND (s.active = false OR s.consecutive_empty_runs >= 3)
             RETURN s.canonical_value AS cv, s.signals_produced AS sp,
                    s.weight AS weight, s.consecutive_empty_runs AS cer,
                    s.gap_context AS gc, s.active AS active
             ORDER BY s.consecutive_empty_runs DESC
             LIMIT 5"
        )
        .param("city", city);

        let mut failures = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            failures.push(SourceBrief {
                canonical_value: row.get("cv").unwrap_or_default(),
                signals_produced: row.get::<i64>("sp").unwrap_or(0) as u32,
                weight: row.get("weight").unwrap_or(0.0),
                consecutive_empty_runs: row.get::<i64>("cer").unwrap_or(0) as u32,
                gap_context: {
                    let gc: String = row.get("gc").unwrap_or_default();
                    if gc.is_empty() { None } else { Some(gc) }
                },
                active: row.get("active").unwrap_or(true),
            });
        }

        Ok((successes, failures))
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
        .param("start", format_datetime(start))
        .param("end", format_datetime(end));

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
        .param("now", format_datetime(&Utc::now()));

        self.client.graph.run(q).await?;
        Ok(())
    }

    // --- Curiosity loop methods ---

    /// Find signals that have no RESPONDS_TO edge to any Tension and haven't been
    /// curiosity-investigated yet (or were `failed` with retry budget remaining).
    ///
    /// Pre-pass: signals with `failed` + retry_count >= 3 are auto-promoted to `abandoned`.
    pub async fn find_curiosity_targets(&self, limit: u32) -> Result<Vec<CuriosityTarget>, neo4rs::Error> {
        // Pre-pass: promote exhausted retries to abandoned
        let promote = query(
            "MATCH (n)
             WHERE (n:Give OR n:Event OR n:Ask OR n:Notice)
               AND n.curiosity_investigated = 'failed'
               AND n.curiosity_retry_count >= 3
             SET n.curiosity_investigated = 'abandoned'"
        );
        self.client.graph.run(promote).await?;

        let q = query(
            "MATCH (n)
             WHERE (n:Give OR n:Event OR n:Ask OR n:Notice)
               AND (n.curiosity_investigated IS NULL OR n.curiosity_investigated = 'failed')
               AND NOT (n)-[:RESPONDS_TO|DRAWN_TO]->(:Tension)
               AND n.confidence >= 0.5
             RETURN n.id AS id, n.title AS title, n.summary AS summary,
                    n.source_url AS source_url,
                    CASE WHEN n:Event THEN 'Event'
                         WHEN n:Give THEN 'Give'
                         WHEN n:Ask THEN 'Ask'
                         WHEN n:Notice THEN 'Notice'
                    END AS label
             ORDER BY n.extracted_at DESC
             LIMIT $limit"
        )
        .param("limit", limit as i64);

        let mut targets = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let id = match Uuid::parse_str(&id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };
            targets.push(CuriosityTarget {
                signal_id: id,
                title: row.get("title").unwrap_or_default(),
                summary: row.get("summary").unwrap_or_default(),
                label: row.get("label").unwrap_or_default(),
                source_url: row.get("source_url").unwrap_or_default(),
            });
        }
        Ok(targets)
    }

    /// Mark a signal with its curiosity investigation outcome.
    ///
    /// - `Done`/`Skipped`/`Abandoned`: permanent — signal won't be retried.
    /// - `Failed`: increments retry_count — signal reappears in `find_curiosity_targets`
    ///   until retry_count reaches 3 (then auto-promoted to `Abandoned`).
    pub async fn mark_curiosity_investigated(
        &self,
        signal_id: Uuid,
        label: &str,
        outcome: CuriosityOutcome,
    ) -> Result<(), neo4rs::Error> {
        let label = match label {
            "Event" | "Give" | "Ask" | "Notice" => label,
            _ => return Ok(()),
        };

        let cypher = if outcome == CuriosityOutcome::Failed {
            format!(
                "MATCH (n:{label} {{id: $id}})
                 SET n.curiosity_investigated = $outcome,
                     n.curiosity_retry_count = coalesce(n.curiosity_retry_count, 0) + 1"
            )
        } else {
            format!(
                "MATCH (n:{label} {{id: $id}})
                 SET n.curiosity_investigated = $outcome"
            )
        };

        let q = query(&cypher)
            .param("id", signal_id.to_string())
            .param("outcome", outcome.as_str());

        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Get existing tension titles+summaries for the curiosity loop's context window.
    pub async fn get_tension_landscape(&self) -> Result<Vec<(String, String)>, neo4rs::Error> {
        let q = query(
            "MATCH (t:Tension)
             RETURN t.title AS title, t.summary AS summary
             ORDER BY t.extracted_at DESC
             LIMIT 50"
        );

        let mut tensions = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let title: String = row.get("title").unwrap_or_default();
            let summary: String = row.get("summary").unwrap_or_default();
            tensions.push((title, summary));
        }
        Ok(tensions)
    }

    // --- StoryWeaver graph queries ---

    /// Find tension hubs ready to materialize as stories: tensions with 2+ responding
    /// signals that aren't already contained in any Story.
    pub async fn find_tension_hubs(&self, limit: u32) -> Result<Vec<TensionHub>, neo4rs::Error> {
        let q = query(
            "MATCH (t:Tension)<-[r:RESPONDS_TO|DRAWN_TO]-(sig)
             WHERE NOT (t)<-[:CONTAINS]-(:Story)
             WITH t, collect({
                 sig_id: sig.id,
                 source_url: sig.source_url,
                 strength: r.match_strength,
                 explanation: r.explanation,
                 edge_type: type(r),
                 gathering_type: r.gathering_type
             }) AS respondents
             WHERE size(respondents) >= 2
             RETURN t.id AS tension_id, t.title AS title, t.summary AS summary,
                    t.category AS category, t.what_would_help AS what_would_help,
                    respondents
             ORDER BY size(respondents) DESC
             LIMIT $limit"
        )
        .param("limit", limit as i64);

        let mut hubs = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("tension_id").unwrap_or_default();
            let tension_id = match Uuid::parse_str(&id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };

            let title: String = row.get("title").unwrap_or_default();
            let summary: String = row.get("summary").unwrap_or_default();
            let category: Option<String> = row.get("category").ok();
            let what_would_help: Option<String> = row.get("what_would_help").ok();

            // Parse respondents from neo4j map list
            let respondent_maps: Vec<neo4rs::BoltMap> = row.get("respondents").unwrap_or_default();
            let mut respondents = Vec::new();
            for map in respondent_maps {
                let sig_id_str = map.get::<String>("sig_id").unwrap_or_default();
                let sig_id = match Uuid::parse_str(&sig_id_str) {
                    Ok(id) => id,
                    Err(_) => continue,
                };
                respondents.push(TensionRespondent {
                    signal_id: sig_id,
                    source_url: map.get::<String>("source_url").unwrap_or_default(),
                    match_strength: map.get::<f64>("strength").unwrap_or(0.0),
                    explanation: map.get::<String>("explanation").unwrap_or_default(),
                    edge_type: map.get::<String>("edge_type").unwrap_or_default(),
                    gathering_type: map.get::<String>("gathering_type").ok(),
                });
            }

            hubs.push(TensionHub {
                tension_id,
                title,
                summary,
                category,
                what_would_help,
                respondents,
            });
        }
        Ok(hubs)
    }

    /// Find existing stories that have new responding signals not yet linked via CONTAINS.
    pub async fn find_story_growth(&self, limit: u32) -> Result<Vec<StoryGrowth>, neo4rs::Error> {
        let q = query(
            "MATCH (t:Tension)<-[:CONTAINS]-(story:Story)
             MATCH (t)<-[r:RESPONDS_TO|DRAWN_TO]-(sig)
             WHERE NOT (story)-[:CONTAINS]->(sig)
             WITH story, t, collect({
                 sig_id: sig.id,
                 source_url: sig.source_url,
                 strength: r.match_strength,
                 explanation: r.explanation,
                 edge_type: type(r),
                 gathering_type: r.gathering_type
             }) AS new_respondents
             WHERE size(new_respondents) >= 1
             RETURN story.id AS story_id, t.id AS tension_id, new_respondents
             LIMIT $limit"
        )
        .param("limit", limit as i64);

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let story_id_str: String = row.get("story_id").unwrap_or_default();
            let story_id = match Uuid::parse_str(&story_id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };
            let tension_id_str: String = row.get("tension_id").unwrap_or_default();
            let tension_id = match Uuid::parse_str(&tension_id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };

            let respondent_maps: Vec<neo4rs::BoltMap> = row.get("new_respondents").unwrap_or_default();
            let mut new_respondents = Vec::new();
            for map in respondent_maps {
                let sig_id_str = map.get::<String>("sig_id").unwrap_or_default();
                let sig_id = match Uuid::parse_str(&sig_id_str) {
                    Ok(id) => id,
                    Err(_) => continue,
                };
                new_respondents.push(TensionRespondent {
                    signal_id: sig_id,
                    source_url: map.get::<String>("source_url").unwrap_or_default(),
                    match_strength: map.get::<f64>("strength").unwrap_or(0.0),
                    explanation: map.get::<String>("explanation").unwrap_or_default(),
                    edge_type: map.get::<String>("edge_type").unwrap_or_default(),
                    gathering_type: map.get::<String>("gathering_type").ok(),
                });
            }

            results.push(StoryGrowth {
                story_id,
                tension_id,
                new_respondents,
            });
        }
        Ok(results)
    }

    /// Count abandoned signals (curiosity_investigated = 'abandoned').
    /// Used by StoryWeaver for coverage gap reporting.
    pub async fn count_abandoned_signals(&self) -> Result<u32, neo4rs::Error> {
        let q = query(
            "MATCH (n)
             WHERE (n:Give OR n:Event OR n:Ask OR n:Notice)
               AND n.curiosity_investigated = 'abandoned'
             RETURN count(n) AS cnt"
        );
        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let cnt: i64 = row.get("cnt").unwrap_or(0);
            return Ok(cnt as u32);
        }
        Ok(0)
    }

    /// Merge near-duplicate Tension nodes.
    ///
    /// Loads all tension embeddings, finds pairs above `threshold` cosine similarity,
    /// and merges the newer tension into the older one — re-pointing all incoming
    /// RESPONDS_TO edges to the survivor and deleting the duplicate.
    ///
    /// Returns the number of tensions merged (deleted).
    pub async fn merge_duplicate_tensions(&self, threshold: f64) -> Result<u32, neo4rs::Error> {
        // Load all tensions with embeddings
        let q = query(
            "MATCH (t:Tension)
             WHERE t.embedding IS NOT NULL
             RETURN t.id AS id, t.embedding AS embedding, t.extracted_at AS extracted_at
             ORDER BY t.extracted_at ASC"
        );

        struct TensionEmbed {
            id: String,
            embedding: Vec<f64>,
        }

        let mut tensions: Vec<TensionEmbed> = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id: String = row.get("id").unwrap_or_default();
            let embedding: Vec<f64> = row.get("embedding").unwrap_or_default();
            if !embedding.is_empty() {
                tensions.push(TensionEmbed { id, embedding });
            }
        }

        if tensions.len() < 2 {
            return Ok(0);
        }

        // Find pairs to merge (older survives, newer is absorbed)
        let mut to_delete: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut merges: Vec<(String, String)> = Vec::new(); // (survivor, duplicate)

        for i in 0..tensions.len() {
            if to_delete.contains(&tensions[i].id) {
                continue;
            }
            for j in (i + 1)..tensions.len() {
                if to_delete.contains(&tensions[j].id) {
                    continue;
                }
                let sim = cosine_sim_f64(&tensions[i].embedding, &tensions[j].embedding);
                if sim >= threshold {
                    to_delete.insert(tensions[j].id.clone());
                    merges.push((tensions[i].id.clone(), tensions[j].id.clone()));
                }
            }
        }

        // Execute merges
        for (survivor_id, dup_id) in &merges {
            // Re-point RESPONDS_TO edges from duplicate to survivor
            let q = query(
                "MATCH (sig)-[r:RESPONDS_TO]->(dup:Tension {id: $dup_id})
                 MATCH (survivor:Tension {id: $survivor_id})
                 WITH sig, r, survivor, dup
                 WHERE NOT (sig)-[:RESPONDS_TO]->(survivor)
                 CREATE (sig)-[:RESPONDS_TO {match_strength: r.match_strength, explanation: r.explanation}]->(survivor)
                 WITH r, dup
                 DELETE r"
            )
            .param("dup_id", dup_id.as_str())
            .param("survivor_id", survivor_id.as_str());
            self.client.graph.run(q).await?;

            // Re-point DRAWN_TO edges from duplicate to survivor
            let q = query(
                "MATCH (sig)-[r:DRAWN_TO]->(dup:Tension {id: $dup_id})
                 MATCH (survivor:Tension {id: $survivor_id})
                 WITH sig, r, survivor, dup
                 WHERE NOT (sig)-[:DRAWN_TO]->(survivor)
                 CREATE (sig)-[:DRAWN_TO {match_strength: r.match_strength, explanation: r.explanation, gathering_type: r.gathering_type}]->(survivor)
                 WITH r, dup
                 DELETE r"
            )
            .param("dup_id", dup_id.as_str())
            .param("survivor_id", survivor_id.as_str());
            self.client.graph.run(q).await?;

            // Re-point CONTAINS edges from stories
            let q = query(
                "MATCH (s:Story)-[r:CONTAINS]->(dup:Tension {id: $dup_id})
                 MATCH (survivor:Tension {id: $survivor_id})
                 WHERE NOT (s)-[:CONTAINS]->(survivor)
                 CREATE (s)-[:CONTAINS]->(survivor)
                 WITH r
                 DELETE r"
            )
            .param("dup_id", dup_id.as_str())
            .param("survivor_id", survivor_id.as_str());
            self.client.graph.run(q).await?;

            // Bump survivor's corroboration count
            let q = query(
                "MATCH (t:Tension {id: $survivor_id})
                 SET t.corroboration_count = coalesce(t.corroboration_count, 0) + 1"
            )
            .param("survivor_id", survivor_id.as_str());
            self.client.graph.run(q).await?;

            // Delete the duplicate and any remaining edges
            let q = query(
                "MATCH (t:Tension {id: $dup_id}) DETACH DELETE t"
            )
            .param("dup_id", dup_id.as_str());
            self.client.graph.run(q).await?;

            info!(
                survivor_id = survivor_id.as_str(),
                duplicate_id = dup_id.as_str(),
                "Merged duplicate tension"
            );
        }

        Ok(merges.len() as u32)
    }

    // --- Feedback loop methods ---

    /// Update a signal's confidence value. Same label-dispatch as mark_investigated.
    pub async fn update_signal_confidence(
        &self,
        signal_id: Uuid,
        node_type: NodeType,
        new_confidence: f32,
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
             SET n.confidence = $confidence",
            label
        ))
        .param("id", signal_id.to_string())
        .param("confidence", new_confidence as f64);

        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Read current confidence for a signal. Returns 0.5 if not found.
    pub async fn get_signal_confidence(
        &self,
        signal_id: Uuid,
        node_type: NodeType,
    ) -> Result<f32, neo4rs::Error> {
        let label = match node_type {
            NodeType::Event => "Event",
            NodeType::Give => "Give",
            NodeType::Ask => "Ask",
            NodeType::Notice => "Notice",
            NodeType::Tension => "Tension",
            NodeType::Evidence => return Ok(0.5),
        };

        let q = query(&format!(
            "MATCH (n:{} {{id: $id}})
             RETURN n.confidence AS confidence",
            label
        ))
        .param("id", signal_id.to_string());

        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let conf: f64 = row.get("confidence").unwrap_or(0.5);
            return Ok(conf as f32);
        }
        Ok(0.5)
    }

    /// Get all evidence linked to a signal via SOURCED_FROM.
    pub async fn get_evidence_summary(
        &self,
        signal_id: Uuid,
        node_type: NodeType,
    ) -> Result<Vec<EvidenceSummary>, neo4rs::Error> {
        let label = match node_type {
            NodeType::Event => "Event",
            NodeType::Give => "Give",
            NodeType::Ask => "Ask",
            NodeType::Notice => "Notice",
            NodeType::Tension => "Tension",
            NodeType::Evidence => return Ok(Vec::new()),
        };

        let q = query(&format!(
            "MATCH (n:{} {{id: $id}})-[:SOURCED_FROM]->(ev:Evidence)
             RETURN ev.relevance AS relevance, ev.evidence_confidence AS confidence",
            label
        ))
        .param("id", signal_id.to_string());

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let relevance: String = row.get("relevance").unwrap_or_default();
            let confidence: f64 = row.get("confidence").unwrap_or(0.0);
            if !relevance.is_empty() {
                results.push(EvidenceSummary {
                    relevance,
                    confidence: confidence as f32,
                });
            }
        }
        Ok(results)
    }

    /// Get gap_type strategy stats for discovery sources in a city.
    /// Parses gap_type from gap_context ("... | Gap: <type> | ...") in Rust.
    pub async fn get_gap_type_stats(&self, city: &str) -> Result<Vec<GapTypeStats>, neo4rs::Error> {
        let q = query(
            "MATCH (s:Source {city: $city})
             WHERE s.discovery_method IN ['gap_analysis', 'tension_seed']
               AND s.gap_context IS NOT NULL
             RETURN s.gap_context AS gc, s.signals_produced AS sp, s.weight AS weight"
        )
        .param("city", city);

        let mut map: std::collections::HashMap<String, (u32, u32, f64)> = std::collections::HashMap::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let gc: String = row.get("gc").unwrap_or_default();
            let sp: i64 = row.get::<i64>("sp").unwrap_or(0);
            let weight: f64 = row.get("weight").unwrap_or(0.0);

            // Parse gap_type from "... | Gap: <type> | ..."
            let gap_type = gc.find("| Gap: ")
                .and_then(|start| {
                    let after = &gc[start + 7..];
                    let end = after.find(" |").unwrap_or(after.len());
                    let gt = after[..end].trim();
                    if gt.is_empty() { None } else { Some(gt.to_string()) }
                })
                .unwrap_or_else(|| "unknown".to_string());

            let entry = map.entry(gap_type).or_insert((0, 0, 0.0));
            entry.0 += 1; // total
            if sp > 0 { entry.1 += 1; } // successful
            entry.2 += weight; // sum of weights
        }

        let mut results: Vec<GapTypeStats> = map.into_iter()
            .map(|(gap_type, (total, successful, weight_sum))| {
                GapTypeStats {
                    gap_type,
                    total_sources: total,
                    successful_sources: successful,
                    avg_weight: if total > 0 { weight_sum / total as f64 } else { 0.0 },
                }
            })
            .collect();
        results.sort_by(|a, b| b.total_sources.cmp(&a.total_sources));
        Ok(results)
    }

    /// Get extraction yield metrics grouped by source_type for a city.
    pub async fn get_extraction_yield(&self, city: &str) -> Result<Vec<ExtractionYield>, neo4rs::Error> {
        // Base metrics from Source nodes
        let q = query(
            "MATCH (s:Source {city: $city})
             WHERE s.active = true
             RETURN s.source_type AS st, s.signals_produced AS sp,
                    s.signals_corroborated AS sc, s.url AS url"
        )
        .param("city", city);

        let mut type_map: std::collections::HashMap<String, (u32, u32, Vec<String>)> = std::collections::HashMap::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let st: String = row.get("st").unwrap_or_default();
            let sp: i64 = row.get::<i64>("sp").unwrap_or(0);
            let sc: i64 = row.get::<i64>("sc").unwrap_or(0);
            let url: String = row.get("url").unwrap_or_default();

            let entry = type_map.entry(st).or_insert((0, 0, Vec::new()));
            entry.0 += sp as u32; // extracted
            entry.1 += sc as u32; // corroborated
            if !url.is_empty() {
                entry.2.push(url);
            }
        }

        let mut results = Vec::new();
        for (source_type, (extracted, corroborated, urls)) in &type_map {
            // Count survived signals (still in graph) per source type via source_url
            let mut survived = 0u32;
            if !urls.is_empty() {
                for url in urls {
                    let q = query(
                        "MATCH (n)
                         WHERE (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension)
                           AND n.source_url = $url
                         RETURN count(n) AS cnt"
                    )
                    .param("url", url.as_str());

                    let mut stream = self.client.graph.execute(q).await?;
                    if let Some(row) = stream.next().await? {
                        survived += row.get::<i64>("cnt").unwrap_or(0) as u32;
                    }
                }
            }

            // Count contradicted signals per source type
            let mut contradicted = 0u32;
            if !urls.is_empty() {
                for url in urls {
                    let q = query(
                        "MATCH (n)-[:SOURCED_FROM]->(ev:Evidence {relevance: 'CONTRADICTING'})
                         WHERE (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension)
                           AND n.source_url = $url
                         RETURN count(DISTINCT n) AS cnt"
                    )
                    .param("url", url.as_str());

                    let mut stream = self.client.graph.execute(q).await?;
                    if let Some(row) = stream.next().await? {
                        contradicted += row.get::<i64>("cnt").unwrap_or(0) as u32;
                    }
                }
            }

            results.push(ExtractionYield {
                source_type: source_type.clone(),
                extracted: *extracted,
                survived,
                corroborated: *corroborated,
                contradicted,
            });
        }

        results.sort_by(|a, b| b.extracted.cmp(&a.extracted));
        Ok(results)
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

    // =============================================================================
    // Response Scout methods
    // =============================================================================

    /// Find tensions that need response discovery.
    /// Prioritizes tensions with fewer responses and higher cause_heat.
    pub async fn find_response_scout_targets(
        &self,
        limit: u32,
    ) -> Result<Vec<ResponseScoutTarget>, neo4rs::Error> {
        let q = query(
            "MATCH (t:Tension)
             WHERE t.confidence >= 0.5
               AND coalesce(datetime(t.response_scouted_at), datetime('2000-01-01'))
                   < datetime() - duration('P14D')
             OPTIONAL MATCH (t)<-[:RESPONDS_TO]-(r)
             WITH t, count(r) AS response_count
             RETURN t.id AS id, t.title AS title, t.summary AS summary,
                    t.severity AS severity, t.category AS category,
                    t.what_would_help AS what_would_help,
                    coalesce(t.cause_heat, 0.0) AS cause_heat,
                    response_count
             ORDER BY response_count ASC, t.cause_heat DESC, t.confidence DESC
             LIMIT $limit",
        )
        .param("limit", limit as i64);

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let Ok(tension_id) = Uuid::parse_str(&id_str) else {
                continue;
            };
            results.push(ResponseScoutTarget {
                tension_id,
                title: row.get("title").unwrap_or_default(),
                summary: row.get("summary").unwrap_or_default(),
                severity: row.get("severity").unwrap_or_default(),
                category: {
                    let s: String = row.get("category").unwrap_or_default();
                    if s.is_empty() { None } else { Some(s) }
                },
                what_would_help: {
                    let s: String = row.get("what_would_help").unwrap_or_default();
                    if s.is_empty() { None } else { Some(s) }
                },
                cause_heat: row.get("cause_heat").unwrap_or(0.0),
                response_count: {
                    let c: i64 = row.get("response_count").unwrap_or(0);
                    c as u32
                },
            });
        }
        Ok(results)
    }

    /// Fetch existing responses for a tension (used as heuristics in the response scout prompt).
    pub async fn get_existing_responses(
        &self,
        tension_id: Uuid,
    ) -> Result<Vec<ResponseHeuristic>, neo4rs::Error> {
        let q = query(
            "MATCH (r)-[:RESPONDS_TO]->(t:Tension {id: $id})
             WHERE r:Give OR r:Event OR r:Ask
             RETURN r.title AS title, r.summary AS summary, labels(r)[0] AS label
             LIMIT 5",
        )
        .param("id", tension_id.to_string());

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            results.push(ResponseHeuristic {
                title: row.get("title").unwrap_or_default(),
                summary: row.get("summary").unwrap_or_default(),
                signal_type: row.get("label").unwrap_or_default(),
            });
        }
        Ok(results)
    }

    /// Mark a tension as having been scouted for responses.
    pub async fn mark_response_scouted(
        &self,
        tension_id: Uuid,
    ) -> Result<(), neo4rs::Error> {
        let now = format_datetime(&Utc::now());
        let q = query(
            "MATCH (t:Tension {id: $id})
             SET t.response_scouted_at = $now",
        )
        .param("id", tension_id.to_string())
        .param("now", now);

        self.client.graph.run(q).await
    }

    // =============================================================================
    // Gravity Scout operations
    // =============================================================================

    /// Find tensions with active heat that need gravity scouting.
    /// Requires cause_heat >= 0.1 (cold tensions don't create gatherings).
    /// Uses exponential backoff based on consecutive miss count.
    pub async fn find_gravity_scout_targets(
        &self,
        limit: u32,
    ) -> Result<Vec<GravityScoutTarget>, neo4rs::Error> {
        let q = query(
            "MATCH (t:Tension)
             WHERE t.confidence >= 0.5
               AND coalesce(t.cause_heat, 0.0) >= 0.1
               AND coalesce(datetime(t.gravity_scouted_at), datetime('2000-01-01'))
                   < datetime() - duration({days:
                       CASE
                         WHEN coalesce(t.gravity_scout_miss_count, 0) = 0 THEN 7
                         WHEN coalesce(t.gravity_scout_miss_count, 0) = 1 THEN 14
                         WHEN coalesce(t.gravity_scout_miss_count, 0) = 2 THEN 21
                         ELSE 30
                       END
                     })
             RETURN t.id AS id, t.title AS title, t.summary AS summary,
                    t.severity AS severity, t.category AS category,
                    t.what_would_help AS what_would_help,
                    coalesce(t.cause_heat, 0.0) AS cause_heat
             ORDER BY t.cause_heat DESC, t.confidence DESC
             LIMIT $limit",
        )
        .param("limit", limit as i64);

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let Ok(tension_id) = Uuid::parse_str(&id_str) else {
                continue;
            };
            results.push(GravityScoutTarget {
                tension_id,
                title: row.get("title").unwrap_or_default(),
                summary: row.get("summary").unwrap_or_default(),
                severity: row.get("severity").unwrap_or_default(),
                category: {
                    let s: String = row.get("category").unwrap_or_default();
                    if s.is_empty() { None } else { Some(s) }
                },
                what_would_help: {
                    let s: String = row.get("what_would_help").unwrap_or_default();
                    if s.is_empty() { None } else { Some(s) }
                },
                cause_heat: row.get("cause_heat").unwrap_or(0.0),
            });
        }
        Ok(results)
    }

    /// Fetch existing gravity signals for a tension (gatherings wired via DRAWN_TO),
    /// filtered to signals within `radius_km` of the given city center.
    pub async fn get_existing_gravity_signals(
        &self,
        tension_id: Uuid,
        city_lat: f64,
        city_lng: f64,
        radius_km: f64,
    ) -> Result<Vec<ResponseHeuristic>, neo4rs::Error> {
        let lat_delta = radius_km / 111.0;
        let lng_delta = radius_km / (111.0 * city_lat.to_radians().cos());
        let q = query(
            "MATCH (r)-[rel:DRAWN_TO]->(t:Tension {id: $id})
             WHERE (r:Give OR r:Event OR r:Ask)
               AND r.lat >= $lat_min AND r.lat <= $lat_max
               AND r.lng >= $lng_min AND r.lng <= $lng_max
             RETURN r.title AS title, r.summary AS summary, labels(r)[0] AS label
             LIMIT 5",
        )
        .param("id", tension_id.to_string())
        .param("lat_min", city_lat - lat_delta)
        .param("lat_max", city_lat + lat_delta)
        .param("lng_min", city_lng - lng_delta)
        .param("lng_max", city_lng + lng_delta);

        let mut results = Vec::new();
        let mut stream = self.client.graph.execute(q).await?;
        while let Some(row) = stream.next().await? {
            results.push(ResponseHeuristic {
                title: row.get("title").unwrap_or_default(),
                summary: row.get("summary").unwrap_or_default(),
                signal_type: row.get("label").unwrap_or_default(),
            });
        }
        Ok(results)
    }

    /// Mark a tension as having been gravity-scouted.
    /// Resets miss_count to 0 on success, increments on failure.
    pub async fn mark_gravity_scouted(
        &self,
        tension_id: Uuid,
        found_gatherings: bool,
    ) -> Result<(), neo4rs::Error> {
        let now = format_datetime(&Utc::now());
        let q = query(
            "MATCH (t:Tension {id: $id})
             SET t.gravity_scouted_at = datetime($now),
                 t.gravity_scout_miss_count = CASE
                     WHEN $found THEN 0
                     ELSE coalesce(t.gravity_scout_miss_count, 0) + 1
                 END",
        )
        .param("id", tension_id.to_string())
        .param("now", now)
        .param("found", found_gatherings);

        self.client.graph.run(q).await
    }

    /// Create a DRAWN_TO edge between a gathering signal and a Tension.
    /// Uses MERGE with ON CREATE/ON MATCH for defensive idempotency.
    pub async fn create_drawn_to_edge(
        &self,
        signal_id: Uuid,
        tension_id: Uuid,
        match_strength: f64,
        explanation: &str,
        gathering_type: &str,
    ) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (resp) WHERE resp.id = $resp_id AND (resp:Give OR resp:Event OR resp:Ask)
             MATCH (t:Tension {id: $tension_id})
             MERGE (resp)-[r:DRAWN_TO]->(t)
             ON CREATE SET
                 r.match_strength = $strength,
                 r.explanation = $explanation,
                 r.gathering_type = $gathering_type
             ON MATCH SET
                 r.match_strength = $strength,
                 r.explanation = $explanation,
                 r.gathering_type = $gathering_type",
        )
        .param("resp_id", signal_id.to_string())
        .param("tension_id", tension_id.to_string())
        .param("strength", match_strength)
        .param("explanation", explanation)
        .param("gathering_type", gathering_type);

        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Find or create a Place node, deduplicating on (slug, city).
    /// Returns the Place's UUID (existing or newly created).
    pub async fn find_or_create_place(
        &self,
        name: &str,
        city: &str,
        lat: f64,
        lng: f64,
    ) -> Result<Uuid, neo4rs::Error> {
        let slug = rootsignal_common::slugify(name);
        let new_id = Uuid::new_v4();
        let now = format_datetime(&Utc::now());

        let q = query(
            "MERGE (p:Place {slug: $slug, city: $city})
             ON CREATE SET
                 p.id = $id,
                 p.name = $name,
                 p.lat = $lat,
                 p.lng = $lng,
                 p.geocoded = false,
                 p.created_at = datetime($now)
             RETURN p.id AS place_id",
        )
        .param("slug", slug.as_str())
        .param("city", city)
        .param("id", new_id.to_string())
        .param("name", name)
        .param("lat", lat)
        .param("lng", lng)
        .param("now", now);

        let mut stream = self.client.graph.execute(q).await?;
        if let Some(row) = stream.next().await? {
            let id_str: String = row.get("place_id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                return Ok(id);
            }
        }
        // Fallback: if MERGE returned nothing (shouldn't happen), return the new_id
        Ok(new_id)
    }

    /// Create a GATHERS_AT edge from a gathering signal to a Place.
    pub async fn create_gathers_at_edge(
        &self,
        signal_id: Uuid,
        place_id: Uuid,
    ) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (s) WHERE s.id = $sid AND (s:Give OR s:Event OR s:Ask)
             MATCH (p:Place {id: $pid})
             MERGE (s)-[:GATHERS_AT]->(p)",
        )
        .param("sid", signal_id.to_string())
        .param("pid", place_id.to_string());

        self.client.graph.run(q).await?;
        Ok(())
    }

    /// Refresh a signal's `last_confirmed_active` timestamp by ID alone.
    /// Used by gravity scout on the dedup path to prevent recurring gatherings from aging out.
    pub async fn touch_signal_timestamp(
        &self,
        signal_id: Uuid,
    ) -> Result<(), neo4rs::Error> {
        let now = format_datetime(&Utc::now());
        let q = query(
            "MATCH (n)
             WHERE n.id = $id AND (n:Give OR n:Event OR n:Ask)
             SET n.last_confirmed_active = datetime($now)",
        )
        .param("id", signal_id.to_string())
        .param("now", now);

        self.client.graph.run(q).await?;
        Ok(())
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

// --- Discovery briefing types ---

/// A tension with its response coverage status.
#[derive(Debug, Clone)]
pub struct UnmetTension {
    pub title: String,
    pub severity: String,
    pub what_would_help: Option<String>,
    pub category: Option<String>,
    pub unmet: bool,
    pub corroboration_count: u32,
    pub source_diversity: u32,
    pub cause_heat: f64,
}

/// A brief summary of a story for the discovery briefing.
#[derive(Debug, Clone)]
pub struct StoryBrief {
    pub headline: String,
    pub arc: Option<String>,
    pub energy: f64,
    pub signal_count: u32,
    pub type_diversity: u32,
    pub dominant_type: String,
    pub source_count: u32,
}

/// Aggregate counts of each signal type.
#[derive(Debug, Clone, Default)]
pub struct SignalTypeCounts {
    pub events: u32,
    pub gives: u32,
    pub asks: u32,
    pub notices: u32,
    pub tensions: u32,
}

/// A brief summary of a source for discovery performance tracking.
#[derive(Debug, Clone)]
pub struct SourceBrief {
    pub canonical_value: String,
    pub signals_produced: u32,
    pub weight: f64,
    pub consecutive_empty_runs: u32,
    pub gap_context: Option<String>,
    pub active: bool,
}

/// Evidence linked to a signal — used for confidence revision.
#[derive(Debug, Clone)]
pub struct EvidenceSummary {
    pub relevance: String,    // "DIRECT", "SUPPORTING", "CONTRADICTING"
    pub confidence: f32,
}

/// Aggregated stats for a gap_type strategy.
#[derive(Debug, Clone)]
pub struct GapTypeStats {
    pub gap_type: String,
    pub total_sources: u32,
    pub successful_sources: u32,  // signals_produced > 0
    pub avg_weight: f64,
}

/// Extraction yield metrics grouped by source_type.
#[derive(Debug, Clone)]
pub struct ExtractionYield {
    pub source_type: String,
    pub extracted: u32,      // from Source.signals_produced
    pub survived: u32,       // count of signals still in graph
    pub corroborated: u32,   // from Source.signals_corroborated
    pub contradicted: u32,   // signals with CONTRADICTING evidence
}

/// Response shape analysis for a tension — what types of responses exist and what's absent.
#[derive(Debug, Clone)]
pub struct TensionResponseShape {
    pub title: String,
    pub what_would_help: Option<String>,
    pub cause_heat: f64,
    pub give_count: u32,
    pub event_count: u32,
    pub ask_count: u32,
    pub sample_titles: Vec<String>,
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

/// A signal without tension context that the curiosity loop should investigate.
#[derive(Debug)]
pub struct CuriosityTarget {
    pub signal_id: Uuid,
    pub title: String,
    pub summary: String,
    pub label: String,
    pub source_url: String,
}

/// Outcome of a curiosity investigation for a signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CuriosityOutcome {
    /// All tensions processed successfully.
    Done,
    /// LLM said "not curious" — permanent, won't retry.
    Skipped,
    /// Investigation or tension processing failed — eligible for retry.
    Failed,
    /// Retry cap hit (3 attempts) — permanent, signals a coverage gap.
    Abandoned,
}

impl CuriosityOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
            Self::Abandoned => "abandoned",
        }
    }
}

/// A tension hub: a Tension node with 2+ responding signals, ready to materialize as a Story.
#[derive(Debug)]
pub struct TensionHub {
    pub tension_id: Uuid,
    pub title: String,
    pub summary: String,
    pub category: Option<String>,
    pub what_would_help: Option<String>,
    pub respondents: Vec<TensionRespondent>,
}

/// A signal that responds to a tension, with edge metadata.
#[derive(Debug)]
pub struct TensionRespondent {
    pub signal_id: Uuid,
    pub source_url: String,
    pub match_strength: f64,
    pub explanation: String,
    /// "RESPONDS_TO" or "DRAWN_TO" — raw Neo4j type(r) value
    pub edge_type: String,
    /// Only present for DRAWN_TO edges
    pub gathering_type: Option<String>,
}

/// New respondent signals for an existing story (not yet linked via CONTAINS).
#[derive(Debug)]
pub struct StoryGrowth {
    pub story_id: Uuid,
    pub tension_id: Uuid,
    pub new_respondents: Vec<TensionRespondent>,
}

// --- Response Scout types ---

/// A tension that needs response discovery.
#[derive(Debug)]
pub struct ResponseScoutTarget {
    pub tension_id: Uuid,
    pub title: String,
    pub summary: String,
    pub severity: String,
    pub category: Option<String>,
    pub what_would_help: Option<String>,
    pub cause_heat: f64,
    pub response_count: u32,
}

/// An existing response signal used as a heuristic hint.
#[derive(Debug)]
pub struct ResponseHeuristic {
    pub title: String,
    pub summary: String,
    pub signal_type: String,
}

// --- Gravity Scout types ---

/// A tension that needs gravity scouting (where are people gathering?).
#[derive(Debug)]
pub struct GravityScoutTarget {
    pub tension_id: Uuid,
    pub title: String,
    pub summary: String,
    pub severity: String,
    pub category: Option<String>,
    pub what_would_help: Option<String>,
    pub cause_heat: f64,
}

/// Add lat/lng params to a query from node metadata.
/// Uses null for nodes without a location.
fn add_location_params(q: neo4rs::Query, meta: &NodeMeta) -> neo4rs::Query {
    match &meta.location {
        Some(loc) => q.param("lat", loc.lat).param("lng", loc.lng),
        None => q.param::<Option<f64>>("lat", None).param::<Option<f64>>("lng", None),
    }
}

fn cosine_sim_f64(a: &[f64], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
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
/// Neo4j's datetime() requires "YYYY-MM-DDThh:mm:ss" format (no +00:00 suffix).
fn format_datetime(dt: &DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
}

/// Public version of format_datetime for use by other modules (e.g. cluster.rs).
pub fn format_datetime_pub(dt: &DateTime<Utc>) -> String {
    format_datetime(dt)
}

// Backwards-compatible aliases
pub use format_datetime_pub as memgraph_datetime_pub;

/// Parse a datetime string back into a DateTime<Utc>.
/// Returns None for empty strings or parse failures.
fn parse_datetime_opt(s: &str) -> Option<DateTime<Utc>> {
    if s.is_empty() {
        return None;
    }
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
        .map(|ndt| ndt.and_utc())
        .ok()
}

/// Read an optional datetime from a neo4rs Row, handling both Neo4j DateTime types
/// (stored via Cypher `datetime()`) and plain string values.
fn row_datetime_opt(row: &neo4rs::Row, key: &str) -> Option<DateTime<Utc>> {
    // Try Neo4j DateTime type (stored with `datetime()` in Cypher → BoltType::DateTime)
    if let Ok(dt) = row.get::<chrono::DateTime<chrono::FixedOffset>>(key) {
        return Some(dt.with_timezone(&Utc));
    }
    // Try NaiveDateTime (BoltType::LocalDateTime)
    if let Ok(ndt) = row.get::<chrono::NaiveDateTime>(key) {
        return Some(ndt.and_utc());
    }
    // Fall back to string parsing (legacy or manually stored values)
    row.get::<String>(key).ok().and_then(|s| parse_datetime_opt(&s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_sim_identical_vectors() {
        let v = vec![1.0, 2.0, 3.0];
        let sim = cosine_sim_f64(&v, &v);
        assert!((sim - 1.0).abs() < 1e-10);
    }

    #[test]
    fn cosine_sim_orthogonal_vectors() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!(cosine_sim_f64(&a, &b).abs() < 1e-10);
    }

    #[test]
    fn cosine_sim_zero_vector() {
        let a = vec![1.0, 2.0];
        let b = vec![0.0, 0.0];
        assert_eq!(cosine_sim_f64(&a, &b), 0.0);
    }

    #[test]
    fn cosine_sim_similar_vectors_above_threshold() {
        // Two nearly identical vectors should be > 0.85
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.98, 0.15, 0.0];
        let sim = cosine_sim_f64(&a, &b);
        assert!(sim > 0.85, "Expected > 0.85, got {sim}");
    }

    #[test]
    fn cosine_sim_dissimilar_vectors_below_threshold() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = cosine_sim_f64(&a, &b);
        assert!(sim < 0.85, "Expected < 0.85, got {sim}");
    }

    // --- CuriosityOutcome tests ---

    #[test]
    fn curiosity_outcome_as_str_roundtrip() {
        assert_eq!(CuriosityOutcome::Done.as_str(), "done");
        assert_eq!(CuriosityOutcome::Skipped.as_str(), "skipped");
        assert_eq!(CuriosityOutcome::Failed.as_str(), "failed");
        assert_eq!(CuriosityOutcome::Abandoned.as_str(), "abandoned");
    }

    #[test]
    fn curiosity_outcome_equality() {
        assert_eq!(CuriosityOutcome::Done, CuriosityOutcome::Done);
        assert_ne!(CuriosityOutcome::Done, CuriosityOutcome::Failed);
        assert_ne!(CuriosityOutcome::Failed, CuriosityOutcome::Abandoned);
    }

    #[test]
    fn curiosity_outcome_is_copy() {
        let outcome = CuriosityOutcome::Failed;
        let copied = outcome; // Copy
        assert_eq!(outcome, copied); // Both still usable
    }

    // --- TensionHub / TensionRespondent tests ---

    #[test]
    fn tension_hub_respondent_count() {
        let hub = TensionHub {
            tension_id: Uuid::new_v4(),
            title: "Housing affordability crisis".to_string(),
            summary: "Rents rising faster than wages".to_string(),
            category: Some("housing".to_string()),
            what_would_help: Some("Rent stabilization policies".to_string()),
            respondents: vec![
                TensionRespondent {
                    signal_id: Uuid::new_v4(),
                    source_url: "https://example.com/a".to_string(),
                    match_strength: 0.9,
                    explanation: "Direct evidence of rent increases".to_string(),
                    edge_type: "RESPONDS_TO".to_string(),
                    gathering_type: None,
                },
                TensionRespondent {
                    signal_id: Uuid::new_v4(),
                    source_url: "https://different.org/b".to_string(),
                    match_strength: 0.7,
                    explanation: "Community response to housing costs".to_string(),
                    edge_type: "DRAWN_TO".to_string(),
                    gathering_type: Some("vigil".to_string()),
                },
            ],
        };

        assert_eq!(hub.respondents.len(), 2);
        assert!(hub.respondents[0].match_strength > hub.respondents[1].match_strength);
        assert_eq!(hub.category.as_deref(), Some("housing"));
    }

    #[test]
    fn story_growth_tracks_new_respondents() {
        let growth = StoryGrowth {
            story_id: Uuid::new_v4(),
            tension_id: Uuid::new_v4(),
            new_respondents: vec![TensionRespondent {
                signal_id: Uuid::new_v4(),
                source_url: "https://new-source.org".to_string(),
                match_strength: 0.85,
                explanation: "New evidence from a different source".to_string(),
                edge_type: "RESPONDS_TO".to_string(),
                gathering_type: None,
            }],
        };

        assert_eq!(growth.new_respondents.len(), 1);
        assert!(growth.new_respondents[0].match_strength >= 0.85);
    }
}
