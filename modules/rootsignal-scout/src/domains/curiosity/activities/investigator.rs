use std::sync::Arc;

use ai_client::{ai_extract, Agent};
use anyhow::Result;
use schemars::JsonSchema;
use serde::{de, Deserialize};
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::types::ChannelType;
use rootsignal_common::ScoutScope;
use rootsignal_graph::{EvidenceSummary, GraphQueries, InvestigationTarget};

use rootsignal_archive::Archive;
use crate::infra::util;

const MAX_QUERIES_PER_SIGNAL: usize = 3;

// ---------------------------------------------------------------------------
// Domain types returned by the investigator
// ---------------------------------------------------------------------------

/// Result of investigating a single signal.
pub struct InvestigationResult {
    pub evidence: Vec<InvestigationEvidence>,
    pub confidence_revision: Option<ConfidenceRevision>,
}

/// A piece of evidence discovered during investigation.
pub struct InvestigationEvidence {
    pub citation_id: Uuid,
    pub signal_id: Uuid,
    pub source_url: String,
    pub content_hash: String,
    pub snippet: Option<String>,
    pub relevance: Option<String>,
    pub channel_type: Option<ChannelType>,
    pub evidence_confidence: Option<f32>,
}

/// Confidence revision based on evidence evaluation.
pub struct ConfidenceRevision {
    pub signal_id: Uuid,
    pub old_confidence: f32,
    pub new_confidence: f32,
}

// ---------------------------------------------------------------------------
// Investigator
// ---------------------------------------------------------------------------

pub struct Investigator<'a> {
    archive: Arc<Archive>,
    ai: &'a dyn Agent,
    region: String,
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

impl<'a> Investigator<'a> {
    pub fn new(
        _graph: &'a dyn GraphQueries,
        archive: Arc<Archive>,
        ai: &'a dyn Agent,
        region: &ScoutScope,
    ) -> Self {
        Self {
            archive,
            ai,
            region: region.name.clone(),
        }
    }

    /// Investigate a single signal: search for evidence, evaluate, compute confidence revision.
    pub async fn investigate_single_signal(
        &self,
        target: &InvestigationTarget,
    ) -> InvestigationResult {
        match self.investigate_signal(target).await {
            Ok(evidence) => {
                info!(
                    signal_id = %target.signal_id,
                    node_type = %target.node_type,
                    title = target.title.as_str(),
                    evidence_count = evidence.len(),
                    "Signal investigated"
                );

                let confidence_revision = if !evidence.is_empty() {
                    let summaries: Vec<EvidenceSummary> = evidence
                        .iter()
                        .filter_map(|e| {
                            Some(EvidenceSummary {
                                relevance: e.relevance.clone()?,
                                confidence: e.evidence_confidence?,
                            })
                        })
                        .collect();
                    compute_confidence_revision(target.signal_id, &summaries)
                } else {
                    None
                };

                InvestigationResult { evidence, confidence_revision }
            }
            Err(e) => {
                warn!(
                    signal_id = %target.signal_id,
                    title = target.title.as_str(),
                    error = %e,
                    "Investigation failed for signal"
                );
                InvestigationResult { evidence: vec![], confidence_revision: None }
            }
        }
    }

    async fn investigate_signal(
        &self,
        target: &InvestigationTarget,
    ) -> Result<Vec<InvestigationEvidence>> {
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
            target.node_type, target.title, target.summary, target.url, self.region,
        );

        let queries: InvestigationQueries =
            ai_extract(self.ai, &system_prompt, &user_prompt).await?;

        let queries: Vec<_> = queries
            .queries
            .into_iter()
            .take(MAX_QUERIES_PER_SIGNAL)
            .collect();
        if queries.is_empty() {
            return Ok(vec![]);
        }

        let source_domain = extract_domain(&target.url);
        let mut all_results = Vec::new();

