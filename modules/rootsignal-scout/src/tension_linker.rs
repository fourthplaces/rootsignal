use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use ai_client::claude::Claude;
use ai_client::tool::{Tool, ToolDefinition};
use ai_client::traits::{Agent, PromptBuilder};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{
    CityNode, GeoPoint, GeoPrecision, Node, NodeMeta, NodeType, SensitivityLevel, Severity,
    TensionNode,
};
use rootsignal_graph::{GraphWriter, TensionLinkerOutcome, TensionLinkerTarget};

use crate::embedder::TextEmbedder;
use crate::scraper::{PageScraper, WebSearcher};

const HAIKU_MODEL: &str = "claude-haiku-4-5-20251001";
const MAX_TENSION_LINKER_TARGETS_PER_RUN: u32 = 10;
const MAX_TOOL_TURNS: usize = 8;
const MAX_TENSIONS_PER_SIGNAL: usize = 3;

// =============================================================================
// Tool Wrappers — give the LLM web_search and read_page capabilities
// =============================================================================

pub(crate) struct WebSearchTool {
    pub(crate) searcher: Arc<dyn WebSearcher>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WebSearchArgs {
    pub(crate) query: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct WebSearchOutput {
    pub(crate) results: Vec<WebSearchResultItem>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WebSearchResultItem {
    pub(crate) url: String,
    pub(crate) title: String,
    pub(crate) snippet: String,
}

#[derive(Debug)]
pub(crate) struct ToolError(pub(crate) String);

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ToolError {}

#[async_trait]
impl Tool for WebSearchTool {
    const NAME: &'static str = "web_search";
    type Error = ToolError;
    type Args = WebSearchArgs;
    type Output = WebSearchOutput;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Search the web for information. Returns URLs, titles, and snippets."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    }
                },
                "required": ["query"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> std::result::Result<Self::Output, Self::Error> {
        let results = self
            .searcher
            .search(&args.query, 5)
            .await
            .map_err(|e| ToolError(format!("Search failed: {e}")))?;

        Ok(WebSearchOutput {
            results: results
                .into_iter()
                .map(|r| WebSearchResultItem {
                    url: r.url,
                    title: r.title,
                    snippet: r.snippet,
                })
                .collect(),
        })
    }
}

pub(crate) struct ReadPageTool {
    pub(crate) scraper: Arc<dyn PageScraper>,
    /// When set, records every URL successfully read for post-hoc validation.
    pub(crate) visited_urls: Option<Arc<Mutex<HashSet<String>>>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReadPageArgs {
    pub(crate) url: String,
}

#[async_trait]
impl Tool for ReadPageTool {
    const NAME: &'static str = "read_page";
    type Error = ToolError;
    type Args = ReadPageArgs;
    type Output = String;

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description:
                "Read the full content of a web page. Returns the page as clean markdown text."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to read"
                    }
                },
                "required": ["url"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> std::result::Result<Self::Output, Self::Error> {
        let content = self
            .scraper
            .scrape(&args.url)
            .await
            .map_err(|e| ToolError(format!("Scrape failed: {e}")))?;

        // Record this URL as successfully visited
        if let Some(ref visited) = self.visited_urls {
            if let Ok(mut set) = visited.lock() {
                set.insert(args.url.clone());
            }
        }

        // Truncate to ~8k chars to fit in context
        let max_len = 8000;
        if content.len() > max_len {
            let mut end = max_len;
            while !content.is_char_boundary(end) {
                end -= 1;
            }
            Ok(format!(
                "{}...\n\n[Content truncated at {} chars]",
                &content[..end],
                max_len
            ))
        } else {
            Ok(content)
        }
    }
}

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

fn investigation_system_prompt(city: &str, tension_landscape: &str) -> String {
    format!(
        "You are investigating a signal to understand WHY it exists in {city}. \
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

Known tensions in {city}:
{tension_landscape}

If your investigation confirms an existing tension, note the match rather than treating it as \
new. Focus on finding tensions NOT yet in the landscape — that's the highest value."
    )
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
        crate::util::TENSION_CATEGORIES,
    )
}

// =============================================================================
// TensionLinker
// =============================================================================

pub struct TensionLinker<'a> {
    writer: &'a GraphWriter,
    claude: Claude,
    embedder: &'a dyn TextEmbedder,
    city: CityNode,
    min_lat: f64,
    max_lat: f64,
    min_lng: f64,
    max_lng: f64,
    cancelled: Arc<AtomicBool>,
    run_id: String,
}

impl<'a> TensionLinker<'a> {
    pub fn new(
        writer: &'a GraphWriter,
        searcher: Arc<dyn WebSearcher>,
        scraper: Arc<dyn PageScraper>,
        embedder: &'a dyn TextEmbedder,
        anthropic_api_key: &str,
        city: CityNode,
        cancelled: Arc<AtomicBool>,
        run_id: String,
    ) -> Self {
        let claude = Claude::new(anthropic_api_key, HAIKU_MODEL)
            .tool(WebSearchTool {
                searcher: searcher.clone(),
            })
            .tool(ReadPageTool {
                scraper: scraper.clone(),
                visited_urls: None,
            });

        let lat_delta = city.radius_km / 111.0;
        let lng_delta = city.radius_km / (111.0 * city.center_lat.to_radians().cos());

        Self {
            writer,
            claude,
            embedder,
            min_lat: city.center_lat - lat_delta,
            max_lat: city.center_lat + lat_delta,
            min_lng: city.center_lng - lng_delta,
            max_lng: city.center_lng + lng_delta,
            city,
            cancelled,
            run_id,
        }
    }

