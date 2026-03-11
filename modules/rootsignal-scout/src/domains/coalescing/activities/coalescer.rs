use std::sync::Arc;

use anyhow::Result;
use schemars::JsonSchema;
use serde::Deserialize;
use tracing::{info, warn};
use uuid::Uuid;

use ai_client::{ai_extract, Agent, DynTool, ToolWrapper};
use rootsignal_graph::GraphQueries;

use crate::infra::embedder::TextEmbedder;

use super::tools::{FindSimilarTool, SearchSignalsTool};
use super::types::{CoalescingResult, FedSignal, ProtoGroup};

const MAX_TOOL_TURNS: usize = 3;
pub const MAX_FEED_GROUPS: u32 = 5;
const MAX_FEED_RESULTS_PER_QUERY: u32 = 10;

// =============================================================================
// Structured output types for ai_extract
// =============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SeedOutput {
    /// Did the investigation find a coherent cluster worth grouping?
    pub found_group: bool,
    /// If no group, why not
    pub skip_reason: Option<String>,
    /// The discovered groups (usually 1, occasionally 2 if the seed reveals distinct threads)
    #[serde(default)]
    pub groups: Vec<SeedGroup>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SeedGroup {
    /// Human-readable label for this group (e.g. "Lake Street corridor safety concerns")
    pub label: String,
    /// Search queries that would find more signals like these (3-5 queries)
    pub queries: Vec<String>,
    /// Signal IDs that belong in this group, with confidence 0.0-1.0
    pub members: Vec<SeedMember>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SeedMember {
    pub signal_id: String,
    pub confidence: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FeedOutput {
    /// Signals to add to this group
    #[serde(default)]
    pub add: Vec<FeedMember>,
    /// Refined queries if the group's character has shifted (empty = keep existing)
    #[serde(default)]
    pub refined_queries: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FeedMember {
    pub signal_id: String,
    pub confidence: f64,
}

// =============================================================================
// Prompts
// =============================================================================

fn seed_system_prompt(seed_title: &str, seed_summary: &str, group_landscape: &str) -> String {
    format!(
        "You are a community signal analyst. You've been given a seed signal — a community concern \
with high tension — and tools to search for related signals in the database.

Your job: find signals that share a common thread with this seed. Look for signals connected by:
- Shared topic or issue (e.g. housing, transit, policing)
- Geographic proximity (same neighborhood, corridor, or area)
- Common actors or organizations
- Cause-and-effect relationships (one signal caused or responds to another)
- Temporal clustering (a burst of related activity)

Use search_signals to search by keywords, phrases, or themes. Use find_similar to explore \
signals similar to ones you've already found. Search broadly first, then refine.

Seed signal:
Title: {seed_title}
Summary: {seed_summary}

{group_landscape_section}

After investigating, describe what you found: which signals cluster together and why. \
If the seed signal is isolated with no meaningful connections, say so — not every signal \
belongs to a group.",
        group_landscape_section = if group_landscape.is_empty() {
            String::new()
        } else {
            format!(
                "Existing groups (avoid duplicating these):\n{group_landscape}\n\n\
                 If your findings overlap with an existing group, note the overlap \
                 rather than creating a duplicate."
            )
        }
    )
}

fn seed_extraction_prompt() -> &'static str {
    "Extract the groups discovered during investigation.

For each group:
- label: Short, specific label (e.g. \"Lake Street corridor safety concerns\")
- queries: 3-5 search queries that would find more signals like these in the future
- members: Signal IDs that belong, with confidence 0.0-1.0

Set found_group=false with a skip_reason if no coherent cluster was found.
Most investigations produce 0 or 1 groups. Only produce 2 if genuinely distinct threads emerged."
}

fn feed_extraction_prompt(group_label: &str, group_queries: &[String]) -> String {
    format!(
        "You are reviewing candidate signals for an existing group.

Group: \"{group_label}\"
Current queries: {queries}

Below are candidate signals found by running the group's queries. For each candidate, \
decide whether it belongs in this group (confidence 0.0-1.0). Only add signals that \
genuinely share the group's theme — not just vaguely related.

If the new signals reveal that the group's theme has shifted or expanded, provide \
refined_queries (3-5 queries). Otherwise leave refined_queries empty.",
        queries = group_queries.join(", ")
    )
}

// =============================================================================
// Coalescer
// =============================================================================

pub struct Coalescer {
    graph: Arc<dyn GraphQueries>,
    ai: Arc<dyn Agent>,
    embedder: Arc<dyn TextEmbedder>,
}

impl Coalescer {
    pub fn new(
        graph: Arc<dyn GraphQueries>,
        ai: Arc<dyn Agent>,
        embedder: Arc<dyn TextEmbedder>,
    ) -> Self {
        Self { graph, ai, embedder }
    }

    /// Run seed mode (new groups from ungrouped signals) + feed mode (grow existing groups).
    /// When `seed_signal_id` is Some, seeds from that specific signal instead of auto-selecting.
    pub async fn run(&self, seed_signal_id: Option<Uuid>) -> Result<CoalescingResult> {
        let new_groups = self.seed_mode(seed_signal_id).await?;
        let (fed_signals, refined_queries) = self.feed_mode().await?;

        Ok(CoalescingResult {
            new_groups,
            fed_signals,
            refined_queries,
        })
    }

    /// Seed mode: investigate a signal and cluster related signals into new groups.
    /// When `seed_signal_id` is Some, fetches that signal directly.
    /// When None, picks the highest-heat ungrouped Concern.
    async fn seed_mode(&self, seed_signal_id: Option<Uuid>) -> Result<Vec<ProtoGroup>> {
        let (seed_id, seed_title, seed_summary, seed_signal_type, seed_cause_heat) =
            if let Some(id) = seed_signal_id {
                let details = self.graph.get_signal_details(&[id]).await?;
                match details.into_iter().next() {
                    Some(d) => (d.id, d.title, d.summary, d.signal_type, d.cause_heat.unwrap_or(0.0)),
                    None => {
                        info!(%id, "Seed signal not found — skipping seed mode");
                        return Ok(vec![]);
                    }
                }
            } else {
                let seeds = self.graph.get_ungrouped_signals(1).await?;
                match seeds.into_iter().next() {
                    Some(s) => (s.id, s.title, s.summary, s.signal_type, s.cause_heat),
                    None => {
                        info!("No ungrouped signals — skipping seed mode");
                        return Ok(vec![]);
                    }
                }
            };

        info!(
            seed_id = %seed_id,
            seed_title = seed_title.as_str(),
            cause_heat = seed_cause_heat,
            "Seed mode: investigating signal"
        );

        let group_landscape = self.format_group_landscape().await?;

        // Phase 1: Agentic investigation — LLM uses search tools to find related signals
        let tools: Vec<Arc<dyn DynTool>> = vec![
            Arc::new(ToolWrapper(SearchSignalsTool {
                graph: self.graph.clone(),
                embedder: self.embedder.clone(),
            })),
            Arc::new(ToolWrapper(FindSimilarTool {
                graph: self.graph.clone(),
            })),
        ];
        let tool_agent = self.ai.with_tools(tools);

        let system = seed_system_prompt(&seed_title, &seed_summary, &group_landscape);
        let user_msg = format!(
            "Investigate this signal and find related signals:\n\
             ID: {}\nType: {}\nTitle: {}\nSummary: {}\nHeat: {:.2}",
            seed_id, seed_signal_type, seed_title, seed_summary, seed_cause_heat
        );

        let reasoning = tool_agent
            .prompt(&user_msg)
            .preamble(&system)
            .temperature(0.7)
            .multi_turn(MAX_TOOL_TURNS)
            .send()
            .await?;

        // Phase 2: Structure the findings into ProtoGroups
        let extraction_user = format!(
            "Seed signal: {} (ID: {})\n\nInvestigation findings:\n{}",
            seed_title, seed_id, reasoning
        );

        let output: SeedOutput =
            ai_extract(self.ai.as_ref(), seed_extraction_prompt(), &extraction_user).await?;

        if !output.found_group {
            info!(
                reason = output.skip_reason.as_deref().unwrap_or("unknown"),
                "Seed mode: no coherent group found"
            );
            return Ok(vec![]);
        }

        let proto_groups: Vec<ProtoGroup> = output
            .groups
            .into_iter()
            .filter_map(|g| {
                let signal_ids: Vec<(Uuid, f64)> = g
                    .members
                    .iter()
                    .filter_map(|m| {
                        Uuid::parse_str(&m.signal_id)
                            .map(|id| (id, m.confidence))
                            .map_err(|e| {
                                warn!(
                                    signal_id = m.signal_id.as_str(),
                                    error = %e,
                                    "Ignoring invalid signal ID from LLM"
                                );
                                e
                            })
                            .ok()
                    })
                    .collect();

                if signal_ids.is_empty() {
                    warn!(label = g.label.as_str(), "Dropping group with no valid signal IDs");
                    return None;
                }

                Some(ProtoGroup {
                    group_id: Uuid::new_v4(),
                    label: g.label,
                    queries: g.queries,
                    signal_ids,
                })
            })
            .collect();

        for g in &proto_groups {
            info!(
                group_id = %g.group_id,
                label = g.label.as_str(),
                signal_count = g.signal_ids.len(),
                query_count = g.queries.len(),
                "Seed mode: created new group"
            );
        }

        Ok(proto_groups)
    }

    /// Feed mode: for each existing group, run its queries to find new matching signals,
    /// then ask the LLM whether they belong.
    async fn feed_mode(&self) -> Result<(Vec<FedSignal>, Vec<(Uuid, Vec<String>)>)> {
        let groups = self.graph.get_group_landscape(MAX_FEED_GROUPS).await?;
        if groups.is_empty() {
            info!("No existing groups — skipping feed mode");
            return Ok((vec![], vec![]));
        }

        info!(group_count = groups.len(), "Feed mode: growing existing groups");

        let mut all_fed = Vec::new();
        let mut all_refined = Vec::new();

        for group in &groups {
            match self.feed_single_group(group).await {
                Ok((fed, refined)) => {
                    all_fed.extend(fed);
                    if let Some(queries) = refined {
                        all_refined.push((group.id, queries));
                    }
                }
                Err(e) => {
                    warn!(
                        group_id = %group.id,
                        label = group.label.as_str(),
                        error = %e,
                        "Feed mode: failed to process group, continuing"
                    );
                }
            }
        }

        Ok((all_fed, all_refined))
    }

    async fn feed_single_group(
        &self,
        group: &rootsignal_graph::GroupBrief,
    ) -> Result<(Vec<FedSignal>, Option<Vec<String>>)> {
        // Run each of the group's stored queries and collect candidate signals
        let mut candidate_ids = Vec::new();
        for query in &group.queries {
            let results = self
                .graph
                .fulltext_search_signals(query, MAX_FEED_RESULTS_PER_QUERY)
                .await?;
            for r in results {
                if !candidate_ids.contains(&r.id) {
                    candidate_ids.push(r.id);
                }
            }
        }

        // Exclude signals already in the group — no point re-reviewing them
        candidate_ids.retain(|id| !group.member_ids.contains(id));

        if candidate_ids.is_empty() {
            return Ok((vec![], None));
        }

        // Batch-fetch details for candidates
        let details = self.graph.get_signal_details(&candidate_ids).await?;
        if details.is_empty() {
            return Ok((vec![], None));
        }

        // Format candidates for LLM review
        let candidates_text: String = details
            .iter()
            .map(|d| {
                format!(
                    "- ID: {} | Type: {} | Title: {} | Summary: {}",
                    d.id, d.signal_type, d.title, d.summary
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let system = feed_extraction_prompt(&group.label, &group.queries);

        let output: FeedOutput =
            ai_extract(self.ai.as_ref(), &system, &candidates_text).await?;

        let fed_signals: Vec<FedSignal> = output
            .add
            .iter()
            .filter_map(|m| {
                Uuid::parse_str(&m.signal_id)
                    .map(|id| FedSignal {
                        signal_id: id,
                        group_id: group.id,
                        confidence: m.confidence,
                    })
                    .map_err(|e| {
                        warn!(
                            signal_id = m.signal_id.as_str(),
                            error = %e,
                            "Ignoring invalid signal ID from LLM in feed mode"
                        );
                        e
                    })
                    .ok()
            })
            // Belt and suspenders: LLM might hallucinate IDs of existing members
            .filter(|f| !group.member_ids.contains(&f.signal_id))
            .collect();

        let refined = if output.refined_queries.is_empty() {
            None
        } else {
            Some(output.refined_queries)
        };

        if !fed_signals.is_empty() {
            info!(
                group_id = %group.id,
                label = group.label.as_str(),
                fed_count = fed_signals.len(),
                "Feed mode: added signals to group"
            );
        }

        Ok((fed_signals, refined))
    }

    async fn format_group_landscape(&self) -> Result<String> {
        let groups = self.graph.get_group_landscape(MAX_FEED_GROUPS).await?;
        if groups.is_empty() {
            return Ok(String::new());
        }

        Ok(groups
            .iter()
            .map(|g| {
                format!(
                    "- \"{}\" ({} signals, queries: {})",
                    g.label,
                    g.signal_count,
                    g.queries.join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }
}
