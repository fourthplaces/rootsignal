use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use ai_client::{ai_extract, Agent, DynTool, ToolWrapper};
use anyhow::Result;
use chrono::Utc;
use schemars::JsonSchema;
use serde::Deserialize;
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::events::{SystemEvent, WorldEvent};
use rootsignal_common::{
    ResourceOfferNode, GatheringNode, HelpRequestNode,
    Node, NodeType, ScoutScope, Severity, SourceNode,
    ConcernNode, Urgency,
};
use rootsignal_graph::{GraphQueries, ResponseFinderTarget, ResponseHeuristic, SituationBrief};
use rootsignal_archive::Archive;
use crate::domains::curiosity::util::{
    self, build_future_query_source, build_node_meta, region_bounds, HAIKU_MODEL, MAX_TOOL_TURNS,
    MAX_FUTURE_QUERIES_PER_TENSION,
};
use crate::infra::agent_tools::{ReadPageTool, WebSearchTool};
use crate::infra::embedder::TextEmbedder;
use crate::store::event_sourced::{node_system_events, node_to_world_event};
use crate::core::extractor::{deserialize_resource_tags, ResourceRole, ResourceTag};

const MAX_RESPONSE_TARGETS_PER_RUN: usize = 5;
const MAX_RESPONSES_PER_TENSION: usize = 8;

/// Result of dedup classification for a discovered response.
#[derive(Debug)]
pub enum ResponseClassification {
    /// Genuinely new — no existing match. Contains the pre-assigned ID.
    New { signal_id: Uuid },
    /// Matched an existing signal in the graph.
    Duplicate { existing_id: Uuid },
}

// =============================================================================
// Structured output types
// =============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ResponseFinding {
    #[serde(default)]
    pub responses: Vec<DiscoveredResponse>,
    #[serde(default)]
    pub emergent_tensions: Vec<EmergentTension>,
    #[serde(default)]
    pub future_queries: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DiscoveredResponse {
    pub title: String,
    pub summary: String,
    /// "resource", "gathering", or "help_request"
    pub signal_type: String,
    /// Must be a URL the agent actually read via read_page
    pub url: String,
    /// Freeform — the LLM can invent new categories
    pub diffusion_mechanism: String,
    /// How this diffuses rather than escalates
    pub explanation: String,
    /// 0.0-1.0 how directly this addresses the tension
    pub match_strength: f64,
    /// Titles of OTHER tensions this also diffuses
    #[serde(default)]
    pub also_addresses: Vec<String>,
    /// ISO date for events (null if not an event or date unknown)
    pub event_date: Option<String>,
    /// True if this is an ongoing program or recurring event
    #[serde(default)]
    pub is_recurring: bool,
    /// Resource capabilities this response requires, prefers, or offers.
    #[serde(default, deserialize_with = "deserialize_resource_tags")]
    pub resources: Vec<ResourceTag>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EmergentTension {
    pub title: String,
    pub summary: String,
    /// "low", "medium", "high", or "critical"
    pub severity: String,
    pub category: String,
    pub opposing: String,
    pub source_url: String,
    /// How this relates to the tension being investigated
    pub relationship: String,
}

// =============================================================================
// Stats
// =============================================================================

#[derive(Debug, Default)]
pub struct ResponseFinderStats {
    pub targets_found: u32,
    pub targets_investigated: u32,
    pub responses_discovered: u32,
    pub responses_deduped: u32,
    pub signals_created: u32,
    pub edges_created: u32,
    pub emergent_tensions: u32,
    pub future_sources_created: u32,
}

impl std::fmt::Display for ResponseFinderStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Response finder: {} targets found, {} investigated, \
             {} responses discovered ({} deduped), {} signals created, \
             {} edges, {} emergent tensions, {} future sources",
            self.targets_found,
            self.targets_investigated,
            self.responses_discovered,
            self.responses_deduped,
            self.signals_created,
            self.edges_created,
            self.emergent_tensions,
            self.future_sources_created,
        )
    }
}

// =============================================================================
// Prompts
// =============================================================================

