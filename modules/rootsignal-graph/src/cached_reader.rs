use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use rootsignal_common::{
    ActorNode, CitationNode, Node, NodeType, TagNode, TensionResponse,
};

use crate::cache::CacheStore;
use crate::reader::passes_display_filter;
use crate::PublicGraphReader;

// --- Graph Explorer types ---

/// A node in the graph explorer response.
pub struct GraphNodeItem {
    pub id: Uuid,
    pub node_type: String,
    pub label: String,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub confidence: Option<f64>,
    pub metadata: String,
}

/// An edge in the graph explorer response.
pub struct GraphEdgeItem {
    pub source_id: Uuid,
    pub target_id: Uuid,
    pub edge_type: String,
}

/// Graph neighborhood result.
pub struct GraphNeighborhoodResult {
    pub nodes: Vec<GraphNodeItem>,
    pub edges: Vec<GraphEdgeItem>,
    pub total_count: u32,
}

/// Read interface that serves public queries from an in-memory cache
/// and delegates vector search + admin queries to Neo4j.
pub struct CachedReader {
    cache: Arc<CacheStore>,
    neo4j_reader: PublicGraphReader,
}

impl CachedReader {
    pub fn new(cache: Arc<CacheStore>, neo4j_reader: PublicGraphReader) -> Self {
        Self { cache, neo4j_reader }
    }

    // ========== Cached public queries ==========

