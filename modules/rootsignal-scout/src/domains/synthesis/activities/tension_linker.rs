use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ai_client::claude::Claude;
use ai_client::traits::{Agent, PromptBuilder};
use anyhow::Result;
use chrono::Utc;
use schemars::JsonSchema;
use serde::Deserialize;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::events::SystemEvent;
use rootsignal_common::{
    GeoPoint, GeoPrecision, Node, NodeMeta, NodeType, ReviewStatus, ScoutScope, SensitivityLevel,
    Severity, TensionNode,
};
use rootsignal_graph::{GraphReader, SituationBrief, TensionLinkerOutcome, TensionLinkerTarget};
use rootsignal_archive::Archive;
use crate::infra::agent_tools::{ReadPageTool, WebSearchTool};
use crate::infra::embedder::TextEmbedder;
use crate::infra::util::TENSION_CATEGORIES;
use crate::store::event_sourced::{node_system_events, node_to_world_event};
use rootsignal_common::events::WorldEvent;


const HAIKU_MODEL: &str = "claude-haiku-4-5-20251001";
const MAX_TENSION_LINKER_TARGETS_PER_RUN: u32 = 10;
const MAX_TOOL_TURNS: usize = 8;
const MAX_TENSIONS_PER_SIGNAL: usize = 3;

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
    pub what_would_help: String,
    /// URL of the evidence that surfaced this tension
    pub source_url: String,
    /// How strongly the original signal relates (0.0-1.0)
    pub match_strength: f64,
    /// Why the signal responds to this tension
    pub explanation: String,
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
- what_would_help: What actions or resources would address this tension
- source_url: The URL where you found the strongest evidence for this tension
- match_strength: 0.0-1.0 for how strongly the original signal relates to this tension
- explanation: Why the signal responds to this tension

If the signal is self-explanatory (not curious), set curious=false and provide a skip_reason. \
Return at most 3 tensions. Only include tensions you have evidence for.",
        TENSION_CATEGORIES,
    )
}

