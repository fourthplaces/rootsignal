use std::collections::HashMap;
use std::sync::Arc;

use uuid::Uuid;

use rootsignal_common::{
    ActorNode, EvidenceNode, Node, NodeType, StoryNode, TagNode, TensionResponse,
};

use crate::cache::CacheStore;
use crate::reader::passes_display_filter;
use crate::PublicGraphReader;

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
                if let Some(loc) = n.meta().and_then(|m| m.location) {
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
                if let Some(loc) = n.meta().and_then(|m| m.location) {
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

    pub async fn stories_in_bounds(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
        limit: u32,
    ) -> Result<Vec<StoryNode>, neo4rs::Error> {
        self.stories_in_bounds_filtered(min_lat, max_lat, min_lng, max_lng, None, limit)
            .await
    }

    pub async fn stories_in_bounds_filtered(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
        tag: Option<&str>,
        limit: u32,
    ) -> Result<Vec<StoryNode>, neo4rs::Error> {
        let snap = self.cache.load_full();

        // If filtering by tag, find the tag index and the set of story_ids with that tag
        let tag_story_filter: Option<std::collections::HashSet<Uuid>> = tag.and_then(|slug| {
            let tag_idx = snap.tags.iter().position(|t| t.slug == slug)?;
            Some(
                snap.tags_by_story
                    .iter()
                    .filter(|(_, indices)| indices.contains(&tag_idx))
                    .map(|(story_id, _)| *story_id)
                    .collect(),
            )
        });

        let mut results: Vec<StoryNode> = snap
            .stories
            .iter()
            .filter(|s| {
                if let (Some(lat), Some(lng)) = (s.centroid_lat, s.centroid_lng) {
                    lat >= min_lat && lat <= max_lat && lng >= min_lng && lng <= max_lng
                } else {
                    false
                }
            })
            .filter(|s| {
                tag_story_filter
                    .as_ref()
                    .map(|f| f.contains(&s.id))
                    .unwrap_or(true)
            })
            .cloned()
            .collect();

        results.sort_by(|a, b| {
            b.energy
                .partial_cmp(&a.energy)
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
    ) -> Result<Option<(Node, Vec<EvidenceNode>)>, neo4rs::Error> {
        let snap = self.cache.load_full();
        let node = snap.signal_by_id.get(&id).map(|&idx| &snap.signals[idx]);
        match node {
            Some(n) if passes_display_filter(n) => {
                let evidence = snap
                    .evidence_by_signal
                    .get(&id)
                    .cloned()
                    .unwrap_or_default();
                Ok(Some((n.clone(), evidence)))
            }
            _ => Ok(None),
        }
    }

    pub async fn get_story_by_id(&self, id: Uuid) -> Result<Option<StoryNode>, neo4rs::Error> {
        let snap = self.cache.load_full();
        Ok(snap.story_by_id.get(&id).map(|&idx| snap.stories[idx].clone()))
    }

    pub async fn get_story_signals(&self, story_id: Uuid) -> Result<Vec<Node>, neo4rs::Error> {
        let snap = self.cache.load_full();
        let signals = snap
            .signals_by_story
            .get(&story_id)
            .map(|indices| {
                indices
                    .iter()
                    .map(|&idx| &snap.signals[idx])
                    .filter(|n| passes_display_filter(n))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        Ok(signals)
    }

    pub async fn get_story_with_signals(
        &self,
        story_id: Uuid,
    ) -> Result<Option<(StoryNode, Vec<Node>)>, neo4rs::Error> {
        let story = self.get_story_by_id(story_id).await?;
        match story {
            Some(s) => {
                let signals = self.get_story_signals(story_id).await?;
                Ok(Some((s, signals)))
            }
            None => Ok(None),
        }
    }

    pub async fn list_recent(
        &self,
        limit: u32,
        node_types: Option<&[NodeType]>,
    ) -> Result<Vec<Node>, neo4rs::Error> {
        let snap = self.cache.load_full();

        // Collect (node, story_type_diversity) for sorting
        let mut ranked: Vec<(Node, u32)> = snap
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
            .map(|n| {
                let tri = n
                    .meta()
                    .and_then(|m| snap.story_by_signal.get(&m.id))
                    .map(|&story_idx| snap.stories[story_idx].type_diversity)
                    .unwrap_or(0);
                (n.clone(), tri)
            })
            .collect();

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
        Ok(ranked.into_iter().map(|(n, _)| n).collect())
    }

    pub async fn list_recent_for_city(
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
                if let Some(loc) = n.meta().and_then(|m| m.location) {
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

    pub async fn top_stories_by_energy(
        &self,
        limit: u32,
        status_filter: Option<&str>,
    ) -> Result<Vec<StoryNode>, neo4rs::Error> {
        let snap = self.cache.load_full();

        let mut results: Vec<StoryNode> = snap
            .stories
            .iter()
            .filter(|s| {
                if let Some(status) = status_filter {
                    s.status == status
                } else {
                    true
                }
            })
            .cloned()
            .collect();

        results.sort_by(|a, b| {
            b.energy
                .partial_cmp(&a.energy)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit as usize);
        Ok(results)
    }

    pub async fn top_stories_for_city(
        &self,
        lat: f64,
        lng: f64,
        radius_km: f64,
        limit: u32,
    ) -> Result<Vec<StoryNode>, neo4rs::Error> {
        let lat_delta = radius_km / 111.0;
        let lng_delta = radius_km / (111.0 * lat.to_radians().cos());

        let snap = self.cache.load_full();
        let mut results: Vec<StoryNode> = snap
            .stories
            .iter()
            .filter(|s| {
                if let (Some(slat), Some(slng)) = (s.centroid_lat, s.centroid_lng) {
                    slat >= lat - lat_delta
                        && slat <= lat + lat_delta
                        && slng >= lng - lng_delta
                        && slng <= lng + lng_delta
                } else {
                    false
                }
            })
            .cloned()
            .collect();

        results.sort_by(|a, b| {
            b.energy
                .partial_cmp(&a.energy)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit as usize);
        Ok(results)
    }

    pub async fn stories_by_category(
        &self,
        category: &str,
        limit: u32,
    ) -> Result<Vec<StoryNode>, neo4rs::Error> {
        let snap = self.cache.load_full();

        let mut results: Vec<StoryNode> = snap
            .stories
            .iter()
            .filter(|s| s.category.as_deref() == Some(category))
            .cloned()
            .collect();

        results.sort_by(|a, b| {
            b.energy
                .partial_cmp(&a.energy)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit as usize);
        Ok(results)
    }

    pub async fn stories_by_arc(
        &self,
        arc: &str,
        limit: u32,
    ) -> Result<Vec<StoryNode>, neo4rs::Error> {
        let snap = self.cache.load_full();

        let mut results: Vec<StoryNode> = snap
            .stories
            .iter()
            .filter(|s| s.arc.as_deref() == Some(arc))
            .cloned()
            .collect();

        results.sort_by(|a, b| {
            b.energy
                .partial_cmp(&a.energy)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit as usize);
        Ok(results)
    }

    pub async fn actors_active_in_area(
        &self,
        region_slug: &str,
        limit: u32,
    ) -> Result<Vec<ActorNode>, neo4rs::Error> {
        let snap = self.cache.load_full();

        let mut results: Vec<ActorNode> = snap
            .actors_by_region
            .get(region_slug)
            .map(|indices| {
                indices
                    .iter()
                    .map(|&idx| snap.actors[idx].clone())
                    .collect()
            })
            .unwrap_or_default();

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

    pub async fn actor_stories(
        &self,
        actor_id: Uuid,
        limit: u32,
    ) -> Result<Vec<StoryNode>, neo4rs::Error> {
        let snap = self.cache.load_full();

        let mut results: Vec<StoryNode> = snap
            .stories_by_actor
            .get(&actor_id)
            .map(|indices| {
                indices
                    .iter()
                    .map(|&idx| snap.stories[idx].clone())
                    .collect()
            })
            .unwrap_or_default();

        results.sort_by(|a, b| {
            b.energy
                .partial_cmp(&a.energy)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit as usize);
        Ok(results)
    }

    pub async fn actors_for_story(
        &self,
        story_id: Uuid,
    ) -> Result<Vec<ActorNode>, neo4rs::Error> {
        let snap = self.cache.load_full();
        let actors = snap
            .actors_for_story
            .get(&story_id)
            .map(|indices| {
                indices
                    .iter()
                    .map(|&idx| snap.actors[idx].clone())
                    .collect()
            })
            .unwrap_or_default();
        Ok(actors)
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
    ) -> Result<Vec<EvidenceNode>, neo4rs::Error> {
        let snap = self.cache.load_full();
        Ok(snap
            .evidence_by_signal
            .get(&signal_id)
            .cloned()
            .unwrap_or_default())
    }

    pub async fn story_evidence_counts(
        &self,
        story_ids: &[Uuid],
    ) -> Result<Vec<(Uuid, u32)>, neo4rs::Error> {
        let snap = self.cache.load_full();
        let mut results = Vec::new();
        for &story_id in story_ids {
            if let Some(signal_indices) = snap.signals_by_story.get(&story_id) {
                let count: u32 = signal_indices
                    .iter()
                    .filter_map(|&idx| {
                        let meta = snap.signals[idx].meta()?;
                        snap.evidence_by_signal
                            .get(&meta.id)
                            .map(|ev| ev.len() as u32)
                    })
                    .sum();
                if count > 0 {
                    results.push((story_id, count));
                }
            }
        }
        Ok(results)
    }

    // --- Batch queries for DataLoaders ---

    pub async fn batch_evidence_by_signal_ids(
        &self,
        ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<EvidenceNode>>, neo4rs::Error> {
        let snap = self.cache.load_full();
        let mut map = HashMap::new();
        for &id in ids {
            if let Some(evidence) = snap.evidence_by_signal.get(&id) {
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

    pub async fn batch_story_by_signal_ids(
        &self,
        ids: &[Uuid],
    ) -> Result<HashMap<Uuid, StoryNode>, neo4rs::Error> {
        let snap = self.cache.load_full();
        let mut map = HashMap::new();
        for &id in ids {
            if let Some(&story_idx) = snap.story_by_signal.get(&id) {
                map.insert(id, snap.stories[story_idx].clone());
            }
        }
        Ok(map)
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

    pub async fn semantic_search_stories_in_bounds(
        &self,
        embedding: &[f32],
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
        limit: u32,
    ) -> Result<Vec<(StoryNode, f64, String)>, neo4rs::Error> {
        self.neo4j_reader
            .semantic_search_stories_in_bounds(
                embedding, min_lat, max_lat, min_lng, max_lng, limit,
            )
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

    pub async fn story_count_by_arc(&self) -> Result<Vec<(String, u64)>, neo4rs::Error> {
        self.neo4j_reader.story_count_by_arc().await
    }

    pub async fn story_count_by_category(&self) -> Result<Vec<(String, u64)>, neo4rs::Error> {
        self.neo4j_reader.story_count_by_category().await
    }

    pub async fn story_count(&self) -> Result<u64, neo4rs::Error> {
        self.neo4j_reader.story_count().await
    }

    pub async fn actor_count(&self) -> Result<u64, neo4rs::Error> {
        self.neo4j_reader.actor_count().await
    }

    pub async fn get_story_signal_evidence(
        &self,
        story_id: Uuid,
    ) -> Result<Vec<(Uuid, Vec<EvidenceNode>)>, neo4rs::Error> {
        // Can serve from cache
        let snap = self.cache.load_full();
        let mut results = Vec::new();
        if let Some(signal_indices) = snap.signals_by_story.get(&story_id) {
            for &idx in signal_indices {
                if let Some(meta) = snap.signals[idx].meta() {
                    if let Some(evidence) = snap.evidence_by_signal.get(&meta.id) {
                        if !evidence.is_empty() {
                            results.push((meta.id, evidence.clone()));
                        }
                    }
                }
            }
        }
        Ok(results)
    }

    pub async fn get_story_tension_responses(
        &self,
        story_id: Uuid,
    ) -> Result<Vec<(Uuid, Vec<serde_json::Value>)>, neo4rs::Error> {
        // This returns JSON values with specific shape; delegate to Neo4j for now
        self.neo4j_reader
            .get_story_tension_responses(story_id)
            .await
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

    /// Find tensions with < 2 respondents, not yet in any story, within bounds.
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

    /// Tags for a single story, served from cache.
    pub async fn tags_for_story(&self, story_id: Uuid) -> Result<Vec<TagNode>, neo4rs::Error> {
        let snap = self.cache.load_full();
        let tags = snap
            .tags_by_story
            .get(&story_id)
            .map(|indices| {
                indices
                    .iter()
                    .map(|&idx| snap.tags[idx].clone())
                    .collect()
            })
            .unwrap_or_default();
        Ok(tags)
    }

    /// Batch tags for multiple stories (dataloader).
    pub async fn batch_tags_by_story_ids(
        &self,
        keys: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<TagNode>>, anyhow::Error> {
        let snap = self.cache.load_full();
        let mut result = HashMap::new();
        for &story_id in keys {
            let tags = snap
                .tags_by_story
                .get(&story_id)
                .map(|indices| {
                    indices
                        .iter()
                        .map(|&idx| snap.tags[idx].clone())
                        .collect()
                })
                .unwrap_or_default();
            result.insert(story_id, tags);
        }
        Ok(result)
    }

    /// Top tags sorted by number of stories they appear on.
    pub async fn top_tags(&self, limit: usize) -> Result<Vec<TagNode>, neo4rs::Error> {
        let snap = self.cache.load_full();

        // Count stories per tag
        let mut tag_story_count: HashMap<usize, usize> = HashMap::new();
        for indices in snap.tags_by_story.values() {
            for &idx in indices {
                *tag_story_count.entry(idx).or_default() += 1;
            }
        }

        let mut ranked: Vec<(usize, usize)> = tag_story_count.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1));
        ranked.truncate(limit);

        Ok(ranked
            .into_iter()
            .map(|(idx, _)| snap.tags[idx].clone())
            .collect())
    }

    /// Stories that have a specific tag, optionally bounded geographically.
    pub async fn stories_by_tag(
        &self,
        tag_slug: &str,
        min_lat: Option<f64>,
        max_lat: Option<f64>,
        min_lng: Option<f64>,
        max_lng: Option<f64>,
        limit: u32,
    ) -> Result<Vec<StoryNode>, neo4rs::Error> {
        let snap = self.cache.load_full();

        // Find the tag index by slug
        let tag_idx = snap.tags.iter().position(|t| t.slug == tag_slug);
        let Some(tag_idx) = tag_idx else {
            return Ok(vec![]);
        };

        // Find all story_ids that have this tag
        let mut results: Vec<StoryNode> = snap
            .tags_by_story
            .iter()
            .filter(|(_, indices)| indices.contains(&tag_idx))
            .filter_map(|(story_id, _)| {
                snap.story_by_id
                    .get(story_id)
                    .map(|&idx| &snap.stories[idx])
            })
            .filter(|s| {
                if let (Some(min_lat), Some(max_lat), Some(min_lng), Some(max_lng)) =
                    (min_lat, max_lat, min_lng, max_lng)
                {
                    if let (Some(lat), Some(lng)) = (s.centroid_lat, s.centroid_lng) {
                        lat >= min_lat && lat <= max_lat && lng >= min_lng && lng <= max_lng
                    } else {
                        false
                    }
                } else {
                    true
                }
            })
            .cloned()
            .collect();

        results.sort_by(|a, b| {
            b.energy
                .partial_cmp(&a.energy)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit as usize);
        Ok(results)
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
