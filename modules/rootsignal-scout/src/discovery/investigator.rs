use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ai_client::claude::Claude;
use anyhow::Result;
use chrono::Utc;
use schemars::JsonSchema;
use serde::{de, Deserialize};
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{ScoutScope, EvidenceNode};
use rootsignal_graph::{EvidenceSummary, GraphWriter, InvestigationTarget};

use crate::pipeline::traits::SignalStore;

use rootsignal_archive::Archive;

const MAX_SEARCH_QUERIES_PER_RUN: usize = 15;
const MAX_SIGNALS_INVESTIGATED: usize = 8;
const MAX_QUERIES_PER_SIGNAL: usize = 3;

pub struct Investigator<'a> {
    writer: &'a GraphWriter,
    store: &'a dyn SignalStore,
    archive: Arc<Archive>,
    claude: Claude,
    region: String,
    min_lat: f64,
    max_lat: f64,
    min_lng: f64,
    max_lng: f64,
    cancelled: Arc<AtomicBool>,
}

/// Stats from an investigation run.
#[derive(Debug, Default)]
pub struct InvestigationStats {
    pub targets_found: u32,
    pub targets_investigated: u32,
    pub targets_failed: u32,
    pub evidence_created: u32,
    pub search_queries_used: u32,
    pub confidence_adjustments: u32,
}

impl std::fmt::Display for InvestigationStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Investigation: {} targets found, {} investigated, {} failed, {} evidence created, {} search queries, {} confidence adjustments",
            self.targets_found, self.targets_investigated, self.targets_failed,
            self.evidence_created, self.search_queries_used, self.confidence_adjustments,
        )
    }
}

// --- LLM structured output types ---

#[derive(Debug, Deserialize, JsonSchema)]
struct InvestigationQueries {
    queries: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct EvidenceEvaluation {
    #[serde(default, deserialize_with = "deserialize_evidence")]
    evidence: Vec<EvidenceItem>,
}

/// Handle LLM returning evidence as either a proper JSON array or a stringified JSON array.
fn deserialize_evidence<'de, D>(deserializer: D) -> std::result::Result<Vec<EvidenceItem>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Array(_) => serde_json::from_value(value).map_err(de::Error::custom),
        serde_json::Value::String(ref s) => serde_json::from_str(s).map_err(de::Error::custom),
        serde_json::Value::Null => Ok(Vec::new()),
        _ => Err(de::Error::custom(
            "evidence must be an array or JSON string",
        )),
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct EvidenceItem {
    source_url: String,
    snippet: String,
    relevance: String,
    confidence: f64,
}

// --- Prompts ---

const QUERY_GENERATION_SYSTEM: &str = "\
You are an investigator for an intelligence system. \
Generate 1-3 targeted web search queries to verify/corroborate the signal. \
Focus on: official sources, org verification (501c3, registration), independent reporting, primary documents. \
Do NOT generate vague queries or queries returning the original source.";

const QUERY_GENERATION_SENSITIVE_SUFFIX: &str = "\
\n\nThis signal involves a sensitive topic. \
Limit queries to official organizational information (registration, public programs). \
Do NOT search for enforcement actions, legal cases, or individual names.";

const EVIDENCE_EVALUATION_SYSTEM: &str = "\
Evaluate which search results provide genuine evidence about the signal described. \
DIRECT = independently confirms same fact. \
SUPPORTING = provides credibility context. \
CONTRADICTING = evidence of inaccuracy. \
Only include genuinely relevant results. Set confidence 0.0-1.0.";

const HAIKU_MODEL: &str = "claude-haiku-4-5-20251001";

impl<'a> Investigator<'a> {
    pub fn new(
        writer: &'a GraphWriter,
        store: &'a dyn SignalStore,
        archive: Arc<Archive>,
        anthropic_api_key: &str,
        region: &ScoutScope,
        cancelled: Arc<AtomicBool>,
    ) -> Self {
        let lat_delta = region.radius_km / 111.0;
        let lng_delta = region.radius_km / (111.0 * region.center_lat.to_radians().cos());
        Self {
            writer,
            store,
            archive,
            claude: Claude::new(anthropic_api_key, HAIKU_MODEL),
            region: region.name.clone(),
            min_lat: region.center_lat - lat_delta,
            max_lat: region.center_lat + lat_delta,
            min_lng: region.center_lng - lng_delta,
            max_lng: region.center_lng + lng_delta,
            cancelled,
        }
    }