    pub async fn run(&self) -> TensionLinkerStats {
        let mut stats = TensionLinkerStats::default();

        let targets = match self
            .writer
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
            .writer
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

        for target in &targets {
            if self.cancelled.load(Ordering::Relaxed) {
                info!("Tension linker cancelled");
                break;
            }

            let outcome = match self.investigate_signal(target, &tension_landscape).await {
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
                            if let Err(e) = self.process_tension(target, &tension, &mut stats).await
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

            if let Err(e) = self
                .writer
                .mark_tension_linker_investigated(target.signal_id, &target.label, outcome)
                .await
            {
                warn!(
                    signal_id = %target.signal_id,
                    error = %e,
                    "Failed to mark signal as curiosity-investigated"
                );
            }
        }

        stats
    }

    async fn investigate_signal(
        &self,
        target: &TensionLinkerTarget,
        tension_landscape: &str,
    ) -> Result<SignalFinding> {
        let system = investigation_system_prompt(&self.city.name, tension_landscape);

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
            .extract(HAIKU_MODEL, &structuring_prompt, &structuring_user)
            .await?;

        Ok(finding)
    }

    async fn process_tension(
        &self,
        target: &TensionLinkerTarget,
        tension: &DiscoveredTension,
        stats: &mut TensionLinkerStats,
    ) -> Result<()> {
        let embed_text = format!("{} {}", tension.title, tension.summary);
        let embedding = self.embedder.embed(&embed_text).await?;

        // Check for duplicate tension (city-scoped)
        let existing = self
            .writer
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
                self.create_tension_node(tension).await?
            }
        };

        self.writer
            .create_response_edge(
                target.signal_id,
                tension_id,
                tension.match_strength.clamp(0.0, 1.0),
                &tension.explanation,
            )
            .await?;
        stats.edges_created += 1;

        Ok(())
    }

    async fn create_tension_node(
        &self,
        tension: &DiscoveredTension,
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
                freshness_score: 1.0,
                corroboration_count: 0,
                location: Some(GeoPoint {
                    lat: self.city.center_lat,
                    lng: self.city.center_lng,
                    precision: GeoPrecision::City,
                }),
                location_name: Some(self.city.name.clone()),
                source_url: tension.source_url.clone(),
                extracted_at: now,
                last_confirmed_active: now,
                source_diversity: 1,
                external_ratio: 1.0,
                cause_heat: 0.0,
                mentioned_actors: vec![],
                implied_queries: vec![],
            },
            severity,
            category: Some(tension.category.clone()),
            what_would_help: Some(tension.what_would_help.clone()),
        };

        let embed_text = format!("{} {}", tension.title, tension.summary);
        let embedding = self.embedder.embed(&embed_text).await?;

        let tension_id = self
            .writer
            .create_node(&Node::Tension(tension_node), &embedding, "tension_linker", &self.run_id)
            .await?;

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
    fn tension_node_gets_city_center_coordinates() {
        // Verify that a DiscoveredTension produces a TensionNode with city-center lat/lng.
        let city = CityNode {
            id: Uuid::new_v4(),
            name: "Minneapolis".to_string(),
            slug: "minneapolis".to_string(),
            center_lat: 44.9778,
            center_lng: -93.2650,
            radius_km: 30.0,
            geo_terms: vec!["Minneapolis".to_string()],
            active: true,
            created_at: Utc::now(),
            last_scout_completed_at: None,
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
                freshness_score: 1.0,
                corroboration_count: 0,
                location: Some(GeoPoint {
                    lat: city.center_lat,
                    lng: city.center_lng,
                    precision: GeoPrecision::City,
                }),
                location_name: Some(city.name.clone()),
                source_url: tension.source_url.clone(),
                extracted_at: now,
                last_confirmed_active: now,
                source_diversity: 1,
                external_ratio: 1.0,
                cause_heat: 0.0,
                mentioned_actors: vec![],
                implied_queries: vec![],
            },
            severity,
            category: Some(tension.category.clone()),
            what_would_help: Some(tension.what_would_help.clone()),
        };

        // Key assertions: location is set to city center
        let loc = tension_node
            .meta
            .location
            .expect("Tension should have location");
        assert!(
            (loc.lat - 44.9778).abs() < 0.001,
            "lat should be city center"
        );
        assert!(
            (loc.lng - (-93.2650)).abs() < 0.001,
            "lng should be city center"
        );
        assert_eq!(loc.precision, GeoPrecision::City);
        assert_eq!(
            tension_node.meta.location_name.as_deref(),
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