fn investigation_system_prompt(city_name: &str) -> String {
    format!(
        "You are investigating what DIFFUSES a community tension in {city_name}.
Find real-world responses — organizations, programs, campaigns, events,
mutual aid efforts, creative actions — that address this problem.

You have two tools: web_search and read_page.

HOW TO INVESTIGATE:
1. Start broad: \"what is being done about [tension] in [region]?\"
2. Read the most promising results — understand the landscape
3. Think about MECHANISMS: what feeds this tension? What starves it?
4. Follow threads creatively — an article about ICE funding might lead \
you to boycott campaigns. A food drive might lead you to mutual aid networks.
5. Search across platforms: donation drives, crowdfunding (GoFundMe, GiveSendGo), \
solidarity funds, Venmo/CashApp mutual aid, fiscal sponsors, org donation pages, \
Reddit, Eventbrite, church networks, legal clinics, government programs
6. Go deep on the most promising threads (2-3 hops)

WHAT DIFFUSES TENSION (examples, not exhaustive):
- Non-compliance: removes the system's power
- Economic pressure: removes funding/oxygen
- Sanctuary: creates zones the tension can't reach
- Mutual aid: makes communities resilient enough to weather the tension
- Legal leverage: uses the system's own rules against it
- Information: dissolves fear through knowledge
- Creative action: art, protest, culture that transforms the narrative
- YOU MAY DISCOVER MECHANISMS NOT ON THIS LIST — report them

DO NOT AMPLIFY responses that ESCALATE:
- Retaliation (force against force creates new tension)
- Counter-violence (adds heat instead of removing it)
- Divisive framing (fractures the community it claims to help)

EMERGENT DISCOVERIES: If your investigation reveals:
- A NEW tension nobody anticipated — report it
- A response that addresses MULTIPLE tensions — note all of them
- Unexpected connections between issues — describe them
These are valuable. Don't constrain yourself to the original question.

IMPORTANT CONSTRAINTS:
- The URL for each response MUST be a page you actually read via read_page. \
Do NOT guess or reconstruct URLs — only report URLs you visited.
- For EVENTS: verify the date. Only extract events happening NOW or in the FUTURE. \
Include the event date when known. Past events are not useful."
    )
}

fn investigation_user_prompt(
    target: &ResponseFinderTarget,
    existing: &[ResponseHeuristic],
    situation_context: &str,
) -> String {
    let mut prompt = format!(
        "TENSION: {}\nSeverity: {}\nSummary: {}",
        target.title, target.severity, target.summary,
    );

    if let Some(ref wwh) = target.opposing {
        prompt.push_str(&format!("\nWhat would help: {wwh}"));
    }
    if let Some(ref cat) = target.category {
        prompt.push_str(&format!("\nCategory: {cat}"));
    }

    if !existing.is_empty() {
        prompt.push_str("\n\nEXISTING RESPONSES (hints about what categories exist):");
        for r in existing {
            prompt.push_str(&format!(
                "\n- [{}] {}: {}",
                r.signal_type, r.title, r.summary,
            ));
        }
        prompt.push_str(
            "\n\nThese hint at response categories. Search broadly for MORE — especially \
             types of responses not yet represented.",
        );
    }

    if !situation_context.is_empty() {
        prompt.push_str(&format!(
            "\n\nSITUATION CONTEXT (causal clusters this tension may be part of):\n{situation_context}\n\n\
             Prioritize finding responses that address the root causes identified in these situations, \
             especially where response gaps exist (low dispatch counts)."
        ));
    }

    prompt
}

