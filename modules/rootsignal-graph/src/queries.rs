//! `GraphQueries` — trait abstraction over `GraphReader` for testability.
//!
//! Handlers and activities take `&dyn GraphQueries` instead of `&GraphReader`,
//! enabling in-memory mocks for unit tests while production uses the real Neo4j-backed
//! implementation.

use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use rootsignal_common::events::{CauseHeatScore, SimilarityEdge, SystemEvent};
use rootsignal_common::{ActorNode, NodeType, PinNode, Region, SourceNode};

use crate::situation_temperature::TemperatureComponents;
use crate::writer::{
    ConcernLinkerTarget, ConcernResponseShape, DuplicateMatch, ExtractionYield,
    GapTypeStats, GatheringFinderTarget, InvestigationTarget,
    ResponseFinderTarget, ResponseHeuristic, SignalTypeCounts, SituationBrief,
    SourceBrief, SourceStats, UnmetTension, WeaveCandidate, WeaveSignal,
};

// --- Coalescing types ---

/// A signal returned from fulltext or vector search.
#[derive(Debug, Clone)]
pub struct SignalSearchResult {
    pub id: Uuid,
    pub title: String,
    pub summary: String,
    pub signal_type: String,
    pub score: f64,
}

/// Batch-fetched signal details for LLM context.
#[derive(Debug, Clone)]
pub struct SignalDetail {
    pub id: Uuid,
    pub title: String,
    pub summary: String,
    pub signal_type: String,
    pub cause_heat: Option<f64>,
}

/// An existing signal group for LLM landscape context.
#[derive(Debug, Clone)]
pub struct GroupBrief {
    pub id: Uuid,
    pub label: String,
    pub queries: Vec<String>,
    pub signal_count: u32,
    pub member_ids: Vec<Uuid>,
}

/// A signal without any group membership, ordered by cause_heat.
#[derive(Debug, Clone)]
pub struct UngroupedSignal {
    pub id: Uuid,
    pub title: String,
    pub summary: String,
    pub signal_type: String,
    pub cause_heat: f64,
}

// --- Cluster detail types ---

/// A cluster member signal for the detail page.
#[derive(Debug, Clone)]
pub struct ClusterMemberRow {
    pub id: Uuid,
    pub title: String,
    pub signal_type: String,
    pub confidence: f64,
    pub source_url: Option<String>,
    pub summary: Option<String>,
}

/// Full cluster detail for the admin detail page.
#[derive(Debug, Clone)]
pub struct ClusterDetailRow {
    pub id: Uuid,
    pub label: String,
    pub queries: Vec<String>,
    pub created_at: String,
    pub members: Vec<ClusterMemberRow>,
    pub woven_situation_id: Option<Uuid>,
}

/// Lightweight cluster summary for spatial listing.
#[derive(Debug, Clone)]
pub struct ClusterSummary {
    pub id: Uuid,
    pub label: String,
    pub queries: Vec<String>,
    pub created_at: String,
    pub member_count: u32,
    pub woven_situation_id: Option<Uuid>,
}

#[async_trait]
pub trait GraphQueries: Send + Sync {
    // --- Source management ---

