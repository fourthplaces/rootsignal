use std::collections::HashSet;

use chrono::{DateTime, Utc};
use neo4rs::query;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{
    ActorNode, DemandSignal, DiscoveryMethod, NodeType, PinNode, Region,
    SourceNode, SourceRole, FRESHNESS_MAX_DAYS,
    GATHERING_PAST_GRACE_HOURS, NEED_EXPIRE_DAYS, NOTICE_EXPIRE_DAYS,
};
use crate::GraphClient;
use std::collections::HashMap;
pub use format_datetime_pub as memgraph_datetime_pub;

/// Pipe-separated location edge types for Cypher MATCH patterns.
const LOC_EDGES: &str = "HELD_AT|AVAILABLE_AT|NEEDED_AT|RELEVANT_TO|AFFECTS|OBSERVED_AT|REFERENCES_LOCATION";

/// Returns a Cypher EXISTS subquery for bounding-box filtering through Location edges.
fn bbox_exists(node_var: &str) -> String {
    format!(
        "EXISTS {{
           MATCH ({node_var})-[:{LOC_EDGES}]->(l:Location)
           WHERE l.lat >= $min_lat AND l.lat <= $max_lat
             AND l.lng >= $min_lng AND l.lng <= $max_lng
         }}"
    )
}

/// Read-only graph access for handlers and activities.
#[derive(Clone)]
pub struct GraphReader {
    client: GraphClient,
}

impl GraphReader {
    pub fn new(client: GraphClient) -> Self {
        Self { client }
    }

    /// Access the underlying GraphClient (for read queries).
    pub fn client(&self) -> &GraphClient {
        &self.client
    }

    /// Check if a Source node with the given URL already exists in the graph.
    pub async fn source_exists(&self, url: &str) -> Result<bool, neo4rs::Error> {
        let q = query(
            "OPTIONAL MATCH (s:Source {url: $url})
             RETURN count(s) > 0 AS exists",
        )
        .param("url", url);

        let mut stream = self.client().execute(q).await?;
        if let Some(row) = stream.next().await? {
            let exists: bool = row.get("exists").unwrap_or(false);
            Ok(exists)
        } else {
            Ok(false)
        }
    }

    /// Find a duplicate signal by vector similarity across all signal types,
    /// scoped to a geographic bounding box. Returns the best match (highest
    /// similarity) above threshold within the bbox.
    pub async fn find_duplicate(
        &self,
        embedding: &[f32],
        _primary_type: NodeType,
        threshold: f64,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Option<DuplicateMatch>, neo4rs::Error> {
        let mut best: Option<DuplicateMatch> = None;

        for nt in &[
            NodeType::Gathering,
            NodeType::Resource,
            NodeType::HelpRequest,
            NodeType::Announcement,
            NodeType::Concern,
        ] {
            if let Some(m) = self
                .vector_search(
                    *nt, embedding, threshold, min_lat, max_lat, min_lng, max_lng,
                )
                .await?
            {
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
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Option<DuplicateMatch>, neo4rs::Error> {
        let index_name = match node_type {
            NodeType::Gathering => "gathering_embedding",
            NodeType::Resource => "aid_embedding",
            NodeType::HelpRequest => "need_embedding",
            NodeType::Announcement => "notice_embedding",
            NodeType::Concern => "concern_embedding",
            NodeType::Condition => "condition_embedding",
            NodeType::Citation => return Ok(None),
        };

        let bbox = bbox_exists("node");
        let q = query(&format!(
            "CALL db.index.vector.queryNodes('{}', 10, $embedding)
             YIELD node, score AS similarity
             WHERE {bbox}
             OPTIONAL MATCH (node)-[:PRODUCED_BY]->(s:Source)
             RETURN node.id AS id, node.url AS url, s.canonical_key AS canonical_key, similarity
             ORDER BY similarity DESC
             LIMIT 1",
            index_name
        ))
        .param("embedding", embedding_to_f64(embedding))
        .param("min_lat", min_lat)
        .param("max_lat", max_lat)
        .param("min_lng", min_lng)
        .param("max_lng", max_lng);

        let mut stream = self.client().execute(q).await?;
        if let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let similarity: f64 = row.get("similarity").unwrap_or(0.0);
            let url: String = row.get("url").unwrap_or_default();
            let canonical_key: String = row.get("canonical_key").unwrap_or_default();
            if similarity >= threshold {
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    return Ok(Some(DuplicateMatch {
                        id,
                        node_type,
                        url,
                        canonical_key,
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
            "MATCH (ev:Citation {content_hash: $hash, source_url: $url})
             RETURN ev LIMIT 1",
        )
        .param("hash", content_hash)
        .param("url", source_url);

        let mut stream = self.client().execute(q).await?;
        Ok(stream.next().await?.is_some())
    }

    /// Return titles of existing signals from a given source URL.
    /// Used for cheap pre-filtering before expensive embedding-based dedup.
    pub async fn existing_titles_for_url(
        &self,
        source_url: &str,
    ) -> Result<Vec<String>, neo4rs::Error> {
        let q = query(
            "MATCH (n)
             WHERE (n:Gathering OR n:Resource OR n:HelpRequest OR n:Announcement OR n:Concern OR n:Condition)
               AND n.url = $url
             RETURN n.title AS title",
        )
        .param("url", source_url);

        let mut titles = Vec::new();
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Ok(title) = row.get::<String>("title") {
                titles.push(title);
            }
        }
        Ok(titles)
    }

    /// Batch-find existing signals by exact title+type (case-insensitive).
    /// Returns a map of lowercase title → (node_id, node_type, canonical_key).
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
        for nt in &[
            NodeType::Gathering,
            NodeType::Resource,
            NodeType::HelpRequest,
            NodeType::Announcement,
            NodeType::Concern,
            NodeType::Condition,
        ] {
            let label = match nt {
                NodeType::Gathering => "Gathering",
                NodeType::Resource => "Resource",
                NodeType::HelpRequest => "HelpRequest",
                NodeType::Announcement => "Announcement",
                NodeType::Concern => "Concern",
                NodeType::Condition => "Condition",
                NodeType::Citation => continue,
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
                 OPTIONAL MATCH (n)-[:PRODUCED_BY]->(s:Source)
                 RETURN toLower(n.title) AS title, n.id AS id, s.canonical_key AS canonical_key"
            ))
            .param("titles", titles_for_type);

            let mut stream = self.client().execute(q).await?;
            while let Some(row) = stream.next().await? {
                let title: String = row.get("title").unwrap_or_default();
                let id_str: String = row.get("id").unwrap_or_default();
                let canonical_key: String = row.get("canonical_key").unwrap_or_default();
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    results.insert((title, *nt), (id, canonical_key));
                }
            }
        }

        Ok(results)
    }

    /// Batch-find existing signals by exact (url, node_type) fingerprint.
    /// Catches re-encounters of the same post without needing embeddings.
    pub async fn find_by_fingerprints(
        &self,
        pairs: &[(String, NodeType)],
    ) -> Result<std::collections::HashMap<(String, NodeType), (Uuid, String, Option<Vec<f32>>)>, neo4rs::Error> {
        let mut results = std::collections::HashMap::new();
        if pairs.is_empty() {
            return Ok(results);
        }

        for nt in &[
            NodeType::Gathering,
            NodeType::Resource,
            NodeType::HelpRequest,
            NodeType::Announcement,
            NodeType::Concern,
            NodeType::Condition,
        ] {
            let label = match nt {
                NodeType::Gathering => "Gathering",
                NodeType::Resource => "Resource",
                NodeType::HelpRequest => "HelpRequest",
                NodeType::Announcement => "Announcement",
                NodeType::Concern => "Concern",
                NodeType::Condition => "Condition",
                NodeType::Citation => continue,
            };

            let urls_for_type: Vec<String> = pairs
                .iter()
                .filter(|(_, t)| t == nt)
                .map(|(url, _)| url.clone())
                .collect();

            if urls_for_type.is_empty() {
                continue;
            }

            let q = query(&format!(
                "MATCH (n:{label})
                 WHERE n.url IN $urls
                 OPTIONAL MATCH (n)-[:PRODUCED_BY]->(s:Source)
                 RETURN n.url AS url, n.id AS id, s.canonical_key AS canonical_key, n.embedding AS embedding"
            ))
            .param("urls", urls_for_type);

            let mut stream = self.client().execute(q).await?;
            while let Some(row) = stream.next().await? {
                let url: String = row.get("url").unwrap_or_default();
                let id_str: String = row.get("id").unwrap_or_default();
                let canonical_key: String = row.get("canonical_key").unwrap_or_default();
                let embedding: Option<Vec<f32>> = row.get::<Vec<f64>>("embedding")
                    .ok()
                    .map(|v| v.into_iter().map(|x| x as f32).collect());
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    results.insert((url, *nt), (id, canonical_key, embedding));
                }
            }
        }

        Ok(results)
    }

    /// Compute source diversity and external ratio for a signal from its evidence nodes.
    pub async fn compute_source_diversity(
        &self,
        node_id: Uuid,
        node_type: NodeType,
        entity_mappings: &[rootsignal_common::EntityMappingOwned],
    ) -> Result<u32, neo4rs::Error> {
        let label = match node_type {
            NodeType::Gathering => "Gathering",
            NodeType::Resource => "Resource",
            NodeType::HelpRequest => "HelpRequest",
            NodeType::Announcement => "Announcement",
            NodeType::Concern => "Concern",
            NodeType::Condition => "Condition",
            NodeType::Citation => return Ok(1),
        };

        let q = query(&format!(
            "MATCH (n:{label} {{id: $id}})
             OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Citation)
             RETURN collect(ev.source_url) AS evidence_urls"
        ))
        .param("id", node_id.to_string());

        let mut stream = self.client().execute(q).await?;
        if let Some(row) = stream.next().await? {
            let evidence_urls: Vec<String> = row.get("evidence_urls").unwrap_or_default();

            let mut entities = std::collections::HashSet::new();
            for url in &evidence_urls {
                entities.insert(rootsignal_common::resolve_entity(url, entity_mappings));
            }

            Ok(entities.len().max(1) as u32)
        } else {
            Ok(1)
        }
    }

    /// Compute channel diversity for a signal from its evidence nodes.
    /// Entity-gated: only distinct (entity, channel_type) pairs count, and only
    /// channels with at least one *external* entity (different from the originating
    /// entity) are counted.
    pub async fn compute_channel_diversity(
        &self,
        node_id: Uuid,
        node_type: NodeType,
        entity_mappings: &[rootsignal_common::EntityMappingOwned],
    ) -> Result<u32, neo4rs::Error> {
        let label = match node_type {
            NodeType::Gathering => "Gathering",
            NodeType::Resource => "Resource",
            NodeType::HelpRequest => "HelpRequest",
            NodeType::Announcement => "Announcement",
            NodeType::Concern => "Concern",
            NodeType::Condition => "Condition",
            NodeType::Citation => return Ok(1),
        };

        let q = query(&format!(
            "MATCH (n:{label} {{id: $id}})
             OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Citation)
             RETURN n.url AS self_url,
                    collect({{url: ev.source_url, channel: coalesce(ev.channel_type, 'press')}}) AS evidence"
        ))
        .param("id", node_id.to_string());

        let mut stream = self.client().execute(q).await?;
        if let Some(row) = stream.next().await? {
            let self_url: String = row.get("self_url").unwrap_or_default();
            let evidence: Vec<neo4rs::BoltMap> = row.get("evidence").unwrap_or_default();

            let self_entity = rootsignal_common::resolve_entity(&self_url, entity_mappings);

            // Collect (entity, channel_type) pairs
            let mut channels_with_external: HashSet<String> = HashSet::new();
            for ev in &evidence {
                let url: String = ev.get::<String>("url").unwrap_or_default();
                let channel: String = ev
                    .get::<String>("channel")
                    .unwrap_or_else(|_| "press".to_string());
                if url.is_empty() {
                    continue;
                }
                let entity = rootsignal_common::resolve_entity(&url, entity_mappings);
                if entity != self_entity {
                    channels_with_external.insert(channel);
                }
            }

            Ok(channels_with_external.len().max(1) as u32)
        } else {
            Ok(1)
        }
    }


    /// Count sources that are overdue for scraping.
    pub async fn count_due_sources(&self) -> Result<u32, neo4rs::Error> {
        let q = query(
            "MATCH (s:Source {active: true})
             WHERE s.last_scraped IS NULL
                OR datetime(s.last_scraped) + duration('PT' + toString(coalesce(s.cadence_hours, 24)) + 'H') < datetime()
             RETURN count(s) AS due"
        );

        let mut result = self.client().execute(q).await?;
        if let Some(row) = result.next().await? {
            let due: i64 = row.get("due").unwrap_or(0);
            return Ok(due as u32);
        }
        Ok(0)
    }

    /// Batch count due sources for multiple cities in a single query.
    pub async fn batch_due_sources(
        &self,
        slugs: &[String],
    ) -> Result<std::collections::HashMap<String, u32>, neo4rs::Error> {
        let mut map = std::collections::HashMap::new();
        if slugs.is_empty() {
            return Ok(map);
        }

        let q = query(
            "UNWIND $slugs AS slug
             OPTIONAL MATCH (s:Source {active: true})
             WHERE s.last_scraped IS NULL
                OR datetime(s.last_scraped) + duration('PT' + toString(coalesce(s.cadence_hours, 24)) + 'H') < datetime()
             RETURN slug, count(s) AS due",
        )
        .param("slugs", slugs.to_vec());

        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            let slug: String = row.get("slug").unwrap_or_default();
            let due: i64 = row.get("due").unwrap_or(0);
            map.insert(slug, due as u32);
        }