    /// Run one investigation cycle. Non-fatal — individual failures are logged.
    pub async fn run(&self) -> InvestigationStats {
        let mut stats = InvestigationStats::default();

        let targets = match self
            .writer
            .find_investigation_targets(
                self.min_lat,
                self.max_lat,
                self.min_lng,
                self.max_lng,
            )
            .await
        {
            Ok(t) => t,
            Err(e) => {
                warn!(error = %e, "Failed to find investigation targets");
                return stats;
            }
        };

        stats.targets_found = targets.len() as u32;
        if targets.is_empty() {
            info!("No investigation targets found");
            return stats;
        }

        // Take up to MAX_SIGNALS_INVESTIGATED, respecting per-domain dedup
        // (the Cypher already does per-domain dedup, but cap total count here)
        let targets: Vec<_> = targets.into_iter().take(MAX_SIGNALS_INVESTIGATED).collect();
        info!(count = targets.len(), "Investigation targets selected");

        for target in &targets {
            if self.cancelled.load(Ordering::Relaxed) {
                info!("Investigation cancelled");
                break;
            }
            if stats.search_queries_used >= MAX_SEARCH_QUERIES_PER_RUN as u32 {
                info!("Search query budget exhausted, stopping investigation");
                break;
            }

            match self.investigate_signal(target, &mut stats).await {
                Ok(evidence_count) => {
                    stats.targets_investigated += 1;
                    stats.evidence_created += evidence_count;
                    info!(
                        signal_id = %target.signal_id,
                        node_type = %target.node_type,
                        title = target.title.as_str(),
                        evidence_count,
                        "Signal investigated"
                    );

                    // Revise confidence based on accumulated evidence
                    if evidence_count > 0 {
                        self.revise_confidence(target, &mut stats).await;
                    }
                }
                Err(e) => {
                    stats.targets_failed += 1;
                    warn!(
                        signal_id = %target.signal_id,
                        title = target.title.as_str(),
                        error = %e,
                        "Investigation failed for signal"
                    );
                }
            }

            // Always mark investigated (even on failure — prevents retry loops)
            if let Err(e) = self
                .writer
                .mark_investigated(target.signal_id, target.node_type)
                .await
            {
                warn!(signal_id = %target.signal_id, error = %e, "Failed to mark signal as investigated");
            }
        }

        stats
    }