        for query in &queries {
            match async {
                let handle = self
                    .archive
                    .source(query)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let search = handle
                    .search(query)
                    .max_results(15)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                Ok::<_, anyhow::Error>(search.results)
            }
            .await
            {
                Ok(results) => {
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
            return Ok(vec![]);
        }

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

        let evaluation: EvidenceEvaluation = ai_extract(
            self.ai,
            EVIDENCE_EVALUATION_SYSTEM,
            &eval_user_prompt,
        )
        .await?;

        let mut evidence = Vec::new();

        for item in evaluation.evidence {
            if item.confidence < 0.5 {
                continue;
            }

            let hash = format!("{:x}", content_hash(&item.source_url));
            let relevance = item.relevance;

            info!(
                signal_id = %target.signal_id,
                evidence_url = item.source_url.as_str(),
                relevance = relevance.as_str(),
                confidence = item.confidence,
                "Evidence created"
            );

            let channel = rootsignal_common::channel_type(&item.source_url);
            evidence.push(InvestigationEvidence {
                citation_id: Uuid::new_v4(),
                signal_id: target.signal_id,
                source_url: item.source_url,
                content_hash: hash,
                snippet: Some(item.snippet),
                relevance: Some(relevance),
                channel_type: Some(channel),
                evidence_confidence: Some(item.confidence as f32),
            });
        }

        Ok(evidence)
    }
}

/// Compute confidence revision from evidence summaries.
fn compute_confidence_revision(
    signal_id: Uuid,
    evidence: &[EvidenceSummary],
) -> Option<ConfidenceRevision> {
    let adjustment = compute_confidence_adjustment(evidence);
    if adjustment.abs() < f32::EPSILON {
        return None;
    }

    let old_confidence = 0.5f32;
    let new_confidence = (old_confidence + adjustment).clamp(0.1, 1.0);
    if (new_confidence - old_confidence).abs() < f32::EPSILON {
        return None;
    }

    info!(
        signal_id = %signal_id,
        old_confidence,
        new_confidence,
        adjustment,
        evidence_count = evidence.len(),
        "Signal confidence revised"
    );

    Some(ConfidenceRevision {
        signal_id,
        old_confidence,
        new_confidence,
    })
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

    direct_boost = direct_boost.min(0.15);
    supporting_boost = supporting_boost.min(0.06);

    direct_boost + supporting_boost - contradicting_penalty
}

fn extract_domain(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_lowercase()))
        .unwrap_or_default()
}

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
        let evidence = vec![
            evidence("CONTRADICTING", 0.9),
            evidence("CONTRADICTING", 0.9),
        ];
        let adj = compute_confidence_adjustment(&evidence);
        assert!((adj - (-0.20)).abs() < 0.001, "Expected -0.20, got {adj}");
    }

    #[test]
    fn confidence_adjustment_mixed_evidence() {
        let evidence = vec![evidence("DIRECT", 0.8), evidence("CONTRADICTING", 0.9)];
        let adj = compute_confidence_adjustment(&evidence);
        assert!((adj - (-0.05)).abs() < 0.001, "Expected -0.05, got {adj}");
    }

    #[test]
    fn confidence_adjustment_low_confidence_ignored() {
        let evidence = vec![evidence("DIRECT", 0.3)];
        let adj = compute_confidence_adjustment(&evidence);
        assert!(adj.abs() < 0.001, "Expected 0, got {adj}");
    }

    #[test]
    fn two_direct_evidence_barely_moves_median_signal() {
        let evidence = vec![evidence("DIRECT", 0.8), evidence("DIRECT", 0.9)];
        let adj = compute_confidence_adjustment(&evidence);
        assert!((adj - 0.10).abs() < 0.001, "Expected +0.10, got {adj}");

        let old = 0.75f32;
        let new = (old + adj).clamp(0.1, 1.0);
        assert!(
            (new - 0.85).abs() < 0.01,
            "Median signal barely moves: {old} → {new}"
        );
    }

    #[test]
    fn confidence_adjustment_clamped_to_bounds() {
        let evidence: Vec<_> = (0..10).map(|_| evidence("CONTRADICTING", 0.9)).collect();
        let adj = compute_confidence_adjustment(&evidence);
        assert!(adj < -0.5, "Expected large negative, got {adj}");

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
