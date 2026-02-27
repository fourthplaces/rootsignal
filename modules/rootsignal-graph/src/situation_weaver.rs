//! SituationWeaver: Living Situations via LLM-Driven Causal Weaving
//!
//! Situations are a root cause + affected population + place. They are the
//! organizational layer on top of the signal graph — not a replacement for it.
//!
//! Pipeline:
//! 1. **Discover** new signals from a scout run (by scout_run_id)
//! 2. **Retrieve** candidate situations via embedding similarity
//! 3. **Weave** via LLM (Haiku): assign signals, write dispatches, update state
//! 4. **Write** graph updates (PART_OF, CITES, Dispatch nodes)
//! 5. **Verify** dispatches post-hoc (citations, PII, fidelity)
//!
//! Dependency inversion: takes `Arc<dyn TextEmbedder>` — concrete Voyage AI
//! implementation is injected by scout, not imported here.

use std::sync::Arc;

use ai_client::claude::Claude;
use chrono::Utc;
use neo4rs::query;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{
    Clarity, DispatchNode, DispatchType, ScoutScope, SensitivityLevel, SituationArc, SituationNode,
    TextEmbedder,
};

use crate::writer::GraphWriter;
use crate::GraphClient;

/// Cosine similarity between two vectors.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut norm_a = 0.0f64;
    let mut norm_b = 0.0f64;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += (*x as f64) * (*y as f64);
        norm_a += (*x as f64) * (*x as f64);
        norm_b += (*y as f64) * (*y as f64);
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

