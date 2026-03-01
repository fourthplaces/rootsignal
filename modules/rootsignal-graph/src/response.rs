use ai_client::claude::Claude;
use tracing::{info, warn};
use uuid::Uuid;

use crate::writer::GraphWriter;
use crate::GraphClient;
use neo4rs::query;

/// Maps responses (Aid/Gathering) to active Tensions/Needs using embedding similarity + LLM verification.
pub struct ResponseMapper {
    client: GraphClient,
    writer: GraphWriter,
    anthropic_api_key: String,
    min_lat: f64,
    max_lat: f64,
    min_lng: f64,
    max_lng: f64,
}

impl ResponseMapper {
    pub fn new(
        client: GraphClient,
        anthropic_api_key: &str,
        center_lat: f64,
        center_lng: f64,
        radius_km: f64,
    ) -> Self {
        let lat_delta = radius_km / 111.0;
        let lng_delta = radius_km / (111.0 * center_lat.to_radians().cos());
        Self {
            writer: GraphWriter::new(client.clone()),
            client,
            anthropic_api_key: anthropic_api_key.to_string(),
            min_lat: center_lat - lat_delta,
            max_lat: center_lat + lat_delta,
            min_lng: center_lng - lng_delta,
            max_lng: center_lng + lng_delta,
        }
    }

    /// For each active Tension/Need, find Aid/Gathering signals that might respond to it.
    /// Uses embedding similarity as a cheap filter, then LLM as a verifier.
    pub async fn map_responses(
        &self,
    ) -> Result<ResponseMappingStats, Box<dyn std::error::Error + Send + Sync>> {
        let mut stats = ResponseMappingStats::default();

        // Get active tensions with embeddings
        let tensions = self
            .writer
            .get_active_tensions(self.min_lat, self.max_lat, self.min_lng, self.max_lng)
            .await?;
        if tensions.is_empty() {
            info!("No active tensions for response mapping");
            return Ok(stats);
        }

        info!(tensions = tensions.len(), "Running response mapping");

        for (tension_id, tension_embedding) in &tensions {
            // Vector search for similar Aid/Gathering signals
            let candidates = self.find_response_candidates(tension_embedding).await?;
            stats.candidates_found += candidates.len() as u32;

            if candidates.is_empty() {
                continue;
            }

            // LLM verification on top candidates (max 5)
            let tension_info = self.get_signal_info(*tension_id).await?;
            let Some(tension_info) = tension_info else {
                continue;
            };

            for (candidate_id, candidate_similarity) in candidates.iter().take(5) {
                let candidate_info = self.get_signal_info(*candidate_id).await?;
                let Some(candidate_info) = candidate_info else {
                    continue;
                };

                match self.verify_response(&tension_info, &candidate_info).await {
                    Ok(Some(explanation)) => {
                        if let Err(e) = self
                            .writer
                            .create_response_edge(
                                *candidate_id,
                                *tension_id,
                                *candidate_similarity,
                                &explanation,
                            )
                            .await
                        {
                            warn!(error = %e, "Failed to create RESPONDS_TO edge");
                        } else {
                            stats.edges_created += 1;
                        }
                    }
                    Ok(None) => {
                        // LLM says not a match
                    }
                    Err(e) => {
                        warn!(error = %e, "LLM verification failed");
                    }
                }
            }
        }

        info!(
            edges = stats.edges_created,
            candidates = stats.candidates_found,
            "Response mapping complete"
        );
        Ok(stats)
    }

    /// Find Aid/Gathering candidates that are similar to a tension's embedding.
    async fn find_response_candidates(
        &self,
        tension_embedding: &[f64],
    ) -> Result<Vec<(Uuid, f64)>, neo4rs::Error> {

        let mut candidates = Vec::new();

        for index in &["aid_embedding", "gathering_embedding", "need_embedding"] {
            let q = query(&format!(
                "CALL db.index.vector.queryNodes('{}', 20, $embedding)
                 YIELD node, score AS similarity
                 WHERE similarity >= 0.4
                   AND node.lat >= $min_lat AND node.lat <= $max_lat
                   AND node.lng >= $min_lng AND node.lng <= $max_lng
                 RETURN node.id AS id, similarity
                 ORDER BY similarity DESC
                 LIMIT 5",
                index
            ))
            .param("embedding", tension_embedding.to_vec())
            .param("min_lat", self.min_lat)
            .param("max_lat", self.max_lat)
            .param("min_lng", self.min_lng)
            .param("max_lng", self.max_lng);

            let mut stream = self.client.graph.execute(q).await?;
            while let Some(row) = stream.next().await? {
                let id_str: String = row.get("id").unwrap_or_default();
                let similarity: f64 = row.get("similarity").unwrap_or(0.0);
                if let Ok(id) = Uuid::parse_str(&id_str) {
                    candidates.push((id, similarity));
                }
            }
        }

        // Sort by similarity descending
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(candidates)
    }

    /// Get basic info about a signal for LLM verification.
    async fn get_signal_info(&self, id: Uuid) -> Result<Option<SignalInfo>, neo4rs::Error> {

        for label in &["Tension", "Need", "Aid", "Gathering"] {
            let q = query(&format!(
                "MATCH (n:{label} {{id: $id}})
                 RETURN n.title AS title, n.summary AS summary"
            ))
            .param("id", id.to_string());

            let mut stream = self.client.graph.execute(q).await?;
            if let Some(row) = stream.next().await? {
                return Ok(Some(SignalInfo {
                    title: row.get("title").unwrap_or_default(),
                    summary: row.get("summary").unwrap_or_default(),
                }));
            }
        }
        Ok(None)
    }

    /// LLM verifies whether a candidate signal actually responds to a tension.
    async fn verify_response(
        &self,
        tension: &SignalInfo,
        candidate: &SignalInfo,
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
        let prompt = format!(
            r#"Does Signal B respond to or help address Problem A?

Problem A: {} — {}
Signal B: {} — {}

If yes, respond with a brief explanation (1 sentence) of how B helps address A.
If no, respond with just "NO".

Respond with ONLY the explanation or "NO"."#,
            tension.title, tension.summary, candidate.title, candidate.summary,
        );

        let claude = Claude::new(&self.anthropic_api_key, "claude-haiku-4-5-20251001");
        let response = claude.chat_completion(
            "You evaluate whether community resources respond to community needs. Be strict — only confirm genuine matches.",
            &prompt,
        ).await?;

        let response = response.trim();
        if response == "NO" || response.to_lowercase().starts_with("no") {
            Ok(None)
        } else {
            Ok(Some(response.to_string()))
        }
    }
}

struct SignalInfo {
    title: String,
    summary: String,
}

#[derive(Debug, Default)]
pub struct ResponseMappingStats {
    pub candidates_found: u32,
    pub edges_created: u32,
}

impl std::fmt::Display for ResponseMappingStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "\n=== Response Mapping ===")?;
        writeln!(f, "Candidates found: {}", self.candidates_found)?;
        writeln!(f, "Edges created:    {}", self.edges_created)?;
        Ok(())
    }
}