pub fn format_situation_context(situations: &[SituationBrief]) -> String {
    if situations.is_empty() {
        return String::new();
    }
    situations
        .iter()
        .filter(|s| s.temperature >= 0.2) // Only include warm+ situations
        .map(|s| {
            let gap_note = if s.dispatch_count == 0 {
                " [NO RESPONSES YET]"
            } else if s.dispatch_count < s.signal_count / 3 {
                " [RESPONSE GAP]"
            } else {
                ""
            };
            format!(
                "- {} [{}] (temp={:.2}, {} signals, {} dispatches){gap_note}",
                s.headline, s.arc, s.temperature, s.signal_count, s.dispatch_count,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

const STRUCTURING_SYSTEM: &str = "\
Based on your investigation, extract your findings as JSON.

For each response you discovered:
- title: short name of the response (org, program, campaign, event)
- summary: 1-2 sentences about what it does
- signal_type: \"aid\" (free resources/services for people in need — NOT commercial offerings), \"gathering\" (people coming together — town halls, cleanups, vigils, solidarity actions), or \"need\" (someone expressing their need with a way to respond — NOT news coverage of problems)
- url: the EXACT URL you read via read_page (do not reconstruct or guess)
- diffusion_mechanism: how this response takes the air out of the tension (freeform — invent a category if needed)
- explanation: why this diffuses rather than escalates
- match_strength: 0.0-1.0 how directly this addresses the tension
- also_addresses: titles of OTHER community tensions this also diffuses (empty if none)
- event_date: ISO date if this is an event (null if not an event or date unknown)
- is_recurring: true if this is an ongoing program or recurring event

For each response, also extract resource capabilities:
- resources: array of {slug, role, confidence, context}
  - role: \"requires\" (Need/Event need this), \"prefers\" (nice to have), or \"offers\" (Give provides this)
  - Use these slugs when they fit: vehicle, bilingual-spanish, bilingual-somali, bilingual-hmong, \
legal-expertise, food, shelter-space, clothing, childcare, medical-professional, mental-health, \
physical-labor, kitchen-space, event-space, storage-space, technology, reliable-internet, \
financial-donation, skilled-trade, administrative
  - Otherwise propose a concise noun-phrase slug
  - Only include when the capability is clear from the content

Also report:
- emergent_tensions: NEW tensions you discovered during investigation (not the original one)
- future_queries: search queries that could find MORE responses (threads you couldn't fully explore)

Return valid JSON matching the ResponseFinding schema.";

// =============================================================================
// Per-target stats (returned by process_single_target)
// =============================================================================

#[derive(Debug, Default)]
pub struct ResponseFinderTargetStats {
    pub inner: ResponseFinderStats,
}

// =============================================================================
// ResponseFinder
// =============================================================================

pub struct ResponseFinder<'a> {
    graph: &'a dyn GraphQueries,
    ai: &'a dyn Agent,
    archive: Arc<Archive>,
    embedder: &'a dyn TextEmbedder,
    region: ScoutScope,
    _region_slug: String,
    min_lat: f64,
    max_lat: f64,
    min_lng: f64,
    max_lng: f64,
    run_id: String,
}

impl<'a> ResponseFinder<'a> {
    pub fn new(
        graph: &'a dyn GraphQueries,
        archive: Arc<Archive>,
        embedder: &'a dyn TextEmbedder,
        ai: &'a dyn Agent,
        region: ScoutScope,
        run_id: String,
    ) -> Self {
        let (min_lat, max_lat, min_lng, max_lng) = region_bounds(&region);
        let region_slug = region.name.clone();
        Self {
            graph,
            ai,
            archive,
            embedder,
            min_lat,
            max_lat,
            min_lng,
            max_lng,
            region,
            _region_slug: region_slug,
            run_id,
        }
    }

    /// Build a tool-armed agent with URL tracking for a single investigation.
    fn build_tracked_agent(&self) -> (Box<dyn Agent>, Arc<Mutex<HashSet<String>>>) {
        let visited = Arc::new(Mutex::new(HashSet::new()));
        let tools: Vec<Arc<dyn DynTool>> = vec![
            Arc::new(ToolWrapper(WebSearchTool {
                archive: self.archive.clone(),
                agent_name: String::new(),
                tension_title: String::new(),
            })),
            Arc::new(ToolWrapper(ReadPageTool {
                archive: self.archive.clone(),
                visited_urls: Some(visited.clone()),
                agent_name: String::new(),
                tension_title: String::new(),
            })),
        ];
        (self.ai.with_tools(tools), visited)
    }

    /// LLM investigation + structuring + URL validation.
    /// Returns validated findings without creating any nodes or events.
    pub async fn investigate_target(
        &self,
        target: &ResponseFinderTarget,
        situation_context: &str,
    ) -> Result<ResponseFinding> {
        let existing = self
            .graph
            .get_existing_responses(target.concern_id)
            .await
            .unwrap_or_default();

        let system = investigation_system_prompt(&self.region.name);
        let user = investigation_user_prompt(target, &existing, situation_context);

        let (agent, visited_urls) = self.build_tracked_agent();

        let reasoning = agent
            .prompt(&user)
            .preamble(&system)
            .temperature(0.7)
            .multi_turn(MAX_TOOL_TURNS)
            .send()
            .await?;

        let structuring_user = format!(
            "Tension investigated: {} — {}\n\nInvestigation findings:\n{}",
            target.title, target.summary, reasoning,
        );

        let finding: ResponseFinding = ai_extract(self.ai, STRUCTURING_SYSTEM, &structuring_user)
            .await?;

        let visited: HashSet<String> = {
            let guard = visited_urls.lock().unwrap_or_else(|e| e.into_inner());
            guard.clone()
        };
        let validated_responses: Vec<_> = finding
            .responses
            .into_iter()
            .filter(|r| {
                if visited.contains(&r.url) {
                    true
                } else {
                    warn!(
                        url = r.url.as_str(),
                        title = r.title.as_str(),
                        "Dropping response with unvisited URL (possible hallucination)"
                    );
                    false
                }
            })
            .collect();

        Ok(ResponseFinding {
            responses: validated_responses,
            emergent_tensions: finding.emergent_tensions,
            future_queries: finding.future_queries,
        })
    }

    /// Embed + dedup a discovered response. Returns whether it's new or a duplicate.
    pub async fn classify_response(
        &self,
        response: &DiscoveredResponse,
    ) -> Result<ResponseClassification> {
        let node_type = match response.signal_type.to_lowercase().as_str() {
            "resource" => NodeType::Resource,
            "gathering" => NodeType::Gathering,
            "help_request" => NodeType::HelpRequest,
            other => {
                anyhow::bail!("Unknown signal type: {}", other);
            }
        };

        let embed_text = format!("{} {}", response.title, response.summary);
        let embedding = self.embedder.embed(&embed_text).await?;

        let existing = self
            .graph
            .find_duplicate(
                &embedding,
                node_type,
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
                    title = response.title.as_str(),
                    "Matched existing signal for response"
                );
                Ok(ResponseClassification::Duplicate {
                    existing_id: dup.id,
                })
            }
            _ => {
                if let Err(ref e) = existing {
                    warn!(error = %e, "Response dedup check failed, creating new");
                }
                Ok(ResponseClassification::New {
                    signal_id: Uuid::new_v4(),
                })
            }
        }
    }

    /// Embed + dedup an emergent tension. Returns Some(tension_id) if new, None if duplicate.
    pub async fn classify_emergent_tension(
        &self,
        tension: &EmergentTension,
    ) -> Result<Option<Uuid>> {
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
                    title = tension.title.as_str(),
                    "Emergent tension matched existing"
                );
                Ok(None)
            }
            _ => {
                if let Err(ref e) = existing {
                    warn!(error = %e, "Emergent tension dedup check failed, creating new");
                }
                Ok(Some(Uuid::new_v4()))
            }
        }
    }

    /// Resolve raw tension titles to concern_ids via embedding similarity.
    pub async fn resolve_also_addresses(
        &self,
        also_addresses: &[String],
    ) -> Vec<(Uuid, f64)> {
        let mut edges = Vec::new();
        for tension_title in also_addresses {
            match util::find_best_tension_match(
                self.embedder, self.graph, &self.region, tension_title, 0.85,
            )
            .await
            {
                Ok(Some((concern_id, sim))) => {
                    edges.push((concern_id, sim));
                }
                Ok(None) => {}
                Err(e) => {
                    warn!(
                        tension_title = tension_title.as_str(),
                        error = %e,
                        "Failed to resolve also_addresses tension"
                    );
                }
            }
        }
        edges
    }

    pub async fn run(&self, events: &mut seesaw_core::Events) -> (ResponseFinderStats, Vec<SourceNode>) {
        let mut stats = ResponseFinderStats::default();
        let mut discovered_sources = Vec::new();

        let targets = match self
            .graph
            .find_response_finder_targets(
                MAX_RESPONSE_TARGETS_PER_RUN as u32,
                self.min_lat,
                self.max_lat,
                self.min_lng,
                self.max_lng,
            )
            .await
        {
            Ok(t) => t,
            Err(e) => {
                warn!(error = %e, "Failed to find response finder targets");
                return (stats, discovered_sources);
            }
        };

        stats.targets_found = targets.len() as u32;
        if targets.is_empty() {
            info!("No response finder targets found");
            return (stats, discovered_sources);
        }

        info!(count = targets.len(), "Response scout targets selected");

        // Load situation landscape — unmet response gaps from situations guide investigation
        let situation_context = match self.graph.get_situation_landscape(15).await {
            Ok(situations) => format_situation_context(&situations),
            Err(e) => {
                warn!(error = %e, "Failed to load situation landscape for response finder");
                String::new()
            }
        };

        for target in &targets {
            let (target_events, target_sources, _target_stats) = self
                .process_single_target(target, &situation_context)
                .await;

            stats.targets_investigated += 1;
            events.extend(target_events);
            discovered_sources.extend(target_sources);
        }

        (stats, discovered_sources)
    }

    /// Process a single target — returns events, discovered sources, and per-target stats.
    /// Used by both the monolithic `run()` and the per-target handler.
    pub async fn process_single_target(
        &self,
        target: &ResponseFinderTarget,
        situation_context: &str,
    ) -> (seesaw_core::Events, Vec<SourceNode>, ResponseFinderTargetStats) {
        let mut events = seesaw_core::Events::new();
        let mut discovered_sources = Vec::new();
        let mut target_stats = ResponseFinderTargetStats::default();

        match self
            .investigate_tension(
                target,
                situation_context,
                &mut target_stats.inner,
                &mut discovered_sources,
                &mut events,
            )
            .await
        {
            Ok(()) => {
                target_stats.inner.targets_investigated += 1;
            }
            Err(e) => {
                warn!(
                    concern_id = %target.concern_id,
                    title = target.title.as_str(),
                    error = %e,
                    "Response scout investigation failed"
                );
            }
        }

        // Emit event — projector sets response_scouted_at on the Tension node
        events.push(SystemEvent::ResponseScouted {
            concern_id: target.concern_id,
            scouted_at: Utc::now(),
        });

        (events, discovered_sources, target_stats)
    }

    async fn investigate_tension(
        &self,
        target: &ResponseFinderTarget,
        situation_context: &str,
        stats: &mut ResponseFinderStats,
        discovered_sources: &mut Vec<SourceNode>,
        events: &mut seesaw_core::Events,
    ) -> Result<()> {
        // Fetch existing response heuristics
        let existing = self
            .graph
            .get_existing_responses(target.concern_id)
            .await
            .unwrap_or_default();

        let system = investigation_system_prompt(&self.region.name);
        let user = investigation_user_prompt(target, &existing, situation_context);

        // Build a tracked agent for this investigation
        let (agent, visited_urls) = self.build_tracked_agent();

        // Phase 1: Agentic investigation with web_search + read_page tools
        let reasoning = agent
            .prompt(&user)
            .preamble(&system)
            .temperature(0.7)
            .multi_turn(MAX_TOOL_TURNS)
            .send()
            .await?;

        // Phase 2: Structure the findings
        let structuring_user = format!(
            "Tension investigated: {} — {}\n\nInvestigation findings:\n{}",
            target.title, target.summary, reasoning,
        );

        let finding: ResponseFinding = ai_extract(self.ai, STRUCTURING_SYSTEM, &structuring_user)
            .await?;

        // Validate URLs: only keep responses whose URLs were actually visited
        // Clone the set and drop the MutexGuard before the async boundary so the
        // future remains Send (required by tokio::spawn).
        let visited: HashSet<String> = {
            let guard = visited_urls.lock().unwrap_or_else(|e| e.into_inner());
            guard.clone()
        };
        let validated_responses: Vec<_> = finding
            .responses
            .into_iter()
            .filter(|r| {
                if visited.contains(&r.url) {
                    true
                } else {
                    warn!(
                        url = r.url.as_str(),
                        title = r.title.as_str(),
                        "Dropping response with unvisited URL (possible hallucination)"
                    );
                    false
                }
            })
            .collect();

        stats.responses_discovered += validated_responses.len() as u32;

        // Process discovered responses
        for response in validated_responses
            .into_iter()
            .take(MAX_RESPONSES_PER_TENSION)
        {
            if let Err(e) = self.process_response(target, &response, stats, events).await {
                warn!(
                    concern_id = %target.concern_id,
                    response_title = response.title.as_str(),
                    error = %e,
                    "Failed to process discovered response"
                );
            }
        }

        // Process emergent tensions
        for tension in &finding.emergent_tensions {
            if let Err(e) = self.process_emergent_tension(tension, stats, events).await {
                warn!(
                    tension_title = tension.title.as_str(),
                    error = %e,
                    "Failed to process emergent tension"
                );
            }
        }

        // Create future query sources
        for query in finding
            .future_queries
            .iter()
            .take(MAX_FUTURE_QUERIES_PER_TENSION)
        {
            discovered_sources.push(build_future_query_source(query, &target.title, "Response finder"));
            stats.future_sources_created += 1;
        }

        info!(
            concern_id = %target.concern_id,
            title = target.title.as_str(),
            responses = stats.responses_discovered,
            "Tension response investigation complete"
        );

        Ok(())
    }

    async fn process_response(
        &self,
        target: &ResponseFinderTarget,
        response: &DiscoveredResponse,
        stats: &mut ResponseFinderStats,
        events: &mut seesaw_core::Events,
    ) -> Result<()> {
        let embed_text = format!("{} {}", response.title, response.summary);
        let embedding = self.embedder.embed(&embed_text).await?;

        let node_type = match response.signal_type.to_lowercase().as_str() {
            "resource" => NodeType::Resource,
            "gathering" => NodeType::Gathering,
            "help_request" => NodeType::HelpRequest,
            other => {
                warn!(
                    signal_type = other,
                    title = response.title.as_str(),
                    "Unknown signal type from LLM, skipping response"
                );
                return Ok(());
            }
        };

        // Check for duplicate (region-scoped)
        let existing = self
            .graph
            .find_duplicate(
                &embedding,
                node_type,
                0.85,
                self.min_lat,
                self.max_lat,
                self.min_lng,
                self.max_lng,
            )
            .await;

        let was_new;
        let signal_id = match existing {
            Ok(Some(dup)) => {
                info!(
                    existing_id = %dup.id,
                    similarity = dup.similarity,
                    title = response.title.as_str(),
                    "Matched existing signal for response"
                );
                stats.responses_deduped += 1;
                was_new = false;
                dup.id
            }
            _ => {
                if let Err(ref e) = existing {
                    warn!(error = %e, "Response dedup check failed, creating new");
                }
                was_new = true;
                self.create_response_node(response, events).await?
            }
        };

        // Wire RESPONDS_TO edge to the target tension
        events.push(SystemEvent::ResponseLinked {
            signal_id,
            concern_id: target.concern_id,
            strength: response.match_strength.clamp(0.0, 1.0),
            explanation: response.explanation.clone(),
            source_url: None,
        });
        stats.edges_created += 1;

        // Wire additional edges for also_addresses
        if !response.also_addresses.is_empty() {
            if let Err(e) = self
                .wire_also_addresses(signal_id, &response.also_addresses, &response.explanation, events)
                .await
            {
                warn!(error = %e, "Failed to wire also_addresses (non-fatal)");
            }
        }

        // Wire resource edges
        if !response.resources.is_empty() {
            if let Err(e) = self
                .wire_resources(signal_id, &response.signal_type, &response.resources, events)
                .await
            {
                warn!(error = %e, "Failed to wire resource edges (non-fatal)");
            }
        }

        if was_new {
            stats.signals_created += 1;
        }

        Ok(())
    }

    /// Create Resource nodes and edges for a signal's resource tags.
    async fn wire_resources(
        &self,
        signal_id: Uuid,
        _signal_type: &str,
        resources: &[ResourceTag],
        events: &mut seesaw_core::Events,
    ) -> Result<()> {
        for tag in resources.iter().filter(|t| t.confidence >= 0.3) {
            let slug = rootsignal_common::slugify(&tag.slug);
            let description = tag.context.as_deref().unwrap_or("");

            events.push(WorldEvent::ResourceIdentified {
                resource_id: Uuid::new_v4(),
                name: tag.slug.clone(),
                slug: slug.clone(),
                description: description.to_string(),
            });

            let confidence = tag.confidence.clamp(0.0, 1.0) as f32;
            let (quantity, capacity) = match tag.role {
                ResourceRole::Requires => (tag.context.clone(), None),
                ResourceRole::Prefers => (None, None),
                ResourceRole::Offers => (None, tag.context.clone()),
            };

            events.push(WorldEvent::ResourceLinked {
                signal_id,
                resource_slug: slug.clone(),
                role: tag.role.to_string(),
                confidence,
                quantity,
                notes: None,
                capacity,
            });

            info!(
                signal_id = %signal_id,
                slug = slug.as_str(),
                role = tag.role.as_str(),
                "Wired resource edge"
            );
        }
        Ok(())
    }

    async fn create_response_node(&self, response: &DiscoveredResponse, events: &mut seesaw_core::Events) -> Result<Uuid> {
        let meta = build_node_meta(
            response.title.clone(),
            response.summary.clone(),
            response.url.clone(),
            &self.region,
            0.7,
        );

        let node = match response.signal_type.to_lowercase().as_str() {
            "gathering" => {
                let starts_at = response.event_date.as_deref().and_then(|d| {
                    chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                        .ok()
                        .map(|nd| nd.and_hms_opt(0, 0, 0).unwrap().and_utc())
                });
                Node::Gathering(GatheringNode {
                    meta,
                    starts_at,
                    ends_at: None,
                    action_url: response.url.clone(),
                    organizer: None,
                    is_recurring: response.is_recurring,
                })
            }
            "help_request" => Node::HelpRequest(HelpRequestNode {
                meta,
                urgency: Urgency::Medium,
                what_needed: Some(response.summary.clone()),
                action_url: Some(response.url.clone()),
                stated_goal: None,
            }),
            _ => Node::Resource(ResourceOfferNode {
                meta,
                action_url: response.url.clone(),
                availability: None,
                eligibility: None,
                is_ongoing: response.is_recurring,
            }),
        };

        let node_id = node.meta().unwrap().id;

        // Push world event + system events into caller's event collection
        let world_event = node_to_world_event(&node);
        let system_events = node_system_events(&node);

        events.push(world_event);
        for se in system_events {
            events.push(se);
        }

        info!(
            node_id = %node_id,
            title = response.title.as_str(),
            signal_type = response.signal_type.as_str(),
            mechanism = response.diffusion_mechanism.as_str(),
            "New response signal created"
        );

        Ok(node_id)
    }

    /// Wire RESPONDS_TO edges to additional tensions that this response also addresses.
    /// Uses embedding similarity against all active tensions (>0.85 threshold).
    async fn wire_also_addresses(
        &self,
        signal_id: Uuid,
        also_addresses: &[String],
        explanation: &str,
        events: &mut seesaw_core::Events,
    ) -> Result<()> {
        for tension_title in also_addresses {
            if let Some((concern_id, sim)) = util::find_best_tension_match(
                self.embedder, self.graph, &self.region, tension_title, 0.85,
            ).await? {
                info!(
                    signal_id = %signal_id,
                    concern_id = %concern_id,
                    similarity = sim,
                    also_addresses = tension_title.as_str(),
                    "Wiring also_addresses edge"
                );
                events.push(SystemEvent::ResponseLinked {
                    signal_id,
                    concern_id,
                    strength: sim.clamp(0.0, 1.0),
                    explanation: explanation.to_string(),
                    source_url: None,
                });
            }
        }

        Ok(())
    }

    async fn process_emergent_tension(
        &self,
        tension: &EmergentTension,
        stats: &mut ResponseFinderStats,
        events: &mut seesaw_core::Events,
    ) -> Result<()> {
        let embed_text = format!("{} {}", tension.title, tension.summary);
        let embedding = self.embedder.embed(&embed_text).await?;

        // Dedup check (region-scoped)
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
                    title = tension.title.as_str(),
                    "Emergent tension matched existing"
                );
                // Don't create duplicate, but still count it
                return Ok(());
            }
            Ok(None) => {}
            Err(e) => {
                warn!(error = %e, "Emergent tension dedup check failed, creating new");
            }
        }

        let severity = match tension.severity.to_lowercase().as_str() {
            "low" => Severity::Low,
            "medium" => Severity::Medium,
            "high" => Severity::High,
            "critical" => Severity::Critical,
            _ => Severity::Medium,
        };

        let tension_node = ConcernNode {
            meta: build_node_meta(
                tension.title.clone(),
                tension.summary.clone(),
                tension.source_url.clone(),
                &self.region,
                0.4, // Capped at 0.4 — below 0.5 target selection threshold
            ),
            severity,
            subject: None,
            opposing: Some(tension.opposing.clone()),
        };

        let node = Node::Concern(tension_node);
        let concern_id = node.meta().unwrap().id;

        // Push world event + system events into caller's event collection
        let world_event = node_to_world_event(&node);
        let system_events = node_system_events(&node);

        events.push(world_event);
        for se in system_events {
            events.push(se);
        }

        info!(
            concern_id = %concern_id,
            title = tension.title.as_str(),
            relationship = tension.relationship.as_str(),
            "Emergent tension discovered by response finder"
        );

        stats.emergent_tensions += 1;
        Ok(())
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_finding_parses_empty() {
        let json = r#"{"responses": [], "emergent_tensions": [], "future_queries": []}"#;
        let finding: ResponseFinding = serde_json::from_str(json).unwrap();
        assert!(finding.responses.is_empty());
        assert!(finding.emergent_tensions.is_empty());
        assert!(finding.future_queries.is_empty());
    }

    #[test]
    fn response_finding_parses_with_responses() {
        let json = r#"{
            "responses": [{
                "title": "Know Your Rights Workshop",
                "summary": "Free legal workshops for immigrants",
                "signal_type": "resource",
                "url": "https://example.com/kyr",
                "diffusion_mechanism": "legal education",
                "explanation": "Dissolves fear through knowledge of rights",
                "match_strength": 0.9,
                "also_addresses": ["Housing instability"],
                "event_date": null
            }],
            "emergent_tensions": [{
                "title": "Retaliation against organizers",
                "summary": "Workshop organizers facing threats",
                "severity": "high",
                "category": "safety",
                "opposing": "Security resources and legal protection",
                "source_url": "https://example.com/threats",
                "relationship": "Discovered while investigating ICE response landscape"
            }],
            "future_queries": ["mutual aid networks Minneapolis immigrants"]
        }"#;
        let finding: ResponseFinding = serde_json::from_str(json).unwrap();
        assert_eq!(finding.responses.len(), 1);
        assert_eq!(finding.responses[0].title, "Know Your Rights Workshop");
        assert_eq!(finding.responses[0].signal_type, "resource");
        assert!((finding.responses[0].match_strength - 0.9).abs() < 0.001);
        assert_eq!(
            finding.responses[0].also_addresses,
            vec!["Housing instability"]
        );
        assert_eq!(finding.emergent_tensions.len(), 1);
        assert_eq!(
            finding.emergent_tensions[0].title,
            "Retaliation against organizers"
        );
        assert_eq!(finding.future_queries.len(), 1);
    }

    #[test]
    fn response_finding_defaults_missing_fields() {
        let json = r#"{}"#;
        let finding: ResponseFinding = serde_json::from_str(json).unwrap();
        assert!(finding.responses.is_empty());
        assert!(finding.emergent_tensions.is_empty());
        assert!(finding.future_queries.is_empty());
    }

    #[test]
    fn response_scout_stats_display() {
        let stats = ResponseFinderStats {
            targets_found: 5,
            targets_investigated: 4,
            responses_discovered: 12,
            responses_deduped: 3,
            signals_created: 9,
            edges_created: 15,
            emergent_tensions: 2,
            future_sources_created: 6,
        };
        let display = format!("{stats}");
        assert!(display.contains("5 targets found"));
        assert!(display.contains("4 investigated"));
        assert!(display.contains("12 responses discovered"));
        assert!(display.contains("9 signals created"));
        assert!(display.contains("2 emergent tensions"));
    }

    #[test]
    fn event_date_parsing() {
        // Valid date
        let date_str = "2026-03-15";
        let parsed = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d");
        assert!(parsed.is_ok());

        // Invalid date
        let bad_str = "not-a-date";
        let parsed = chrono::NaiveDate::parse_from_str(bad_str, "%Y-%m-%d");
        assert!(parsed.is_err());
    }

    #[test]
    fn response_node_gets_region_center_coordinates() {
        let region = ScoutScope {
            name: "Minneapolis".to_string(),
            center_lat: 44.9778,
            center_lng: -93.2650,
            radius_km: 30.0,
        };

        let meta = build_node_meta(
            "Know Your Rights Workshop".to_string(),
            "Free legal workshops".to_string(),
            "https://example.com/kyr".to_string(),
            &region,
            0.7,
        );

        let node = Node::Resource(ResourceOfferNode {
            meta,
            action_url: "https://example.com/kyr".to_string(),
            availability: None,
            eligibility: None,
            is_ongoing: true,
        });

        let loc = node.meta().unwrap().about_location.as_ref().unwrap();
        assert!((loc.lat - 44.9778).abs() < 0.001);
        assert!((loc.lng - (-93.2650)).abs() < 0.001);
        assert_eq!(loc.precision, rootsignal_common::GeoPrecision::Approximate);
    }
}