// --- LLM response schemas ---

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WeavingResponse {
    pub assignments: Vec<SignalAssignment>,
    #[serde(default)]
    pub new_situations: Vec<NewSituation>,
    #[serde(default)]
    pub dispatches: Vec<DispatchInput>,
    #[serde(default)]
    pub state_updates: Vec<StateUpdate>,
    #[serde(default)]
    pub splits: Vec<SplitMerge>,
    #[serde(default)]
    pub merges: Vec<SplitMerge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SignalAssignment {
    pub signal_id: String,
    pub situation_id: String,
    pub confidence: f64,
    pub reasoning: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NewSituation {
    pub temp_id: String,
    pub headline: String,
    pub lede: String,
    pub location_name: String,
    #[serde(default)]
    pub initial_structured_state: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DispatchInput {
    pub situation_id: String,
    pub body: String,
    pub signal_ids: Vec<String>,
    pub dispatch_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StateUpdate {
    pub situation_id: String,
    pub structured_state_patch: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SplitMerge {
    pub from_situation_id: String,
    pub to_situation_ids: Vec<String>,
    pub reasoning: String,
}

// --- Internal signal representation ---

struct DiscoveredSignal {
    id: Uuid,
    title: String,
    summary: String,
    node_type: String,
    source_url: String,
    cause_heat: f64,
    lat: Option<f64>,
    lng: Option<f64>,
    embedding: Vec<f32>,
    content_date: Option<String>,
}

struct CandidateSituation {
    id: Uuid,
    headline: String,
    structured_state: String,
    narrative_embedding: Vec<f32>,
    causal_embedding: Vec<f32>,
    arc: String,
}

// --- Stats ---

#[derive(Debug, Default)]
pub struct SituationWeaverStats {
    pub signals_discovered: u32,
    pub signals_assigned: u32,
    pub situations_created: u32,
    pub situations_updated: u32,
    pub dispatches_written: u32,
    pub dispatches_flagged: u32,
    pub splits: u32,
    pub merges: u32,
}

impl std::fmt::Display for SituationWeaverStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SituationWeaver: {} discovered, {} assigned, {} created, {} updated, {} dispatches ({} flagged)",
            self.signals_discovered, self.signals_assigned,
            self.situations_created, self.situations_updated,
            self.dispatches_written, self.dispatches_flagged,
        )
    }
}

// --- Weaver ---

pub struct SituationWeaver {
    client: GraphClient,
    writer: GraphWriter,
    embedder: Arc<dyn TextEmbedder>,
    anthropic_api_key: String,
    scope: ScoutScope,
}

const NARRATIVE_SIMILARITY_THRESHOLD: f64 = 0.6;
const WIDE_NET_THRESHOLD: f64 = 0.45;
const WIDE_NET_HEAT_MIN: f64 = 0.5;
const COLD_REACTIVATION_NARRATIVE: f64 = 0.75;
const COLD_REACTIVATION_CAUSAL: f64 = 0.80;
const TOP_K_CANDIDATES: usize = 5;

impl SituationWeaver {
    pub fn new(
        client: GraphClient,
        anthropic_api_key: &str,
        embedder: Arc<dyn TextEmbedder>,
        scope: ScoutScope,
    ) -> Self {
        Self {
            writer: GraphWriter::new(client.clone()),
            client,
            embedder,
            anthropic_api_key: anthropic_api_key.to_string(),
            scope,
        }
    }

    /// Run the situation weaving pipeline for signals from this scout run.
    pub async fn run(
        &self,
        scout_run_id: &str,
        has_budget: bool,
    ) -> Result<SituationWeaverStats, Box<dyn std::error::Error + Send + Sync>> {
        let mut stats = SituationWeaverStats::default();

        // Phase 1: Discover unassigned signals
        let signals = self.discover_signals(scout_run_id).await?;
        stats.signals_discovered = signals.len() as u32;

        if signals.is_empty() {
            info!("SituationWeaver: no unassigned signals, skipping");
            return Ok(stats);
        }
        info!(
            count = signals.len(),
            "SituationWeaver: discovered unassigned signals"
        );

        if !has_budget {
            warn!("SituationWeaver: no LLM budget, marking signals as pending");
            self.mark_signals_pending(scout_run_id).await?;
            return Ok(stats);
        }

        // Phase 2: Load all candidate situations
        let candidates = self.load_candidate_situations().await?;
        info!(
            count = candidates.len(),
            "SituationWeaver: loaded candidate situations"
        );

        // Phase 3-4: Batch signals, weave via LLM, write graph updates
        // Process sequentially to prevent duplicate situation creation
        let batch_size = 5;
        let mut temp_id_map: std::collections::HashMap<String, Uuid> =
            std::collections::HashMap::new();
        for chunk in signals.chunks(batch_size) {
            match self.weave_batch(chunk, &candidates, &mut temp_id_map).await {
                Ok(batch_stats) => {
                    stats.signals_assigned += batch_stats.signals_assigned;
                    stats.situations_created += batch_stats.situations_created;
                    stats.situations_updated += batch_stats.situations_updated;
                    stats.dispatches_written += batch_stats.dispatches_written;
                    stats.splits += batch_stats.splits;
                    stats.merges += batch_stats.merges;
                }
                Err(e) => {
                    warn!(error = %e, "SituationWeaver: batch weaving failed, continuing");
                }
            }
        }

        // Collect all situations affected by this run
        let affected_situations = self.find_affected_situations(scout_run_id).await?;

        // Phase 5: Recompute temperature for all affected situations
        for sit_id in &affected_situations {
            match crate::situation_temperature::recompute_situation_temperature(
                &self.client,
                &self.writer,
                sit_id,
            )
            .await
            {
                Ok(components) => {
                    info!(
                        situation_id = %sit_id,
                        temperature = components.temperature,
                        arc = %components.arc,
                        "Temperature recomputed"
                    );
                }
                Err(e) => {
                    warn!(error = %e, situation_id = %sit_id, "Temperature recomputation failed");
                }
            }
        }

        // Phase 6: Post-hoc verification of new dispatches
        let flagged = self.verify_dispatches().await?;
        stats.dispatches_flagged = flagged;

        info!(%stats, "SituationWeaver run complete");
        Ok(stats)
    }

    /// Phase 1: Discover unassigned signals from this scout run.
    async fn discover_signals(
        &self,
        scout_run_id: &str,
    ) -> Result<Vec<DiscoveredSignal>, neo4rs::Error> {
        let g = &self.client.graph;
        let mut signals = Vec::new();

        let labels = ["Gathering", "Aid", "Need", "Notice", "Tension"];
        for label in &labels {
            let q = query(&format!(
                "MATCH (n:{label} {{scout_run_id: $run_id}})
                 WHERE NOT (n)-[:PART_OF]->(:Situation)
                   AND NOT n:Citation
                 RETURN n.id AS id, n.title AS title, n.summary AS summary,
                        '{label}' AS node_type, n.embedding AS embedding,
                        n.source_url AS source_url,
                        coalesce(n.cause_heat, 0.0) AS cause_heat,
                        n.lat AS lat, n.lng AS lng,
                        n.content_date AS content_date"
            ))
            .param("run_id", scout_run_id);

            let mut stream = g.execute(q).await?;
            while let Some(row) = stream.next().await? {
                let id_str: String = row.get("id").unwrap_or_default();
                let id = match Uuid::parse_str(&id_str) {
                    Ok(id) => id,
                    Err(_) => continue,
                };
                let embedding: Vec<f32> = row.get("embedding").unwrap_or_default();
                if embedding.is_empty() {
                    continue; // Skip signals without embeddings
                }

                signals.push(DiscoveredSignal {
                    id,
                    title: row.get("title").unwrap_or_default(),
                    summary: row.get("summary").unwrap_or_default(),
                    node_type: row.get("node_type").unwrap_or_default(),
                    source_url: row.get("source_url").unwrap_or_default(),
                    cause_heat: row.get("cause_heat").unwrap_or(0.0),
                    lat: row.get("lat").ok(),
                    lng: row.get("lng").ok(),
                    embedding,
                    content_date: row.get("content_date").ok(),
                });
            }
        }

        Ok(signals)
    }

    /// Phase 2: Load all non-cold situations as candidates.
    async fn load_candidate_situations(&self) -> Result<Vec<CandidateSituation>, neo4rs::Error> {
        let g = &self.client.graph;
        let mut candidates = Vec::new();

        // Load active/developing/emerging/cooling situations
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
            let id = match Uuid::parse_str(&id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };

            candidates.push(CandidateSituation {
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

    /// Find top candidate situations for a signal based on embedding similarity.
    fn find_candidates(
        &self,
        signal: &DiscoveredSignal,
        candidates: &[CandidateSituation],
    ) -> Vec<(Uuid, f64, f64)> {
        // (situation_id, narrative_sim, causal_sim)
        let mut scored: Vec<(Uuid, f64, f64)> = candidates
            .iter()
            .filter(|c| !c.narrative_embedding.is_empty())
            .map(|c| {
                let narrative_sim = cosine_similarity(&signal.embedding, &c.narrative_embedding);
                let causal_sim = cosine_similarity(&signal.embedding, &c.causal_embedding);
                (c.id, narrative_sim, causal_sim)
            })
            .collect();

        // Sort by max of narrative and causal similarity
        scored.sort_by(|a, b| {
            let max_a = a.1.max(a.2);
            let max_b = b.1.max(b.2);
            max_b
                .partial_cmp(&max_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Cold reactivation check: if top match is cold, use higher thresholds
        let mut results: Vec<(Uuid, f64, f64)> = scored
            .iter()
            .filter(|(id, narrative_sim, causal_sim)| {
                let is_cold = candidates
                    .iter()
                    .find(|c| c.id == *id)
                    .map(|c| c.arc == "cold")
                    .unwrap_or(false);

                if is_cold {
                    *narrative_sim >= COLD_REACTIVATION_NARRATIVE
                        || *causal_sim >= COLD_REACTIVATION_CAUSAL
                } else {
                    *narrative_sim >= NARRATIVE_SIMILARITY_THRESHOLD
                        || *causal_sim >= NARRATIVE_SIMILARITY_THRESHOLD
                }
            })
            .take(TOP_K_CANDIDATES)
            .copied()
            .collect();

        // Wide Net fallback: high cause_heat signal with low similarity
        if results.is_empty()
            && signal.cause_heat >= WIDE_NET_HEAT_MIN
            && scored.first().map(|s| s.1.max(s.2)).unwrap_or(0.0) < NARRATIVE_SIMILARITY_THRESHOLD
        {
            let developing_matches: Vec<(Uuid, f64, f64)> = scored
                .iter()
                .filter(|(id, narrative_sim, causal_sim)| {
                    let is_developing = candidates
                        .iter()
                        .find(|c| c.id == *id)
                        .map(|c| c.arc == "developing")
                        .unwrap_or(false);
                    is_developing
                        && (*narrative_sim >= WIDE_NET_THRESHOLD
                            || *causal_sim >= WIDE_NET_THRESHOLD)
                })
                .take(TOP_K_CANDIDATES)
                .copied()
                .collect();

            if !developing_matches.is_empty() {
                results = developing_matches;
            }
        }

        results
    }

    /// Phase 3-4: Weave a batch of signals via LLM and write graph updates.
    async fn weave_batch(
        &self,
        signals: &[DiscoveredSignal],
        candidates: &[CandidateSituation],
        temp_id_map: &mut std::collections::HashMap<String, Uuid>,
    ) -> Result<SituationWeaverStats, Box<dyn std::error::Error + Send + Sync>> {
        let mut stats = SituationWeaverStats::default();

        // Build per-signal candidate lists
        let mut signal_candidates: Vec<serde_json::Value> = Vec::new();
        let mut all_candidate_ids: std::collections::HashSet<Uuid> =
            std::collections::HashSet::new();

        // Sort signals chronologically before building JSON
        let mut sorted_signals: Vec<&DiscoveredSignal> = signals.iter().collect();
        sorted_signals.sort_by(|a, b| a.content_date.cmp(&b.content_date));

        let signals_json: Vec<serde_json::Value> = sorted_signals
            .iter()
            .map(|s| {
                let cands = self.find_candidates(s, candidates);
                for (id, _, _) in &cands {
                    all_candidate_ids.insert(*id);
                }
                signal_candidates.push(serde_json::json!(cands
                    .iter()
                    .map(|(id, n, c)| {
                        serde_json::json!({
                            "situation_id": id.to_string(),
                            "narrative_similarity": format!("{:.2}", n),
                            "causal_similarity": format!("{:.2}", c),
                        })
                    })
                    .collect::<Vec<_>>()));

                signal_to_json(s)
            })
            .collect();

        // Build candidate situation context for the LLM
        let candidate_context: Vec<serde_json::Value> = candidates
            .iter()
            .filter(|c| all_candidate_ids.contains(&c.id))
            .map(|c| {
                serde_json::json!({
                    "id": c.id.to_string(),
                    "headline": c.headline,
                    "arc": c.arc,
                    "structured_state": truncate(&c.structured_state, 500),
                })
            })
            .collect();

        let prompt = build_weaving_prompt(
            &signals_json,
            &signal_candidates,
            &candidate_context,
            &self.scope,
        );

        let claude = Claude::new(&self.anthropic_api_key, "claude-haiku-4-5-20251001");
        let response: WeavingResponse = claude.extract(SYSTEM_PROMPT, &prompt).await?;

        // Process new situations first (so assignments can reference them)
        for new_sit in &response.new_situations {
            let sit_id = Uuid::new_v4();
            temp_id_map.insert(new_sit.temp_id.clone(), sit_id);

            // Compute initial embedding from signals assigned to this situation
            let assigned_signal_ids: Vec<&str> = response
                .assignments
                .iter()
                .filter(|a| a.situation_id == new_sit.temp_id)
                .map(|a| a.signal_id.as_str())
                .collect();

            let (narrative_emb, centroid_lat, centroid_lng) =
                self.compute_initial_embeddings(signals, &assigned_signal_ids);
            let causal_emb = if !new_sit.initial_structured_state.is_null() {
                let thesis = new_sit
                    .initial_structured_state
                    .get("root_cause_thesis")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&new_sit.headline);
                self.embedder
                    .embed(thesis)
                    .await
                    .unwrap_or(narrative_emb.clone())
            } else {
                narrative_emb.clone()
            };

            let now = Utc::now();
            let situation = SituationNode {
                id: sit_id,
                headline: new_sit.headline.clone(),
                lede: new_sit.lede.clone(),
                arc: SituationArc::Emerging,
                temperature: 0.0, // computed in Phase 3 (temperature)
                tension_heat: 0.0,
                entity_velocity: 0.0,
                amplification: 0.0,
                response_coverage: 0.0,
                clarity_need: 1.0, // new situation = fuzzy = high clarity need
                clarity: Clarity::Fuzzy,
                centroid_lat,
                centroid_lng,
                location_name: Some(new_sit.location_name.clone()).filter(|s| !s.is_empty()),
                structured_state: serde_json::to_string(&new_sit.initial_structured_state)
                    .unwrap_or_else(|_| "{}".to_string()),
                signal_count: 0,
                tension_count: 0,
                dispatch_count: 0,
                first_seen: now,
                last_updated: now,
                sensitivity: SensitivityLevel::General,
                category: None,
            };

            self.writer
                .create_situation(&situation, &narrative_emb, &causal_emb)
                .await?;
            stats.situations_created += 1;
        }

        // Process assignments
        for assignment in &response.assignments {
            let signal_id = match Uuid::parse_str(&assignment.signal_id) {
                Ok(id) => id,
                Err(_) => continue,
            };

            let situation_id = if let Some(mapped) = temp_id_map.get(&assignment.situation_id) {
                *mapped
            } else {
                match Uuid::parse_str(&assignment.situation_id) {
                    Ok(id) => id,
                    Err(_) => continue,
                }
            };

            // Find the signal's label for the edge
            let label = signals
                .iter()
                .find(|s| s.id == signal_id)
                .map(|s| s.node_type.as_str())
                .unwrap_or("Tension");

            self.writer
                .merge_evidence_edge(&signal_id, label, &situation_id, assignment.confidence)
                .await?;
            stats.signals_assigned += 1;

            // Update signal count on situation
            let g = &self.client.graph;
            let count_q = query(
                "MATCH (s:Situation {id: $id})
                 SET s.signal_count = s.signal_count + 1
                 WITH s
                 MATCH (sig)-[:PART_OF]->(s)
                 WHERE sig:Tension
                 WITH s, count(sig) AS tc
                 SET s.tension_count = tc",
            )
            .param("id", situation_id.to_string());
            let _ = g.run(count_q).await;

            // Aggregate signal tags → situation tags
            let _ = self.writer.aggregate_situation_tags(situation_id).await;
        }

        // Process dispatches
        for dispatch_input in &response.dispatches {
            let situation_id = if let Some(mapped) = temp_id_map.get(&dispatch_input.situation_id) {
                *mapped
            } else {
                match Uuid::parse_str(&dispatch_input.situation_id) {
                    Ok(id) => id,
                    Err(_) => continue,
                }
            };

            let signal_ids: Vec<Uuid> = dispatch_input
                .signal_ids
                .iter()
                .filter_map(|s| Uuid::parse_str(s).ok())
                .collect();

            let dispatch_type = dispatch_input
                .dispatch_type
                .parse::<DispatchType>()
                .unwrap_or(DispatchType::Update);

            let dispatch = DispatchNode {
                id: Uuid::new_v4(),
                situation_id,
                body: dispatch_input.body.clone(),
                signal_ids: signal_ids.clone(),
                created_at: Utc::now(),
                dispatch_type,
                supersedes: None,
                flagged_for_review: false,
                flag_reason: None,
                fidelity_score: None,
            };

            self.writer.create_dispatch(&dispatch).await?;
            self.writer
                .merge_cites_edges(&dispatch.id, &signal_ids)
                .await?;
            stats.dispatches_written += 1;

            // Track situations that got dispatches
            stats.situations_updated += 1;
        }

        // Process state updates
        for update in &response.state_updates {
            let situation_id = if let Some(mapped) = temp_id_map.get(&update.situation_id) {
                *mapped
            } else {
                match Uuid::parse_str(&update.situation_id) {
                    Ok(id) => id,
                    Err(_) => continue,
                }
            };

            let state_json = serde_json::to_string(&update.structured_state_patch)
                .unwrap_or_else(|_| "{}".to_string());
            let _ = self
                .writer
                .update_situation_state(&situation_id, &state_json)
                .await;
        }

        Ok(stats)
    }

    /// Compute initial narrative embedding as mean of assigned signal embeddings.
    /// Also returns centroid lat/lng.
    fn compute_initial_embeddings(
        &self,
        signals: &[DiscoveredSignal],
        assigned_ids: &[&str],
    ) -> (Vec<f32>, Option<f64>, Option<f64>) {
        let assigned: Vec<&DiscoveredSignal> = signals
            .iter()
            .filter(|s| assigned_ids.contains(&s.id.to_string().as_str()))
            .collect();

        if assigned.is_empty() {
            return (vec![0.0; 1024], None, None);
        }

        // Mean embedding
        let dim = assigned[0].embedding.len();
        let mut centroid = vec![0.0f64; dim];
        for s in &assigned {
            for (i, v) in s.embedding.iter().enumerate() {
                if i < dim {
                    centroid[i] += *v as f64;
                }
            }
        }
        let n = assigned.len() as f64;
        let embedding: Vec<f32> = centroid.iter().map(|v| (*v / n) as f32).collect();

        // Mean lat/lng
        let lats: Vec<f64> = assigned.iter().filter_map(|s| s.lat).collect();
        let lngs: Vec<f64> = assigned.iter().filter_map(|s| s.lng).collect();
        let centroid_lat = if lats.is_empty() {
            None
        } else {
            Some(lats.iter().sum::<f64>() / lats.len() as f64)
        };
        let centroid_lng = if lngs.is_empty() {
            None
        } else {
            Some(lngs.iter().sum::<f64>() / lngs.len() as f64)
        };

        (embedding, centroid_lat, centroid_lng)
    }

    /// Phase 6: Post-hoc verification of recent dispatches.
    async fn verify_dispatches(&self) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
        let g = &self.client.graph;
        let mut flagged = 0u32;

        // Find dispatches that haven't been verified yet
        let q = query(
            "MATCH (d:Dispatch)
             WHERE d.flagged_for_review = false
               AND d.fidelity_score IS NULL
             RETURN d.id AS id, d.body AS body, d.situation_id AS situation_id
             ORDER BY d.created_at DESC
             LIMIT 50",
        );

        let mut stream = g.execute(q).await?;
        let mut dispatches_to_check: Vec<(Uuid, String)> = Vec::new();
        while let Some(row) = stream.next().await? {
            let id_str: String = row.get("id").unwrap_or_default();
            let id = match Uuid::parse_str(&id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };
            let body: String = row.get("body").unwrap_or_default();
            dispatches_to_check.push((id, body));
        }

        for (dispatch_id, body) in &dispatches_to_check {
            // 1. Citation check: every [signal:UUID] must exist
            let cited_ids = extract_signal_citations(body);
            if !cited_ids.is_empty() {
                let missing = self.writer.verify_signal_ids(&cited_ids).await?;
                if !missing.is_empty() {
                    self.writer
                        .flag_dispatch_for_review(dispatch_id, "invalid_citation", None)
                        .await?;
                    flagged += 1;
                    continue;
                }
            }

            // 2. PII check
            if !rootsignal_common::detect_pii(body).is_empty() {
                self.writer
                    .flag_dispatch_for_review(dispatch_id, "pii_detected", None)
                    .await?;
                flagged += 1;
                continue;
            }

            // 3. Citation coverage: factual sentences need citations
            if has_uncited_factual_claims(body) {
                self.writer
                    .flag_dispatch_for_review(dispatch_id, "uncited_claim", None)
                    .await?;
                flagged += 1;
                continue;
            }
        }

        Ok(flagged)
    }

    /// Find all situations that have signals from this scout run.
    async fn find_affected_situations(
        &self,
        scout_run_id: &str,
    ) -> Result<Vec<Uuid>, neo4rs::Error> {
        let g = &self.client.graph;
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
            if let Ok(id) = Uuid::parse_str(&id_str) {
                situations.push(id);
            }
        }

        Ok(situations)
    }

    /// Mark signals as pending for next weaving run (when no LLM budget).
    async fn mark_signals_pending(&self, scout_run_id: &str) -> Result<(), neo4rs::Error> {
        let g = &self.client.graph;
        for label in &["Gathering", "Aid", "Need", "Notice", "Tension"] {
            let q = query(&format!(
                "MATCH (n:{label} {{scout_run_id: $run_id}})
                 WHERE NOT (n)-[:PART_OF]->(:Situation)
                 SET n.situation_pending = true"
            ))
            .param("run_id", scout_run_id);
            let _ = g.run(q).await;
        }
        Ok(())
    }
}

// --- Prompt building ---

const SYSTEM_PROMPT: &str = r#"You are a situation tracker for a civic intelligence system. Your job is to assign signals to situations and write factual dispatches.

HARD RULES:
1. Every claim in a dispatch MUST cite a specific signal by ID using [signal:UUID] format.
2. If sources disagree on cause, present ALL claims side by side. Never pick a winner.
3. Describe what is happening, not what it means. "3 new eviction filings" not "the crisis deepens."
4. Use invitational, factual tone. Urgency is about opportunity windows, not threats.
5. If a signal doesn't fit any candidate situation, create a new one.
6. Do not infer geographic parallels. Only associate a signal with a situation if the source EXPLICITLY references that place/issue.
7. Different root cause = different situation, even if same place and same surface effect.
8. Do NOT assign actor roles (decision-maker, beneficiary, etc). Only list actor NAMES that appear in signals. Role analysis requires a separate, higher-accuracy process.
9. If new evidence contradicts an existing dispatch, write a "correction" dispatch that references what it supersedes. Do not silently change the structured state.
10. Actively challenge the existing root_cause_thesis when new evidence suggests alternatives. Do not confirm the thesis by default.
11. SEMANTIC FRICTION: If two signals are geographically close but semantically distant, you MUST explain why they belong to the SAME situation. Default to separate situations when geography overlaps but content diverges.
12. LEAD WITH RESPONSES: When writing dispatches about situations that have both tensions AND responses, lead with the response. The response is the primary signal; the tension provides context.
13. Signals are listed in chronological order by publication date. Respect this ordering in dispatches.

Respond with valid JSON matching the WeavingResponse schema."#;

fn build_weaving_prompt(
    signals: &[serde_json::Value],
    signal_candidates: &[serde_json::Value],
    candidate_situations: &[serde_json::Value],
    scope: &ScoutScope,
) -> String {
    let signals_with_candidates: Vec<serde_json::Value> = signals
        .iter()
        .zip(signal_candidates.iter())
        .map(|(s, c)| {
            let mut obj = s.clone();
            obj.as_object_mut()
                .unwrap()
                .insert("candidate_situations".to_string(), c.clone());
            obj
        })
        .collect();

    format!(
        r#"Assign these new signals to situations and write dispatches.

NEW SIGNALS:
{}

CANDIDATE SITUATIONS:
{}

SCOPE: {} (lat: {}, lng: {})

For each signal:
- If it matches an existing situation (same root cause + place), assign it
- If no situation matches, create a new one with temp_id "NEW-1", "NEW-2", etc.
- Write a dispatch for each affected situation summarizing the new information
- Update structured_state if the thesis, confidence, or timeline changes

Return JSON with: assignments, new_situations, dispatches, state_updates"#,
        serde_json::to_string_pretty(&signals_with_candidates).unwrap_or_default(),
        serde_json::to_string_pretty(&candidate_situations).unwrap_or_default(),
        scope.name,
        scope.center_lat,
        scope.center_lng,
    )
}

// --- Utility functions ---

/// Format a signal as JSON for the LLM prompt. Extracted for testability.
fn signal_to_json(s: &DiscoveredSignal) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "id": s.id.to_string(),
        "title": s.title,
        "summary": truncate(&s.summary, 300),
        "type": s.node_type,
        "cause_heat": format!("{:.2}", s.cause_heat),
        "source_url": s.source_url,
        "location": format_location(s.lat, s.lng),
    });
    if let Some(date) = &s.content_date {
        obj.as_object_mut()
            .unwrap()
            .insert("published".to_string(), serde_json::json!(date));
    }
    obj
}

fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        &s[..s
            .char_indices()
            .take(max_len)
            .last()
            .map(|(i, _)| i)
            .unwrap_or(max_len)]
    }
}