        Ok(map)
    }

    /// Get the earliest time a source becomes due for scraping.
    pub async fn next_source_due(&self) -> Result<Option<chrono::DateTime<Utc>>, neo4rs::Error> {
        let q = query(
            "MATCH (s:Source {active: true})
             WHERE s.last_scraped IS NOT NULL
             RETURN min(datetime(s.last_scraped) + duration('PT' + toString(coalesce(s.cadence_hours, 24)) + 'H')) AS next_due"
        );

        let mut result = self.client().execute(q).await?;
        if let Some(row) = result.next().await? {
            let next_due_str: String = row.get("next_due").unwrap_or_default();
            if !next_due_str.is_empty() {
                if let Ok(ndt) =
                    chrono::NaiveDateTime::parse_from_str(&next_due_str, "%Y-%m-%dT%H:%M:%S%.f")
                {
                    return Ok(Some(ndt.and_utc()));
                }
            }
        }
        Ok(None)
    }

    // --- Region operations ---

    /// Get a Region by id.
    pub async fn get_region(&self, id: &str) -> Result<Option<Region>, neo4rs::Error> {
        let q = query(
            "MATCH (r:Region {id: $id})
             RETURN r.id AS id, r.name AS name,
                    r.center_lat AS center_lat, r.center_lng AS center_lng,
                    r.radius_km AS radius_km, r.geo_terms AS geo_terms,
                    r.is_leaf AS is_leaf, r.created_at AS created_at",
        )
        .param("id", id);

        let mut stream = self.client().execute(q).await?;
        if let Some(row) = stream.next().await? {
            Ok(Some(row_to_region(&row)))
        } else {
            Ok(None)
        }
    }

    /// Get the region that WATCHES a source (if any).
    pub async fn get_region_for_source(&self, source_id: &str) -> Result<Option<Region>, neo4rs::Error> {
        let q = query(
            "MATCH (r:Region)-[:WATCHES]->(s:Source {id: $id})
             RETURN r.id AS id, r.name AS name,
                    r.center_lat AS center_lat, r.center_lng AS center_lng,
                    r.radius_km AS radius_km, r.geo_terms AS geo_terms,
                    r.is_leaf AS is_leaf, r.created_at AS created_at
             LIMIT 1",
        )
        .param("id", source_id);

        let mut stream = self.client().execute(q).await?;
        if let Some(row) = stream.next().await? {
            Ok(Some(row_to_region(&row)))
        } else {
            Ok(None)
        }
    }

    /// Get the region for a signal (via Source WATCHES edge).
    pub async fn get_region_for_signal(&self, signal_id: &str) -> Result<Option<Region>, neo4rs::Error> {
        let q = query(
            "MATCH (s:Signal {id: $id})<-[:SCRAPE_PRODUCED]-(src:Source)<-[:WATCHES]-(r:Region)
             RETURN r.id AS id, r.name AS name,
                    r.center_lat AS center_lat, r.center_lng AS center_lng,
                    r.radius_km AS radius_km, r.geo_terms AS geo_terms,
                    r.is_leaf AS is_leaf, r.created_at AS created_at
             LIMIT 1",
        )
        .param("id", signal_id);

        let mut stream = self.client().execute(q).await?;
        if let Some(row) = stream.next().await? {
            Ok(Some(row_to_region(&row)))
        } else {
            Ok(None)
        }
    }

    /// List regions, optionally filtered by is_leaf. Ordered by name.
    pub async fn list_regions(
        &self,
        leaf_only: Option<bool>,
        limit: u32,
    ) -> Result<Vec<Region>, neo4rs::Error> {
        let cypher = if leaf_only.is_some() {
            "MATCH (r:Region {is_leaf: $is_leaf})
             RETURN r.id AS id, r.name AS name,
                    r.center_lat AS center_lat, r.center_lng AS center_lng,
                    r.radius_km AS radius_km, r.geo_terms AS geo_terms,
                    r.is_leaf AS is_leaf, r.created_at AS created_at
             ORDER BY r.name
             LIMIT $limit"
        } else {
            "MATCH (r:Region)
             RETURN r.id AS id, r.name AS name,
                    r.center_lat AS center_lat, r.center_lng AS center_lng,
                    r.radius_km AS radius_km, r.geo_terms AS geo_terms,
                    r.is_leaf AS is_leaf, r.created_at AS created_at
             ORDER BY r.name
             LIMIT $limit"
        };

        let q = query(cypher)
            .param("is_leaf", leaf_only.unwrap_or(true))
            .param("limit", limit as i64);

        let mut stream = self.client().execute(q).await?;
        let mut regions = Vec::new();
        while let Some(row) = stream.next().await? {
            regions.push(row_to_region(&row));
        }
        Ok(regions)
    }

    /// Create or update a Region node. MERGE on id for idempotency.
    pub async fn upsert_region(&self, region: &Region) -> Result<(), neo4rs::Error> {
        let q = query(
            "MERGE (r:Region {id: $id})
             SET r.name = $name,
                 r.center_lat = $center_lat,
                 r.center_lng = $center_lng,
                 r.radius_km = $radius_km,
                 r.geo_terms = $geo_terms,
                 r.is_leaf = $is_leaf,
                 r.created_at = datetime($created_at)",
        )
        .param("id", region.id.to_string())
        .param("name", region.name.as_str())
        .param("center_lat", region.center_lat)
        .param("center_lng", region.center_lng)
        .param("radius_km", region.radius_km)
        .param("geo_terms", region.geo_terms.clone())
        .param("is_leaf", region.is_leaf)
        .param("created_at", format_datetime(&region.created_at));

        self.client().run(q).await?;
        info!(id = %region.id, name = region.name.as_str(), "Region upserted");
        Ok(())
    }

    /// Delete a Region node and its CONTAINS/WATCHES edges.
    pub async fn delete_region(&self, id: &str) -> Result<bool, neo4rs::Error> {
        self.client().run(query(
            "MATCH (r:Region {id: $id}) DETACH DELETE r",
        ).param("id", id)).await?;
        info!(id, "Region deleted");
        Ok(true)
    }

    /// Add a WATCHES edge from a region to a source.
    pub async fn add_region_source(
        &self,
        region_id: &str,
        source_id: &str,
    ) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (r:Region {id: $region_id}), (s:Source {id: $source_id})
             MERGE (r)-[:WATCHES]->(s)",
        )
        .param("region_id", region_id)
        .param("source_id", source_id);

        self.client().run(q).await?;
        info!(region_id, source_id, "Region WATCHES source");
        Ok(())
    }

    /// Remove a WATCHES edge from a region to a source.
    pub async fn remove_region_source(
        &self,
        region_id: &str,
        source_id: &str,
    ) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (r:Region {id: $region_id})-[w:WATCHES]->(s:Source {id: $source_id})
             DELETE w",
        )
        .param("region_id", region_id)
        .param("source_id", source_id);

        self.client().run(q).await?;
        info!(region_id, source_id, "Region WATCHES edge removed");
        Ok(())
    }

    /// Add a CONTAINS edge from parent region to child region.
    pub async fn nest_region(
        &self,
        parent_id: &str,
        child_id: &str,
    ) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (p:Region {id: $parent_id}), (c:Region {id: $child_id})
             MERGE (p)-[:CONTAINS]->(c)",
        )
        .param("parent_id", parent_id)
        .param("child_id", child_id);

        self.client().run(q).await?;
        info!(parent_id, child_id, "Region CONTAINS edge created");
        Ok(())
    }

    /// List sources watched by a region.
    pub async fn list_region_sources(
        &self,
        region_id: &str,
    ) -> Result<Vec<SourceNode>, neo4rs::Error> {
        let q = query(
            "MATCH (r:Region {id: $region_id})-[:WATCHES]->(s:Source)
             RETURN s.id AS id, s.canonical_key AS canonical_key,
                    s.url AS url, s.name AS name, s.role AS role,
                    s.discovery_method AS discovery_method,
                    s.last_scraped_at AS last_scraped_at,
                    s.scrape_count AS scrape_count,
                    s.sources_discovered AS sources_discovered,
                    s.cw_page AS cw_page, s.cw_feed AS cw_feed,
                    s.cw_media AS cw_media, s.cw_discussion AS cw_discussion,
                    s.cw_events AS cw_events
             ORDER BY s.canonical_key",
        )
        .param("region_id", region_id);

        let mut stream = self.client().execute(q).await?;
        let mut sources = Vec::new();
        while let Some(row) = stream.next().await? {
            if let Some(source) = row_to_source_node(&row) {
                sources.push(source);
            }
        }
        Ok(sources)
    }

    /// List child regions contained by a parent region.
    pub async fn list_child_regions(
        &self,
        parent_id: &str,
    ) -> Result<Vec<Region>, neo4rs::Error> {
        let q = query(
            "MATCH (p:Region {id: $parent_id})-[:CONTAINS]->(c:Region)
             RETURN c.id AS id, c.name AS name,
                    c.center_lat AS center_lat, c.center_lng AS center_lng,
                    c.radius_km AS radius_km, c.geo_terms AS geo_terms,
                    c.is_leaf AS is_leaf, c.created_at AS created_at
             ORDER BY c.name",
        )
        .param("parent_id", parent_id);

        let mut stream = self.client().execute(q).await?;
        let mut regions = Vec::new();
        while let Some(row) = stream.next().await? {
            regions.push(row_to_region(&row));
        }
        Ok(regions)
    }

    /// Check if a region has any sources (used by scrape to decide whether to auto-bootstrap).
    pub async fn region_has_sources(&self, region_id: &str) -> Result<bool, neo4rs::Error> {
        let q = query(
            "MATCH (r:Region {id: $region_id})-[:WATCHES]->(:Source)
             RETURN count(*) > 0 AS has_sources",
        )
        .param("region_id", region_id);

        let mut stream = self.client().execute(q).await?;
        if let Some(row) = stream.next().await? {
            let has: bool = row.get("has_sources").unwrap_or(false);
            Ok(has)
        } else {
            Ok(false)
        }
    }

    /// Get all active sources (global — used for dedup checks).
    pub async fn get_active_sources(&self) -> Result<Vec<SourceNode>, neo4rs::Error> {
        self.search_sources(None).await
    }

    /// Get sources by their UUIDs (active or inactive).
    pub async fn get_sources_by_ids(&self, ids: &[Uuid]) -> Result<Vec<SourceNode>, neo4rs::Error> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let id_strings: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
        let q = query(
            "UNWIND $ids AS sid
             MATCH (s:Source {id: sid})
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
                    s.sources_discovered AS sources_discovered,
                    s.cw_page AS cw_page, s.cw_feed AS cw_feed,
                    s.cw_media AS cw_media, s.cw_discussion AS cw_discussion,
                    s.cw_events AS cw_events",
        )
        .param("ids", id_strings);

        let mut sources = Vec::new();
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Some(source) = row_to_source_node(&row) {
                sources.push(source);
            }
        }
        Ok(sources)
    }

    /// Search sources with optional text filter on canonical_value, url, and source_role.
    pub async fn search_sources(&self, search: Option<&str>) -> Result<Vec<SourceNode>, neo4rs::Error> {
        let cypher = match search {
            Some(_) => "MATCH (s:Source)
                WHERE toLower(s.canonical_value) CONTAINS toLower($search)
                   OR (s.url IS NOT NULL AND toLower(s.url) CONTAINS toLower($search))
                   OR toLower(s.source_role) CONTAINS toLower($search)
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
                       s.sources_discovered AS sources_discovered,
                    s.cw_page AS cw_page, s.cw_feed AS cw_feed,
                    s.cw_media AS cw_media, s.cw_discussion AS cw_discussion,
                    s.cw_events AS cw_events",
            None => "MATCH (s:Source)
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
                       s.sources_discovered AS sources_discovered,
                    s.cw_page AS cw_page, s.cw_feed AS cw_feed,
                    s.cw_media AS cw_media, s.cw_discussion AS cw_discussion,
                    s.cw_events AS cw_events",
        };

        let q = match search {
            Some(s) => query(cypher).param("search", s.to_string()),
            None => query(cypher),
        };

        let mut sources = Vec::new();
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Some(source) = row_to_source_node(&row) {
                sources.push(source);
            }
        }

        Ok(sources)
    }

    /// Get active sources that have produced signals in a geographic region.
    /// Region membership is derived from signal locations, not stamped on sources.
    pub async fn get_sources_for_region(
        &self,
        lat: f64,
        lng: f64,
        radius_km: f64,
    ) -> Result<Vec<SourceNode>, neo4rs::Error> {
        let padded_radius = radius_km * 1.5;
        let lat_delta = padded_radius / 111.0;
        let lng_delta = padded_radius / (111.0 * lat.to_radians().cos());
        let min_lat = lat - lat_delta;
        let max_lat = lat + lat_delta;
        let min_lng = lng - lng_delta;
        let max_lng = lng + lng_delta;

        let bbox = bbox_exists("n");
        let q = query(&format!(
            "MATCH (s:Source {{active: true}})
             WHERE (s.signals_produced = 0 AND s.last_scraped IS NULL)
                OR EXISTS {{
                    MATCH (n) WHERE n.url = s.canonical_value
                      AND {bbox}
                }}
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
                    s.scrape_count AS scrape_count",
        ))
        .param("min_lat", min_lat)
        .param("max_lat", max_lat)
        .param("min_lng", min_lng)
        .param("max_lng", max_lng);

        let mut sources = Vec::new();
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            if let Some(source) = row_to_source_node(&row) {
                sources.push(source);
            }
        }

        Ok(sources)
    }

    /// Count tension signals produced by a specific source.
    pub async fn count_source_tensions(&self, canonical_key: &str) -> Result<u32, neo4rs::Error> {
        // Look up URL from canonical_key, then count Concern nodes with matching url
        let q = query(
            "MATCH (s:Source {canonical_key: $key})
             WITH s.url AS url, s.canonical_value AS cv
             OPTIONAL MATCH (t:Concern)
             WHERE t.url = url OR t.url CONTAINS cv

             RETURN count(t) AS cnt",
        )
        .param("key", canonical_key);

        let mut stream = self.client().execute(q).await?;
        if let Some(row) = stream.next().await? {
            Ok(row.get::<i64>("cnt").unwrap_or(0) as u32)
        } else {
            Ok(0)
        }
    }

    /// Find sources that should be deactivated due to consecutive empty runs.
    /// Returns IDs only — caller emits `SourceDeactivated` events.
    /// Same criteria as `deactivate_dead_sources`: 10+ consecutive empty runs,
    /// excluding curated and human-submitted sources.
    pub async fn find_dead_sources(&self, max_empty_runs: u32) -> Result<Vec<Uuid>, neo4rs::Error> {
        let q = query(
            "MATCH (s:Source {active: true})
             WHERE s.consecutive_empty_runs >= $max
               AND s.discovery_method <> 'curated'
               AND s.discovery_method <> 'human_submission'
             RETURN s.id AS id",
        )
        .param("max", max_empty_runs as i64);

        let mut ids = Vec::new();
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                ids.push(id);
            }
        }
        Ok(ids)
    }

    /// Find unproductive web query sources that should be deactivated.
    /// Returns IDs only — caller emits `SourceDeactivated` events.
    /// Same criteria as `deactivate_dead_web_queries`: 5+ consecutive empty runs,
    /// 3+ total scrapes, 0 signals ever produced, excluding curated/human sources.
    pub async fn find_dead_web_queries(&self) -> Result<Vec<Uuid>, neo4rs::Error> {
        let q = query(
            "MATCH (s:Source {active: true})
             WHERE NOT (s.canonical_value STARTS WITH 'http://' OR s.canonical_value STARTS WITH 'https://')
               AND s.consecutive_empty_runs >= 5
               AND coalesce(s.scrape_count, 0) >= 3
               AND s.signals_produced = 0
               AND coalesce(s.sources_discovered, 0) = 0
               AND s.discovery_method <> 'curated'
               AND s.discovery_method <> 'human_submission'
             RETURN s.id AS id",
        );

        let mut ids = Vec::new();
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                ids.push(id);
            }
        }
        Ok(ids)
    }

    /// Get all active WebQuery canonical_values (used for expansion dedup).
    pub async fn get_active_web_queries(&self) -> Result<Vec<String>, neo4rs::Error> {
        let q = query(
            "MATCH (s:Source {active: true})
             WHERE NOT (s.canonical_value STARTS WITH 'http://' OR s.canonical_value STARTS WITH 'https://')
             RETURN s.canonical_value AS query",
        );

        let mut queries = Vec::new();
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            let query_str: String = row.get("query").unwrap_or_default();
            if !query_str.is_empty() {
                queries.push(query_str);
            }
        }
        Ok(queries)
    }

    /// Find an existing active WebQuery source with a semantically similar embedding.
    /// Uses the `source_query_embedding` vector index. Returns the canonical_key and
    /// similarity score of the best match above `threshold`.
    pub async fn find_similar_query(
        &self,
        embedding: &[f32],
        threshold: f64,
    ) -> Result<Option<(String, f64)>, neo4rs::Error> {
        // Neo4j vector search returns top-K results; we filter by active + threshold.
        let q = query(
            "CALL db.index.vector.queryNodes('source_query_embedding', 5, $embedding)
             YIELD node, score
             WHERE node.active = true
               AND NOT (node.canonical_value STARTS WITH 'http://' OR node.canonical_value STARTS WITH 'https://')
               AND score >= $threshold
             RETURN node.canonical_key AS canonical_key, score
             ORDER BY score DESC
             LIMIT 1",
        )
        .param("embedding", embedding.to_vec())
        .param("threshold", threshold);

        let mut stream = self.client().execute(q).await?;
        if let Some(row) = stream.next().await? {
            let ck: String = row.get("canonical_key").unwrap_or_default();
            let score: f64 = row.get("score").unwrap_or(0.0);
            if !ck.is_empty() {
                return Ok(Some((ck, score)));
            }
        }
        Ok(None)
    }

    /// Get tension response shape analysis for discovery briefing.
    pub async fn get_tension_response_shape(
        &self,
        limit: u32,
    ) -> Result<Vec<ConcernResponseShape>, neo4rs::Error> {
        let q = query(
            "MATCH (t:Concern)
             WHERE t.confidence >= 0.5
               AND coalesce(t.cause_heat, 0.0) >= 0.1
             WITH t
             ORDER BY coalesce(t.cause_heat, 0.0) DESC
             LIMIT $limit
             OPTIONAL MATCH (r)-[:RESPONDS_TO]->(t)
             WHERE r:Resource OR r:Gathering OR r:HelpRequest
             WITH t,
                  count(CASE WHEN r:Resource THEN 1 END) AS aid_count,
                  count(CASE WHEN r:Gathering THEN 1 END) AS gathering_count,
                  count(CASE WHEN r:HelpRequest THEN 1 END) AS need_count,
                  collect(DISTINCT r.title)[..5] AS sample_titles
             WHERE aid_count + gathering_count + need_count > 0
             RETURN t.title AS title,
                    t.opposing AS opposing,
                    coalesce(t.cause_heat, 0.0) AS cause_heat,
                    aid_count, gathering_count, need_count,
                    sample_titles",
        )
        .param("limit", limit as i64);

        let mut shapes = Vec::new();
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            let title: String = row.get("title").unwrap_or_default();
            let opposing: Option<String> = row.get("opposing").ok();
            let cause_heat: f64 = row.get("cause_heat").unwrap_or(0.0);
            let aid_count: i64 = row.get("aid_count").unwrap_or(0);
            let gathering_count: i64 = row.get("gathering_count").unwrap_or(0);
            let need_count: i64 = row.get("need_count").unwrap_or(0);
            let sample_titles: Vec<String> = row.get("sample_titles").unwrap_or_default();

            shapes.push(ConcernResponseShape {
                title,
                opposing,
                cause_heat,
                aid_count: aid_count as u32,
                gathering_count: gathering_count as u32,
                need_count: need_count as u32,
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
             RETURN b LIMIT 1",
        )
        .param("url", url);

        let mut stream = self.client().execute(q).await?;
        Ok(stream.next().await?.is_some())
    }

    /// Return the subset of `urls` that match a blocked source pattern.
    pub async fn blocked_urls(&self, urls: &[String]) -> Result<HashSet<String>, neo4rs::Error> {
        if urls.is_empty() {
            return Ok(HashSet::new());
        }
        let q = query(
            "MATCH (b:BlockedSource)
             WITH collect(b.url_pattern) AS patterns
             UNWIND $urls AS url
             WITH url, patterns
             WHERE any(p IN patterns WHERE url CONTAINS p OR p = url)
             RETURN url",
        )
        .param("urls", urls.to_vec());

        let mut stream = self.client().execute(q).await?;
        let mut blocked = HashSet::new();
        while let Some(row) = stream.next().await? {
            if let Ok(url) = row.get::<String>("url") {
                blocked.insert(url);
            }
        }
        Ok(blocked)
    }

    /// Get source-level stats for reporting.
    pub async fn get_source_stats(&self) -> Result<SourceStats, neo4rs::Error> {
        let q = query(
            "MATCH (s:Source)
             RETURN count(s) AS total,
                    count(CASE WHEN s.active THEN 1 END) AS active,
                    count(CASE WHEN s.discovery_method = 'curated' THEN 1 END) AS curated,
                    count(CASE WHEN s.discovery_method <> 'curated' THEN 1 END) AS discovered",
        );

        let mut stream = self.client().execute(q).await?;
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

    /// Find actors with linked social accounts within a bounding box.
    /// Returns (ActorNode, Vec<SourceNode>) pairs.
    pub async fn find_actors_in_region(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<(ActorNode, Vec<SourceNode>)>, neo4rs::Error> {
        let q = query(
            "MATCH (a:Actor)-[:HAS_SOURCE]->(s:Source)
             WHERE a.location_lat >= $min_lat AND a.location_lat <= $max_lat
               AND a.location_lng >= $min_lng AND a.location_lng <= $max_lng
             RETURN a, collect(s) AS sources",
        )
        .param("min_lat", min_lat)
        .param("max_lat", max_lat)
        .param("min_lng", min_lng)
        .param("max_lng", max_lng);

        let mut stream = self.client().execute(q).await?;
        let mut results = Vec::new();

        while let Some(row) = stream.next().await? {
            let actor_node: neo4rs::Node = match row.get("a") {
                Ok(n) => n,
                Err(_) => continue,
            };

            let id_str: String = actor_node.get("id").unwrap_or_default();
            let id = match Uuid::parse_str(&id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };

            let name: String = actor_node.get("name").unwrap_or_default();
            let actor_type_str: String = actor_node.get("actor_type").unwrap_or_default();
            let actor_type = match actor_type_str.as_str() {
                "organization" => rootsignal_common::ActorType::Organization,
                "individual" => rootsignal_common::ActorType::Individual,
                "government_body" => rootsignal_common::ActorType::GovernmentBody,
                "coalition" => rootsignal_common::ActorType::Coalition,
                _ => rootsignal_common::ActorType::Organization,
            };
            let canonical_key: String = actor_node.get("canonical_key").unwrap_or_default();
            let bio: Option<String> = actor_node.get("bio").ok();
            let external_url: Option<String> = actor_node.get("external_url").ok().filter(|u: &String| !u.is_empty());
            let location_lat: Option<f64> = actor_node.get("location_lat").ok();
            let location_lng: Option<f64> = actor_node.get("location_lng").ok();
            let location_name: Option<String> = actor_node.get("location_name").ok();

            let actor = ActorNode {
                id,
                name,
                actor_type,
                canonical_key,
                domains: actor_node.get("domains").unwrap_or_default(),
                social_urls: actor_node.get("social_urls").unwrap_or_default(),
                description: actor_node.get("description").unwrap_or_default(),
                signal_count: actor_node.get::<i64>("signal_count").unwrap_or(0) as u32,
                first_seen: chrono::Utc::now(),
                last_active: chrono::Utc::now(),
                typical_roles: actor_node.get("typical_roles").unwrap_or_default(),
                bio,
                external_url,
                location_lat,
                location_lng,
                location_name,
                discovery_depth: actor_node.get::<i64>("discovery_depth").unwrap_or(0) as u32,
            };

            // Parse source nodes from the collected list
            let source_nodes: Vec<neo4rs::Node> = row.get("sources").unwrap_or_default();
            let mut sources = Vec::new();
            for sn in source_nodes {
                let s_id_str: String = sn.get("id").unwrap_or_default();
                let s_id = match Uuid::parse_str(&s_id_str) {
                    Ok(id) => id,
                    Err(_) => continue,
                };
                let canonical_key: String = sn.get("canonical_key").unwrap_or_default();
                let canonical_value: String = sn.get("canonical_value").unwrap_or_default();
                let url: Option<String> = sn.get::<String>("url").ok().filter(|u| !u.is_empty());
                let dm_str: String = sn.get("discovery_method").unwrap_or_default();
                let discovery_method = match dm_str.as_str() {
                    "curated" => DiscoveryMethod::Curated,
                    "actor_account" => DiscoveryMethod::ActorAccount,
                    "social_graph_follow" => DiscoveryMethod::SocialGraphFollow,
                    "linked_from" => DiscoveryMethod::LinkedFrom,
                    "human_submission" => DiscoveryMethod::HumanSubmission,
                    _ => DiscoveryMethod::ActorAccount,
                };
                let active: bool = sn.get("active").unwrap_or(true);
                let weight: f64 = sn.get("weight").unwrap_or(0.7);

                let channel_weights = rootsignal_common::ChannelWeights::default_for(
                    &rootsignal_common::scraping_strategy(
                        url.as_deref().unwrap_or(&canonical_value),
                    ),
                );
                sources.push(SourceNode {
                    id: s_id,
                    canonical_key,
                    canonical_value,
                    url,
                    discovery_method,
                    created_at: chrono::Utc::now(),
                    last_scraped: None,
                    last_produced_signal: None,
                    signals_produced: 0,
                    signals_corroborated: 0,
                    consecutive_empty_runs: 0,
                    active,
                    gap_context: None,
                    weight,
                    cadence_hours: Some(12),
                    avg_signals_per_scrape: 0.0,
                    quality_penalty: 1.0,
                    source_role: SourceRole::Mixed,
                    scrape_count: 0,
                    sources_discovered: 0,
                    discovered_from_key: None,
                    channel_weights,
                });
            }

            if !sources.is_empty() {
                results.push((actor, sources));
            }
        }

        info!(
            count = results.len(),
            "Found actors with accounts in region"
        );
        Ok(results)
    }

    /// Lookup an actor by canonical_key. Returns the actor's UUID if found.
    pub async fn find_actor_by_canonical_key(
        &self,
        canonical_key: &str,
    ) -> Result<Option<Uuid>, neo4rs::Error> {
        let q = query(
            "MATCH (a:Actor {canonical_key: $canonical_key})
             RETURN a.id AS id LIMIT 1",
        )
        .param("canonical_key", canonical_key);

        let mut stream = self.client().execute(q).await?;
        if let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    /// Find an actor by name (case-insensitive).
    pub async fn find_actor_by_name(&self, name: &str) -> Result<Option<Uuid>, neo4rs::Error> {
        let q = query(
            "MATCH (a:Actor) WHERE toLower(a.name) = toLower($name)
             RETURN a.id AS id LIMIT 1",
        )
        .param("name", name);

        let mut stream = self.client().execute(q).await?;
        if let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    /// Get recent tension titles and opposing for discovery queries.
    pub async fn get_recent_tensions(
        &self,
        limit: u32,
    ) -> Result<Vec<(String, Option<String>)>, neo4rs::Error> {
        let q = query(
            "MATCH (t:Concern)
             RETURN t.title AS title, t.opposing AS help
             ORDER BY t.extracted_at DESC
             LIMIT $limit",
        )
        .param("limit", limit as i64);

        let mut results = Vec::new();
        let mut stream = self.client().execute(q).await?;
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
    /// When `max_depth` is Some, only actors with discovery_depth < max_depth are returned.
    pub async fn get_actors_with_domains(
        &self,
        max_depth: Option<u32>,
    ) -> Result<Vec<(String, Vec<String>, Vec<String>, String)>, neo4rs::Error> {
        let depth_clause = if max_depth.is_some() {
            "AND coalesce(a.discovery_depth, 0) < $max_depth"
        } else {
            ""
        };
        let cypher = format!(
            "MATCH (a:Actor)
             WHERE (size(a.domains) > 0 OR size(a.social_urls) > 0)
             {depth_clause}
             OPTIONAL MATCH (a)-[:ACTED_IN]->(n)
             WITH a,
                  count(CASE WHEN n:Resource OR n:Gathering THEN 1 END) AS response_signals,
                  count(CASE WHEN n:Concern THEN 1 END) AS tension_signals
             RETURN a.name AS name, a.domains AS domains, a.social_urls AS social_urls,
                    CASE
                      WHEN response_signals > tension_signals THEN 'response'
                      WHEN tension_signals > response_signals THEN 'tension'
                      ELSE 'mixed'
                    END AS dominant_role"
        );
        let mut q = query(&cypher);
        if let Some(depth) = max_depth {
            q = q.param("max_depth", depth as i64);
        }

        let mut results = Vec::new();
        let mut stream = self.client().execute(q).await?;
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

    // --- Pin operations ---

    /// Find pins within a bounding box, joined with their source nodes.
    pub async fn find_pins_in_region(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<(PinNode, SourceNode)>, neo4rs::Error> {
        let q = query(
            "MATCH (p:Pin)
             WHERE p.location_lat >= $min_lat AND p.location_lat <= $max_lat
               AND p.location_lng >= $min_lng AND p.location_lng <= $max_lng
             MATCH (s:Source {id: p.source_id})
             RETURN p, s",
        )
        .param("min_lat", min_lat)
        .param("max_lat", max_lat)
        .param("min_lng", min_lng)
        .param("max_lng", max_lng);

        let mut results = Vec::new();
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            let p: neo4rs::Node = match row.get("p") {
                Ok(n) => n,
                Err(_) => continue,
            };
            let s: neo4rs::Node = match row.get("s") {
                Ok(n) => n,
                Err(_) => continue,
            };

            let pin_id_str: String = p.get("id").unwrap_or_default();
            let pin_id = match Uuid::parse_str(&pin_id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };
            let source_id_str: String = p.get("source_id").unwrap_or_default();
            let source_id = match Uuid::parse_str(&source_id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };

            let pin = PinNode {
                id: pin_id,
                location_lat: p.get("location_lat").unwrap_or(0.0),
                location_lng: p.get("location_lng").unwrap_or(0.0),
                source_id,
                created_by: p.get("created_by").unwrap_or_default(),
                created_at: {
                    let s: String = p.get("created_at").unwrap_or_default();
                    DateTime::parse_from_rfc3339(&s)
                        .map(|d| d.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now())
                },
            };

            let s_id_str: String = s.get("id").unwrap_or_default();
            let s_id = match Uuid::parse_str(&s_id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };
            let source = SourceNode {
                id: s_id,
                canonical_key: s.get("canonical_key").unwrap_or_default(),
                canonical_value: s.get("canonical_value").unwrap_or_default(),
                url: s.get::<String>("url").ok().filter(|u| !u.is_empty()),
                discovery_method: match s
                    .get::<String>("discovery_method")
                    .unwrap_or_default()
                    .as_str()
                {
                    "curated" => DiscoveryMethod::Curated,
                    "actor_account" => DiscoveryMethod::ActorAccount,
                    "social_graph_follow" => DiscoveryMethod::SocialGraphFollow,
                    "linked_from" => DiscoveryMethod::LinkedFrom,
                    "human_submission" => DiscoveryMethod::HumanSubmission,
                    _ => DiscoveryMethod::ColdStart,
                },
                created_at: chrono::Utc::now(),
                last_scraped: None,
                last_produced_signal: None,
                signals_produced: 0,
                signals_corroborated: 0,
                consecutive_empty_runs: 0,
                active: s.get("active").unwrap_or(true),
                gap_context: None,
                weight: s.get("weight").unwrap_or(0.7),
                cadence_hours: Some(12),
                avg_signals_per_scrape: 0.0,
                quality_penalty: 1.0,
                source_role: SourceRole::Mixed,
                scrape_count: 0,
                sources_discovered: 0,
                discovered_from_key: None,
                channel_weights: {
                    let val = s.get::<String>("url").ok()
                        .filter(|u| !u.is_empty())
                        .unwrap_or_else(|| s.get("canonical_value").unwrap_or_default());
                    rootsignal_common::ChannelWeights::default_for(
                        &rootsignal_common::scraping_strategy(&val),
                    )
                },
            };
            results.push((pin, source));
        }
        Ok(results)
    }

    // --- Discovery briefing queries ---

    /// Get tensions ordered by: unmet first, then by severity. Includes response coverage.
    pub async fn get_unmet_tensions(&self, limit: u32) -> Result<Vec<UnmetTension>, neo4rs::Error> {
        let q = query(
            "MATCH (t:Concern)
             WHERE datetime(t.last_confirmed_active) >= datetime() - duration('P30D')
             OPTIONAL MATCH (resp)-[:RESPONDS_TO]->(t)
             WITH t, count(resp) AS response_count
             RETURN t.title AS title, t.severity AS severity,
                    t.opposing AS opposing, t.category AS category,
                    response_count = 0 AS unmet,
                    COALESCE(t.corroboration_count, 0) AS corroboration_count,
                    COALESCE(t.source_diversity, 0) AS source_diversity,
                    COALESCE(t.cause_heat, 0.0) AS cause_heat
             ORDER BY response_count ASC,
                      (COALESCE(t.corroboration_count, 0) + COALESCE(t.source_diversity, 0)) DESC,
                      t.cause_heat DESC,
                      t.severity DESC
             LIMIT $limit",
        )
        .param("limit", limit as i64);

        let mut results = Vec::new();
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            let title: String = row.get("title").unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            results.push(UnmetTension {
                title,
                severity: row.get("severity").unwrap_or_default(),
                opposing: {
                    let h: String = row.get("opposing").unwrap_or_default();
                    if h.is_empty() {
                        None
                    } else {
                        Some(h)
                    }
                },
                category: {
                    let c: String = row.get("category").unwrap_or_default();
                    if c.is_empty() {
                        None
                    } else {
                        Some(c)
                    }
                },
                unmet: row.get("unmet").unwrap_or(true),
                corroboration_count: row.get::<i64>("corroboration_count").unwrap_or(0) as u32,
                source_diversity: row.get::<i64>("source_diversity").unwrap_or(0) as u32,
                cause_heat: row.get("cause_heat").unwrap_or(0.0),
            });
        }
        Ok(results)
    }

    /// Active situations by temperature — gives the LLM a sense of what causal situations exist.
    pub async fn get_situation_landscape(
        &self,
        limit: u32,
    ) -> Result<Vec<SituationBrief>, neo4rs::Error> {
        let q = query(
            "MATCH (s:Situation)
             WHERE s.temperature > 0.1
             RETURN s.headline AS headline, s.arc AS arc, s.temperature AS temperature,
                    s.clarity AS clarity, s.signal_count AS signal_count,
                    s.tension_count AS tension_count, s.dispatch_count AS dispatch_count,
                    s.location_name AS location_name, s.sensitivity AS sensitivity
             ORDER BY s.temperature DESC LIMIT $limit",
        )
        .param("limit", limit as i64);

        let mut results = Vec::new();
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            results.push(SituationBrief {
                headline: row.get("headline").unwrap_or_default(),
                arc: row.get("arc").unwrap_or_default(),
                temperature: row.get("temperature").unwrap_or(0.0),
                clarity: row.get("clarity").unwrap_or_default(),
                signal_count: row.get::<i64>("signal_count").unwrap_or(0) as u32,
                tension_count: row.get::<i64>("tension_count").unwrap_or(0) as u32,
                dispatch_count: row.get::<i64>("dispatch_count").unwrap_or(0) as u32,
                location_name: {
                    let name: String = row.get("location_name").unwrap_or_default();
                    if name.is_empty() {
                        None
                    } else {
                        Some(name)
                    }
                },
                sensitivity: row.get("sensitivity").unwrap_or_default(),
            });
        }
        Ok(results)
    }

    /// Aggregate counts of each active signal type. Reveals systemic imbalances.
    pub async fn get_signal_type_counts(&self) -> Result<SignalTypeCounts, neo4rs::Error> {
        let mut counts = SignalTypeCounts::default();

        for (label, field) in &[
            ("Gathering", "gatherings"),
            ("Resource", "aids"),
            ("HelpRequest", "needs"),
            ("Announcement", "notices"),
            ("Concern", "tensions"),
        ] {
            let q = query(&format!(
                "MATCH (n:{label})
                 WHERE datetime(n.last_confirmed_active) >= datetime() - duration('P30D')
                 RETURN count(n) AS cnt"
            ));
            let mut stream = self.client().execute(q).await?;
            if let Some(row) = stream.next().await? {
                let cnt = row.get::<i64>("cnt").unwrap_or(0) as u32;
                match *field {
                    "gatherings" => counts.gatherings = cnt,
                    "aids" => counts.aids = cnt,
                    "needs" => counts.needs = cnt,
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
    ) -> Result<(Vec<SourceBrief>, Vec<SourceBrief>), neo4rs::Error> {
        // Top 5 successful: active, signals_produced > 0, ordered by weight DESC
        let q = query(
            "MATCH (s:Source {active: true})
             WHERE s.discovery_method IN ['gap_analysis', 'tension_seed']
               AND s.signals_produced > 0
             RETURN s.canonical_value AS cv, s.signals_produced AS sp,
                    s.weight AS weight, s.consecutive_empty_runs AS cer,
                    s.gap_context AS gc, s.active AS active
             ORDER BY s.weight DESC
             LIMIT 5",
        );

        let mut successes = Vec::new();
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            successes.push(SourceBrief {
                canonical_value: row.get("cv").unwrap_or_default(),
                signals_produced: row.get::<i64>("sp").unwrap_or(0) as u32,
                weight: row.get("weight").unwrap_or(0.0),
                consecutive_empty_runs: row.get::<i64>("cer").unwrap_or(0) as u32,
                gap_context: {
                    let gc: String = row.get("gc").unwrap_or_default();
                    if gc.is_empty() {
                        None
                    } else {
                        Some(gc)
                    }
                },
                active: row.get("active").unwrap_or(true),
            });
        }

        // Bottom 5 failures: deactivated or 3+ consecutive empty runs
        let q = query(
            "MATCH (s:Source)
             WHERE s.discovery_method IN ['gap_analysis', 'tension_seed']
               AND (s.active = false OR s.consecutive_empty_runs >= 3)
             RETURN s.canonical_value AS cv, s.signals_produced AS sp,
                    s.weight AS weight, s.consecutive_empty_runs AS cer,
                    s.gap_context AS gc, s.active AS active
             ORDER BY s.consecutive_empty_runs DESC
             LIMIT 5",
        );

        let mut failures = Vec::new();
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            failures.push(SourceBrief {
                canonical_value: row.get("cv").unwrap_or_default(),
                signals_produced: row.get::<i64>("sp").unwrap_or(0) as u32,
                weight: row.get("weight").unwrap_or(0.0),
                consecutive_empty_runs: row.get::<i64>("cer").unwrap_or(0) as u32,
                gap_context: {
                    let gc: String = row.get("gc").unwrap_or_default();
                    if gc.is_empty() {
                        None
                    } else {
                        Some(gc)
                    }
                },
                active: row.get("active").unwrap_or(true),
            });
        }

        Ok((successes, failures))
    }

    /// Get active tensions for response mapping.
    pub async fn get_active_tensions(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<(Uuid, Vec<f64>)>, neo4rs::Error> {
        let bbox = bbox_exists("t");
        let q = query(&format!(
            "MATCH (t:Concern)
             WHERE datetime(t.last_confirmed_active) >= datetime() - duration('P30D')
               AND {bbox}
             RETURN t.id AS id, t.embedding AS embedding",
        ))
        .param("min_lat", min_lat)
        .param("max_lat", max_lat)
        .param("min_lng", min_lng)
        .param("max_lng", max_lng);

        let mut results = Vec::new();
        let mut stream = self.client().execute(q).await?;
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

    /// Find response candidates by vector similarity within a geographic bounding box.
    /// Searches across aid, gathering, and need embedding indexes.
    pub async fn find_response_candidates(
        &self,
        concern_embedding: &[f64],
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<(Uuid, f64)>, neo4rs::Error> {
        let mut candidates = Vec::new();
        let bbox = bbox_exists("node");

        for index in &["aid_embedding", "gathering_embedding", "need_embedding"] {
            let q = query(&format!(
                "CALL db.index.vector.queryNodes('{}', 20, $embedding)
                 YIELD node, score AS similarity
                 WHERE similarity >= 0.4
                   AND {bbox}
                 RETURN node.id AS id, similarity
                 ORDER BY similarity DESC
                 LIMIT 5",
                index
            ))
            .param("embedding", concern_embedding.to_vec())
            .param("min_lat", min_lat)
            .param("max_lat", max_lat)
            .param("min_lng", min_lng)
            .param("max_lng", max_lng);

            let mut stream = self.client().execute(q).await?;
            while let Some(row) = stream.next().await? {
                let id_str: String = row.get("id").unwrap_or_default();
                let similarity: f64 = row.get("similarity").unwrap_or(0.0);
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    candidates.push((id, similarity));
                }
            }
        }

        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(candidates)
    }

    /// Get title and summary for a signal node by UUID.
    /// Searches across Tension, Need, Aid, and Gathering labels.
    pub async fn get_signal_info(
        &self,
        id: Uuid,
    ) -> Result<Option<(String, String)>, neo4rs::Error> {
        for label in &["Concern", "HelpRequest", "Resource", "Gathering"] {
            let q = query(&format!(
                "MATCH (n:{label} {{id: $id}})
                 RETURN n.title AS title, n.summary AS summary"
            ))
            .param("id", id.to_string());

            let mut stream = self.client().execute(q).await?;
            if let Some(row) = stream.next().await? {
                return Ok(Some((
                    row.get("title").unwrap_or_default(),
                    row.get("summary").unwrap_or_default(),
                )));
            }
        }
        Ok(None)
    }

    // --- Investigation operations ---

    /// Find signals that warrant investigation. Returns candidates across 3 priority
    /// categories with per-source-domain dedup (max 1 per domain to prevent budget exhaustion).
    pub async fn find_investigation_targets(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<InvestigationTarget>, neo4rs::Error> {
        let mut targets = Vec::new();
        let mut seen_domains = std::collections::HashSet::new();

        let bbox_t = bbox_exists("t");
        let bbox_a = bbox_exists("a");
        let bbox_n = bbox_exists("n");

        // Priority 1: New tensions (last 24h, < 2 evidence nodes, not investigated in 7d)
        let q = query(&format!(
            "MATCH (t:Concern)
             WHERE datetime(t.extracted_at) > datetime() - duration('P1D')
               AND {bbox_t}
               AND (t.investigated_at IS NULL OR datetime(t.investigated_at) < datetime() - duration('P7D'))
             OPTIONAL MATCH (t)-[:SOURCED_FROM]->(ev:Citation)
             WITH t, count(ev) AS ev_count
             WHERE ev_count < 2
             RETURN t.id AS id, 'Tension' AS label, t.title AS title, t.summary AS summary,
                    t.url AS url, t.sensitivity AS sensitivity
             LIMIT 10"
        ))
        .param("min_lat", min_lat)
        .param("max_lat", max_lat)
        .param("min_lng", min_lng)
        .param("max_lng", max_lng);
        self.collect_investigation_targets(&mut targets, &mut seen_domains, q)
            .await?;

        // Priority 2: High-urgency needs (urgency high/critical, < 2 evidence nodes)
        let q = query(&format!(
            "MATCH (a:HelpRequest)
             WHERE a.urgency IN ['high', 'critical']
               AND {bbox_a}
               AND (a.investigated_at IS NULL OR datetime(a.investigated_at) < datetime() - duration('P7D'))
             OPTIONAL MATCH (a)-[:SOURCED_FROM]->(ev:Citation)
             WITH a, count(ev) AS ev_count
             WHERE ev_count < 2
             RETURN a.id AS id, 'Need' AS label, a.title AS title, a.summary AS summary,
                    a.url AS url, a.sensitivity AS sensitivity
             LIMIT 10"
        ))
        .param("min_lat", min_lat)
        .param("max_lat", max_lat)
        .param("min_lng", min_lng)
        .param("max_lng", max_lng);
        self.collect_investigation_targets(&mut targets, &mut seen_domains, q)
            .await?;

        // Priority 3: Thin-story signals (from emerging situations, < 2 citation nodes)
        let q = query(&format!(
            "MATCH (n)-[:PART_OF]->(s:Situation {{arc: 'emerging'}})
             WHERE (n:Gathering OR n:Resource OR n:HelpRequest OR n:Announcement OR n:Concern OR n:Condition)
               AND {bbox_n}
               AND (n.investigated_at IS NULL OR datetime(n.investigated_at) < datetime() - duration('P7D'))
             OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Citation)
             WITH n, count(ev) AS ev_count,
                  CASE WHEN n:Gathering THEN 'Gathering'
                       WHEN n:Resource THEN 'Aid'
                       WHEN n:HelpRequest THEN 'Need'
                       WHEN n:Announcement THEN 'Notice'
                       WHEN n:Concern THEN 'Concern'
                  END AS label
             WHERE ev_count < 2
             RETURN n.id AS id, label, n.title AS title, n.summary AS summary,
                    n.url AS url, n.sensitivity AS sensitivity
             LIMIT 10"
        ))
        .param("min_lat", min_lat)
        .param("max_lat", max_lat)
        .param("min_lng", min_lng)
        .param("max_lng", max_lng);
        self.collect_investigation_targets(&mut targets, &mut seen_domains, q)
            .await?;

        Ok(targets)
    }

    /// Helper to collect targets from a Cypher query, enforcing per-domain dedup.
    async fn collect_investigation_targets(
        &self,
        targets: &mut Vec<InvestigationTarget>,
        seen_domains: &mut std::collections::HashSet<String>,
        q: neo4rs::Query,
    ) -> Result<(), neo4rs::Error> {
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let id = match Uuid::parse_str(&id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };

            let label: String = row.get("label").unwrap_or_default();
            let node_type = match label.as_str() {
                "Gathering" => NodeType::Gathering,
                "Resource" => NodeType::Resource,
                "HelpRequest" => NodeType::HelpRequest,
                "Announcement" => NodeType::Announcement,
                "Concern" => NodeType::Concern,
                _ => continue,
            };

            let url: String = row.get("url").unwrap_or_default();
            let domain = url::Url::parse(&url)
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
                url,
                is_sensitive,
            });
        }
        Ok(())
    }

    /// Get existing tension titles+summaries for the curiosity loop's context window,
    /// scoped to a geographic bounding box so the LLM only sees region-local tensions.
    pub async fn get_tension_landscape(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<(String, String)>, neo4rs::Error> {
        let bbox = bbox_exists("t");
        let q = query(&format!(
            "MATCH (t:Concern)
             WHERE {bbox}
             RETURN t.title AS title, t.summary AS summary
             ORDER BY t.extracted_at DESC
             LIMIT 50",
        ))
        .param("min_lat", min_lat)
        .param("max_lat", max_lat)
        .param("min_lng", min_lng)
        .param("max_lng", max_lng);

        let mut tensions = Vec::new();
        let mut stream = self.client().execute(q).await?;
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
    pub async fn find_tension_hubs(
        &self,
        limit: u32,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<ConcernHub>, neo4rs::Error> {
        let bbox = bbox_exists("sig");
        let q = query(&format!(
            "MATCH (t:Concern)<-[r:RESPONDS_TO|DRAWN_TO]-(sig)
             WHERE NOT (t)-[:PART_OF]->(:Situation)
               AND {bbox}
             WITH t, collect({{
                 sig_id: sig.id,
                 url: sig.url,
                 strength: r.match_strength,
                 explanation: r.explanation,
                 edge_type: type(r),
                 gathering_type: r.gathering_type
             }}) AS respondents
             WHERE size(respondents) >= 2
             RETURN t.id AS concern_id, t.title AS title, t.summary AS summary,
                    t.category AS category, t.opposing AS opposing,
                    t.cause_heat AS cause_heat,
                    respondents
             ORDER BY size(respondents) DESC, coalesce(t.cause_heat, 0.0) DESC
             LIMIT $limit",
        ))
        .param("limit", limit as i64)
        .param("min_lat", min_lat)
        .param("max_lat", max_lat)
        .param("min_lng", min_lng)
        .param("max_lng", max_lng);

        let mut hubs = Vec::new();
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("concern_id").unwrap_or_default();
            let concern_id = match Uuid::parse_str(&id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };

            let title: String = row.get("title").unwrap_or_default();
            let summary: String = row.get("summary").unwrap_or_default();
            let category: Option<String> = row.get("category").ok();
            let opposing: Option<String> = row.get("opposing").ok();
            let cause_heat: f64 = row.get("cause_heat").unwrap_or(0.0);

            // Parse respondents from neo4j map list
            let respondent_maps: Vec<neo4rs::BoltMap> = row.get("respondents").unwrap_or_default();
            let mut respondents = Vec::new();
            for map in respondent_maps {
                let sig_id_str = map.get::<String>("sig_id").unwrap_or_default();
                let sig_id = match Uuid::parse_str(&sig_id_str) {
                    Ok(id) => id,
                    Err(_) => continue,
                };
                respondents.push(ConcernRespondent {
                    signal_id: sig_id,
                    url: map.get::<String>("url").unwrap_or_default(),
                    match_strength: map.get::<f64>("strength").unwrap_or(0.0),
                    explanation: map.get::<String>("explanation").unwrap_or_default(),
                    edge_type: map.get::<String>("edge_type").unwrap_or_default(),
                    gathering_type: map.get::<String>("gathering_type").ok(),
                });
            }

            hubs.push(ConcernHub {
                concern_id,
                title,
                summary,
                category,
                opposing,
                cause_heat,
                respondents,
            });
        }
        Ok(hubs)
    }

    /// Count abandoned signals (curiosity_investigated = 'abandoned').
    /// Used by StoryWeaver for coverage gap reporting.
    pub async fn count_abandoned_signals(&self) -> Result<u32, neo4rs::Error> {
        let q = query(
            "MATCH (n)
             WHERE (n:Resource OR n:Gathering OR n:HelpRequest OR n:Announcement)
               AND n.curiosity_investigated = 'abandoned'
             RETURN count(n) AS cnt",
        );
        let mut stream = self.client().execute(q).await?;
        if let Some(row) = stream.next().await? {
            let cnt: i64 = row.get("cnt").unwrap_or(0);
            return Ok(cnt as u32);
        }
        Ok(0)
    }

    /// Read current confidence for a signal. Returns 0.5 if not found.
    pub async fn get_signal_confidence(
        &self,
        signal_id: Uuid,
        node_type: NodeType,
    ) -> Result<f32, neo4rs::Error> {
        let label = match node_type {
            NodeType::Gathering => "Gathering",
            NodeType::Resource => "Resource",
            NodeType::HelpRequest => "HelpRequest",
            NodeType::Announcement => "Announcement",
            NodeType::Concern => "Concern",
            NodeType::Condition => "Condition",
            NodeType::Citation => return Ok(0.5),
        };

        let q = query(&format!(
            "MATCH (n:{} {{id: $id}})
             RETURN n.confidence AS confidence",
            label
        ))
        .param("id", signal_id.to_string());

        let mut stream = self.client().execute(q).await?;
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
            NodeType::Gathering => "Gathering",
            NodeType::Resource => "Resource",
            NodeType::HelpRequest => "HelpRequest",
            NodeType::Announcement => "Announcement",
            NodeType::Concern => "Concern",
            NodeType::Condition => "Condition",
            NodeType::Citation => return Ok(Vec::new()),
        };

        let q = query(&format!(
            "MATCH (n:{} {{id: $id}})-[:SOURCED_FROM]->(ev:Citation)
             RETURN ev.relevance AS relevance, ev.evidence_confidence AS confidence",
            label
        ))
        .param("id", signal_id.to_string());

        let mut results = Vec::new();
        let mut stream = self.client().execute(q).await?;
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

    /// Get gap_type strategy stats for discovery sources.
    /// Parses gap_type from gap_context ("... | Gap: <type> | ...") in Rust.
    pub async fn get_gap_type_stats(&self) -> Result<Vec<GapTypeStats>, neo4rs::Error> {
        let q = query(
            "MATCH (s:Source)
             WHERE s.discovery_method IN ['gap_analysis', 'tension_seed']
               AND s.gap_context IS NOT NULL
             RETURN s.gap_context AS gc, s.signals_produced AS sp, s.weight AS weight",
        );

        let mut map: std::collections::HashMap<String, (u32, u32, f64)> =
            std::collections::HashMap::new();
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            let gc: String = row.get("gc").unwrap_or_default();
            let sp: i64 = row.get::<i64>("sp").unwrap_or(0);
            let weight: f64 = row.get("weight").unwrap_or(0.0);

            // Parse gap_type from "... | Gap: <type> | ..."
            let gap_type = gc
                .find("| Gap: ")
                .and_then(|start| {
                    let after = &gc[start + 7..];
                    let end = after.find(" |").unwrap_or(after.len());
                    let gt = after[..end].trim();
                    if gt.is_empty() {
                        None
                    } else {
                        Some(gt.to_string())
                    }
                })
                .unwrap_or_else(|| "unknown".to_string());

            let entry = map.entry(gap_type).or_insert((0, 0, 0.0));
            entry.0 += 1; // total
            if sp > 0 {
                entry.1 += 1;
            } // successful
            entry.2 += weight; // sum of weights
        }

        let mut results: Vec<GapTypeStats> = map
            .into_iter()
            .map(|(gap_type, (total, successful, weight_sum))| GapTypeStats {
                gap_type,
                total_sources: total,
                successful_sources: successful,
                avg_weight: if total > 0 {
                    weight_sum / total as f64
                } else {
                    0.0
                },
            })
            .collect();
        results.sort_by(|a, b| b.total_sources.cmp(&a.total_sources));
        Ok(results)
    }

    /// Get extraction yield metrics grouped by source domain.
    pub async fn get_extraction_yield(&self) -> Result<Vec<ExtractionYield>, neo4rs::Error> {
        // Base metrics from Source nodes
        let q = query(
            "MATCH (s:Source)
             WHERE s.active = true
             WITH s,
                  CASE
                    WHEN NOT (s.canonical_value STARTS WITH 'http://' OR s.canonical_value STARTS WITH 'https://')
                      THEN 'search'
                    WHEN s.canonical_value STARTS WITH 'https://'
                      THEN split(replace(substring(s.canonical_value, 8), 'www.', ''), '/')[0]
                    WHEN s.canonical_value STARTS WITH 'http://'
                      THEN split(replace(substring(s.canonical_value, 7), 'www.', ''), '/')[0]
                    ELSE split(s.canonical_value, '/')[0]
                  END AS st
             RETURN st, s.signals_produced AS sp,
                    s.signals_corroborated AS sc, s.url AS url",
        );

        let mut type_map: std::collections::HashMap<String, (u32, u32, Vec<String>)> =
            std::collections::HashMap::new();
        let mut stream = self.client().execute(q).await?;
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
        for (source_label, (extracted, corroborated, urls)) in &type_map {
            // Count survived signals (still in graph) per source type via url
            let mut survived = 0u32;
            if !urls.is_empty() {
                for url in urls {
                    let q = query(
                        "MATCH (n)
                         WHERE (n:Gathering OR n:Resource OR n:HelpRequest OR n:Announcement OR n:Concern OR n:Condition)
                           AND n.url = $url
                         RETURN count(n) AS cnt",
                    )
                    .param("url", url.as_str());

                    let mut stream = self.client().execute(q).await?;
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
                        "MATCH (n)-[:SOURCED_FROM]->(ev:Citation {relevance: 'CONTRADICTING'})
                         WHERE (n:Gathering OR n:Resource OR n:HelpRequest OR n:Announcement OR n:Concern OR n:Condition)
                           AND n.url = $url
                         RETURN count(DISTINCT n) AS cnt",
                    )
                    .param("url", url.as_str());

                    let mut stream = self.client().execute(q).await?;
                    if let Some(row) = stream.next().await? {
                        contradicted += row.get::<i64>("cnt").unwrap_or(0) as u32;
                    }
                }
            }

            results.push(ExtractionYield {
                source_label: source_label.clone(),
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
    pub async fn get_snapshot_entity_count_7d_ago(
        &self,
        story_id: Uuid,
    ) -> Result<Option<u32>, neo4rs::Error> {
        let q = query(
            "MATCH (cs:ClusterSnapshot {story_id: $story_id})
             WHERE datetime(cs.run_at) >= datetime() - duration('P8D')
               AND datetime(cs.run_at) <= datetime() - duration('P6D')
             RETURN cs.entity_count AS cnt
             ORDER BY cs.run_at ASC
             LIMIT 1",
        )
        .param("story_id", story_id.to_string());

        let mut stream = self.client().execute(q).await?;
        if let Some(row) = stream.next().await? {
            let cnt: i64 = row.get("cnt").unwrap_or(0);
            return Ok(Some(cnt as u32));
        }

        Ok(None)
    }

    /// Get the snapshot gap score (ask_count - give_count) from 7 days ago for gap velocity calculation.
    pub async fn get_snapshot_gap_7d_ago(
        &self,
        story_id: Uuid,
    ) -> Result<Option<i32>, neo4rs::Error> {
        let q = query(
            "MATCH (cs:ClusterSnapshot {story_id: $story_id})
             WHERE datetime(cs.run_at) >= datetime() - duration('P8D')
               AND datetime(cs.run_at) <= datetime() - duration('P6D')
             RETURN (cs.ask_count - cs.give_count) AS gap
             ORDER BY cs.run_at ASC
             LIMIT 1",
        )
        .param("story_id", story_id.to_string());

        let mut stream = self.client().execute(q).await?;
        if let Some(row) = stream.next().await? {
            let gap: i64 = row.get("gap").unwrap_or(0);
            return Ok(Some(gap as i32));
        }

        Ok(None)
    }

    // =============================================================================
    // Response Scout methods
    // =============================================================================

    /// Find tensions that need response discovery.
    /// Prioritizes tensions with fewer responses and higher cause_heat.
    pub async fn find_response_finder_targets(
        &self,
        limit: u32,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<ResponseFinderTarget>, neo4rs::Error> {
        let bbox = bbox_exists("t");
        let q = query(&format!(
            "MATCH (t:Concern)
             WHERE t.confidence >= 0.5
               AND {bbox}
               AND coalesce(datetime(t.response_scouted_at), datetime('2000-01-01'))
                   < datetime() - duration('P14D')
             OPTIONAL MATCH (t)<-[:RESPONDS_TO]-(r)
             WITH t, count(r) AS response_count
             RETURN t.id AS id, t.title AS title, t.summary AS summary,
                    t.severity AS severity, t.category AS category,
                    t.opposing AS opposing,
                    coalesce(t.cause_heat, 0.0) AS cause_heat,
                    response_count
             ORDER BY response_count ASC, t.cause_heat DESC, t.confidence DESC
             LIMIT $limit",
        ))
        .param("limit", limit as i64)
        .param("min_lat", min_lat)
        .param("max_lat", max_lat)
        .param("min_lng", min_lng)
        .param("max_lng", max_lng);

        let mut results = Vec::new();
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let Ok(concern_id) = Uuid::parse_str(&id_str) else {
                continue;
            };
            results.push(ResponseFinderTarget {
                concern_id,
                title: row.get("title").unwrap_or_default(),
                summary: row.get("summary").unwrap_or_default(),
                severity: row.get("severity").unwrap_or_default(),
                category: {
                    let s: String = row.get("category").unwrap_or_default();
                    if s.is_empty() {
                        None
                    } else {
                        Some(s)
                    }
                },
                opposing: {
                    let s: String = row.get("opposing").unwrap_or_default();
                    if s.is_empty() {
                        None
                    } else {
                        Some(s)
                    }
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
        concern_id: Uuid,
    ) -> Result<Vec<ResponseHeuristic>, neo4rs::Error> {
        let q = query(
            "MATCH (r)-[:RESPONDS_TO]->(t:Concern {id: $id})
             WHERE r:Resource OR r:Gathering OR r:HelpRequest
             RETURN r.title AS title, r.summary AS summary, labels(r)[0] AS label
             LIMIT 5",
        )
        .param("id", concern_id.to_string());

        let mut results = Vec::new();
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            results.push(ResponseHeuristic {
                title: row.get("title").unwrap_or_default(),
                summary: row.get("summary").unwrap_or_default(),
                signal_type: row.get("label").unwrap_or_default(),
            });
        }
        Ok(results)
    }

    /// Find tensions with active heat that need gravity scouting.
    /// Requires cause_heat >= 0.1 (cold tensions don't create gatherings).
    /// Uses exponential backoff based on consecutive miss count.
    pub async fn find_gathering_finder_targets(
        &self,
        limit: u32,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<GatheringFinderTarget>, neo4rs::Error> {
        let bbox = bbox_exists("t");
        let q = query(&format!(
            "MATCH (t:Concern)
             WHERE t.confidence >= 0.5
               AND {bbox}
               AND coalesce(t.cause_heat, 0.0) >= 0.1
               AND coalesce(datetime(t.gravity_scouted_at), datetime('2000-01-01'))
                   < datetime() - duration({{days:
                       CASE
                         WHEN coalesce(t.gravity_scout_miss_count, 0) = 0 THEN 7
                         WHEN coalesce(t.gravity_scout_miss_count, 0) = 1 THEN 14
                         WHEN coalesce(t.gravity_scout_miss_count, 0) = 2 THEN 21
                         ELSE 30
                       END
                     }})
             RETURN t.id AS id, t.title AS title, t.summary AS summary,
                    t.severity AS severity, t.category AS category,
                    t.opposing AS opposing,
                    coalesce(t.cause_heat, 0.0) AS cause_heat
             ORDER BY t.cause_heat DESC, t.confidence DESC
             LIMIT $limit",
        ))
        .param("limit", limit as i64)
        .param("min_lat", min_lat)
        .param("max_lat", max_lat)
        .param("min_lng", min_lng)
        .param("max_lng", max_lng);

        let mut results = Vec::new();
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let Ok(concern_id) = Uuid::parse_str(&id_str) else {
                continue;
            };
            results.push(GatheringFinderTarget {
                concern_id,
                title: row.get("title").unwrap_or_default(),
                summary: row.get("summary").unwrap_or_default(),
                severity: row.get("severity").unwrap_or_default(),
                category: {
                    let s: String = row.get("category").unwrap_or_default();
                    if s.is_empty() {
                        None
                    } else {
                        Some(s)
                    }
                },
                opposing: {
                    let s: String = row.get("opposing").unwrap_or_default();
                    if s.is_empty() {
                        None
                    } else {
                        Some(s)
                    }
                },
                cause_heat: row.get("cause_heat").unwrap_or(0.0),
            });
        }
        Ok(results)
    }

    /// Fetch existing gravity signals for a tension (gatherings wired via DRAWN_TO),
    /// filtered to signals within `radius_km` of the given center point.
    pub async fn get_existing_gathering_signals(
        &self,
        concern_id: Uuid,
        center_lat: f64,
        center_lng: f64,
        radius_km: f64,
    ) -> Result<Vec<ResponseHeuristic>, neo4rs::Error> {
        let lat_delta = radius_km / 111.0;
        let lng_delta = radius_km / (111.0 * center_lat.to_radians().cos());
        let q = query(&format!(
            "MATCH (r)-[rel:DRAWN_TO]->(t:Concern {{id: $id}})
             WHERE (r:Resource OR r:Gathering OR r:HelpRequest)
               AND EXISTS {{
                 MATCH (r)-[:{LOC_EDGES}]->(l:Location)
                 WHERE l.lat >= $min_lat AND l.lat <= $max_lat
                   AND l.lng >= $min_lng AND l.lng <= $max_lng
               }}
             RETURN r.title AS title, r.summary AS summary, labels(r)[0] AS label
             LIMIT 5",
        ))
        .param("id", concern_id.to_string())
        .param("min_lat", center_lat - lat_delta)
        .param("max_lat", center_lat + lat_delta)
        .param("min_lng", center_lng - lng_delta)
        .param("max_lng", center_lng + lng_delta);

        let mut results = Vec::new();
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            results.push(ResponseHeuristic {
                title: row.get("title").unwrap_or_default(),
                summary: row.get("summary").unwrap_or_default(),
                signal_type: row.get("label").unwrap_or_default(),
            });
        }
        Ok(results)
    }

    /// Look up a Resource node by its slug. Returns the UUID if found.
    pub async fn find_resource_by_slug(&self, slug: &str) -> Result<Option<Uuid>, neo4rs::Error> {
        let q = query(
            "MATCH (r:Resource {slug: $slug})
             RETURN r.id AS resource_id",
        )
        .param("slug", slug);

        let mut stream = self.client().execute(q).await?;
        if let Some(row) = stream.next().await? {
            let id_str: String = row.get("resource_id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    /// Find the closest existing Resource by embedding similarity.
    /// Returns (UUID, similarity) if a Resource exceeds the threshold.
    /// Uses brute-force pairwise comparison (Resource count expected < 500).
    pub async fn find_resource_by_embedding(
        &self,
        embedding: &[f32],
        threshold: f64,
    ) -> Result<Option<(Uuid, f64)>, neo4rs::Error> {
        let q = query(
            "MATCH (r:Resource)
             WHERE r.embedding IS NOT NULL
             RETURN r.id AS rid, r.embedding AS emb",
        );

        let emb_f64 = embedding_to_f64(embedding);
        let mut best: Option<(Uuid, f64)> = None;

        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("rid").unwrap_or_default();
            let stored: Vec<f64> = row.get("emb").unwrap_or_default();
            if stored.is_empty() {
                continue;
            }
            let sim = cosine_sim_f64(&emb_f64, &stored);
            if sim >= threshold {
                if best.as_ref().map_or(true, |(_, s)| sim > *s) {
                    if let Ok(id) = Uuid::parse_str(&id_str) {
                        best = Some((id, sim));
                    }
                }
            }
        }
        Ok(best)
    }

    /// Verify that all signal UUIDs actually exist in the graph. Returns the set of missing IDs.
    pub async fn verify_signal_ids(&self, signal_ids: &[Uuid]) -> Result<Vec<Uuid>, neo4rs::Error> {
        let g = &self.client();
        let mut missing = Vec::new();

        for id in signal_ids {
            let q = query(
                "MATCH (n) WHERE n.id = $id
                   AND (n:Gathering OR n:Resource OR n:HelpRequest OR n:Announcement OR n:Concern OR n:Condition)
                 RETURN n.id AS id",
            )
            .param("id", id.to_string());

            let mut stream = g.execute(q).await?;
            if stream.next().await?.is_none() {
                missing.push(*id);
            }
        }

        Ok(missing)
    }

    /// Find signals from a scout run that aren't yet assigned to any situation.
    pub async fn find_unassigned_signals(
        &self,
        scout_run_id: &str,
    ) -> Result<Vec<(Uuid, String)>, neo4rs::Error> {
        let g = &self.client();

        let labels = ["Gathering", "Resource", "HelpRequest", "Announcement", "Concern", "Condition"];
        let mut results = Vec::new();

        for label in &labels {
            let q = query(&format!(
                "MATCH (n:{label} {{scout_run_id: $run_id}})
                 WHERE NOT (n)-[:PART_OF]->(:Situation)
                 RETURN n.id AS id"
            ))
            .param("run_id", scout_run_id);

            let mut stream = g.execute(q).await?;
            while let Some(row) = stream.next().await? {
                let id: String = row.get("id").unwrap_or_default();
                if let Ok(uuid) = Uuid::parse_str(&id) {
                    results.push((uuid, label.to_string()));
                }
            }
        }

        Ok(results)
    }

    /// Get signal location observations for an actor's authored signals.
    /// Returns (lat, lng, location_name, extracted_at) per Location edge.
    pub async fn get_signals_for_actor(
        &self,
        actor_id: Uuid,
    ) -> Result<Vec<(f64, f64, String, DateTime<Utc>)>, neo4rs::Error> {
        let q = query(&format!(
            "MATCH (a:Actor {{id: $id}})-[:ACTED_IN {{role: 'authored'}}]->(n)
             MATCH (n)-[:{LOC_EDGES}]->(loc:Location)
             RETURN loc.lat AS lat, loc.lng AS lng,
                    coalesce(loc.name, '') AS name, n.extracted_at AS ts",
        ))
        .param("id", actor_id.to_string());

        let g = self.client().clone();
        let mut stream = g.execute(q).await?;
        let mut results = Vec::new();

        while let Some(row) = stream.next().await? {
            let lat: f64 = match row.get("lat") {
                Ok(v) => v,
                Err(_) => continue,
            };
            let lng: f64 = match row.get("lng") {
                Ok(v) => v,
                Err(_) => continue,
            };
            let name: String = row.get("name").unwrap_or_default();
            let ts: String = row.get("ts").unwrap_or_default();
            let parsed_ts = ts.parse::<DateTime<Utc>>().unwrap_or_else(|_| Utc::now());
            results.push((lat, lng, name, parsed_ts));
        }

        Ok(results)
    }

    /// List all actors with their linked sources.
    pub async fn list_all_actors(
        &self,
    ) -> Result<Vec<(ActorNode, Vec<SourceNode>)>, neo4rs::Error> {
        // Reuse find_actors_in_region with world-spanning bounds
        self.find_actors_in_region(-90.0, 90.0, -180.0, 180.0).await
    }

    /// Batch-fetch inference data for all Notices in a bounding box.
    pub async fn notice_inference_batch(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<NoticeInferenceRow>, neo4rs::Error> {
        let bbox = bbox_exists("n");
        let cypher = format!(
            "MATCH (n:Announcement)
            WHERE {bbox}
              AND n.review_status IN ['staged', 'accepted']
            OPTIONAL MATCH (n)-[:PRODUCED_BY]->(s:Source)
            WITH n, s, EXISTS((n)-[:EVIDENCE_OF]->(:Concern)) AS has_evidence
            RETURN n.id AS id, n.severity AS severity,
                   n.corroboration_count AS corr, n.source_diversity AS div,
                   has_evidence,
                   s.scrape_count AS sc, s.signals_corroborated AS scorr,
                   s.quality_penalty AS qp, s.avg_signals_per_scrape AS avg_sps"
        );

        let mut result = self
            .client
            .execute(
                query(&cypher)
                    .param("min_lat", min_lat)
                    .param("max_lat", max_lat)
                    .param("min_lng", min_lng)
                    .param("max_lng", max_lng),
            )
            .await?;

        let mut rows = Vec::new();
        while let Some(row) = result.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let Some(notice_id) = Uuid::parse_str(&id_str).ok() else {
                continue;
            };
            let severity: String = row.get("severity").unwrap_or_default();
            let corr: i64 = row.get("corr").unwrap_or(0);
            let div: i64 = row.get("div").unwrap_or(0);
            let has_evidence: bool = row.get("has_evidence").unwrap_or(false);

            let source = {
                let sc: Option<i64> = row.get("sc").ok();
                sc.map(|sc| {
                    let mut s = SourceNode::new(
                        String::new(),
                        String::new(),
                        None,
                        rootsignal_common::DiscoveryMethod::Curated,
                        0.5,
                        rootsignal_common::SourceRole::Mixed,
                        None,
                    );
                    s.scrape_count = sc as u32;
                    s.signals_corroborated = row.get::<i64>("scorr").unwrap_or(0) as u32;
                    s.quality_penalty = row.get("qp").unwrap_or(1.0);
                    s.avg_signals_per_scrape = row.get("avg_sps").unwrap_or(0.0);
                    s
                })
            };

            rows.push(NoticeInferenceRow {
                notice_id,
                severity,
                corroboration_count: corr as u32,
                source_diversity: div as u32,
                has_evidence_of: has_evidence,
                source,
            });
        }
        Ok(rows)
    }

    /// Find situations eligible for curiosity re-investigation.
    /// Returns (situation_id, signal_ids) pairs for emerging/fuzzy situations
    /// that haven't been curiosity-triggered in 7 days.
    pub async fn find_curiosity_candidates(
        &self,
    ) -> Result<Vec<(Uuid, Vec<Uuid>)>, neo4rs::Error> {
        let q = query(
            "MATCH (sig)-[:PART_OF]->(s:Situation)
             WHERE (s.arc = 'emerging' OR s.clarity = 'Fuzzy')
               AND s.temperature >= 0.3
               AND s.sensitivity <> 'SENSITIVE' AND s.sensitivity <> 'RESTRICTED'
               AND (s.curiosity_triggered_at IS NULL
                    OR datetime(s.curiosity_triggered_at) < datetime() - duration('P7D'))
             WITH s, collect(sig) AS signals
             LIMIT 5
             UNWIND signals AS sig
             WITH s, sig
             WHERE (sig.curiosity_investigated IS NULL OR sig.curiosity_investigated = 'failed')
               AND NOT sig:Concern
             WITH s, collect(sig.id) AS sig_ids
             WHERE size(sig_ids) > 0
             RETURN s.id AS situation_id, sig_ids",
        );
        let mut stream = self.client().execute(q).await?;
        let mut results = Vec::new();
        while let Some(row) = stream.next().await? {
            let sit_id_str: String = row.get("situation_id").unwrap_or_default();
            let sig_id_strs: Vec<String> = row.get("sig_ids").unwrap_or_default();
            if let Ok(sit_id) = Uuid::parse_str(&sit_id_str) {
                let sig_ids: Vec<Uuid> = sig_id_strs
                    .iter()
                    .filter_map(|s| Uuid::parse_str(s).ok())
                    .collect();
                if !sig_ids.is_empty() {
                    results.push((sit_id, sig_ids));
                }
            }
        }
        Ok(results)
    }

    // =============================================================================
    // Read-only methods (split from former GraphStore mixed read+write methods)
    // =============================================================================

    /// Get implied queries from Aid/Gathering signals recently linked to heated tensions.
    /// Returns (queries, signal_ids) — the caller emits ImpliedQueriesConsumed with the IDs.
    pub async fn get_recently_linked_signals_with_queries(
        &self,
    ) -> Result<(Vec<String>, Vec<Uuid>), neo4rs::Error> {
        let q = query(
            "MATCH (s)-[:RESPONDS_TO|DRAWN_TO]->(t:Concern)
             WHERE (s:Resource OR s:Gathering)
               AND s.implied_queries IS NOT NULL
               AND size(s.implied_queries) > 0
               AND coalesce(t.cause_heat, 0.0) >= 0.1
             WITH DISTINCT s
             RETURN s.implied_queries AS queries, s.id AS id",
        );

        let mut all_queries = Vec::new();
        let mut signal_ids = Vec::new();
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            let queries: Vec<String> = row.get("queries").unwrap_or_default();
            all_queries.extend(queries);
            let id_str: String = row.get("id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                signal_ids.push(id);
            }
        }

        // Clear implied_queries on collected signals so they aren't returned again
        if !signal_ids.is_empty() {
            let ids: Vec<String> = signal_ids.iter().map(|id| id.to_string()).collect();
            let q = query(
                "UNWIND $ids AS sid
                 MATCH (s {id: sid})
                 SET s.implied_queries = null",
            )
            .param("ids", ids);
            self.client().run(q).await?;
        }

        Ok((all_queries, signal_ids))
    }

    /// Find signals eligible for tension-linker investigation.
    /// Read-only — the pre-pass promotion of exhausted retries is handled by ExhaustedRetriesPromoted event.
    pub async fn find_tension_linker_targets(
        &self,
        limit: u32,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<ConcernLinkerTarget>, neo4rs::Error> {
        let bbox = bbox_exists("n");
        let q = query(&format!(
            "MATCH (n)
             WHERE (n:Resource OR n:Gathering OR n:HelpRequest OR n:Announcement)
               AND (n.curiosity_investigated IS NULL OR n.curiosity_investigated = 'failed')
               AND NOT (n)-[:RESPONDS_TO|DRAWN_TO]->(:Concern)
               AND n.confidence >= 0.5
               AND {bbox}
             RETURN n.id AS id, n.title AS title, n.summary AS summary,
                    n.url AS url,
                    CASE WHEN n:Gathering THEN 'Gathering'
                         WHEN n:Resource THEN 'Aid'
                         WHEN n:HelpRequest THEN 'Need'
                         WHEN n:Announcement THEN 'Notice'
                    END AS label
             ORDER BY n.extracted_at DESC
             LIMIT $limit",
        ))
        .param("limit", limit as i64)
        .param("min_lat", min_lat)
        .param("max_lat", max_lat)
        .param("min_lng", min_lng)
        .param("max_lng", max_lng);

        let mut targets = Vec::new();
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let id = match Uuid::parse_str(&id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };
            targets.push(ConcernLinkerTarget {
                signal_id: id,
                title: row.get("title").unwrap_or_default(),
                summary: row.get("summary").unwrap_or_default(),
                label: row.get("label").unwrap_or_default(),
                url: row.get("url").unwrap_or_default(),
            });
        }
        Ok(targets)
    }

    /// Find near-duplicate Tension pairs within a bounding box.
    /// Returns (survivor_id, duplicate_id) pairs — the caller emits DuplicateTensionMerged per pair.
    pub async fn find_duplicate_tension_pairs(
        &self,
        threshold: f64,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<(Uuid, Uuid)>, neo4rs::Error> {
        let bbox = bbox_exists("t");
        let q = query(&format!(
            "MATCH (t:Concern)
             WHERE t.embedding IS NOT NULL
               AND {bbox}
             RETURN t.id AS id, t.embedding AS embedding, t.extracted_at AS extracted_at
             ORDER BY t.extracted_at ASC",
        ))
        .param("min_lat", min_lat)
        .param("max_lat", max_lat)
        .param("min_lng", min_lng)
        .param("max_lng", max_lng);

        struct TensionEmbed {
            id: String,
            embedding: Vec<f64>,
        }

        let mut tensions: Vec<TensionEmbed> = Vec::new();
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id: String = row.get("id").unwrap_or_default();
            let embedding: Vec<f64> = row.get("embedding").unwrap_or_default();
            if !embedding.is_empty() {
                tensions.push(TensionEmbed { id, embedding });
            }
        }

        if tensions.len() < 2 {
            return Ok(Vec::new());
        }

        let mut to_delete: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut merges: Vec<(Uuid, Uuid)> = Vec::new();

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
                    if let (Ok(survivor), Ok(duplicate)) = (
                        Uuid::parse_str(&tensions[i].id),
                        Uuid::parse_str(&tensions[j].id),
                    ) {
                        merges.push((survivor, duplicate));
                    }
                }
            }
        }

        Ok(merges)
    }

    // -----------------------------------------------------------------
    // Situation weaving reads
    // -----------------------------------------------------------------

    /// Discover unassigned signals from a scout run (signals without a PART_OF→Situation edge).
    pub async fn discover_unassigned_signals(
        &self,
        scout_run_id: &str,
    ) -> Result<Vec<WeaveSignal>, neo4rs::Error> {
        let g = &self.client;
        let mut signals = Vec::new();

        let labels = ["Gathering", "Resource", "HelpRequest", "Announcement", "Concern", "Condition"];
        for label in &labels {
            let q = query(&format!(
                "MATCH (n:{label} {{scout_run_id: $run_id}})
                 WHERE NOT (n)-[:PART_OF]->(:Situation)
                   AND NOT n:Citation
                 OPTIONAL MATCH (n)-[:{LOC_EDGES}]->(loc:Location)
                 WITH n, head(collect(loc)) AS primary_loc
                 RETURN n.id AS id, n.title AS title, n.summary AS summary,
                        '{label}' AS node_type, n.embedding AS embedding,
                        n.url AS url,
                        coalesce(n.cause_heat, 0.0) AS cause_heat,
                        primary_loc.lat AS lat, primary_loc.lng AS lng,
                        n.published_at AS published_at"
            ))
            .param("run_id", scout_run_id);

            let mut stream = g.execute(q).await?;
            while let Some(row) = stream.next().await? {
                let id_str: String = row.get("id").unwrap_or_default();
                let id = match uuid::Uuid::parse_str(&id_str) {
                    Ok(id) => id,
                    Err(_) => continue,
                };
                let embedding: Vec<f32> = row.get("embedding").unwrap_or_default();
                if embedding.is_empty() {
                    continue;
                }

                signals.push(WeaveSignal {
                    id,
                    title: row.get("title").unwrap_or_default(),
                    summary: row.get("summary").unwrap_or_default(),
                    node_type: row.get("node_type").unwrap_or_default(),
                    url: row.get("url").unwrap_or_default(),
                    cause_heat: row.get("cause_heat").unwrap_or(0.0),
                    lat: row.get("lat").ok(),
                    lng: row.get("lng").ok(),
                    embedding,
                    published_at: row.get("published_at").ok(),
                });
            }
        }

        Ok(signals)
    }

    /// Load all situations as weaving candidates.
    pub async fn load_weave_candidates(&self) -> Result<Vec<WeaveCandidate>, neo4rs::Error> {
        let g = &self.client;
        let mut candidates = Vec::new();

        let q = query(
            "MATCH (s:Situation)
             RETURN s.id AS id, s.headline AS headline,
                    s.structured_state AS structured_state,
                    s.narrative_embedding AS narrative_embedding,
                    s.causal_embedding AS causal_embedding,
                    s.arc AS arc",
        );

        let mut stream = g.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let id = match uuid::Uuid::parse_str(&id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };

            candidates.push(WeaveCandidate {
                id,
                headline: row.get("headline").unwrap_or_default(),
                structured_state: row.get("structured_state").unwrap_or_default(),
                narrative_embedding: row.get("narrative_embedding").unwrap_or_default(),
                causal_embedding: row.get("causal_embedding").unwrap_or_default(),
                arc: row.get("arc").unwrap_or_default(),
            });
        }

        Ok(candidates)
    }

    /// Find all situations that have signals from this scout run.
    pub async fn find_affected_situations(
        &self,
        scout_run_id: &str,
    ) -> Result<Vec<uuid::Uuid>, neo4rs::Error> {
        let g = &self.client;
        let mut situations = Vec::new();

        let q = query(
            "MATCH (sig)-[:PART_OF]->(s:Situation)
             WHERE sig.scout_run_id = $run_id
             RETURN DISTINCT s.id AS id",
        )
        .param("run_id", scout_run_id);

        let mut stream = g.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            if let Ok(id) = uuid::Uuid::parse_str(&id_str) {
                situations.push(id);
            }
        }

        Ok(situations)
    }

    /// Fetch unverified dispatches for post-hoc verification.
    pub async fn unverified_dispatches(
        &self,
        limit: usize,
    ) -> Result<Vec<(uuid::Uuid, String)>, neo4rs::Error> {
        let g = &self.client;
        let mut dispatches = Vec::new();

        let q = query(
            "MATCH (d:Dispatch)
             WHERE d.flagged_for_review = false
               AND d.fidelity_score IS NULL
             RETURN d.id AS id, d.body AS body
             ORDER BY d.created_at DESC
             LIMIT $limit",
        )
        .param("limit", limit as i64);

        let mut stream = g.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let id = match uuid::Uuid::parse_str(&id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };
            let body: String = row.get("body").unwrap_or_default();
            dispatches.push((id, body));
        }

        Ok(dispatches)
    }

    /// Check if signal IDs exist in the graph. Returns missing IDs.
    pub async fn check_signal_ids_exist(
        &self,
        signal_ids: &[uuid::Uuid],
    ) -> Result<Vec<uuid::Uuid>, neo4rs::Error> {
        let g = &self.client;
        let mut missing = Vec::new();

        for id in signal_ids {
            let q = query(
                "MATCH (n) WHERE n.id = $id
                   AND (n:Gathering OR n:Resource OR n:HelpRequest OR n:Announcement OR n:Concern OR n:Condition)
                 RETURN n.id AS found",
            )
            .param("id", id.to_string());

            let mut stream = g.execute(q).await?;
            if stream.next().await?.is_none() {
                missing.push(*id);
            }
        }

        Ok(missing)
    }

    /// Batch-load evidence per signal for diversity computation.
    /// Returns (signal_id, self_url, evidence_pairs) per signal.
    pub async fn signal_evidence_for_diversity(
        &self,
        label: &str,
    ) -> Result<Vec<(Uuid, String, Vec<(String, String)>)>, neo4rs::Error> {
        let g = &self.client;
        let q = query(&format!(
            "MATCH (n:{label})
             OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Citation)
             RETURN n.id AS id, n.url AS self_url,
                    collect({{url: ev.source_url, channel: coalesce(ev.channel_type, 'press')}}) AS evidence"
        ));

        let mut rows = Vec::new();
        let mut stream = g.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let id = match Uuid::parse_str(&id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };
            let self_url: String = row.get("self_url").unwrap_or_default();
            let evidence: Vec<neo4rs::BoltMap> = row.get("evidence").unwrap_or_default();

            let ev_pairs: Vec<(String, String)> = evidence
                .iter()
                .filter_map(|ev| {
                    let url: String = ev.get("url").unwrap_or_default();
                    if url.is_empty() {
                        return None;
                    }
                    let channel: String = ev
                        .get::<String>("channel")
                        .unwrap_or_else(|_| "press".to_string());
                    Some((url, channel))
                })
                .collect();

            rows.push((id, self_url, ev_pairs));
        }

        Ok(rows)
    }

    /// Count ACTED_IN edges per actor.
    pub async fn actor_signal_counts(&self) -> Result<Vec<(Uuid, u32)>, neo4rs::Error> {
        let g = &self.client;
        let q = query(
            "MATCH (a:Actor)-[r:ACTED_IN]->()
             WITH a, count(r) AS cnt
             RETURN a.id AS id, cnt",
        );

        let mut results = Vec::new();
        let mut stream = g.execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let id = match Uuid::parse_str(&id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };
            let cnt: i64 = row.get("cnt").unwrap_or(0);
            results.push((id, cnt as u32));
        }

        Ok(results)
    }
}

/// A signal discovered during weaving (returned by `discover_unassigned_signals`).
pub struct WeaveSignal {
    pub id: uuid::Uuid,
    pub title: String,
    pub summary: String,
    pub node_type: String,
    pub url: String,
    pub cause_heat: f64,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub embedding: Vec<f32>,
    pub published_at: Option<String>,
}

/// A candidate situation for weaving (returned by `load_weave_candidates`).
pub struct WeaveCandidate {
    pub id: uuid::Uuid,
    pub headline: String,
    pub structured_state: String,
    pub narrative_embedding: Vec<f32>,
    pub causal_embedding: Vec<f32>,
    pub arc: String,
}

/// Graph store for scout — reads and writes.
/// Derefs to `GraphReader` for read access; write methods live here.
#[derive(Clone)]
pub struct GraphStore {
    inner: GraphReader,
}

impl std::ops::Deref for GraphStore {
    type Target = GraphReader;
    fn deref(&self) -> &GraphReader {
        &self.inner
    }
}

impl GraphStore {
    pub fn new(client: GraphClient) -> Self {
        Self { inner: GraphReader::new(client) }
    }

    /// Reap expired signals from the graph. Runs at the start of each scout cycle.
    ///
    /// Deletes:
    /// - Non-recurring events whose end (or start) is past the grace period
    /// - Need signals older than NEED_EXPIRE_DAYS
    /// - Any signal not confirmed within FRESHNESS_MAX_DAYS (except ongoing gives, recurring events)
    ///
    /// Also detaches and deletes orphaned Evidence nodes.
    pub async fn reap_expired(&self) -> Result<ReapStats, neo4rs::Error> {
        let mut stats = ReapStats::default();

        // 1. Past non-recurring events (only those with a known start date)
        let q = query(&format!(
            "MATCH (n:Gathering)
             WHERE n.is_recurring = false
               AND n.starts_at IS NOT NULL AND n.starts_at <> ''
               AND CASE
                   WHEN n.ends_at IS NOT NULL AND n.ends_at <> ''
                   THEN datetime(n.ends_at) < datetime() - duration('PT{}H')
                   ELSE datetime(n.starts_at) < datetime() - duration('PT{}H')
               END
             OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Citation)
             DETACH DELETE n, ev
             RETURN count(DISTINCT n) AS deleted",
            GATHERING_PAST_GRACE_HOURS, GATHERING_PAST_GRACE_HOURS
        ));
        if let Some(row) = self.client().execute(q).await?.next().await? {
            stats.gatherings = row.get::<i64>("deleted").unwrap_or(0) as u64;
        }

        // 2. Expired needs
        let q = query(&format!(
            "MATCH (n:HelpRequest)
             WHERE datetime(n.extracted_at) < datetime() - duration('P{}D')
             OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Citation)
             DETACH DELETE n, ev
             RETURN count(DISTINCT n) AS deleted",
            NEED_EXPIRE_DAYS
        ));
        if let Some(row) = self.client().execute(q).await?.next().await? {
            stats.needs = row.get::<i64>("deleted").unwrap_or(0) as u64;
        }

        // 3. Expired notices
        let q = query(&format!(
            "MATCH (n:Announcement)
             WHERE datetime(n.extracted_at) < datetime() - duration('P{}D')
             OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Citation)
             DETACH DELETE n, ev
             RETURN count(DISTINCT n) AS deleted",
            NOTICE_EXPIRE_DAYS
        ));
        if let Some(row) = self.client().execute(q).await?.next().await? {
            stats.stale += row.get::<i64>("deleted").unwrap_or(0) as u64;
        }

        // 4. Stale unconfirmed signals (all signals must be re-confirmed within FRESHNESS_MAX_DAYS)
        for label in &["Resource", "Concern"] {
            let q = query(&format!(
                "MATCH (n:{label})
                 WHERE datetime(n.last_confirmed_active) < datetime() - duration('P{days}D')
                 OPTIONAL MATCH (n)-[:SOURCED_FROM]->(ev:Citation)
                 DETACH DELETE n, ev
                 RETURN count(DISTINCT n) AS deleted",
                label = label,
                days = FRESHNESS_MAX_DAYS,
            ));
            if let Some(row) = self.client().execute(q).await?.next().await? {
                stats.stale += row.get::<i64>("deleted").unwrap_or(0) as u64;
            }
        }

        let total = stats.gatherings + stats.needs + stats.stale;
        if total > 0 {
            info!(
                gatherings = stats.gatherings,
                needs = stats.needs,
                stale = stats.stale,
                "Reaped expired signals"
            );
        }

        Ok(stats)
    }

    /// Delete all nodes sourced from a given URL (opt-out support).
    pub async fn delete_by_url(&self, url: &str) -> Result<u64, neo4rs::Error> {
        // Delete evidence nodes linked to signals from this URL, then the signals themselves
        let q = query(
            "MATCH (n)-[:SOURCED_FROM]->(ev:Citation)
             WHERE n.url = $url
             DETACH DELETE n, ev
             RETURN count(*) AS deleted",
        )
        .param("url", url);

        let mut stream = self.client().execute(q).await?;
        let deleted = if let Some(row) = stream.next().await? {
            row.get::<i64>("deleted").unwrap_or(0) as u64
        } else {
            0
        };

        warn!(%url, deleted, "Deleted nodes by source URL (opt-out)");
        Ok(deleted)
    }

    // --- Demand Signal operations (Driver A) ---

    /// Store a raw demand signal from a user search.
    pub async fn upsert_demand_signal(&self, signal: &DemandSignal) -> Result<(), neo4rs::Error> {
        let q = query(
            "MERGE (d:DemandSignal {id: $id})
             SET d.query = $query,
                 d.center_lat = $center_lat,
                 d.center_lng = $center_lng,
                 d.radius_km = $radius_km,
                 d.created_at = datetime($created_at)",
        )
        .param("id", signal.id.to_string())
        .param("query", signal.query.as_str())
        .param("center_lat", signal.center_lat)
        .param("center_lng", signal.center_lng)
        .param("radius_km", signal.radius_km)
        .param("created_at", format_datetime(&signal.created_at));

        self.client().run(q).await?;
        info!(id = %signal.id, query = signal.query.as_str(), "DemandSignal stored");
        Ok(())
    }

    // --- Source operations (emergent source discovery) ---

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
                submitted_at: datetime($submitted_at)
            })
            WITH sub
            MATCH (s:Source {canonical_key: $canonical_key})
            MERGE (sub)-[:SUBMITTED_FOR]->(s)",
        )
        .param("id", submission.id.to_string())
        .param("url", submission.url.as_str())
        .param("reason", submission.reason.clone().unwrap_or_default())
        .param("submitted_at", format_datetime(&submission.submitted_at))
        .param("canonical_key", source_canonical_key);

        self.client().run(q).await?;
        Ok(())
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
                     s.scrape_count = coalesce(s.scrape_count, 0) + 1",
            )
            .param("key", canonical_key)
            .param("now", format_datetime(&now))
            .param("count", signals_produced as i64);
            self.client().run(q).await?;
        } else {
            let q = query(
                "MATCH (s:Source {canonical_key: $key})
                 SET s.last_scraped = datetime($now),
                     s.consecutive_empty_runs = s.consecutive_empty_runs + 1,
                     s.scrape_count = coalesce(s.scrape_count, 0) + 1",
            )
            .param("key", canonical_key)
            .param("now", format_datetime(&now));
            self.client().run(q).await?;
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
             SET s.weight = $weight, s.cadence_hours = $cadence",
        )
        .param("key", canonical_key)
        .param("weight", weight)
        .param("cadence", cadence_hours as i64);
        self.client().run(q).await?;
        Ok(())
    }

    /// Deactivate specific sources by their UUIDs (operator-initiated).
    pub async fn deactivate_sources_by_id(&self, ids: &[Uuid]) -> Result<u32, neo4rs::Error> {
        if ids.is_empty() {
            return Ok(0);
        }
        let id_strings: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
        let q = query(
            "UNWIND $ids AS sid
             MATCH (s:Source {id: sid, active: true})
             SET s.active = false
             RETURN count(s) AS deactivated",
        )
        .param("ids", id_strings);

        let mut stream = self.client().execute(q).await?;
        if let Some(row) = stream.next().await? {
            Ok(row.get::<i64>("deactivated").unwrap_or(0) as u32)
        } else {
            Ok(0)
        }
    }

    /// Deactivate sources that have had too many consecutive empty runs.
    /// Protects curated and human-submitted sources.
    pub async fn deactivate_dead_sources(&self, max_empty_runs: u32) -> Result<u32, neo4rs::Error> {
        let q = query(
            "MATCH (s:Source {active: true})
             WHERE s.consecutive_empty_runs >= $max
               AND s.discovery_method <> 'curated'
               AND s.discovery_method <> 'human_submission'
             SET s.active = false
             RETURN count(s) AS deactivated",
        )
        .param("max", max_empty_runs as i64);

        let mut stream = self.client().execute(q).await?;
        if let Some(row) = stream.next().await? {
            Ok(row.get::<i64>("deactivated").unwrap_or(0) as u32)
        } else {
            Ok(0)
        }
    }

    /// Deactivate web query sources that have proven unproductive.
    /// Stricter criteria than general `deactivate_dead_sources`:
    /// - 5+ consecutive empty runs (backoff has already slowed them)
    /// - 3+ total scrapes (gave it a fair chance)
    /// - 0 signals ever produced (never contributed anything)
    /// Protects curated and human-submitted sources.
    pub async fn deactivate_dead_web_queries(&self) -> Result<u32, neo4rs::Error> {
        let q = query(
            "MATCH (s:Source {active: true})
             WHERE NOT (s.canonical_value STARTS WITH 'http://' OR s.canonical_value STARTS WITH 'https://')
               AND s.consecutive_empty_runs >= 5
               AND coalesce(s.scrape_count, 0) >= 3
               AND s.signals_produced = 0
               AND coalesce(s.sources_discovered, 0) = 0
               AND s.discovery_method <> 'curated'
               AND s.discovery_method <> 'human_submission'
             SET s.active = false
             RETURN count(s) AS deactivated",
        );

        let mut stream = self.client().execute(q).await?;
        if let Some(row) = stream.next().await? {
            Ok(row.get::<i64>("deactivated").unwrap_or(0) as u32)
        } else {
            Ok(0)
        }
    }

    /// Store an embedding on a Source node's `query_embedding` property.
    /// Used after creating a new WebQuery source so it can be found by
    /// `find_similar_query` on subsequent runs.
    pub async fn set_query_embedding(
        &self,
        canonical_key: &str,
        embedding: &[f32],
    ) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (s:Source {canonical_key: $key})
             SET s.query_embedding = $embedding",
        )
        .param("key", canonical_key)
        .param("embedding", embedding.to_vec());
        self.client().run(q).await?;
        Ok(())
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
                 a.last_active = datetime($now)",
        )
        .param("id", actor_id.to_string())
        .param("now", format_datetime(&now));

        self.client().run(q).await?;
        Ok(())
    }

    // --- Response mapping operations ---

    /// Create a RESPONDS_TO edge between a Aid/Gathering signal and a Tension.
    pub async fn create_response_edge(
        &self,
        responder_id: Uuid,
        concern_id: Uuid,
        match_strength: f64,
        explanation: &str,
    ) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (resp) WHERE resp.id = $resp_id AND (resp:Resource OR resp:Gathering OR resp:HelpRequest)
             MATCH (t:Concern {id: $concern_id})
             MERGE (resp)-[:RESPONDS_TO {match_strength: $strength, explanation: $explanation}]->(t)"
        )
        .param("resp_id", responder_id.to_string())
        .param("concern_id", concern_id.to_string())
        .param("strength", match_strength)
        .param("explanation", explanation);

        self.client().run(q).await?;
        Ok(())
    }

    /// Create a Pin node. MERGE on (source_id, location_lat, location_lng) for idempotency.
    pub async fn create_pin(&self, pin: &PinNode) -> Result<(), neo4rs::Error> {
        let q = query(
            "MERGE (p:Pin {source_id: $source_id, location_lat: $location_lat, location_lng: $location_lng})
             ON CREATE SET
                p.id = $id,
                p.created_by = $created_by,
                p.created_at = datetime($created_at)",
        )
        .param("id", pin.id.to_string())
        .param("source_id", pin.source_id.to_string())
        .param("location_lat", pin.location_lat)
        .param("location_lng", pin.location_lng)
        .param("created_by", pin.created_by.as_str())
        .param("created_at", format_datetime(&pin.created_at));

        self.client().run(q).await?;
        Ok(())
    }

    /// Delete pins by ID. Uses UNWIND for batch deletion.
    pub async fn delete_pins(&self, pin_ids: &[Uuid]) -> Result<(), neo4rs::Error> {
        if pin_ids.is_empty() {
            return Ok(());
        }
        let ids: Vec<String> = pin_ids.iter().map(|id| id.to_string()).collect();
        let q = query(
            "UNWIND $ids AS pid
             MATCH (p:Pin {id: pid})
             DETACH DELETE p",
        )
        .param("ids", ids);

        self.client().run(q).await?;
        Ok(())
    }

    /// Boost source weights for sources that contributed signals evidencing a hot situation.
    /// The boost is multiplicative (e.g. factor=1.2 means 20% increase), capped at 5.0.
    pub async fn boost_sources_for_situation_headline(
        &self,
        headline: &str,
        factor: f64,
    ) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (sig)-[:PART_OF]->(s:Situation {headline: $headline})
             WITH collect(DISTINCT sig.url) AS urls
             UNWIND urls AS url
             MATCH (src:Source {active: true})
             WHERE src.url = url AND src.weight IS NOT NULL
             SET src.weight = CASE WHEN src.weight * $factor > 5.0 THEN 5.0 ELSE src.weight * $factor END",
        )
        .param("headline", headline)
        .param("factor", factor);
        self.client().run(q).await?;
        Ok(())
    }

    /// Queue signals from emerging/fuzzy situations for re-investigation by the tension linker.
    /// Uses a 7-day cooldown per situation to avoid repeated re-triggering.
    /// Returns the number of signals queued.
    pub async fn trigger_situation_curiosity(&self) -> Result<u32, neo4rs::Error> {
        // Find situations that are emerging or fuzzy, haven't been curiosity-triggered in 7 days
        let q = query(
            "MATCH (sig)-[:PART_OF]->(s:Situation)
             WHERE (s.arc = 'emerging' OR s.clarity = 'Fuzzy')
               AND s.temperature >= 0.3
               AND s.sensitivity <> 'SENSITIVE' AND s.sensitivity <> 'RESTRICTED'
               AND (s.curiosity_triggered_at IS NULL
                    OR datetime(s.curiosity_triggered_at) < datetime() - duration('P7D'))
             WITH s, collect(sig) AS signals
             LIMIT 5
             UNWIND signals AS sig
             WITH s, sig
             WHERE (sig.curiosity_investigated IS NULL OR sig.curiosity_investigated = 'failed')
               AND NOT sig:Concern
             SET sig.curiosity_investigated = NULL
             WITH DISTINCT s
             SET s.curiosity_triggered_at = datetime()
             RETURN count(s) AS triggered",
        );
        let mut stream = self.client().execute(q).await?;
        if let Some(row) = stream.next().await? {
            Ok(row.get::<i64>("triggered").unwrap_or(0) as u32)
        } else {
            Ok(0)
        }
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
            NodeType::Gathering => "Gathering",
            NodeType::Resource => "Resource",
            NodeType::HelpRequest => "HelpRequest",
            NodeType::Announcement => "Announcement",
            NodeType::Concern => "Concern",
            NodeType::Condition => "Condition",
            NodeType::Citation => return Ok(()),
        };

        let q = query(&format!(
            "MATCH (n:{} {{id: $id}})
             SET n.confidence = $confidence",
            label
        ))
        .param("id", signal_id.to_string())
        .param("confidence", new_confidence as f64);

        self.client().run(q).await?;
        Ok(())
    }

    /// Mark a tension as having been scouted for responses.
    pub async fn mark_response_found(&self, concern_id: Uuid) -> Result<(), neo4rs::Error> {
        let now = format_datetime(&Utc::now());
        let q = query(
            "MATCH (t:Concern {id: $id})
             SET t.response_scouted_at = $now",
        )
        .param("id", concern_id.to_string())
        .param("now", now);

        self.client().run(q).await
    }

    // =============================================================================
    // Gravity Scout operations
    // =============================================================================

    /// Create a DRAWN_TO edge between a gathering signal and a Tension.
    /// Uses MERGE with ON CREATE/ON MATCH for defensive idempotency.
    pub async fn create_drawn_to_edge(
        &self,
        signal_id: Uuid,
        concern_id: Uuid,
        match_strength: f64,
        explanation: &str,
        gathering_type: &str,
    ) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (resp) WHERE resp.id = $resp_id AND (resp:Resource OR resp:Gathering OR resp:HelpRequest)
             MATCH (t:Concern {id: $concern_id})
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
        .param("concern_id", concern_id.to_string())
        .param("strength", match_strength)
        .param("explanation", explanation)
        .param("gathering_type", gathering_type);

        self.client().run(q).await?;
        Ok(())
    }

    // ─── Resource Capability Matching ────────────────────────────────

    /// Find or create a Resource node, deduplicating on slug.
    /// Returns the Resource's UUID (existing or newly created).
    pub async fn find_or_create_resource(
        &self,
        name: &str,
        slug: &str,
        description: &str,
        embedding: &[f32],
    ) -> Result<Uuid, neo4rs::Error> {
        let new_id = Uuid::new_v4();
        let now = format_datetime(&Utc::now());
        let emb = embedding_to_f64(embedding);

        let q = query(
            "MERGE (r:Resource {slug: $slug})
             ON CREATE SET
                 r.id = $id,
                 r.name = $name,
                 r.description = $description,
                 r.embedding = $embedding,
                 r.signal_count = 1,
                 r.sensitivity = 'general',
                 r.confidence = 0.5,
                 r.created_at = datetime($now),
                 r.last_seen = datetime($now)
             ON MATCH SET
                 r.signal_count = r.signal_count + 1,
                 r.last_seen = datetime($now)
             RETURN r.id AS resource_id",
        )
        .param("slug", slug)
        .param("id", new_id.to_string())
        .param("name", name)
        .param("description", description)
        .param("embedding", emb)
        .param("now", now);

        let mut stream = self.client().execute(q).await?;
        if let Some(row) = stream.next().await? {
            let id_str: String = row.get("resource_id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                return Ok(id);
            }
        }
        Ok(new_id)
    }

    /// Create a REQUIRES edge from a signal (Need/Gathering) to a Resource.
    /// Uses MERGE for idempotency; updates properties on match.
    pub async fn create_requires_edge(
        &self,
        signal_id: Uuid,
        resource_id: Uuid,
        confidence: f32,
        quantity: Option<&str>,
        notes: Option<&str>,
    ) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (s) WHERE s.id = $sid AND (s:HelpRequest OR s:Gathering)
             MATCH (r:Resource {id: $rid})
             MERGE (s)-[e:REQUIRES]->(r)
             ON CREATE SET
                 e.confidence = $conf,
                 e.quantity = $qty,
                 e.notes = $notes
             ON MATCH SET
                 e.confidence = $conf,
                 e.quantity = $qty,
                 e.notes = $notes",
        )
        .param("sid", signal_id.to_string())
        .param("rid", resource_id.to_string())
        .param("conf", confidence as f64)
        .param("qty", quantity.unwrap_or(""))
        .param("notes", notes.unwrap_or(""));

        self.client().run(q).await?;
        Ok(())
    }

    /// Create a PREFERS edge from a signal (Need/Gathering) to a Resource.
    /// Uses MERGE for idempotency.
    pub async fn create_prefers_edge(
        &self,
        signal_id: Uuid,
        resource_id: Uuid,
        confidence: f32,
    ) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (s) WHERE s.id = $sid AND (s:HelpRequest OR s:Gathering)
             MATCH (r:Resource {id: $rid})
             MERGE (s)-[e:PREFERS]->(r)
             ON CREATE SET e.confidence = $conf
             ON MATCH SET e.confidence = $conf",
        )
        .param("sid", signal_id.to_string())
        .param("rid", resource_id.to_string())
        .param("conf", confidence as f64);

        self.client().run(q).await?;
        Ok(())
    }

    /// Create an OFFERS edge from an Aid signal to a Resource.
    /// Uses MERGE for idempotency.
    pub async fn create_offers_edge(
        &self,
        signal_id: Uuid,
        resource_id: Uuid,
        confidence: f32,
        capacity: Option<&str>,
    ) -> Result<(), neo4rs::Error> {
        let q = query(
            "MATCH (s:Resource {id: $sid})
             MATCH (r:Resource {id: $rid})
             MERGE (s)-[e:OFFERS]->(r)
             ON CREATE SET
                 e.confidence = $conf,
                 e.capacity = $cap
             ON MATCH SET
                 e.confidence = $conf,
                 e.capacity = $cap",
        )
        .param("sid", signal_id.to_string())
        .param("rid", resource_id.to_string())
        .param("conf", confidence as f64)
        .param("cap", capacity.unwrap_or(""));

        self.client().run(q).await?;
        Ok(())
    }

    /// Merge near-duplicate Resource nodes based on embedding similarity.
    /// Picks the highest signal_count as canonical, re-points edges, deletes duplicates.
    pub async fn consolidate_resources(
        &self,
        threshold: f64,
    ) -> Result<ConsolidationStats, neo4rs::Error> {
        let mut stats = ConsolidationStats::default();

        // Load all resources with embeddings
        let q = query(
            "MATCH (r:Resource)
             WHERE r.embedding IS NOT NULL AND r.slug IS NOT NULL
             RETURN r.id AS id, r.slug AS slug, r.embedding AS emb, r.signal_count AS sc
             ORDER BY r.signal_count DESC",
        );

        struct ResourceEmbed {
            id: String,
            slug: String,
            embedding: Vec<f64>,
        }

        let mut resources: Vec<ResourceEmbed> = Vec::new();
        let mut stream = self.client().execute(q).await?;
        while let Some(row) = stream.next().await? {
            let id: String = row.get("id").unwrap_or_default();
            let slug: String = row.get("slug").unwrap_or_default();
            let embedding: Vec<f64> = row.get("emb").unwrap_or_default();
            let _signal_count: i64 = row.get("sc").unwrap_or(0);
            if !embedding.is_empty() {
                resources.push(ResourceEmbed {
                    id,
                    slug,
                    embedding,
                });
            }
        }

        if resources.len() < 2 {
            return Ok(stats);
        }

        // Find clusters: canonical (highest signal_count) absorbs duplicates
        let mut absorbed: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut merges: Vec<(String, String)> = Vec::new(); // (canonical_id, dup_id)

        for i in 0..resources.len() {
            if absorbed.contains(&resources[i].id) {
                continue;
            }
            for j in (i + 1)..resources.len() {
                if absorbed.contains(&resources[j].id) {
                    continue;
                }
                let sim = cosine_sim_f64(&resources[i].embedding, &resources[j].embedding);
                if sim >= threshold {
                    // resources[i] has higher signal_count (sorted DESC)
                    absorbed.insert(resources[j].id.clone());
                    merges.push((resources[i].id.clone(), resources[j].id.clone()));
                    info!(
                        canonical = resources[i].slug.as_str(),
                        duplicate = resources[j].slug.as_str(),
                        similarity = sim,
                        "Resource merge candidate"
                    );
                }
            }
        }

        if merges.is_empty() {
            return Ok(stats);
        }

        stats.clusters_found = merges.len() as u32;

        for (canonical_id, dup_id) in &merges {
            // Re-point REQUIRES edges
            let q = query(
                "MATCH (dup:Resource {id: $dup_id})<-[old:REQUIRES]-(s)
                 MATCH (canonical:Resource {id: $canonical_id})
                 MERGE (s)-[new:REQUIRES]->(canonical)
                 ON CREATE SET new.confidence = old.confidence, new.quantity = old.quantity, new.notes = old.notes
                 WITH old
                 DELETE old
                 RETURN count(*) AS moved",
            )
            .param("dup_id", dup_id.as_str())
            .param("canonical_id", canonical_id.as_str());
            if let Ok(mut s) = self.client().execute(q).await {
                if let Some(Ok(row)) = s.next().await.ok().flatten().map(Ok::<_, neo4rs::Error>) {
                    stats.edges_redirected += row.get::<i64>("moved").unwrap_or(0) as u32;
                }
            }

            // Re-point PREFERS edges
            let q = query(
                "MATCH (dup:Resource {id: $dup_id})<-[old:PREFERS]-(s)
                 MATCH (canonical:Resource {id: $canonical_id})
                 MERGE (s)-[new:PREFERS]->(canonical)
                 ON CREATE SET new.confidence = old.confidence
                 WITH old
                 DELETE old
                 RETURN count(*) AS moved",
            )
            .param("dup_id", dup_id.as_str())
            .param("canonical_id", canonical_id.as_str());
            if let Ok(mut s) = self.client().execute(q).await {
                if let Some(Ok(row)) = s.next().await.ok().flatten().map(Ok::<_, neo4rs::Error>) {
                    stats.edges_redirected += row.get::<i64>("moved").unwrap_or(0) as u32;
                }
            }

            // Re-point OFFERS edges
            let q = query(
                "MATCH (dup:Resource {id: $dup_id})<-[old:OFFERS]-(s)
                 MATCH (canonical:Resource {id: $canonical_id})
                 MERGE (s)-[new:OFFERS]->(canonical)
                 ON CREATE SET new.confidence = old.confidence, new.capacity = old.capacity
                 WITH old
                 DELETE old
                 RETURN count(*) AS moved",
            )
            .param("dup_id", dup_id.as_str())
            .param("canonical_id", canonical_id.as_str());
            if let Ok(mut s) = self.client().execute(q).await {
                if let Some(Ok(row)) = s.next().await.ok().flatten().map(Ok::<_, neo4rs::Error>) {
                    stats.edges_redirected += row.get::<i64>("moved").unwrap_or(0) as u32;
                }
            }

            // Sum signal_count into canonical
            let q = query(
                "MATCH (dup:Resource {id: $dup_id}), (canonical:Resource {id: $canonical_id})
                 SET canonical.signal_count = canonical.signal_count + dup.signal_count",
            )
            .param("dup_id", dup_id.as_str())
            .param("canonical_id", canonical_id.as_str());
            self.client().run(q).await?;

            // Delete the duplicate
            let q =
                query("MATCH (r:Resource {id: $id}) DETACH DELETE r").param("id", dup_id.as_str());
            self.client().run(q).await?;

            stats.nodes_merged += 1;
            info!(canonical_id, dup_id, "Merged duplicate resource");
        }

        info!(
            clusters = stats.clusters_found,
            merged = stats.nodes_merged,
            edges = stats.edges_redirected,
            "Resource consolidation complete"
        );
        Ok(stats)
    }

    /// Aggregate tags from a situation's constituent signals.
    /// Tags appearing on 2+ signals bubble up to the situation.
    /// Respects SUPPRESSED_TAG edges (admin-removed tags won't reappear).
    pub async fn aggregate_situation_tags(&self, situation_id: Uuid) -> Result<(), neo4rs::Error> {
        let now = format_datetime(&Utc::now());

        let q = query(
            "MATCH (s:Situation {id: $sid})<-[:PART_OF]-(sig)-[:TAGGED]->(t:Tag)
             WITH s, t, count(sig) AS freq
             WHERE freq >= 2
               AND NOT (s)-[:SUPPRESSED_TAG]->(t)
             MERGE (s)-[r:TAGGED]->(t)
               ON CREATE SET r.assigned_at = datetime($now)",
        )
        .param("sid", situation_id.to_string())
        .param("now", now);

        self.client().run(q).await
    }

    /// Remove a tag from a situation: delete TAGGED edge + create SUPPRESSED_TAG.
    /// This prevents auto-aggregation from re-adding the tag.
    pub async fn suppress_situation_tag(
        &self,
        situation_id: Uuid,
        tag_slug: &str,
    ) -> Result<(), neo4rs::Error> {
        let now = format_datetime(&Utc::now());

        let q = query(
            "MATCH (s:Situation {id: $sid})-[r:TAGGED]->(t:Tag {slug: $slug})
             DELETE r
             MERGE (s)-[sup:SUPPRESSED_TAG]->(t)
               ON CREATE SET sup.suppressed_at = datetime($now)",
        )
        .param("sid", situation_id.to_string())
        .param("slug", tag_slug)
        .param("now", now);

        self.client().run(q).await
    }

    /// Merge source tag into target tag. Atomic: repoints all edges, deletes source.
    pub async fn merge_tags(
        &self,
        source_slug: &str,
        target_slug: &str,
    ) -> Result<(), neo4rs::Error> {
        // Repoint TAGGED edges
        let q1 = query(
            "MATCH (src:Tag {slug: $source}), (tgt:Tag {slug: $target})
             WITH src, tgt
             OPTIONAL MATCH (n)-[old:TAGGED]->(src)
             WITH src, tgt, n, old
             WHERE old IS NOT NULL
             MERGE (n)-[:TAGGED]->(tgt)
             DELETE old",
        )
        .param("source", source_slug)
        .param("target", target_slug);

        self.client().run(q1).await?;

        // Repoint SUPPRESSED_TAG edges
        let q2 = query(
            "MATCH (src:Tag {slug: $source}), (tgt:Tag {slug: $target})
             WITH src, tgt
             OPTIONAL MATCH (s)-[old:SUPPRESSED_TAG]->(src)
             WITH src, tgt, s, old
             WHERE old IS NOT NULL
             MERGE (s)-[:SUPPRESSED_TAG]->(tgt)
             DELETE old",
        )
        .param("source", source_slug)
        .param("target", target_slug);

        self.client().run(q2).await?;

        // Delete source tag
        let q3 =
            query("MATCH (t:Tag {slug: $source}) DETACH DELETE t").param("source", source_slug);

        self.client().run(q3).await
    }

    // ========== Supervisor / Validation Issues ==========

    /// Dismiss a validation issue by ID.
    pub async fn dismiss_validation_issue(&self, id: &str) -> Result<bool, neo4rs::Error> {
        let q = query(
            "MATCH (v:ValidationIssue {id: $id})
             WHERE v.status = 'open'
             SET v.status = 'dismissed',
                 v.resolved_at = datetime(),
                 v.resolution = 'dismissed by admin'
             RETURN v.id AS id",
        )
        .param("id", id.to_string());

        let mut stream = self.client.execute(q).await?;
        Ok(stream.next().await?.is_some())
    }

}


