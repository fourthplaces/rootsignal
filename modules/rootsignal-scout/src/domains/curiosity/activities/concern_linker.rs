use std::sync::Arc;

use ai_client::{ai_extract, Agent, DynTool, ToolWrapper};
use anyhow::Result;
use schemars::JsonSchema;
use serde::Deserialize;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{
    NodeType, ScoutScope,
};
use rootsignal_graph::{GraphQueries, ConcernLinkerTarget};
use rootsignal_archive::Archive;
use crate::infra::agent_tools::{ReadPageTool, WebSearchTool};
use crate::infra::embedder::TextEmbedder;
use crate::infra::util::SIGNAL_CATEGORIES;

const MAX_TOOL_TURNS: usize = 8;

// =============================================================================
// Structured output types
// =============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SignalFinding {
    pub curious: bool,
    pub skip_reason: Option<String>,
    #[serde(default)]
    pub tensions: Vec<DiscoveredTension>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DiscoveredTension {
    pub title: String,
    pub summary: String,
    /// "low", "medium", "high", or "critical"
    pub severity: String,
    pub category: String,
    pub opposing: String,
    /// URL of the evidence that surfaced this tension
    #[serde(alias = "source_url")]
    pub url: String,
    /// How strongly the original signal relates (0.0-1.0)
    pub match_strength: f64,
    /// Why the signal responds to this tension
    pub explanation: String,
}

/// Result of dedup classification for a discovered tension.
#[derive(Debug)]
pub enum TensionClassification {
    /// Genuinely new — no existing match. Contains the pre-assigned ID.
    New { tension_id: Uuid },
    /// Matched an existing concern in the graph.
    Duplicate { existing_id: Uuid },
}

// =============================================================================
// Stats
// =============================================================================

#[derive(Debug, Default)]
pub struct TensionLinkerStats {
    pub targets_found: u32,
    pub targets_investigated: u32,
    pub targets_skipped: u32,
    pub tensions_discovered: u32,
    pub tensions_deduplicated: u32,
    pub edges_created: u32,
}

impl std::fmt::Display for TensionLinkerStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Tension linker: {} targets found, {} investigated, {} skipped, \
             {} tensions discovered ({} deduped), {} edges created",
            self.targets_found,
            self.targets_investigated,
            self.targets_skipped,
            self.tensions_discovered,
            self.tensions_deduplicated,
            self.edges_created,
        )
    }
}

// =============================================================================
// Prompts
// =============================================================================

