//! `GraphQueries` — trait abstraction over `GraphReader` for testability.
//!
//! Handlers and activities take `&dyn GraphQueries` instead of `&GraphReader`,
//! enabling in-memory mocks for unit tests while production uses the real Neo4j-backed
//! implementation.

use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use rootsignal_common::events::{CauseHeatScore, SimilarityEdge, SystemEvent};
use rootsignal_common::{ActorNode, NodeType, PinNode, SourceNode};

use crate::situation_temperature::TemperatureComponents;
use crate::writer::{
    ConcernLinkerTarget, ConcernResponseShape, DuplicateMatch, ExtractionYield,
    GapTypeStats, GatheringFinderTarget, InvestigationTarget,
    ResponseFinderTarget, ResponseHeuristic, SignalTypeCounts, SituationBrief,
    SourceBrief, SourceStats, UnmetTension, WeaveCandidate, WeaveSignal,
};

/// Signal info for actor extraction (wraps the raw Cypher result from actor_extractor).
#[derive(Debug, Clone)]
pub struct UnlinkedSignal {
    pub id: Uuid,
    pub title: String,
    pub summary: String,
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

    /// Find signals without actor links in a bounding box. Wraps raw Cypher in actor_extractor.
    async fn find_signals_without_actors(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<UnlinkedSignal>>;

    /// Compute situation temperature from graph mechanics. Wraps `situation_temperature::compute_temperature_events`.
    async fn compute_situation_temperature(
        &self,
        situation_id: &Uuid,
    ) -> Result<(TemperatureComponents, Vec<SystemEvent>)>;

    /// Re-evaluate severity for all Notices in a bounding box. Wraps `severity_inference::compute_severity_inference`.
    async fn compute_severity_inference(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<(u32, Vec<SystemEvent>)>;

    /// Compute cause heat for signals in a bounding box. Wraps `cause_heat::compute_cause_heat`.
    async fn compute_cause_heat(
        &self,
        threshold: f64,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<CauseHeatScore>>;
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

    async fn find_signals_without_actors(
        &self,
        min_lat: f64,
        max_lat: f64,
        min_lng: f64,
        max_lng: f64,
    ) -> Result<Vec<UnlinkedSignal>> {
        use neo4rs::query;

        let q = query(
            "MATCH (n)
             WHERE (n:Gathering OR n:Resource OR n:HelpRequest OR n:Announcement OR n:Concern OR n:Condition)
               AND NOT (n)<-[:ACTED_IN]-(:Actor)
               AND n.lat >= $min_lat AND n.lat <= $max_lat
               AND n.lng >= $min_lng AND n.lng <= $max_lng
             RETURN n.id AS id, n.title AS title, n.summary AS summary
             ORDER BY n.extracted_at DESC
             LIMIT 200",
        )
        .param("min_lat", min_lat)
        .param("max_lat", max_lat)
        .param("min_lng", min_lng)
        .param("max_lng", max_lng);

        let mut stream = self.client().execute(q).await?;
        let mut signals = Vec::new();
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let id = match Uuid::parse_str(&id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };
            let title: String = row.get("title").unwrap_or_default();
            let summary: String = row.get("summary").unwrap_or_default();
            if title.is_empty() && summary.is_empty() {
                continue;
            }
            signals.push(UnlinkedSignal { id, title, summary });
        }
        Ok(signals)
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
    ) -> Result<(u32, Vec<SystemEvent>)> {
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
}