/// Stats from resource consolidation batch job.
#[derive(Debug, Default)]
pub struct ConsolidationStats {
    pub clusters_found: u32,
    pub nodes_merged: u32,
    pub edges_redirected: u32,
}

#[derive(Debug, Default)]
pub struct ReapStats {
    pub gatherings: u64,
    pub needs: u64,
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
    pub url: String,
    pub canonical_key: String,
    pub similarity: f64,
}

// --- Discovery briefing types ---

/// A tension with its response coverage status.
#[derive(Debug, Clone)]
pub struct UnmetTension {
    pub title: String,
    pub severity: String,
    pub opposing: Option<String>,
    pub category: Option<String>,
    pub unmet: bool,
    pub corroboration_count: u32,
    pub source_diversity: u32,
    pub cause_heat: f64,
}

/// A brief summary of a situation for the discovery briefing.
#[derive(Debug, Clone)]
pub struct SituationBrief {
    pub headline: String,
    pub arc: String,
    pub temperature: f64,
    pub clarity: String,
    pub signal_count: u32,
    pub tension_count: u32,
    pub dispatch_count: u32,
    pub location_name: Option<String>,
    pub sensitivity: String,
}

/// Aggregate counts of each signal type.
#[derive(Debug, Clone, Default)]
pub struct SignalTypeCounts {
    pub gatherings: u32,
    pub aids: u32,
    pub needs: u32,
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
    pub relevance: String, // "DIRECT", "SUPPORTING", "CONTRADICTING"
    pub confidence: f32,
}