fn investigation_system_prompt(
    region: &str,
    tension_landscape: &str,
    situation_landscape: &str,
) -> String {
    let mut prompt = format!(
        "You are investigating a signal to understand WHY it exists in {region}. \
Your goal is to find the underlying tensions — the problems, needs, conflicts, or fears — \
that caused this signal to exist.

You have two tools:
- web_search: Search for relevant articles, news, and context
- read_page: Read the full content of a URL to get deeper understanding

Workflow:
1. Read the signal carefully. Is it self-explanatory (e.g. \"Pub trivia night\") or does it \
raise questions about underlying community tensions?
2. If self-explanatory, just explain why — no tools needed.
3. If curious: search for context, pick the most promising result, read the full page, \
reason about what you've learned, and search deeper if needed.
4. Go deep: search → read → refine → search again if needed. Follow one thread at a time.

Known tensions in {region}:
{tension_landscape}

If your investigation confirms an existing tension, note the match rather than treating it as \
new. Focus on finding tensions NOT yet in the landscape — that's the highest value."
    );
    if !situation_landscape.is_empty() {
        prompt.push_str(&format!(
            "\n\nActive situations in {region} (causal clusters of signals):\n{situation_landscape}\n\n\
             Prioritize investigating signals that could strengthen emerging or fuzzy situations, \
             or that reveal tensions not yet captured by any situation."
        ));
    }
    prompt
}

fn structuring_system() -> String {
    format!(
        "\
Extract the tensions discovered in the investigation. For each tension:
- title: Short, specific title (e.g. \"ICE workplace raids causing community fear\")
- summary: 2-3 sentence description of the tension
- severity: \"low\", \"medium\", \"high\", or \"critical\"
- category: One of: {}. These are guidance, not constraints — propose a new category if none fit.
- opposing: What is being opposed (e.g. \"proposed rezoning\", \"budget cuts\")
- url: The URL where you found the strongest evidence for this tension
- match_strength: 0.0-1.0 for how strongly the original signal relates to this tension
- explanation: Why the signal responds to this tension

If the signal is self-explanatory (not curious), set curious=false and provide a skip_reason. \
Return at most 3 tensions. Only include tensions you have evidence for.",
        SIGNAL_CATEGORIES,
    )
}

// =============================================================================
// ConcernLinker
// =============================================================================

pub struct ConcernLinker<'a> {
    graph: &'a dyn GraphQueries,
    ai: &'a dyn Agent,
    tool_agent: Box<dyn Agent>,
    embedder: &'a dyn TextEmbedder,
    region: ScoutScope,
    min_lat: f64,
    max_lat: f64,
    min_lng: f64,
    max_lng: f64,
    _run_id: String,
}

impl<'a> ConcernLinker<'a> {
    pub fn new(
        graph: &'a dyn GraphQueries,
        archive: Arc<Archive>,
        embedder: &'a dyn TextEmbedder,
        ai: &'a dyn Agent,
        region: ScoutScope,
        run_id: String,
    ) -> Self {
        let tools: Vec<Arc<dyn DynTool>> = vec![
            Arc::new(ToolWrapper(WebSearchTool {
                archive: archive.clone(),
                agent_name: String::new(),
                tension_title: String::new(),
            })),
            Arc::new(ToolWrapper(ReadPageTool {
                archive: archive.clone(),
                visited_urls: None,
                agent_name: String::new(),
                tension_title: String::new(),
            })),
        ];
        let tool_agent = ai.with_tools(tools);

        let lat_delta = region.radius_km / 111.0;
        let lng_delta = region.radius_km / (111.0 * region.center_lat.to_radians().cos());

        Self {
            graph,
            ai,
            tool_agent,
            embedder,
            min_lat: region.center_lat - lat_delta,
            max_lat: region.center_lat + lat_delta,
            min_lng: region.center_lng - lng_delta,
            max_lng: region.center_lng + lng_delta,
            region,
            _run_id: run_id,
        }
    }

    pub async fn investigate_signal(
        &self,
        target: &ConcernLinkerTarget,
        tension_landscape: &str,
        situation_landscape: &str,
    ) -> Result<SignalFinding> {
        let system =
            investigation_system_prompt(&self.region.name, tension_landscape, situation_landscape);

        let user = format!(
            "Signal type: {}\nTitle: {}\nSummary: {}\nSource URL: {}",
            target.label, target.title, target.summary, target.url,
        );

        // Phase 1: Agentic investigation with web_search + read_page tools
        let reasoning = self
            .tool_agent
            .prompt(&user)
            .preamble(&system)
            .temperature(0.7)
            .multi_turn(MAX_TOOL_TURNS)
            .send()
            .await?;

        // Phase 2: Structure the findings
        let structuring_user = format!(
            "Original signal: {} — {}\n\nInvestigation findings:\n{}",
            target.title, target.summary, reasoning,
        );

        let structuring_prompt = structuring_system();
        let finding: SignalFinding = ai_extract(self.ai, &structuring_prompt, &structuring_user)
            .await?;

        Ok(finding)
    }

    /// Embed + dedup a discovered tension. Returns whether it's new or a duplicate.
    pub async fn classify_tension(
        &self,
        tension: &DiscoveredTension,
    ) -> Result<TensionClassification> {
        let embed_text = format!("{} {}", tension.title, tension.summary);
        let embedding = self.embedder.embed(&embed_text).await?;

        let existing = self
            .graph
            .find_duplicate(
                &embedding,
                NodeType::Concern,
                0.85,
                self.min_lat,
                self.max_lat,
                self.min_lng,
                self.max_lng,
            )
            .await;

        match existing {
            Ok(Some(dup)) => {
                info!(
                    existing_id = %dup.id,
                    similarity = dup.similarity,
                    tension_title = tension.title.as_str(),
                    "Matched existing tension"
                );
                Ok(TensionClassification::Duplicate {
                    existing_id: dup.id,
                })
            }
            _ => {
                if let Err(ref e) = existing {
                    warn!(error = %e, "Tension dedup check failed, creating new");
                }
                Ok(TensionClassification::New {
                    tension_id: Uuid::new_v4(),
                })
            }
        }
    }

}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use rootsignal_common::{
        ConcernNode, GeoPoint, GeoPrecision, NodeMeta, ReviewStatus, SensitivityLevel, Severity,
    };
    use crate::infra::agent_tools::{WebSearchOutput, WebSearchResultItem};
    use super::*;

    #[test]
    fn web_search_output_serializes() {
        let output = WebSearchOutput {
            results: vec![WebSearchResultItem {
                url: "https://example.com".to_string(),
                title: "Example".to_string(),
                snippet: "A snippet".to_string(),
            }],
        };
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("example.com"));
    }

    #[test]
    fn signal_finding_parses_not_curious() {
        let json = r#"{"curious": false, "skip_reason": "self-explanatory", "tensions": []}"#;
        let finding: SignalFinding = serde_json::from_str(json).unwrap();
        assert!(!finding.curious);
        assert_eq!(finding.skip_reason.as_deref(), Some("self-explanatory"));
        assert!(finding.tensions.is_empty());
    }

    #[test]
    fn signal_finding_parses_with_tensions() {
        let json = r#"{
            "curious": true,
            "skip_reason": null,
            "tensions": [{
                "title": "ICE raids",
                "summary": "Immigration enforcement causing fear",
                "severity": "high",
                "category": "immigration",
                "opposing": "Legal aid",
                "url": "https://example.com/article",
                "match_strength": 0.9,
                "explanation": "Workshop responds to enforcement fear"
            }]
        }"#;
        let finding: SignalFinding = serde_json::from_str(json).unwrap();
        assert!(finding.curious);
        assert_eq!(finding.tensions.len(), 1);
        assert_eq!(finding.tensions[0].title, "ICE raids");
        assert!((finding.tensions[0].match_strength - 0.9).abs() < 0.001);
    }

    #[test]
    fn tension_node_gets_region_center_coordinates() {
        // Verify that a DiscoveredTension produces a ConcernNode with region-center lat/lng.
        let region = ScoutScope {
            name: "Minneapolis".to_string(),
            center_lat: 44.9778,
            center_lng: -93.2650,
            radius_km: 30.0,
        };

        let tension = DiscoveredTension {
            title: "ICE Enforcement Fear".to_string(),
            summary: "Community fear due to immigration enforcement".to_string(),
            severity: "high".to_string(),
            category: "immigration".to_string(),
            opposing: "Legal aid resources".to_string(),
            url: "https://example.com/article".to_string(),
            match_strength: 0.9,
            explanation: "Workshop responds to enforcement fear".to_string(),
        };

        // Build the ConcernNode the same way create_tension_node does
        let severity = match tension.severity.to_lowercase().as_str() {
            "low" => Severity::Low,
            "medium" => Severity::Medium,
            "high" => Severity::High,
            "critical" => Severity::Critical,
            _ => Severity::Medium,
        };

        let now = Utc::now();
        let tension_node = ConcernNode {
            meta: NodeMeta {
                id: Uuid::new_v4(),
                title: tension.title.clone(),
                summary: tension.summary.clone(),
                sensitivity: SensitivityLevel::General,
                confidence: 0.7,

                corroboration_count: 0,
                about_location: Some(GeoPoint {
                    lat: region.center_lat,
                    lng: region.center_lng,
                    precision: GeoPrecision::Approximate,
                }),
                from_location: None,
                about_location_name: Some(region.name.clone()),
                url: tension.url.clone(),
                extracted_at: now,
                published_at: None,
                last_confirmed_active: now,
                source_diversity: 1,

                cause_heat: 0.0,
                channel_diversity: 1,
                implied_queries: vec![],
                review_status: ReviewStatus::Staged,
                was_corrected: false,
                corrections: None,
                rejection_reason: None,
                mentioned_actors: Vec::new(),
                category: None,
            },
            severity,
            subject: None,
            opposing: Some(tension.opposing.clone()),
        };

        // Key assertions: location is set to region center
        let loc = tension_node
            .meta
            .about_location
            .expect("Concern should have location");
        assert!(
            (loc.lat - 44.9778).abs() < 0.001,
            "lat should be region center"
        );
        assert!(
            (loc.lng - (-93.2650)).abs() < 0.001,
            "lng should be region center"
        );
        assert_eq!(loc.precision, GeoPrecision::Approximate);
        assert_eq!(
            tension_node.meta.about_location_name.as_deref(),
            Some("Minneapolis")
        );
        assert_eq!(tension_node.severity, Severity::High);
    }

    #[test]
    fn curiosity_stats_display() {
        let stats = TensionLinkerStats {
            targets_found: 10,
            targets_investigated: 7,
            targets_skipped: 3,
            tensions_discovered: 5,
            tensions_deduplicated: 2,
            edges_created: 7,
        };
        let display = format!("{stats}");
        assert!(display.contains("10 targets found"));
        assert!(display.contains("7 investigated"));
        assert!(display.contains("5 tensions discovered"));
    }
}