    async fn investigate_signal(
        &self,
        target: &InvestigationTarget,
        stats: &mut InvestigationStats,
    ) -> Result<u32> {
        // 1. Generate search queries via LLM
        let system_prompt = if target.is_sensitive {
            format!(
                "{}{}",
                QUERY_GENERATION_SYSTEM, QUERY_GENERATION_SENSITIVE_SUFFIX
            )
        } else {
            QUERY_GENERATION_SYSTEM.to_string()
        };

        let user_prompt = format!(
            "Signal type: {}\nTitle: {}\nSummary: {}\nSource URL: {}\nCity: {}",
            target.node_type, target.title, target.summary, target.source_url, self.region,
        );

        let queries: InvestigationQueries = self
            .claude
            .extract(HAIKU_MODEL, &system_prompt, &user_prompt)
            .await?;

        let queries: Vec<_> = queries
            .queries
            .into_iter()
            .take(MAX_QUERIES_PER_SIGNAL)
            .collect();
        if queries.is_empty() {
            return Ok(0);
        }

        // 2. Execute web searches (budget-limited)
        let source_domain = extract_domain(&target.source_url);
        let mut all_results = Vec::new();

        for query in &queries {
            if stats.search_queries_used >= MAX_SEARCH_QUERIES_PER_RUN as u32 {
                break;
            }
            stats.search_queries_used += 1;

            match async {
                let handle = self.archive.source(query).await.map_err(|e| anyhow::anyhow!("{e}"))?;
                let search = handle.search(query).max_results(15).await.map_err(|e| anyhow::anyhow!("{e}"))?;
                Ok::<_, anyhow::Error>(search.results)
            }
            .await
            {
                Ok(results) => {
                    // Filter out same-domain results
                    for r in results {
                        let result_domain = extract_domain(&r.url);
                        if result_domain != source_domain {
                            all_results.push(r);
                        }
                    }
                }
                Err(e) => {
                    warn!(query, error = %e, "Investigation search failed");
                }
            }
        }

        if all_results.is_empty() {
            return Ok(0);
        }

        // 3. LLM evaluates results
        let results_text: String = all_results
            .iter()
            .enumerate()
            .map(|(i, r)| {
                format!(
                    "--- Result {} ---\nURL: {}\nTitle: {}\nSnippet: {}",
                    i + 1,
                    r.url,
                    r.title,
                    r.snippet,
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let eval_user_prompt = format!(
            "Signal: {} — {}\n\nSearch results:\n{}",
            target.title, target.summary, results_text,
        );

        let evaluation: EvidenceEvaluation = self
            .claude
            .extract(HAIKU_MODEL, EVIDENCE_EVALUATION_SYSTEM, &eval_user_prompt)
            .await?;

        // 4. Create EvidenceNodes for items with confidence >= 0.5
        let now = Utc::now();
        let mut evidence_count = 0u32;

        for item in evaluation.evidence {
            if item.confidence < 0.5 {
                continue;
            }

            let content_hash = format!("{:x}", content_hash(&item.source_url));
            let relevance = item.relevance;
            let evidence = EvidenceNode {
                id: Uuid::new_v4(),
                source_url: item.source_url.clone(),
                retrieved_at: now,
                content_hash,
                snippet: Some(item.snippet),
                relevance: Some(relevance.clone()),
                evidence_confidence: Some(item.confidence as f32),
                channel_type: Some(rootsignal_common::channel_type(&item.source_url)),
            };

            match self
                .store
                .create_evidence(&evidence, target.signal_id)
                .await
            {
                Ok(()) => {
                    evidence_count += 1;
                    info!(
                        signal_id = %target.signal_id,
                        evidence_url = item.source_url.as_str(),
                        relevance = relevance.as_str(),
                        confidence = item.confidence,
                        "Evidence created"
                    );
                }
                Err(e) => {
                    warn!(
                        signal_id = %target.signal_id,
                        evidence_url = item.source_url.as_str(),
                        error = %e,
                        "Failed to create evidence node"
                    );
                }
            }
        }

        Ok(evidence_count)
    }

    /// Revise signal confidence based on accumulated evidence.
    async fn revise_confidence(
        &self,
        target: &InvestigationTarget,
        stats: &mut InvestigationStats,
    ) {
        let evidence = match self
            .writer
            .get_evidence_summary(target.signal_id, target.node_type)
            .await
        {
            Ok(e) => e,
            Err(e) => {
                warn!(signal_id = %target.signal_id, error = %e, "Failed to get evidence summary for confidence revision");
                return;
            }
        };

        if evidence.is_empty() {
            return;
        }

        let adjustment = compute_confidence_adjustment(&evidence);
        if adjustment.abs() < f32::EPSILON {
            return;
        }

        let old_confidence = match self
            .writer
            .get_signal_confidence(target.signal_id, target.node_type)
            .await
        {
            Ok(c) => c,
            Err(e) => {
                warn!(signal_id = %target.signal_id, error = %e, "Failed to read signal confidence");
                return;
            }
        };

        let new_confidence = (old_confidence + adjustment).clamp(0.1, 1.0);
        if (new_confidence - old_confidence).abs() < f32::EPSILON {
            return;
        }

        if let Err(e) = self
            .writer
            .update_signal_confidence(target.signal_id, target.node_type, new_confidence)
            .await
        {
            warn!(signal_id = %target.signal_id, error = %e, "Failed to update signal confidence");
            return;
        }

        stats.confidence_adjustments += 1;
        info!(
            signal_id = %target.signal_id,
            old_confidence,
            new_confidence,
            adjustment,
            evidence_count = evidence.len(),
            "Signal confidence revised"
        );
    }
}

/// Compute confidence adjustment from evidence.
/// Contradiction hits harder than confirmation helps.
pub fn compute_confidence_adjustment(evidence: &[EvidenceSummary]) -> f32 {
    let mut direct_boost = 0.0f32;
    let mut supporting_boost = 0.0f32;
    let mut contradicting_penalty = 0.0f32;

    for e in evidence {
        match e.relevance.as_str() {
            "DIRECT" if e.confidence >= 0.7 => {
                direct_boost += 0.05;
            }
            "SUPPORTING" if e.confidence >= 0.5 => {
                supporting_boost += 0.02;
            }
            "CONTRADICTING" if e.confidence >= 0.7 => {
                contradicting_penalty += 0.10;
            }
            _ => {}
        }
    }

    // Cap positive contributions, no cap on contradictions
    direct_boost = direct_boost.min(0.15);
    supporting_boost = supporting_boost.min(0.06);

    direct_boost + supporting_boost - contradicting_penalty
}

/// Extract domain from a URL for same-domain filtering.
fn extract_domain(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_lowercase()))
        .unwrap_or_default()
}

use crate::infra::util;

/// FNV-1a content hash — delegates to shared implementation in `util.rs`.
fn content_hash(content: &str) -> u64 {
    util::content_hash(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(relevance: &str, confidence: f32) -> EvidenceSummary {
        EvidenceSummary {
            relevance: relevance.to_string(),
            confidence,
        }
    }

    #[test]
    fn confidence_adjustment_direct_evidence_boosts() {
        // 3 DIRECT at 0.8 → +0.05 * 3 = +0.15 (capped at 0.15)
        let evidence = vec![
            evidence("DIRECT", 0.8),
            evidence("DIRECT", 0.8),
            evidence("DIRECT", 0.8),
        ];
        let adj = compute_confidence_adjustment(&evidence);
        assert!((adj - 0.15).abs() < 0.001, "Expected +0.15, got {adj}");
    }

    #[test]
    fn confidence_adjustment_contradicting_reduces() {
        // 2 CONTRADICTING at 0.9 → -0.10 * 2 = -0.20
        let evidence = vec![
            evidence("CONTRADICTING", 0.9),
            evidence("CONTRADICTING", 0.9),
        ];
        let adj = compute_confidence_adjustment(&evidence);
        assert!((adj - (-0.20)).abs() < 0.001, "Expected -0.20, got {adj}");
    }

    #[test]
    fn confidence_adjustment_mixed_evidence() {
        // 1 DIRECT (0.8) + 1 CONTRADICTING (0.9) → +0.05 - 0.10 = -0.05
        let evidence = vec![evidence("DIRECT", 0.8), evidence("CONTRADICTING", 0.9)];
        let adj = compute_confidence_adjustment(&evidence);
        assert!((adj - (-0.05)).abs() < 0.001, "Expected -0.05, got {adj}");
    }

    #[test]
    fn confidence_adjustment_low_confidence_ignored() {
        // DIRECT at 0.3 → below 0.7 threshold, no adjustment
        let evidence = vec![evidence("DIRECT", 0.3)];
        let adj = compute_confidence_adjustment(&evidence);
        assert!(adj.abs() < 0.001, "Expected 0, got {adj}");
    }

    #[test]
    fn two_direct_evidence_barely_moves_median_signal() {
        // Document: 2 DIRECT evidence pieces at high confidence = +0.10 total
        // For a signal at median confidence (~0.75), this moves it to 0.85
        // This is a 13% relative increase — barely perceptible
        let evidence = vec![evidence("DIRECT", 0.8), evidence("DIRECT", 0.9)];
        let adj = compute_confidence_adjustment(&evidence);
        assert!((adj - 0.10).abs() < 0.001, "Expected +0.10, got {adj}");

        // Apply to a median-confidence signal
        let old = 0.75f32;
        let new = (old + adj).clamp(0.1, 1.0);
        assert!(
            (new - 0.85).abs() < 0.01,
            "Median signal barely moves: {old} → {new}"
        );
    }

    #[test]
    fn confidence_adjustment_clamped_to_bounds() {
        // 10 CONTRADICTING at 0.9 → -1.0, but clamped to [0.1, 1.0] at usage site
        let evidence: Vec<_> = (0..10).map(|_| evidence("CONTRADICTING", 0.9)).collect();
        let adj = compute_confidence_adjustment(&evidence);
        assert!(adj < -0.5, "Expected large negative, got {adj}");

        // Verify clamping works at usage site
        let old_confidence = 0.7f32;
        let new_confidence = (old_confidence + adj).clamp(0.1, 1.0);
        assert!(
            new_confidence >= 0.1,
            "Clamped confidence below 0.1: {new_confidence}"
        );
        assert!(
            new_confidence <= 1.0,
            "Clamped confidence above 1.0: {new_confidence}"
        );
    }
}
