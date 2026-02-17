use ai_client::claude::Claude;
use anyhow::Result;
use chrono::Utc;
use schemars::JsonSchema;
use serde::{Deserialize, de};
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::EvidenceNode;
use rootsignal_graph::{GraphWriter, InvestigationTarget};

use crate::scraper::TavilySearcher;

const MAX_TAVILY_QUERIES_PER_RUN: usize = 10;
const MAX_SIGNALS_INVESTIGATED: usize = 5;
const MAX_QUERIES_PER_SIGNAL: usize = 3;

pub struct Investigator<'a> {
    writer: &'a GraphWriter,
    tavily: &'a TavilySearcher,
    claude: Claude,
    city: String,
}

/// Stats from an investigation run.
#[derive(Debug, Default)]
pub struct InvestigationStats {
    pub targets_found: u32,
    pub targets_investigated: u32,
    pub targets_failed: u32,
    pub evidence_created: u32,
    pub tavily_queries_used: u32,
}

impl std::fmt::Display for InvestigationStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Investigation: {} targets found, {} investigated, {} failed, {} evidence created, {} Tavily queries",
            self.targets_found, self.targets_investigated, self.targets_failed,
            self.evidence_created, self.tavily_queries_used,
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
        serde_json::Value::Array(_) => {
            serde_json::from_value(value).map_err(de::Error::custom)
        }
        serde_json::Value::String(ref s) => {
            serde_json::from_str(s).map_err(de::Error::custom)
        }
        serde_json::Value::Null => Ok(Vec::new()),
        _ => Err(de::Error::custom("evidence must be an array or JSON string")),
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
You are an investigator for a civic intelligence system. \
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
        tavily: &'a TavilySearcher,
        anthropic_api_key: &str,
        city: &str,
    ) -> Self {
        Self {
            writer,
            tavily,
            claude: Claude::new(anthropic_api_key, HAIKU_MODEL),
            city: city.to_string(),
        }
    }

    /// Run one investigation cycle. Non-fatal — individual failures are logged.
    pub async fn run(&self) -> InvestigationStats {
        let mut stats = InvestigationStats::default();

        let targets = match self.writer.find_investigation_targets().await {
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
            if stats.tavily_queries_used >= MAX_TAVILY_QUERIES_PER_RUN as u32 {
                info!("Tavily query budget exhausted, stopping investigation");
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
            if let Err(e) = self.writer.mark_investigated(target.signal_id, target.node_type).await {
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
            format!("{}{}", QUERY_GENERATION_SYSTEM, QUERY_GENERATION_SENSITIVE_SUFFIX)
        } else {
            QUERY_GENERATION_SYSTEM.to_string()
        };

        let user_prompt = format!(
            "Signal type: {}\nTitle: {}\nSummary: {}\nSource URL: {}\nCity: {}",
            target.node_type, target.title, target.summary, target.source_url, self.city,
        );

        let queries: InvestigationQueries = self
            .claude
            .extract(HAIKU_MODEL, &system_prompt, &user_prompt)
            .await?;

        let queries: Vec<_> = queries.queries.into_iter().take(MAX_QUERIES_PER_SIGNAL).collect();
        if queries.is_empty() {
            return Ok(0);
        }

        // 2. Execute Tavily searches (budget-limited)
        let source_domain = extract_domain(&target.source_url);
        let mut all_results = Vec::new();

        for query in &queries {
            if stats.tavily_queries_used >= MAX_TAVILY_QUERIES_PER_RUN as u32 {
                break;
            }
            stats.tavily_queries_used += 1;

            match self.tavily.search(query, 5).await {
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
                    warn!(query, error = %e, "Investigation Tavily search failed");
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
                    i + 1, r.url, r.title, r.snippet,
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
            };

            match self.writer.create_evidence(&evidence, target.signal_id).await {
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
}

/// Extract domain from a URL for same-domain filtering.
fn extract_domain(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_lowercase()))
        .unwrap_or_default()
}

/// FNV-1a content hash (same as scout.rs).
fn content_hash(content: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in content.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