/// Aggregated stats for a gap_type strategy.
#[derive(Debug, Clone)]
pub struct GapTypeStats {
    pub gap_type: String,
    pub total_sources: u32,
    pub successful_sources: u32, // signals_produced > 0
    pub avg_weight: f64,
}

/// Extraction yield metrics grouped by source label (derived from URL domain).
#[derive(Debug, Clone)]
pub struct ExtractionYield {
    pub source_label: String,
    pub extracted: u32,    // from Source.signals_produced
    pub survived: u32,     // count of signals still in graph
    pub corroborated: u32, // from Source.signals_corroborated
    pub contradicted: u32, // signals with CONTRADICTING evidence
}

/// Response shape analysis for a tension — what types of responses exist and what's absent.
#[derive(Debug, Clone)]
pub struct ConcernResponseShape {
    pub title: String,
    pub opposing: Option<String>,
    pub cause_heat: f64,
    pub aid_count: u32,
    pub gathering_count: u32,
    pub need_count: u32,
    pub sample_titles: Vec<String>,
}

/// A signal that warrants investigation.
#[derive(Debug)]
pub struct InvestigationTarget {
    pub signal_id: Uuid,
    pub node_type: NodeType,
    pub title: String,
    pub summary: String,
    pub url: String,
    pub is_sensitive: bool,
}