    async fn source_exists(&self, url: &str) -> Result<bool>;
    async fn get_active_sources(&self) -> Result<Vec<SourceNode>>;
    async fn get_sources_for_region(
        &self,
        lat: f64,
        lng: f64,
        radius_km: f64,
    ) -> Result<Vec<SourceNode>>;
    async fn find_actors_in_region(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<(ActorNode, Vec<SourceNode>)>>;
    async fn find_pins_in_region(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<(PinNode, SourceNode)>>;

    // --- Source health & metrics ---

    async fn count_source_tensions(&self, canonical_key: &str) -> Result<u32>;
    async fn find_dead_sources(&self, max_empty_runs: u32) -> Result<Vec<Uuid>>;
    async fn find_dead_web_queries(&self) -> Result<Vec<Uuid>>;
    async fn get_active_web_queries(&self) -> Result<Vec<String>>;
    async fn get_source_stats(&self) -> Result<SourceStats>;

    // --- Discovery ---

    async fn get_unmet_tensions(&self, limit: u32) -> Result<Vec<UnmetTension>>;

    // --- Expansion ---

    async fn get_recently_linked_signals_with_queries(
        &self,
    ) -> Result<(Vec<String>, Vec<Uuid>)>;
    async fn find_similar_query(
        &self,
        embedding: &[f32],
        threshold: f64,
    ) -> Result<Option<(String, f64)>>;

    // --- Situation landscape ---

    async fn get_situation_landscape(&self, limit: u32) -> Result<Vec<SituationBrief>>;
    async fn find_curiosity_candidates(&self) -> Result<Vec<(Uuid, Vec<Uuid>)>>;

    // --- Signal info ---

    async fn get_signal_info(&self, id: Uuid) -> Result<Option<(String, String)>>;

    // --- Situation weaving ---

    async fn discover_unassigned_signals(
        &self,
        scout_run_id: &str,
    ) -> Result<Vec<WeaveSignal>>;
    async fn load_weave_candidates(&self) -> Result<Vec<WeaveCandidate>>;
    async fn find_affected_situations(&self, scout_run_id: &str) -> Result<Vec<Uuid>>;
    async fn unverified_dispatches(&self, limit: usize) -> Result<Vec<(Uuid, String)>>;
    async fn check_signal_ids_exist(&self, signal_ids: &[Uuid]) -> Result<Vec<Uuid>>;

    // --- Curiosity: investigation ---

    async fn find_investigation_targets(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<InvestigationTarget>>;

    // --- Curiosity: concern linking ---

    async fn find_tension_linker_targets(
        &self,
        limit: u32,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<ConcernLinkerTarget>>;
    async fn get_tension_landscape(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<(String, String)>>;

    // --- Dedup (used by curiosity finders, also by SignalReader impl) ---

    async fn find_duplicate(
        &self,
        embedding: &[f32],
        primary_type: NodeType,
        threshold: f64,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Option<DuplicateMatch>>;

    // --- Curiosity: response finding ---

    async fn get_existing_responses(
        &self,
        concern_id: Uuid,
    ) -> Result<Vec<ResponseHeuristic>>;
    async fn find_response_finder_targets(
        &self,
        limit: u32,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<ResponseFinderTarget>>;

    // --- Curiosity: gathering finding ---

    async fn get_existing_gathering_signals(
        &self,
        concern_id: Uuid,
        center_lat: f64,
        center_lng: f64,
        radius_km: f64,
    ) -> Result<Vec<ResponseHeuristic>>;
    async fn find_gathering_finder_targets(
        &self,
        limit: u32,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<GatheringFinderTarget>>;

    // --- Tension & response mapping ---

    async fn get_active_tensions(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<(Uuid, Vec<f64>)>>;
    async fn find_response_candidates(
        &self,
        concern_embedding: &[f64],
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<(Uuid, f64)>>;

    // --- Discovery: source finder analytics ---

    async fn get_actors_with_domains(
        &self,
        max_depth: Option<u32>,
    ) -> Result<Vec<(String, Vec<String>, Vec<String>, String)>>;
    async fn get_signal_type_counts(&self) -> Result<SignalTypeCounts>;
    async fn get_discovery_performance(&self) -> Result<(Vec<SourceBrief>, Vec<SourceBrief>)>;
    async fn get_gap_type_stats(&self) -> Result<Vec<GapTypeStats>>;
    async fn get_extraction_yield(&self) -> Result<Vec<ExtractionYield>>;
    async fn get_tension_response_shape(&self, limit: u32) -> Result<Vec<ConcernResponseShape>>;

    // --- Enrichment ---

    async fn actor_signal_counts(&self) -> Result<Vec<(Uuid, u32)>>;
    async fn signal_evidence_for_diversity(
        &self,
        label: &str,
    ) -> Result<Vec<(Uuid, String, Vec<(String, String)>)>>;

    // --- Supervisor ---

    async fn find_duplicate_tension_pairs(
        &self,
        threshold: f64,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<(Uuid, Uuid)>>;

    // --- Wrapped operations (replace raw client() access) ---

    /// Compute similarity edges across all signals. Wraps `similarity::compute_edges`.
    async fn compute_similarity_edges(&self) -> Result<Vec<SimilarityEdge>>;

    /// Compute situation temperature from graph mechanics. Wraps `situation_temperature::compute_temperature_events`.
    async fn compute_situation_temperature(
        &self,
        situation_id: &Uuid,
    ) -> Result<(TemperatureComponents, Vec<SystemEvent>)>;

    /// Re-evaluate severity for all Notices in a bounding box. Returns revisions for changed signals.
    async fn compute_severity_inference(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<crate::severity_inference::SeverityRevision>>;

    /// Compute cause heat for signals in a bounding box. Wraps `cause_heat::compute_cause_heat`.
    async fn compute_cause_heat(
        &self,
        threshold: f64,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<CauseHeatScore>>;

    // --- Coalescing ---

    /// Fulltext search across all 6 signal types by title+summary.
    async fn fulltext_search_signals(&self, query: &str, limit: u32) -> Result<Vec<SignalSearchResult>>;

    /// Vector similarity search, optional geographic bounding box.
    async fn vector_search_signals(
        &self,
        embedding: &[f32],
        limit: u32,
        bbox: Option<(f64, f64, f64, f64)>,
    ) -> Result<Vec<SignalSearchResult>>;

    /// Highest cause_heat signals without MEMBER_OF edges, for seed selection.
    async fn get_ungrouped_signals(&self, limit: u32) -> Result<Vec<UngroupedSignal>>;

    /// Existing groups for LLM context during coalescing.
    async fn get_group_landscape(&self, limit: u32) -> Result<Vec<GroupBrief>>;

    /// Batch-fetch signal details by ID for LLM tool results.
    async fn get_signal_details(&self, ids: &[Uuid]) -> Result<Vec<SignalDetail>>;

    // --- Signal location & spatial region lookup ---

    /// Get the geocoded location (lat/lng) for a signal via its Location node.
    async fn get_signal_location(&self, signal_id: &str) -> Result<Option<(f64, f64)>>;

    /// Find regions that spatially contain a signal's geocoded location.
    /// Returns regions sorted by radius ascending (most specific first).
    async fn get_regions_for_signal_by_location(&self, signal_id: &str) -> Result<Vec<Region>>;

    /// Check if a Region with the given name already exists.
    async fn region_exists_by_name(&self, name: &str) -> Result<bool>;

    // --- Cluster weaving ---

    /// Read a SignalGroup + members + optional WOVEN_INTO situation for the detail page.
    async fn get_cluster_detail(&self, group_id: Uuid) -> Result<Option<ClusterDetailRow>>;

    /// Read member signals as WeaveSignal structs for the cluster weaver.
    async fn get_cluster_members(&self, group_id: Uuid) -> Result<Vec<WeaveSignal>>;

    /// Delta signals: members added to the group after the last weave.
    async fn get_cluster_delta_signals(&self, group_id: Uuid) -> Result<Vec<WeaveSignal>>;
}

// --- GraphReader implementation ---

#[async_trait]
impl GraphQueries for crate::writer::GraphReader {
    async fn source_exists(&self, url: &str) -> Result<bool> {
        Ok(self.source_exists(url).await?)
    }

    async fn get_active_sources(&self) -> Result<Vec<SourceNode>> {
        Ok(self.get_active_sources().await?)
    }

    async fn get_sources_for_region(
        &self,
        lat: f64,
        lng: f64,
        radius_km: f64,
    ) -> Result<Vec<SourceNode>> {
        Ok(self.get_sources_for_region(lat, lng, radius_km).await?)
    }

    async fn find_actors_in_region(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<(ActorNode, Vec<SourceNode>)>> {
        Ok(self.find_actors_in_region(min_lat, max_lat, min_lng, max_lng).await?)
    }

    async fn find_pins_in_region(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<(PinNode, SourceNode)>> {
        Ok(self.find_pins_in_region(min_lat, max_lat, min_lng, max_lng).await?)
    }

    async fn count_source_tensions(&self, canonical_key: &str) -> Result<u32> {
        Ok(self.count_source_tensions(canonical_key).await?)
    }

    async fn find_dead_sources(&self, max_empty_runs: u32) -> Result<Vec<Uuid>> {
        Ok(self.find_dead_sources(max_empty_runs).await?)
    }

    async fn find_dead_web_queries(&self) -> Result<Vec<Uuid>> {
        Ok(self.find_dead_web_queries().await?)
    }

    async fn get_active_web_queries(&self) -> Result<Vec<String>> {
        Ok(self.get_active_web_queries().await?)
    }

    async fn get_source_stats(&self) -> Result<SourceStats> {
        Ok(self.get_source_stats().await?)
    }

    async fn get_unmet_tensions(&self, limit: u32) -> Result<Vec<UnmetTension>> {
        Ok(self.get_unmet_tensions(limit).await?)
    }

    async fn get_recently_linked_signals_with_queries(
        &self,
    ) -> Result<(Vec<String>, Vec<Uuid>)> {
        Ok(self.get_recently_linked_signals_with_queries().await?)
    }

    async fn find_similar_query(
        &self,
        embedding: &[f32],
        threshold: f64,
    ) -> Result<Option<(String, f64)>> {
        Ok(self.find_similar_query(embedding, threshold).await?)
    }

    async fn get_situation_landscape(&self, limit: u32) -> Result<Vec<SituationBrief>> {
        Ok(self.get_situation_landscape(limit).await?)
    }

    async fn find_curiosity_candidates(&self) -> Result<Vec<(Uuid, Vec<Uuid>)>> {
        Ok(self.find_curiosity_candidates().await?)
    }

    async fn get_signal_info(&self, id: Uuid) -> Result<Option<(String, String)>> {
        Ok(self.get_signal_info(id).await?)
    }

    async fn discover_unassigned_signals(
        &self,
        scout_run_id: &str,
    ) -> Result<Vec<WeaveSignal>> {
        Ok(self.discover_unassigned_signals(scout_run_id).await?)
    }

    async fn load_weave_candidates(&self) -> Result<Vec<WeaveCandidate>> {
        Ok(self.load_weave_candidates().await?)
    }

    async fn find_affected_situations(&self, scout_run_id: &str) -> Result<Vec<Uuid>> {
        Ok(self.find_affected_situations(scout_run_id).await?)
    }

    async fn unverified_dispatches(&self, limit: usize) -> Result<Vec<(Uuid, String)>> {
        Ok(self.unverified_dispatches(limit).await?)
    }

    async fn check_signal_ids_exist(&self, signal_ids: &[Uuid]) -> Result<Vec<Uuid>> {
        Ok(self.check_signal_ids_exist(signal_ids).await?)
    }

    async fn find_investigation_targets(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<InvestigationTarget>> {
        Ok(self.find_investigation_targets(min_lat, max_lat, min_lng, max_lng).await?)
    }

    async fn find_tension_linker_targets(
        &self,
        limit: u32,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<ConcernLinkerTarget>> {
        Ok(self.find_tension_linker_targets(limit, min_lat, max_lat, min_lng, max_lng).await?)
    }

    async fn get_tension_landscape(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<(String, String)>> {
        Ok(self.get_tension_landscape(min_lat, max_lat, min_lng, max_lng).await?)
    }

    async fn find_duplicate(
        &self,
        embedding: &[f32],
        primary_type: NodeType,
        threshold: f64,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Option<DuplicateMatch>> {
        Ok(self.find_duplicate(embedding, primary_type, threshold, min_lat, max_lat, min_lng, max_lng).await?)
    }

    async fn get_existing_responses(
        &self,
        concern_id: Uuid,
    ) -> Result<Vec<ResponseHeuristic>> {
        Ok(self.get_existing_responses(concern_id).await?)
    }

    async fn find_response_finder_targets(
        &self,
        limit: u32,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<ResponseFinderTarget>> {
        Ok(self.find_response_finder_targets(limit, min_lat, max_lat, min_lng, max_lng).await?)
    }

    async fn get_existing_gathering_signals(
        &self,
        concern_id: Uuid,
        center_lat: f64,
        center_lng: f64,
        radius_km: f64,
    ) -> Result<Vec<ResponseHeuristic>> {
        Ok(self.get_existing_gathering_signals(concern_id, center_lat, center_lng, radius_km).await?)
    }

    async fn find_gathering_finder_targets(
        &self,
        limit: u32,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<GatheringFinderTarget>> {
        Ok(self.find_gathering_finder_targets(limit, min_lat, max_lat, min_lng, max_lng).await?)
    }

    async fn get_active_tensions(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<(Uuid, Vec<f64>)>> {
        Ok(self.get_active_tensions(min_lat, max_lat, min_lng, max_lng).await?)
    }

    async fn find_response_candidates(
        &self,
        concern_embedding: &[f64],
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<(Uuid, f64)>> {
        Ok(self.find_response_candidates(concern_embedding, min_lat, max_lat, min_lng, max_lng).await?)
    }

    async fn actor_signal_counts(&self) -> Result<Vec<(Uuid, u32)>> {
        Ok(self.actor_signal_counts().await?)
    }

    async fn signal_evidence_for_diversity(
        &self,
        label: &str,
    ) -> Result<Vec<(Uuid, String, Vec<(String, String)>)>> {
        Ok(self.signal_evidence_for_diversity(label).await?)
    }

    async fn get_actors_with_domains(
        &self,
        max_depth: Option<u32>,
    ) -> Result<Vec<(String, Vec<String>, Vec<String>, String)>> {
        Ok(self.get_actors_with_domains(max_depth).await?)
    }

    async fn get_signal_type_counts(&self) -> Result<SignalTypeCounts> {
        Ok(self.get_signal_type_counts().await?)
    }

    async fn get_discovery_performance(&self) -> Result<(Vec<SourceBrief>, Vec<SourceBrief>)> {
        Ok(self.get_discovery_performance().await?)
    }

    async fn get_gap_type_stats(&self) -> Result<Vec<GapTypeStats>> {
        Ok(self.get_gap_type_stats().await?)
    }

    async fn get_extraction_yield(&self) -> Result<Vec<ExtractionYield>> {
        Ok(self.get_extraction_yield().await?)
    }

    async fn get_tension_response_shape(&self, limit: u32) -> Result<Vec<ConcernResponseShape>> {
        Ok(self.get_tension_response_shape(limit).await?)
    }

    async fn find_duplicate_tension_pairs(
        &self,
        threshold: f64,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<(Uuid, Uuid)>> {
        Ok(self.find_duplicate_tension_pairs(threshold, min_lat, max_lat, min_lng, max_lng).await?)
    }

    async fn compute_similarity_edges(&self) -> Result<Vec<SimilarityEdge>> {
        Ok(crate::similarity::compute_edges(self.client()).await?)
    }

    async fn compute_situation_temperature(
        &self,
        situation_id: &Uuid,
    ) -> Result<(TemperatureComponents, Vec<SystemEvent>)> {
        let (components, events) =
            crate::situation_temperature::compute_temperature_events(self.client(), situation_id)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok((components, events))
    }

    async fn compute_severity_inference(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<crate::severity_inference::SeverityRevision>> {
        crate::severity_inference::compute_severity_inference(self, min_lat, max_lat, min_lng, max_lng).await
    }

    async fn compute_cause_heat(
        &self,
        threshold: f64,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<CauseHeatScore>> {
        Ok(crate::cause_heat::compute_cause_heat(self.client(), threshold, min_lat, max_lat, min_lng, max_lng).await?)
    }

    async fn fulltext_search_signals(&self, query_text: &str, limit: u32) -> Result<Vec<SignalSearchResult>> {
        use neo4rs::query;

        let labels = ["gathering", "resource", "helprequest", "announcement", "concern", "condition"];
        let mut results = Vec::new();

        for index_name in &labels {
            let index = format!("{}_text", index_name);
            let q = query(
                "CALL db.index.fulltext.queryNodes($index, $query) YIELD node, score
                 WHERE score > 0.5
                 RETURN node.id AS id, node.title AS title, node.summary AS summary,
                        labels(node)[0] AS signal_type, score
                 LIMIT $limit",
            )
            .param("index", index.as_str())
            .param("query", query_text)
            .param("limit", limit as i64);

            let mut stream = self.client().execute(q).await?;
            while let Some(row) = stream.next().await? {
                let id_str: String = row.get("id").unwrap_or_default();
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    results.push(SignalSearchResult {
                        id,
                        title: row.get("title").unwrap_or_default(),
                        summary: row.get("summary").unwrap_or_default(),
                        signal_type: row.get("signal_type").unwrap_or_default(),
                        score: row.get::<f64>("score").unwrap_or(0.0),
                    });
                }
            }
        }

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit as usize);
        Ok(results)
    }

    async fn vector_search_signals(
        &self,
        embedding: &[f32],
        limit: u32,
        bbox: Option<(f64, f64, f64, f64)>,
    ) -> Result<Vec<SignalSearchResult>> {
        use neo4rs::query;

        let labels = ["Gathering", "Resource", "HelpRequest", "Announcement", "Concern", "Condition"];
        let embedding_vec: Vec<f64> = embedding.iter().map(|&x| x as f64).collect();
        let mut results = Vec::new();

        for label in &labels {
            let index_name = format!("{}_embedding", label.to_lowercase());
            let cypher = if bbox.is_some() {
                format!(
                    "CALL db.index.vector.queryNodes($index, $limit, $embedding) YIELD node, score
                     WHERE node.lat >= $min_lat AND node.lat <= $max_lat
                       AND node.lng >= $min_lng AND node.lng <= $max_lng
                     RETURN node.id AS id, node.title AS title, node.summary AS summary,
                            '{label}' AS signal_type, score
                     LIMIT $limit"
                )
            } else {
                format!(
                    "CALL db.index.vector.queryNodes($index, $limit, $embedding) YIELD node, score
                     RETURN node.id AS id, node.title AS title, node.summary AS summary,
                            '{label}' AS signal_type, score
                     LIMIT $limit"
                )
            };

            let mut q = query(&cypher)
                .param("index", index_name.as_str())
                .param("limit", limit as i64)
                .param("embedding", embedding_vec.clone());

            if let Some((min_lat, max_lat, min_lng, max_lng)) = bbox {
                q = q.param("min_lat", min_lat)
                    .param("max_lat", max_lat)
                    .param("min_lng", min_lng)
                    .param("max_lng", max_lng);
            }

            let mut stream = self.client().execute(q).await?;
            while let Some(row) = stream.next().await? {
                let id_str: String = row.get("id").unwrap_or_default();
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    results.push(SignalSearchResult {
                        id,
                        title: row.get("title").unwrap_or_default(),
                        summary: row.get("summary").unwrap_or_default(),
                        signal_type: row.get("signal_type").unwrap_or_default(),
                        score: row.get::<f64>("score").unwrap_or(0.0),
                    });
                }
            }
        }

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit as usize);
        Ok(results)
    }

    async fn get_ungrouped_signals(&self, limit: u32) -> Result<Vec<UngroupedSignal>> {
        use neo4rs::query;

        let q = query(
            "MATCH (n:Concern)
             WHERE NOT (n)-[:MEMBER_OF]->(:SignalGroup)
               AND n.review_status IN ['accepted', 'staged']
             RETURN n.id AS id, n.title AS title, n.summary AS summary,
                    'Concern' AS signal_type,
                    coalesce(n.cause_heat, 0.0) AS cause_heat
             ORDER BY cause_heat DESC
             LIMIT $limit",
        )
        .param("limit", limit as i64);

        let mut stream = self.client().execute(q).await?;
        let mut results = Vec::new();
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                results.push(UngroupedSignal {
                    id,
                    title: row.get("title").unwrap_or_default(),
                    summary: row.get("summary").unwrap_or_default(),
                    signal_type: row.get("signal_type").unwrap_or_default(),
                    cause_heat: row.get::<f64>("cause_heat").unwrap_or(0.0),
                });
            }
        }
        Ok(results)
    }

    async fn get_group_landscape(&self, limit: u32) -> Result<Vec<GroupBrief>> {
        use neo4rs::query;

        let q = query(
            "MATCH (g:SignalGroup)
             OPTIONAL MATCH (sig)-[:MEMBER_OF]->(g)
             WITH g, count(sig) AS sc, collect(sig.id) AS mids
             RETURN g.id AS id, g.label AS label, g.queries AS queries,
                    sc AS signal_count, mids AS member_ids
             ORDER BY sc DESC
             LIMIT $limit",
        )
        .param("limit", limit as i64);

        let mut stream = self.client().execute(q).await?;
        let mut results = Vec::new();
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                let queries: Vec<String> = row.get("queries").unwrap_or_default();
                let raw_member_ids: Vec<String> = row.get("member_ids").unwrap_or_default();
                let member_ids: Vec<Uuid> = raw_member_ids
                    .iter()
                    .filter_map(|s| Uuid::parse_str(s).ok())
                    .collect();
                results.push(GroupBrief {
                    id,
                    label: row.get("label").unwrap_or_default(),
                    queries,
                    signal_count: row.get::<i64>("signal_count").unwrap_or(0) as u32,
                    member_ids,
                });
            }
        }
        Ok(results)
    }

    async fn get_signal_details(&self, ids: &[Uuid]) -> Result<Vec<SignalDetail>> {
        use neo4rs::query;

        if ids.is_empty() {
            return Ok(vec![]);
        }

        let id_strs: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
        let q = query(
            "UNWIND $ids AS sid
             MATCH (n) WHERE n.id = sid
               AND (n:Gathering OR n:Resource OR n:HelpRequest OR n:Announcement OR n:Concern OR n:Condition)
             RETURN n.id AS id, n.title AS title, n.summary AS summary,
                    labels(n)[0] AS signal_type,
                    n.cause_heat AS cause_heat",
        )
        .param("ids", id_strs);

        let mut stream = self.client().execute(q).await?;
        let mut results = Vec::new();
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(&id_str) {
                results.push(SignalDetail {
                    id,
                    title: row.get("title").unwrap_or_default(),
                    summary: row.get("summary").unwrap_or_default(),
                    signal_type: row.get("signal_type").unwrap_or_default(),
                    cause_heat: row.get::<f64>("cause_heat").ok(),
                });
            }
        }
        Ok(results)
    }

    async fn get_signal_location(&self, signal_id: &str) -> Result<Option<(f64, f64)>> {
        Ok(self.get_signal_location(signal_id).await?)
    }

    async fn get_regions_for_signal_by_location(&self, signal_id: &str) -> Result<Vec<Region>> {
        Ok(self.get_regions_for_signal_by_location(signal_id).await?)
    }

    async fn region_exists_by_name(&self, name: &str) -> Result<bool> {
        Ok(self.region_exists_by_name(name).await?)
    }

    async fn get_cluster_detail(&self, group_id: Uuid) -> Result<Option<ClusterDetailRow>> {
        Ok(self.get_cluster_detail(group_id).await?)
    }

    async fn get_cluster_members(&self, group_id: Uuid) -> Result<Vec<WeaveSignal>> {
        Ok(self.get_cluster_members(group_id).await?)
    }

    async fn get_cluster_delta_signals(&self, group_id: Uuid) -> Result<Vec<WeaveSignal>> {
        Ok(self.get_cluster_delta_signals(group_id).await?)
    }
}