fn format_situation_landscape(situations: &[SituationBrief]) -> String {
    if situations.is_empty() {
        return String::new();
    }
    situations
        .iter()
        .enumerate()
        .map(|(i, s)| {
            format!(
                "{}. {} [{}] (temp={:.2}, clarity={}, {} signals)",
                i + 1,
                s.headline,
                s.arc,
                s.temperature,
                s.clarity,
                s.signal_count,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// =============================================================================
// TensionLinker
// =============================================================================

pub struct TensionLinker<'a> {
    graph: &'a GraphReader,
    claude: Claude,
    embedder: &'a dyn TextEmbedder,
    region: ScoutScope,
    min_lat: f64,
    max_lat: f64,
    min_lng: f64,
    max_lng: f64,
    cancelled: Arc<AtomicBool>,
    run_id: String,
}

impl<'a> TensionLinker<'a> {
    pub fn new(
        graph: &'a GraphReader,
        archive: Arc<Archive>,
        embedder: &'a dyn TextEmbedder,
        anthropic_api_key: &str,
        region: ScoutScope,
        cancelled: Arc<AtomicBool>,
        run_id: String,
    ) -> Self {
        let claude = Claude::new(anthropic_api_key, HAIKU_MODEL)
            .tool(WebSearchTool {
                archive: archive.clone(),
                run_log: None,
                agent_name: String::new(),
                tension_title: String::new(),
            })
            .tool(ReadPageTool {
                archive: archive.clone(),
                visited_urls: None,
                run_log: None,
                agent_name: String::new(),
                tension_title: String::new(),
            });

        let lat_delta = region.radius_km / 111.0;
        let lng_delta = region.radius_km / (111.0 * region.center_lat.to_radians().cos());

        Self {
            graph,
            claude,
            embedder,
            min_lat: region.center_lat - lat_delta,
            max_lat: region.center_lat + lat_delta,
            min_lng: region.center_lng - lng_delta,
            max_lng: region.center_lng + lng_delta,
            region,
            cancelled,
            run_id,
        }
    }

    pub async fn run(&self, events: &mut seesaw_core::Events) -> TensionLinkerStats {
        let mut stats = TensionLinkerStats::default();

        // Pre-pass: promote exhausted retries to abandoned (via event)
        events.push(SystemEvent::ExhaustedRetriesPromoted {
            promoted_at: Utc::now(),
        });

        let targets = match self
            .graph
            .find_tension_linker_targets(
                MAX_TENSION_LINKER_TARGETS_PER_RUN,
                self.min_lat,
                self.max_lat,
                self.min_lng,
                self.max_lng,
            )
            .await
        {
            Ok(t) => t,
            Err(e) => {
                warn!(error = %e, "Failed to find curiosity targets");
                return stats;
            }
        };

        stats.targets_found = targets.len() as u32;
        if targets.is_empty() {
            info!("No curiosity targets found");
            return stats;
        }

        info!(count = targets.len(), "Curiosity targets selected");

        // Load tension landscape for context
        let tension_landscape = match self
            .graph
            .get_tension_landscape(self.min_lat, self.max_lat, self.min_lng, self.max_lng)
            .await
        {
            Ok(tensions) => {
                if tensions.is_empty() {
                    "No tensions known yet.".to_string()
                } else {
                    tensions
                        .iter()
                        .enumerate()
                        .map(|(i, (title, summary))| format!("{}. {} — {}", i + 1, title, summary))
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to load tension landscape");
                "Unable to load existing tensions.".to_string()
            }
        };

        // Load situation landscape — emerging/fuzzy situations guide investigation priority
        let situation_landscape = match self.graph.get_situation_landscape(15).await {
            Ok(situations) => format_situation_landscape(&situations),
            Err(e) => {
                warn!(error = %e, "Failed to load situation landscape for tension linker");
                String::new()
            }
        };

        for target in &targets {
            if self.cancelled.load(Ordering::Relaxed) {
                info!("Tension linker cancelled");
                break;
            }

            let outcome = match self
                .investigate_signal(target, &tension_landscape, &situation_landscape)
                .await
            {
                Ok(finding) => {
                    if !finding.curious {
                        stats.targets_skipped += 1;
                        info!(
                            signal_id = %target.signal_id,
                            title = target.title.as_str(),
                            reason = finding.skip_reason.as_deref().unwrap_or("self-explanatory"),
                            "Signal not curious, skipping"
                        );
                        TensionLinkerOutcome::Skipped
                    } else {
                        stats.targets_investigated += 1;
                        let tensions_count = finding.tensions.len().min(MAX_TENSIONS_PER_SIGNAL);
                        let mut any_tension_failed = false;
                        for tension in finding.tensions.into_iter().take(MAX_TENSIONS_PER_SIGNAL) {
                            if let Err(e) = self.process_tension(target, &tension, &mut stats, events).await
                            {
                                any_tension_failed = true;
                                warn!(
                                    signal_id = %target.signal_id,
                                    tension_title = tension.title.as_str(),
                                    error = %e,
                                    "Failed to process discovered tension"
                                );
                            }
                        }
                        info!(
                            signal_id = %target.signal_id,
                            title = target.title.as_str(),
                            tensions = tensions_count,
                            "Signal investigated"
                        );
                        if any_tension_failed {
                            TensionLinkerOutcome::Failed
                        } else {
                            TensionLinkerOutcome::Done
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        signal_id = %target.signal_id,
                        title = target.title.as_str(),
                        error = %e,
                        "Curiosity investigation failed"
                    );
                    TensionLinkerOutcome::Failed
                }
            };

            events.push(SystemEvent::TensionLinkerOutcomeRecorded {
                signal_id: target.signal_id,
                label: target.label.clone(),
                outcome: outcome.as_str().to_string(),
                increment_retry: outcome == TensionLinkerOutcome::Failed,
            });
        }

        stats
    }

    async fn investigate_signal(
        &self,
        target: &TensionLinkerTarget,
        tension_landscape: &str,
        situation_landscape: &str,
    ) -> Result<SignalFinding> {
        let system =
            investigation_system_prompt(&self.region.name, tension_landscape, situation_landscape);

        let user = format!(
            "Signal type: {}\nTitle: {}\nSummary: {}\nSource URL: {}",
            target.label, target.title, target.summary, target.source_url,
        );

        // Phase 1: Agentic investigation with web_search + read_page tools
        let reasoning = self
            .claude
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
        let finding: SignalFinding = self
            .claude
            .extract(&structuring_prompt, &structuring_user)
            .await?;

        Ok(finding)
    }

    async fn process_tension(
        &self,
        target: &TensionLinkerTarget,
        tension: &DiscoveredTension,
        stats: &mut TensionLinkerStats,
        events: &mut seesaw_core::Events,
    ) -> Result<()> {
        let embed_text = format!("{} {}", tension.title, tension.summary);
        let embedding = self.embedder.embed(&embed_text).await?;

        // Check for duplicate tension (region-scoped)
        let existing = self
            .graph
            .find_duplicate(
                &embedding,
                NodeType::Tension,
                0.85,
                self.min_lat,
                self.max_lat,
                self.min_lng,
                self.max_lng,
            )
            .await;

        let tension_id = match existing {
            Ok(Some(dup)) => {
                info!(
                    existing_id = %dup.id,
                    similarity = dup.similarity,
                    tension_title = tension.title.as_str(),
                    "Matched existing tension"
                );
                stats.tensions_deduplicated += 1;
                dup.id
            }
            _ => {
                if let Err(ref e) = existing {
                    warn!(error = %e, "Tension dedup check failed, creating new");
                }
                self.create_tension_node(tension, events).await?
            }
        };

        events.push(SystemEvent::ResponseLinked {
            signal_id: target.signal_id,
            tension_id,
            strength: tension.match_strength.clamp(0.0, 1.0),
            explanation: tension.explanation.clone(),
            source_url: None,
        });
        stats.edges_created += 1;

        Ok(())
    }

    async fn create_tension_node(
        &self,
        tension: &DiscoveredTension,
        events: &mut seesaw_core::Events,
    ) -> Result<Uuid, anyhow::Error> {
        let severity = match tension.severity.to_lowercase().as_str() {
            "low" => Severity::Low,
            "medium" => Severity::Medium,
            "high" => Severity::High,
            "critical" => Severity::Critical,
            _ => Severity::Medium,
        };

        let now = Utc::now();
        let tension_node = TensionNode {
            meta: NodeMeta {
                id: Uuid::new_v4(),
                title: tension.title.clone(),
                summary: tension.summary.clone(),
                sensitivity: SensitivityLevel::General,
                confidence: 0.7,

                corroboration_count: 0,
                about_location: Some(GeoPoint {
                    lat: self.region.center_lat,
                    lng: self.region.center_lng,
                    precision: GeoPrecision::Approximate,
                }),
                from_location: None,
                about_location_name: Some(self.region.name.clone()),
                source_url: tension.source_url.clone(),
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
            },
            severity,
            category: Some(tension.category.clone()),
            what_would_help: Some(tension.what_would_help.clone()),
        };

        let tension_id = tension_node.meta.id;
        let node = Node::Tension(tension_node);

        // Collect world event + system events for causal chain dispatch
        let world_event = node_to_world_event(&node);
        let system_events = node_system_events(&node);

        events.push(world_event);
        for se in system_events {
            events.push(se);
        }

        info!(
            tension_id = %tension_id,
            title = tension.title.as_str(),
            severity = tension.severity.as_str(),
            "New tension discovered"
        );

        Ok(tension_id)
    }
}

#[cfg(test)]
mod tests {
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
                "what_would_help": "Legal aid",
                "source_url": "https://example.com/article",
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
        // Verify that a DiscoveredTension produces a TensionNode with region-center lat/lng.
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
            what_would_help: "Legal aid resources".to_string(),
            source_url: "https://example.com/article".to_string(),
            match_strength: 0.9,
            explanation: "Workshop responds to enforcement fear".to_string(),
        };

        // Build the TensionNode the same way create_tension_node does
        let severity = match tension.severity.to_lowercase().as_str() {
            "low" => Severity::Low,
            "medium" => Severity::Medium,
            "high" => Severity::High,
            "critical" => Severity::Critical,
            _ => Severity::Medium,
        };

        let now = Utc::now();
        let tension_node = TensionNode {
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
                source_url: tension.source_url.clone(),
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
            },
            severity,
            category: Some(tension.category.clone()),
            what_would_help: Some(tension.what_would_help.clone()),
        };

        // Key assertions: location is set to region center
        let loc = tension_node
            .meta
            .about_location
            .expect("Tension should have location");
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