/// A signal without tension context that the tension linker should investigate.
#[derive(Debug)]
pub struct ConcernLinkerTarget {
    pub signal_id: Uuid,
    pub title: String,
    pub summary: String,
    pub label: String,
    pub url: String,
}

/// Outcome of a tension linker investigation for a signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConcernLinkerOutcome {
    /// All tensions processed successfully.
    Done,
    /// LLM said "not curious" — permanent, won't retry.
    Skipped,
    /// Investigation or tension processing failed — eligible for retry.
    Failed,
    /// Retry cap hit (3 attempts) — permanent, signals a coverage gap.
    Abandoned,
}

impl ConcernLinkerOutcome {
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
pub struct ConcernHub {
    pub concern_id: Uuid,
    pub title: String,
    pub summary: String,
    pub category: Option<String>,
    pub opposing: Option<String>,
    pub cause_heat: f64,
    pub respondents: Vec<ConcernRespondent>,
}

/// A signal that responds to a tension, with edge metadata.
#[derive(Debug)]
pub struct ConcernRespondent {
    pub signal_id: Uuid,
    pub url: String,
    pub match_strength: f64,
    pub explanation: String,
    /// "RESPONDS_TO" or "DRAWN_TO" — raw Neo4j type(r) value
    pub edge_type: String,
    /// Only present for DRAWN_TO edges
    pub gathering_type: Option<String>,
}

// --- Response Finder types ---

/// A tension that needs response discovery.
#[derive(Debug)]
pub struct ResponseFinderTarget {
    pub concern_id: Uuid,
    pub title: String,
    pub summary: String,
    pub severity: String,
    pub category: Option<String>,
    pub opposing: Option<String>,
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

// --- Gathering Finder types ---

/// A tension that needs gathering discovery (where are people gathering?).
#[derive(Debug)]
pub struct GatheringFinderTarget {
    pub concern_id: Uuid,
    pub title: String,
    pub summary: String,
    pub severity: String,
    pub category: Option<String>,
    pub opposing: Option<String>,
    pub cause_heat: f64,
}

// --- Review / lint types ---

/// A field-level correction applied by the review pipeline.
#[derive(Debug, Clone)]
pub struct FieldCorrection {
    pub field: String,
    pub old_value: String,
    pub new_value: String,
    pub reason: String,
}

/// Data needed to infer severity for a single Notice.
#[derive(Debug)]
pub struct NoticeInferenceRow {
    pub notice_id: Uuid,
    pub severity: String,
    pub corroboration_count: u32,
    pub source_diversity: u32,
    pub has_evidence_of: bool,
    pub source: Option<SourceNode>,
}

/// A signal in `staged` review_status, awaiting lint verification.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StagedSignal {
    pub id: String,
    pub signal_type: String,
    pub title: String,
    pub summary: String,
    pub confidence: f64,
    pub url: String,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub location_name: Option<String>,
    pub starts_at: Option<String>,
    pub ends_at: Option<String>,
    pub published_at: Option<String>,
    pub action_url: Option<String>,
    pub organizer: Option<String>,
    pub sensitivity: Option<String>,
}

/// Compact signal summary for dashboards and LLM context.
#[derive(Debug, Clone)]
pub struct SignalBrief {
    pub id: Uuid,
    pub title: String,
    pub signal_type: String,
    pub confidence: f32,
    pub extracted_at: Option<DateTime<Utc>>,
    pub url: String,
    pub review_status: String,
    pub location_name: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
}

/// A node in the source discovery tree.
#[derive(Debug, Clone)]
pub struct DiscoveryTreeNode {
    pub id: Uuid,
    pub canonical_value: String,
    pub discovery_method: String,
    pub active: bool,
    pub signals_produced: u32,
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

fn embedding_to_f64(embedding: &[f32]) -> Vec<f64> {
    embedding.iter().map(|&v| v as f64).collect()
}

/// Format a DateTime<Utc> as a local datetime string without timezone offset.
/// Neo4j's datetime() requires "YYYY-MM-DDThh:mm:ss" format (no +00:00 suffix).
fn format_datetime(dt: &DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
}

/// Public version of format_datetime for use by other modules (e.g. story_weaver.rs).
pub fn format_datetime_pub(dt: &DateTime<Utc>) -> String {
    format_datetime(dt)
}

/// Parse a neo4rs Row (with aliased columns) into a SourceNode.
/// Returns None if the row is missing required fields (e.g. invalid UUID).
pub fn row_to_source_node(row: &neo4rs::Row) -> Option<SourceNode> {
    let id_str: String = row.get("id").unwrap_or_default();
    let id = Uuid::parse_str(&id_str).ok()?;

    let discovery_str: String = row.get("discovery_method").unwrap_or_default();
    let discovery_method = match discovery_str.as_str() {
        "gap_analysis" => DiscoveryMethod::GapAnalysis,
        "signal_reference" => DiscoveryMethod::SignalReference,
        "hashtag_discovery" => DiscoveryMethod::HashtagDiscovery,
        "cold_start" => DiscoveryMethod::ColdStart,
        "tension_seed" => DiscoveryMethod::ConcernSeed,
        "human_submission" => DiscoveryMethod::HumanSubmission,
        "signal_expansion" => DiscoveryMethod::SignalExpansion,
        "actor_account" => DiscoveryMethod::ActorAccount,
        "social_graph_follow" => DiscoveryMethod::SocialGraphFollow,
        "linked_from" => DiscoveryMethod::LinkedFrom,
        _ => DiscoveryMethod::Curated,
    };

    let created_at = row_datetime_opt(row, "created_at").unwrap_or_else(Utc::now);
    let last_scraped = row_datetime_opt(row, "last_scraped");
    let last_produced_signal = row_datetime_opt(row, "last_produced_signal");
    let gap_context: String = row.get("gap_context").unwrap_or_default();
    let url: String = row.get("url").unwrap_or_default();
    let cadence: i64 = row.get::<i64>("cadence_hours").unwrap_or(0);
    let canonical_value: String = row.get("canonical_value").unwrap_or_default();
    let channel_weights = {
        let has_cw = row.get::<f64>("cw_page").is_ok()
            || row.get::<f64>("cw_feed").is_ok()
            || row.get::<f64>("cw_media").is_ok();
        if has_cw {
            rootsignal_common::ChannelWeights {
                page: row.get("cw_page").unwrap_or(0.0),
                feed: row.get("cw_feed").unwrap_or(0.0),
                media: row.get("cw_media").unwrap_or(0.0),
                discussion: row.get("cw_discussion").unwrap_or(0.0),
                events: row.get("cw_events").unwrap_or(0.0),
            }
        } else {
            let value = if url.is_empty() {
                &canonical_value
            } else {
                &url
            };
            rootsignal_common::ChannelWeights::default_for(
                &rootsignal_common::scraping_strategy(value),
            )
        }
    };
    Some(SourceNode {
        id,
        canonical_key: row.get("canonical_key").unwrap_or_default(),
        canonical_value,
        url: if url.is_empty() { None } else { Some(url) },
        discovery_method,
        created_at,
        last_scraped,
        last_produced_signal,
        signals_produced: row.get::<i64>("signals_produced").unwrap_or(0) as u32,
        signals_corroborated: row.get::<i64>("signals_corroborated").unwrap_or(0) as u32,
        consecutive_empty_runs: row.get::<i64>("consecutive_empty_runs").unwrap_or(0) as u32,
        active: row.get("active").unwrap_or(true),
        gap_context: if gap_context.is_empty() {
            None
        } else {
            Some(gap_context)
        },
        weight: row.get("weight").unwrap_or(0.5),
        cadence_hours: if cadence > 0 {
            Some(cadence as u32)
        } else {
            None
        },
        avg_signals_per_scrape: row.get("avg_signals_per_scrape").unwrap_or(0.0),
        quality_penalty: row.get("quality_penalty").unwrap_or(1.0),
        source_role: SourceRole::from_str_loose(
            &row.get::<String>("source_role").unwrap_or_default(),
        ),
        scrape_count: row.get::<i64>("scrape_count").unwrap_or(0) as u32,
        sources_discovered: row.get::<i64>("sources_discovered").unwrap_or(0) as u32,
        discovered_from_key: None,
        channel_weights,
    })
}

/// Parse a neo4rs Row (with aliased columns) into a Region.
pub fn row_to_region(row: &neo4rs::Row) -> Region {
    let id_str: String = row.get("id").unwrap_or_default();

    Region {
        id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::nil()),
        name: row.get("name").unwrap_or_default(),
        center_lat: row.get("center_lat").unwrap_or(0.0),
        center_lng: row.get("center_lng").unwrap_or(0.0),
        radius_km: row.get("radius_km").unwrap_or(0.0),
        geo_terms: row.get("geo_terms").unwrap_or_default(),
        is_leaf: row.get("is_leaf").unwrap_or(true),
        created_at: row_datetime_opt(row, "created_at").unwrap_or_else(Utc::now),
    }
}

// Backwards-compatible aliases

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
    row.get::<String>(key)
        .ok()
        .and_then(|s| parse_datetime_opt(&s))
}