    pub async fn find_nodes_near(
        &self,
        lat: f64,
        lng: f64,
        radius_km: f64,
        node_types: Option<&[NodeType]>,
    ) -> Result<Vec<Node>, neo4rs::Error> {
        let snap = self.cache.load_full();
        let lat_delta = radius_km / 111.0;
        let lng_delta = radius_km / (111.0 * lat.to_radians().cos());
        let min_lat = lat - lat_delta;
        let max_lat = lat + lat_delta;
        let min_lng = lng - lng_delta;
        let max_lng = lng + lng_delta;

        let mut results: Vec<Node> = snap
            .signals
            .iter()
            .filter(|n| {
                if !passes_display_filter(n) {
                    return false;
                }
                if let Some(types) = node_types {
                    if !types.contains(&n.node_type()) {
                        return false;
                    }
                }
                if let Some(loc) = n.meta().and_then(|m| m.about_location) {
                    loc.lat >= min_lat
                        && loc.lat <= max_lat
                        && loc.lng >= min_lng
                        && loc.lng <= max_lng
                } else {
                    false
                }
            })
            .cloned()
            .collect();

        results.sort_by(|a, b| {
            let a_heat = a.meta().map(|m| m.cause_heat).unwrap_or(0.0);
            let b_heat = b.meta().map(|m| m.cause_heat).unwrap_or(0.0);
            b_heat
                .partial_cmp(&a_heat)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(200);
        Ok(results)
    }

    /// Return signals that have no location set (about_location is None).
    pub async fn signals_without_location(&self, limit: u32) -> Result<Vec<Node>, neo4rs::Error> {
        let snap = self.cache.load_full();

        let mut results: Vec<Node> = snap
            .signals
            .iter()
            .filter(|n| {
                if !passes_display_filter(n) {
                    return false;
                }
                n.meta().and_then(|m| m.about_location).is_none()
            })
            .cloned()
            .collect();

        results.sort_by(|a, b| {
            let a_time = a.meta().map(|m| m.extracted_at);
            let b_time = b.meta().map(|m| m.extracted_at);
            b_time.cmp(&a_time)
        });
        results.truncate(limit as usize);
        Ok(results)
    }

    pub async fn signals_in_bounds(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
        limit: u32,
    ) -> Result<Vec<Node>, neo4rs::Error> {
        let snap = self.cache.load_full();

        let mut results: Vec<Node> = snap
            .signals
            .iter()
            .filter(|n| {
                if !passes_display_filter(n) {
                    return false;
                }
                if let Some(loc) = n.meta().and_then(|m| m.about_location) {
                    loc.lat >= min_lat
                        && loc.lat <= max_lat
                        && loc.lng >= min_lng
                        && loc.lng <= max_lng
                } else {
                    false
                }
            })
            .cloned()
            .collect();

        results.sort_by(|a, b| {
            let a_heat = a.meta().map(|m| m.cause_heat).unwrap_or(0.0);
            let b_heat = b.meta().map(|m| m.cause_heat).unwrap_or(0.0);
            b_heat
                .partial_cmp(&a_heat)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit as usize);
        Ok(results)
    }

    pub async fn get_signal_by_id(&self, id: Uuid) -> Result<Option<Node>, neo4rs::Error> {
        let snap = self.cache.load_full();
        let node = snap.signal_by_id.get(&id).map(|&idx| &snap.signals[idx]);
        Ok(node.filter(|n| passes_display_filter(n)).cloned())
    }

    pub async fn get_node_detail(
        &self,
        id: Uuid,
    ) -> Result<Option<(Node, Vec<CitationNode>)>, neo4rs::Error> {
        let snap = self.cache.load_full();
        let node = snap.signal_by_id.get(&id).map(|&idx| &snap.signals[idx]);
        match node {
            Some(n) if passes_display_filter(n) => {
                let evidence = snap
                    .citation_by_signal
                    .get(&id)
                    .cloned()
                    .unwrap_or_default();
                Ok(Some((n.clone(), evidence)))
            }
            _ => Ok(None),
        }
    }

    pub async fn list_recent(
        &self,
        limit: u32,
        node_types: Option<&[NodeType]>,
    ) -> Result<Vec<Node>, neo4rs::Error> {
        let snap = self.cache.load_full();

        let mut results: Vec<Node> = snap
            .signals
            .iter()
            .filter(|n| {
                if !passes_display_filter(n) {
                    return false;
                }
                if let Some(types) = node_types {
                    types.contains(&n.node_type())
                } else {
                    true
                }
            })
            .cloned()
            .collect();

        results.sort_by(|a, b| {
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

        results.truncate(limit as usize);
        Ok(results)
    }

    pub async fn list_recent_in_bbox(
        &self,
        lat: f64,
        lng: f64,
        radius_km: f64,
        limit: u32,
    ) -> Result<Vec<Node>, neo4rs::Error> {
        let snap = self.cache.load_full();
        let lat_delta = radius_km / 111.0;
        let lng_delta = radius_km / (111.0 * lat.to_radians().cos());

        let mut results: Vec<Node> = snap
            .signals
            .iter()
            .filter(|n| {
                if let Some(loc) = n.meta().and_then(|m| m.about_location) {
                    loc.lat >= lat - lat_delta
                        && loc.lat <= lat + lat_delta
                        && loc.lng >= lng - lng_delta
                        && loc.lng <= lng + lng_delta
                } else {
                    false
                }
            })
            .cloned()
            .collect();

        results.sort_by(|a, b| {
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
        results.truncate(limit as usize);
        Ok(results)
    }

    pub async fn actors_in_bounds(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
        limit: u32,
    ) -> Result<Vec<ActorNode>, neo4rs::Error> {
        let snap = self.cache.load_full();

        let mut results: Vec<ActorNode> = snap
            .actors
            .iter()
            .filter(|a| {
                if let (Some(lat), Some(lng)) = (a.location_lat, a.location_lng) {
                    lat >= min_lat && lat <= max_lat && lng >= min_lng && lng <= max_lng
                } else {
                    false
                }
            })
            .cloned()
            .collect();

        results.sort_by(|a, b| b.last_active.cmp(&a.last_active));
        results.truncate(limit as usize);
        Ok(results)
    }

    pub async fn actor_detail(
        &self,
        actor_id: Uuid,
    ) -> Result<Option<ActorNode>, neo4rs::Error> {
        let snap = self.cache.load_full();
        Ok(snap
            .actor_by_id
            .get(&actor_id)
            .map(|&idx| snap.actors[idx].clone()))
    }

    pub async fn tension_responses(
        &self,
        tension_id: Uuid,
    ) -> Result<Vec<TensionResponse>, neo4rs::Error> {
        let snap = self.cache.load_full();
        let responses = snap
            .tension_responses
            .get(&tension_id)
            .map(|v| {
                v.iter()
                    .filter(|tr| passes_display_filter(&tr.node))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        Ok(responses)
    }

    pub async fn get_signal_evidence(
        &self,
        signal_id: Uuid,
    ) -> Result<Vec<CitationNode>, neo4rs::Error> {
        let snap = self.cache.load_full();
        Ok(snap
            .citation_by_signal
            .get(&signal_id)
            .cloned()
            .unwrap_or_default())
    }

    // --- Graph Explorer ---

    /// Return nodes and edges within bounds/time range for the graph explorer.
    /// Reads entirely from in-memory cache — no Neo4j queries.
    pub async fn graph_neighborhood(
        &self,
        min_lat: Option<f64>,
        max_lat: Option<f64>,
        min_lng: Option<f64>,
        max_lng: Option<f64>,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        node_types: &[String],
        limit: u32,
    ) -> Result<GraphNeighborhoodResult, neo4rs::Error> {
        let snap = self.cache.load_full();
        let has_bounds = min_lat.is_some()
            && max_lat.is_some()
            && min_lng.is_some()
            && max_lng.is_some();
        let type_set: HashSet<&str> = node_types.iter().map(|s| s.as_str()).collect();

        let mut nodes: Vec<GraphNodeItem> = Vec::new();
        let mut node_ids: HashSet<Uuid> = HashSet::new();
        let mut total_count: u32 = 0;

        // --- Collect signals ---
        let want_signals = ["Gathering", "Aid", "Need", "Notice", "Tension"]
            .iter()
            .any(|t| type_set.contains(t));

        if want_signals {
            for signal in &snap.signals {
                let Some(meta) = signal.meta() else {
                    continue;
                };

                // Time filter
                let signal_time = meta.content_date.unwrap_or(meta.extracted_at);
                if signal_time < from || signal_time > to {
                    continue;
                }

                let type_name = match signal.node_type() {
                    NodeType::Gathering => "Gathering",
                    NodeType::Aid => "Aid",
                    NodeType::Need => "Need",
                    NodeType::Notice => "Notice",
                    NodeType::Tension => "Tension",
                    NodeType::Citation => continue,
                };

                if !type_set.contains(type_name) {
                    continue;
                }

                // Bounds filter
                if has_bounds {
                    if let Some(loc) = meta.about_location {
                        if loc.lat < min_lat.unwrap()
                            || loc.lat > max_lat.unwrap()
                            || loc.lng < min_lng.unwrap()
                            || loc.lng > max_lng.unwrap()
                        {
                            continue;
                        }
                    }
                    // No location → still include (don't hide locationless signals)
                }

                total_count += 1;
                if nodes.len() < limit as usize {
                    let (lat, lng) = meta
                        .about_location
                        .map(|l| (Some(l.lat), Some(l.lng)))
                        .unwrap_or((None, None));

                    nodes.push(GraphNodeItem {
                        id: meta.id,
                        node_type: type_name.to_string(),
                        label: meta.title.clone(),
                        lat,
                        lng,
                        confidence: Some(meta.confidence as f64),
                        metadata: serde_json::json!({
                            "sourceUrl": meta.source_url,
                            "extractedAt": meta.extracted_at.to_rfc3339(),
                            "contentDate": meta.content_date.map(|d| d.to_rfc3339()),
                            "reviewStatus": meta.review_status,
                            "summary": meta.summary,
                        })
                        .to_string(),
                    });
                    node_ids.insert(meta.id);
                }
            }
        }

        // --- Collect actors ---
        if type_set.contains("Actor") {
            for actor in &snap.actors {
                // Actors don't have content_date; use last_active as proxy
                if actor.last_active < from || actor.last_active > to {
                    continue;
                }

                if has_bounds {
                    if let (Some(lat), Some(lng)) = (actor.location_lat, actor.location_lng) {
                        if lat < min_lat.unwrap()
                            || lat > max_lat.unwrap()
                            || lng < min_lng.unwrap()
                            || lng > max_lng.unwrap()
                        {
                            continue;
                        }
                    }
                }

                total_count += 1;
                if nodes.len() < limit as usize {
                    nodes.push(GraphNodeItem {
                        id: actor.id,
                        node_type: "Actor".to_string(),
                        label: actor.name.clone(),
                        lat: actor.location_lat,
                        lng: actor.location_lng,
                        confidence: None,
                        metadata: serde_json::json!({
                            "actorType": actor.actor_type,
                            "signalCount": actor.signal_count,
                        })
                        .to_string(),
                    });
                    node_ids.insert(actor.id);
                }
            }
        }

        // --- Extract edges where both endpoints are in node_ids ---
        let mut edges: Vec<GraphEdgeItem> = Vec::new();

        // Signal → Actors (ActedIn: actor acted in signal)
        for (&signal_id, actor_indices) in &snap.actors_by_signal {
            if !node_ids.contains(&signal_id) {
                continue;
            }
            for &actor_idx in actor_indices {
                let actor_id = snap.actors[actor_idx].id;
                if node_ids.contains(&actor_id) {
                    edges.push(GraphEdgeItem {
                        source_id: actor_id,
                        target_id: signal_id,
                        edge_type: "ActedIn".to_string(),
                    });
                }
            }
        }

        // Tension → Responses (RespondsTo)
        for (&tension_id, responses) in &snap.tension_responses {
            if !node_ids.contains(&tension_id) {
                continue;
            }
            for tr in responses {
                let responder_id = tr.node.id();
                if node_ids.contains(&responder_id) {
                    edges.push(GraphEdgeItem {
                        source_id: responder_id,
                        target_id: tension_id,
                        edge_type: "RespondsTo".to_string(),
                    });
                }
            }
        }

        // Signal → Citations (SourcedFrom)
        if type_set.contains("Citation") {
            // Also add citation nodes inline
            for (&signal_id, citations) in &snap.citation_by_signal {
                if !node_ids.contains(&signal_id) {
                    continue;
                }
                for cit in citations {
                    if nodes.len() < limit as usize && !node_ids.contains(&cit.id) {
                        nodes.push(GraphNodeItem {
                            id: cit.id,
                            node_type: "Citation".to_string(),
                            label: cit.source_url.clone(),
                            lat: None,
                            lng: None,
                            confidence: cit.confidence.map(|c| c as f64),
                            metadata: serde_json::json!({
                                "snippet": cit.snippet,
                                "sourceUrl": cit.source_url,
                            })
                            .to_string(),
                        });
                        node_ids.insert(cit.id);
                        total_count += 1;
                    }
                    if node_ids.contains(&cit.id) {
                        edges.push(GraphEdgeItem {
                            source_id: signal_id,
                            target_id: cit.id,
                            edge_type: "SourcedFrom".to_string(),
                        });
                    }
                }
            }
        }

        Ok(GraphNeighborhoodResult {
            nodes,
            edges,
            total_count,
        })
    }

    // --- Batch queries for DataLoaders ---

    pub async fn batch_citation_by_signal_ids(
        &self,
        ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<CitationNode>>, neo4rs::Error> {
        let snap = self.cache.load_full();
        let mut map = HashMap::new();
        for &id in ids {
            if let Some(evidence) = snap.citation_by_signal.get(&id) {
                map.insert(id, evidence.clone());
            }
        }
        Ok(map)
    }

    pub async fn batch_actors_by_signal_ids(
        &self,
        ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<ActorNode>>, neo4rs::Error> {
        let snap = self.cache.load_full();
        let mut map = HashMap::new();
        for &id in ids {
            if let Some(actor_indices) = snap.actors_by_signal.get(&id) {
                let actors: Vec<ActorNode> = actor_indices
                    .iter()
                    .map(|&idx| snap.actors[idx].clone())
                    .collect();
                map.insert(id, actors);
            }
        }
        Ok(map)
    }

    /// Batch situations for signals (dataloader). Delegates to Neo4j since situations
    /// are not cached yet.
    pub async fn batch_situations_by_signal_ids(
        &self,
        ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<rootsignal_common::SituationNode>>, neo4rs::Error> {
        let mut map = HashMap::new();
        for &id in ids {
            let situations = self.neo4j_reader.situations_for_signal(&id).await?;
            if !situations.is_empty() {
                map.insert(id, situations);
            }
        }
        Ok(map)
    }

    /// Batch schedules for signals (dataloader). Delegates to Neo4j since schedules
    /// are not cached.
    pub async fn batch_schedules_by_signal_ids(
        &self,
        ids: &[Uuid],
    ) -> Result<HashMap<Uuid, rootsignal_common::ScheduleNode>, neo4rs::Error> {
        self.neo4j_reader
            .batch_schedules_by_signal_ids(ids)
            .await
    }

    // ========== Delegated to Neo4j ==========

    pub async fn semantic_search_signals_in_bounds(
        &self,
        embedding: &[f32],
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
        limit: u32,
    ) -> Result<Vec<(Node, f64)>, neo4rs::Error> {
        self.neo4j_reader
            .semantic_search_signals_in_bounds(embedding, min_lat, max_lat, min_lng, max_lng, limit)
            .await
    }

    // --- Admin queries (delegate to Neo4j) ---

    pub async fn count_by_type(&self) -> Result<Vec<(NodeType, u64)>, neo4rs::Error> {
        self.neo4j_reader.count_by_type().await
    }

    pub async fn confidence_distribution(&self) -> Result<Vec<(String, u64)>, neo4rs::Error> {
        self.neo4j_reader.confidence_distribution().await
    }

    pub async fn freshness_distribution(&self) -> Result<Vec<(String, u64)>, neo4rs::Error> {
        self.neo4j_reader.freshness_distribution().await
    }

    pub async fn total_count(&self) -> Result<u64, neo4rs::Error> {
        self.neo4j_reader.total_count().await
    }

    pub async fn signal_volume_by_day(
        &self,
    ) -> Result<Vec<(String, u64, u64, u64, u64, u64)>, neo4rs::Error> {
        self.neo4j_reader.signal_volume_by_day().await
    }

    pub async fn actor_count(&self) -> Result<u64, neo4rs::Error> {
        self.neo4j_reader.actor_count().await
    }

    // --- Resource queries (delegate to Neo4j — involve Resource nodes not in cache) ---

    pub async fn find_needs_by_resource(
        &self,
        slug: &str,
        lat: f64,
        lng: f64,
        radius_km: f64,
        limit: u32,
    ) -> Result<Vec<crate::ResourceMatch>, neo4rs::Error> {
        self.neo4j_reader
            .find_needs_by_resource(slug, lat, lng, radius_km, limit)
            .await
    }

    pub async fn find_needs_by_resources(
        &self,
        slugs: &[String],
        lat: f64,
        lng: f64,
        radius_km: f64,
        limit: u32,
    ) -> Result<Vec<crate::ResourceMatch>, neo4rs::Error> {
        self.neo4j_reader
            .find_needs_by_resources(slugs, lat, lng, radius_km, limit)
            .await
    }

    pub async fn find_aids_by_resource(
        &self,
        slug: &str,
        lat: f64,
        lng: f64,
        radius_km: f64,
        limit: u32,
    ) -> Result<Vec<crate::ResourceMatch>, neo4rs::Error> {
        self.neo4j_reader
            .find_aids_by_resource(slug, lat, lng, radius_km, limit)
            .await
    }

    pub async fn list_resources(
        &self,
        limit: u32,
    ) -> Result<Vec<rootsignal_common::ResourceNode>, neo4rs::Error> {
        self.neo4j_reader.list_resources(limit).await
    }

    pub async fn resource_gap_analysis(&self) -> Result<Vec<crate::ResourceGap>, neo4rs::Error> {
        self.neo4j_reader.resource_gap_analysis().await
    }

    /// Find tensions with < 2 respondents within bounds.
    /// Delegates to Neo4j reader (not cached).
    pub async fn unresponded_tensions_in_bounds(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
        limit: u32,
    ) -> Result<Vec<Node>, neo4rs::Error> {
        self.neo4j_reader
            .unresponded_tensions_in_bounds(min_lat, max_lat, min_lng, max_lng, limit)
            .await
    }

    /// Tags for a single situation, served from cache.
    pub async fn tags_for_situation(&self, situation_id: Uuid) -> Result<Vec<TagNode>, neo4rs::Error> {
        let snap = self.cache.load_full();
        let tags = snap
            .tags_by_situation
            .get(&situation_id)
            .map(|indices| {
                indices
                    .iter()
                    .map(|&idx| snap.tags[idx].clone())
                    .collect()
            })
            .unwrap_or_default();
        Ok(tags)
    }

    /// Batch tags for multiple situations (dataloader).
    pub async fn batch_tags_by_situation_ids(
        &self,
        keys: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<TagNode>>, anyhow::Error> {
        let snap = self.cache.load_full();
        let mut result = HashMap::new();
        for &situation_id in keys {
            let tags = snap
                .tags_by_situation
                .get(&situation_id)
                .map(|indices| {
                    indices
                        .iter()
                        .map(|&idx| snap.tags[idx].clone())
                        .collect()
                })
                .unwrap_or_default();
            result.insert(situation_id, tags);
        }
        Ok(result)
    }

    /// Top tags sorted by number of situations they appear on.
    pub async fn top_tags(&self, limit: usize) -> Result<Vec<TagNode>, neo4rs::Error> {
        let snap = self.cache.load_full();

        // Count situations per tag
        let mut tag_count: HashMap<usize, usize> = HashMap::new();
        for indices in snap.tags_by_situation.values() {
            for &idx in indices {
                *tag_count.entry(idx).or_default() += 1;
            }
        }

        let mut ranked: Vec<(usize, usize)> = tag_count.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1));
        ranked.truncate(limit);

        Ok(ranked
            .into_iter()
            .map(|(idx, _)| snap.tags[idx].clone())
            .collect())
    }

    // ========== Supervisor / Validation Issues (delegated to Neo4j) ==========

    pub async fn list_validation_issues(
        &self,
        region: &str,
        status_filter: Option<&str>,
        limit: i64,
    ) -> Result<Vec<crate::reader::ValidationIssueRow>, neo4rs::Error> {
        self.neo4j_reader
            .list_validation_issues(region, status_filter, limit)
            .await
    }

    pub async fn validation_issue_summary(
        &self,
        region: &str,
    ) -> Result<crate::reader::ValidationIssueSummary, neo4rs::Error> {
        self.neo4j_reader
            .validation_issue_summary(region)
            .await
    }
}