fn format_location(lat: Option<f64>, lng: Option<f64>) -> String {
    match (lat, lng) {
        (Some(lat), Some(lng)) => format!("{:.4}, {:.4}", lat, lng),
        _ => "unknown".to_string(),
    }
}

/// Extract [signal:UUID] citations from dispatch text.
fn extract_signal_citations(body: &str) -> Vec<Uuid> {
    let mut ids = Vec::new();
    let marker = "[signal:";
    let mut search = body;
    while let Some(start) = search.find(marker) {
        let after = &search[start + marker.len()..];
        if let Some(end) = after.find(']') {
            if let Ok(id) = Uuid::parse_str(&after[..end]) {
                ids.push(id);
            }
            search = &after[end..];
        } else {
            break;
        }
    }
    ids
}

/// Check if dispatch body contains factual claims without signal citations.
fn has_uncited_factual_claims(body: &str) -> bool {
    // Split into sentences
    let sentences: Vec<&str> = body
        .split(|c: char| c == '.' || c == '!' || c == '?')
        .filter(|s| !s.trim().is_empty())
        .collect();

    for sentence in &sentences {
        let trimmed = sentence.trim();
        // Skip short connective phrases
        if trimmed.split_whitespace().count() < 4 {
            continue;
        }
        // Skip sentences that have citations
        if trimmed.contains("[signal:") {
            continue;
        }
        // Check for factual indicators: numbers, proper nouns (uppercase words), dates
        let has_number = trimmed.chars().any(|c| c.is_ascii_digit());
        let has_proper_noun = trimmed
            .split_whitespace()
            .skip(1) // skip first word (sentence start)
            .any(|w| w.chars().next().map(|c| c.is_uppercase()).unwrap_or(false));
        if has_number || has_proper_noun {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_sorted_chronologically_with_published_field() {
        // Two signals fed in reverse chronological order
        let signals = vec![
            DiscoveredSignal {
                id: Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap(),
                title: "Resolution announced".into(),
                summary: "Council approved fix".into(),
                node_type: "Aid".into(),
                source_url: "https://example.com/resolution".into(),
                cause_heat: 0.0,
                lat: None,
                lng: None,
                embedding: vec![],
                content_date: Some("2024-01-15".into()),
            },
            DiscoveredSignal {
                id: Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
                title: "Tension identified".into(),
                summary: "Water main break reported".into(),
                node_type: "Tension".into(),
                source_url: "https://example.com/tension".into(),
                cause_heat: 0.8,
                lat: None,
                lng: None,
                embedding: vec![],
                content_date: Some("2024-01-05".into()),
            },
        ];

        // Sort the same way weave_batch does
        let mut sorted: Vec<&DiscoveredSignal> = signals.iter().collect();
        sorted.sort_by(|a, b| a.content_date.cmp(&b.content_date));

        let json_values: Vec<serde_json::Value> =
            sorted.iter().map(|s| signal_to_json(s)).collect();

        // First element should be the tension (Jan 5), second the resolution (Jan 15)
        assert_eq!(json_values[0]["title"], "Tension identified");
        assert_eq!(json_values[1]["title"], "Resolution announced");

        // Both should have "published" field
        assert_eq!(json_values[0]["published"], "2024-01-05");
        assert_eq!(json_values[1]["published"], "2024-01-15");
    }

    #[test]
    fn signal_without_content_date_omits_published_field() {
        let signal = DiscoveredSignal {
            id: Uuid::new_v4(),
            title: "No date signal".into(),
            summary: "Some summary".into(),
            node_type: "Notice".into(),
            source_url: "https://example.com".into(),
            cause_heat: 0.0,
            lat: None,
            lng: None,
            embedding: vec![],
            content_date: None,
        };

        let json = signal_to_json(&signal);
        assert!(
            json.get("published").is_none(),
            "Should not have published field when content_date is None"
        );
    }
}