/// Public version of row_datetime_opt for use by cache.rs.
pub fn row_datetime_opt_pub(row: &neo4rs::Row, key: &str) -> Option<DateTime<Utc>> {
    row_datetime_opt(row, key)
}

// --- Situation / Dispatch writer methods ---


impl GraphStore {
    /// Create a Situation node in the graph. Returns the situation's UUID.
    pub async fn create_situation(
        &self,
        situation: &rootsignal_common::SituationNode,
        narrative_embedding: &[f32],
        causal_embedding: &[f32],
    ) -> Result<Uuid, neo4rs::Error> {
        let g = &self.client();

        let q = query(
            "CREATE (s:Situation {
                id: $id,
                headline: $headline,
                lede: $lede,
                arc: $arc,
                temperature: $temperature,
                tension_heat: $tension_heat,
                entity_velocity: $entity_velocity,
                amplification: $amplification,
                response_coverage: $response_coverage,
                clarity_need: $clarity_need,
                clarity: $clarity,
                centroid_lat: $centroid_lat,
                centroid_lng: $centroid_lng,
                location_name: $location_name,
                structured_state: $structured_state,
                signal_count: $signal_count,
                tension_count: $tension_count,
                dispatch_count: $dispatch_count,
                first_seen: datetime($first_seen),
                last_updated: datetime($last_updated),
                sensitivity: $sensitivity,
                category: $category,
                narrative_embedding: $narrative_embedding,
                causal_embedding: $causal_embedding
            })",
        )
        .param("id", situation.id.to_string())
        .param("headline", situation.headline.as_str())
        .param("lede", situation.lede.as_str())
        .param("arc", situation.arc.to_string())
        .param("temperature", situation.temperature)
        .param("tension_heat", situation.tension_heat)
        .param("entity_velocity", situation.entity_velocity)
        .param("amplification", situation.amplification)
        .param("response_coverage", situation.response_coverage)
        .param("clarity_need", situation.clarity_need)
        .param("clarity", situation.clarity.to_string())
        .param("centroid_lat", situation.centroid_lat.unwrap_or(0.0))
        .param("centroid_lng", situation.centroid_lng.unwrap_or(0.0))
        .param(
            "location_name",
            situation.location_name.as_deref().unwrap_or(""),
        )
        .param("structured_state", situation.structured_state.as_str())
        .param("signal_count", situation.signal_count as i64)
        .param("tension_count", situation.tension_count as i64)
        .param("dispatch_count", situation.dispatch_count as i64)
        .param("first_seen", situation.first_seen.to_rfc3339())
        .param("last_updated", situation.last_updated.to_rfc3339())
        .param("sensitivity", situation.sensitivity.as_str())
        .param("category", situation.category.as_deref().unwrap_or(""))
        .param("narrative_embedding", narrative_embedding.to_vec())
        .param("causal_embedding", causal_embedding.to_vec());

        g.run(q).await?;
        info!(id = %situation.id, headline = %situation.headline, "Created Situation node");
        Ok(situation.id)
    }

    /// Create a Dispatch node and link it to its Situation via HAS_DISPATCH.
    pub async fn create_dispatch(
        &self,
        dispatch: &rootsignal_common::DispatchNode,
    ) -> Result<Uuid, neo4rs::Error> {
        let g = &self.client();

        let signal_ids_json: Vec<String> = dispatch
            .signal_ids
            .iter()
            .map(|id| id.to_string())
            .collect();

        let q = query(
            "MATCH (s:Situation {id: $situation_id})
             CREATE (d:Dispatch {
                id: $id,
                situation_id: $situation_id,
                body: $body,
                signal_ids: $signal_ids,
                created_at: datetime($created_at),
                dispatch_type: $dispatch_type,
                supersedes: $supersedes,
                flagged_for_review: $flagged_for_review,
                flag_reason: $flag_reason,
                fidelity_score: $fidelity_score
             })
             CREATE (s)-[:HAS_DISPATCH {position: s.dispatch_count}]->(d)
             SET s.dispatch_count = s.dispatch_count + 1,
                 s.last_updated = datetime($created_at)",
        )
        .param("id", dispatch.id.to_string())
        .param("situation_id", dispatch.situation_id.to_string())
        .param("body", dispatch.body.as_str())
        .param("signal_ids", signal_ids_json)
        .param("created_at", dispatch.created_at.to_rfc3339())
        .param("dispatch_type", dispatch.dispatch_type.to_string())
        .param(
            "supersedes",
            dispatch
                .supersedes
                .map(|id| id.to_string())
                .unwrap_or_default(),
        )
        .param("flagged_for_review", dispatch.flagged_for_review)
        .param("flag_reason", dispatch.flag_reason.as_deref().unwrap_or(""))
        .param("fidelity_score", dispatch.fidelity_score.unwrap_or(-1.0));

        g.run(q).await?;
        info!(id = %dispatch.id, situation_id = %dispatch.situation_id, "Created Dispatch node");
        Ok(dispatch.id)
    }

    /// Create or update a PART_OF edge (signal → situation, many-to-many).
    pub async fn merge_evidence_edge(
        &self,
        signal_id: &Uuid,
        signal_label: &str,
        situation_id: &Uuid,
        match_confidence: f64,
    ) -> Result<(), neo4rs::Error> {
        let g = &self.client();

        let q = query(&format!(
            "MATCH (sig:{signal_label} {{id: $signal_id}})
             MATCH (sit:Situation {{id: $situation_id}})
             MERGE (sig)-[e:PART_OF]->(sit)
             SET e.assigned_at = datetime(),
                 e.match_confidence = $confidence,
                 e.debunked = false"
        ))
        .param("signal_id", signal_id.to_string())
        .param("situation_id", situation_id.to_string())
        .param("confidence", match_confidence);

        g.run(q).await?;
        Ok(())
    }

    /// Create CITES edges from a dispatch to its cited signals.
    pub async fn merge_cites_edges(
        &self,
        dispatch_id: &Uuid,
        signal_ids: &[Uuid],
    ) -> Result<(), neo4rs::Error> {
        let g = &self.client();

        for signal_id in signal_ids {
            let q = query(
                "MATCH (d:Dispatch {id: $dispatch_id})
                 MATCH (sig) WHERE sig.id = $signal_id
                   AND (sig:Gathering OR sig:Resource OR sig:HelpRequest OR sig:Announcement OR sig:Concern OR sig:Condition)
                 MERGE (d)-[:CITES]->(sig)",
            )
            .param("dispatch_id", dispatch_id.to_string())
            .param("signal_id", signal_id.to_string());

            g.run(q).await?;
        }
        Ok(())
    }

    /// Update a situation's structured_state JSON blob.
    pub async fn update_situation_state(
        &self,
        situation_id: &Uuid,
        structured_state: &str,
    ) -> Result<(), neo4rs::Error> {
        let g = &self.client();

        let q = query(
            "MATCH (s:Situation {id: $id})
             SET s.structured_state = $state,
                 s.last_updated = datetime()",
        )
        .param("id", situation_id.to_string())
        .param("state", structured_state);

        g.run(q).await
    }

    /// Update a situation's temperature components and derived arc.
    pub async fn update_situation_temperature(
        &self,
        situation_id: &Uuid,
        temperature: f64,
        tension_heat: f64,
        entity_velocity: f64,
        amplification: f64,
        response_coverage: f64,
        clarity_need: f64,
        arc: &rootsignal_common::SituationArc,
        clarity: &rootsignal_common::Clarity,
    ) -> Result<(), neo4rs::Error> {
        let g = &self.client();

        let q = query(
            "MATCH (s:Situation {id: $id})
             SET s.temperature = $temperature,
                 s.tension_heat = $tension_heat,
                 s.entity_velocity = $entity_velocity,
                 s.amplification = $amplification,
                 s.response_coverage = $response_coverage,
                 s.clarity_need = $clarity_need,
                 s.arc = $arc,
                 s.clarity = $clarity,
                 s.last_updated = datetime()",
        )
        .param("id", situation_id.to_string())
        .param("temperature", temperature)
        .param("tension_heat", tension_heat)
        .param("entity_velocity", entity_velocity)
        .param("amplification", amplification)
        .param("response_coverage", response_coverage)
        .param("clarity_need", clarity_need)
        .param("arc", arc.to_string())
        .param("clarity", clarity.to_string());

        g.run(q).await
    }

    /// Update a situation's dual embeddings.
    pub async fn update_situation_embedding(
        &self,
        situation_id: &Uuid,
        narrative_embedding: &[f32],
        causal_embedding: &[f32],
    ) -> Result<(), neo4rs::Error> {
        let g = &self.client();

        let q = query(
            "MATCH (s:Situation {id: $id})
             SET s.narrative_embedding = $narrative_embedding,
                 s.causal_embedding = $causal_embedding",
        )
        .param("id", situation_id.to_string())
        .param("narrative_embedding", narrative_embedding.to_vec())
        .param("causal_embedding", causal_embedding.to_vec());

        g.run(q).await
    }

    /// Flag a dispatch for review by post-hoc verification.
    pub async fn flag_dispatch_for_review(
        &self,
        dispatch_id: &Uuid,
        flag_reason: &str,
        fidelity_score: Option<f64>,
    ) -> Result<(), neo4rs::Error> {
        let g = &self.client();

        let q = query(
            "MATCH (d:Dispatch {id: $id})
             SET d.flagged_for_review = true,
                 d.flag_reason = $reason,
                 d.fidelity_score = $fidelity",
        )
        .param("id", dispatch_id.to_string())
        .param("reason", flag_reason)
        .param("fidelity", fidelity_score.unwrap_or(-1.0));

        g.run(q).await
    }

    // --- Actor location enrichment ---

    /// Update allowlisted fields on a signal node by ID.
    pub async fn update_signal_fields(
        &self,
        signal_id: Uuid,
        corrections: &[FieldCorrection],
    ) -> Result<(), neo4rs::Error> {
        if corrections.is_empty() {
            return Ok(());
        }

        let set_clauses: Vec<String> = corrections
            .iter()
            .filter_map(|c| {
                let field = match c.field.as_str() {
                    "title" | "summary" | "location_name" | "action_url" | "organizer"
                    | "what_needed" | "category" | "sensitivity" | "severity" => {
                        Some(c.field.as_str())
                    }
                    _ => None,
                };
                field.map(|f| format!("n.{f} = ${f}"))
            })
            .collect();

        if set_clauses.is_empty() {
            return Ok(());
        }

        let labels = ["Gathering", "Resource", "HelpRequest", "Announcement", "Concern", "Condition"];
        for label in &labels {
            let cypher = format!(
                "MATCH (n:{label}) WHERE n.id = $id SET {}",
                set_clauses.join(", ")
            );
            let mut q = query(&cypher).param("id", signal_id.to_string());

            for c in corrections {
                match c.field.as_str() {
                    "title" | "summary" | "location_name" | "action_url" | "organizer"
                    | "what_needed" | "category" | "sensitivity" | "severity" => {
                        q = q.param(c.field.as_str(), c.new_value.as_str());
                    }
                    _ => {}
                }
            }

            self.client().run(q).await?;
        }

        Ok(())
    }

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

    // --- ConcernLinkerOutcome tests ---

    #[test]
    fn curiosity_outcome_as_str_roundtrip() {
        assert_eq!(ConcernLinkerOutcome::Done.as_str(), "done");
        assert_eq!(ConcernLinkerOutcome::Skipped.as_str(), "skipped");
        assert_eq!(ConcernLinkerOutcome::Failed.as_str(), "failed");
        assert_eq!(ConcernLinkerOutcome::Abandoned.as_str(), "abandoned");
    }

    #[test]
    fn curiosity_outcome_equality() {
        assert_eq!(ConcernLinkerOutcome::Done, ConcernLinkerOutcome::Done);
        assert_ne!(ConcernLinkerOutcome::Done, ConcernLinkerOutcome::Failed);
        assert_ne!(
            ConcernLinkerOutcome::Failed,
            ConcernLinkerOutcome::Abandoned
        );
    }

    #[test]
    fn curiosity_outcome_is_copy() {
        let outcome = ConcernLinkerOutcome::Failed;
        let copied = outcome; // Copy
        assert_eq!(outcome, copied); // Both still usable
    }

    // --- ConcernHub / ConcernRespondent tests ---

    #[test]
    fn tension_hub_respondent_count() {
        let hub = ConcernHub {
            concern_id: Uuid::new_v4(),
            title: "Housing affordability crisis".to_string(),
            summary: "Rents rising faster than wages".to_string(),
            category: Some("housing".to_string()),
            opposing: Some("Rent stabilization policies".to_string()),
            cause_heat: 0.7,
            respondents: vec![
                ConcernRespondent {
                    signal_id: Uuid::new_v4(),
                    url: "https://example.com/a".to_string(),
                    match_strength: 0.9,
                    explanation: "Direct evidence of rent increases".to_string(),
                    edge_type: "RESPONDS_TO".to_string(),
                    gathering_type: None,
                },
                ConcernRespondent {
                    signal_id: Uuid::new_v4(),
                    url: "https://different.org/b".to_string(),
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
}

