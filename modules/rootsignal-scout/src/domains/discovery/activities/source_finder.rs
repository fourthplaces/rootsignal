use std::collections::HashSet;

use ai_client::claude::Claude;
use rootsignal_common::{canonical_value, is_web_query, DiscoveryMethod, SourceNode, SourceRole};
use rootsignal_graph::{
    ExtractionYield, GapTypeStats, GraphStore, SignalTypeCounts, SituationBrief, SourceBrief,
    TensionResponseShape, UnmetTension,
};
use schemars::JsonSchema;
use serde::{de, Deserialize};
use tracing::{info, warn};
use crate::domains::scheduling::activities::budget::{BudgetTracker, OperationCost};
use crate::infra::embedder::TextEmbedder;


const HAIKU_MODEL: &str = "claude-haiku-4-5-20251001";
const MAX_CURIOSITY_QUERIES: usize = 12;
const MAX_DISCOVERY_DEPTH: u32 = 2;

/// Stats from a discovery run.
///
/// Counts reflect discovery intent (sources added to the output vec),
/// not persistence outcome. The reducer's `sources_discovered` stat
/// is the authoritative count of sources that reached the event store.
#[derive(Debug, Default)]
pub struct SourceFinderStats {
    pub actor_sources: u32,
    pub link_sources: u32,
    pub gap_sources: u32,
    pub duplicates_skipped: u32,
}

impl std::fmt::Display for SourceFinderStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Discovery: actors={}, links={}, gaps={}, skipped={}",
            self.actor_sources, self.link_sources, self.gap_sources, self.duplicates_skipped
        )
    }
}

// --- Discovery Briefing ---

/// Assembled from graph queries — everything the LLM needs to decide where to look next.
#[derive(Debug, Clone)]
pub struct DiscoveryBriefing {
    pub tensions: Vec<UnmetTension>,
    pub situations: Vec<SituationBrief>,
    pub signal_counts: SignalTypeCounts,
    pub successes: Vec<SourceBrief>,
    pub failures: Vec<SourceBrief>,
    pub existing_queries: Vec<String>,
    pub region_name: String,
    pub gap_type_stats: Vec<GapTypeStats>,
    pub extraction_yield: Vec<ExtractionYield>,
    pub response_shapes: Vec<TensionResponseShape>,
}

impl DiscoveryBriefing {
    /// True when the graph is too sparse for LLM discovery to be useful.
    pub fn is_cold_start(&self) -> bool {
        self.tensions.len() < 3 && self.situations.is_empty()
    }

    /// Render the briefing as structured natural language for the LLM.
    pub fn format_prompt(&self) -> String {
        let mut out = String::with_capacity(4096);

        // Tensions
        let unmet: Vec<_> = self.tensions.iter().filter(|t| t.unmet).collect();
        let met: Vec<_> = self.tensions.iter().filter(|t| !t.unmet).collect();

        if !unmet.is_empty() {
            out.push_str("## UNMET TENSIONS (no response found)\n");
            for (i, t) in unmet.iter().enumerate() {
                let help = t.what_would_help.as_deref().unwrap_or("unknown");
                out.push_str(&format!(
                    "{}. [{}] \"{}\" — What would help: {}\n",
                    i + 1,
                    t.severity.to_uppercase(),
                    t.title,
                    help,
                ));
                out.push_str(&format!(
                    "   community attention: {} sources, {} corroborations, heat={:.1}\n",
                    t.source_diversity, t.corroboration_count, t.cause_heat,
                ));
            }
            out.push('\n');
        }

        if !met.is_empty() {
            out.push_str("## TENSIONS WITH RESPONSES (lower priority)\n");
            for (i, t) in met.iter().enumerate() {
                let help = t.what_would_help.as_deref().unwrap_or("unknown");
                out.push_str(&format!(
                    "{}. [{}] \"{}\" — What would help: {}\n",
                    i + 1,
                    t.severity.to_uppercase(),
                    t.title,
                    help,
                ));
                out.push_str(&format!(
                    "   community attention: {} sources, {} corroborations, heat={:.1}\n",
                    t.source_diversity, t.corroboration_count, t.cause_heat,
                ));
            }
            out.push('\n');
        }

        // Response shapes
        if !self.response_shapes.is_empty() {
            out.push_str("## RESPONSE SHAPE (what's missing from each tension's response)\n\n");
            out.push_str(
                "These tensions HAVE responses, but coverage may be uneven. Look for what's\n",
            );
            out.push_str(
                "MISSING — if a tension has legal aid but no housing help, search for housing.\n",
            );
            out.push_str("If it has events but no donation channels, search for giving.\n\n");
            for rs in &self.response_shapes {
                let help = rs.what_would_help.as_deref().unwrap_or("unknown");
                out.push_str(&format!(
                    "- \"{}\" (heat: {:.1})\n",
                    rs.title, rs.cause_heat,
                ));
                out.push_str(&format!("  What would help: {}\n", help));
                out.push_str(&format!(
                    "  Aids: {}, Gatherings: {}, Needs: {}\n",
                    rs.aid_count, rs.gathering_count, rs.need_count,
                ));
                if !rs.sample_titles.is_empty() {
                    let titles = rs
                        .sample_titles
                        .iter()
                        .map(|t| format!("\"{}\"", t))
                        .collect::<Vec<_>>()
                        .join(", ");
                    out.push_str(&format!("  Known: {}\n", titles));
                }
            }
            out.push('\n');
        }

        // Situation Landscape
        if !self.situations.is_empty() {
            out.push_str("## SITUATION LANDSCAPE\n");
            out.push_str("Living situations — causal groupings of signals around a root cause.\n");
            out.push_str("Fuzzy situations need more evidence. Hot situations may need response discovery.\n\n");
            for (i, s) in self.situations.iter().enumerate() {
                let loc = s.location_name.as_deref().unwrap_or("unknown location");
                out.push_str(&format!(
                    "{}. \"{}\" [{loc}]\n   arc={}, temp={:.2}, clarity={}, signals={}, tensions={}\n",
                    i + 1,
                    s.headline,
                    s.arc,
                    s.temperature,
                    s.clarity,
                    s.signal_count,
                    s.tension_count,
                ));
                if s.sensitivity == "SENSITIVE" || s.sensitivity == "RESTRICTED" {
                    out.push_str("   ⚠ SENSITIVE — handle with care in discovery\n");
                }
            }
            out.push('\n');
        }

        // Signal balance
        let sc = &self.signal_counts;
        out.push_str("## SIGNAL BALANCE\n");
        out.push_str(&format!(
            "Gatherings: {} | Aids: {} | Needs: {} | Notices: {} | Tensions: {}\n",
            sc.gatherings, sc.aids, sc.needs, sc.notices, sc.tensions,
        ));
        // Annotate significant imbalances
        let total = sc.gatherings + sc.aids + sc.needs + sc.notices + sc.tensions;
        if total > 5 {
            if sc.tensions > 0 && sc.aids < sc.tensions / 3 {
                out.push_str(
                    "→ Aid signals significantly underrepresented relative to tensions.\n",
                );
            }
            if sc.needs > 0 && sc.aids < sc.needs / 2 {
                out.push_str("→ Few Aid signals to match the Need signals.\n");
            }
        }
        out.push('\n');

        // Past discovery results
        if !self.successes.is_empty() || !self.failures.is_empty() {
            out.push_str("## PAST DISCOVERY RESULTS\n");
            if !self.successes.is_empty() {
                out.push_str("Worked well:\n");
                for s in &self.successes {
                    let reason = s.gap_context.as_deref().unwrap_or("unknown");
                    out.push_str(&format!(
                        "- \"{}\" → {} signals, weight {:.1} (reason: {})\n",
                        s.canonical_value, s.signals_produced, s.weight, reason,
                    ));
                }
            }
            if !self.failures.is_empty() {
                out.push_str("Didn't work:\n");
                for f in &self.failures {
                    let reason = f.gap_context.as_deref().unwrap_or("unknown");
                    out.push_str(&format!(
                        "- \"{}\" → {} signals, {} empty runs (reason: {})\n",
                        f.canonical_value, f.signals_produced, f.consecutive_empty_runs, reason,
                    ));
                }
            }
            out.push('\n');
        }

        // Strategy performance
        if !self.gap_type_stats.is_empty() {
            out.push_str("## STRATEGY PERFORMANCE\n");
            for g in &self.gap_type_stats {
                let pct = if g.total_sources > 0 {
                    (g.successful_sources as f64 / g.total_sources as f64 * 100.0) as u32
                } else {
                    0
                };
                out.push_str(&format!(
                    "- {}: {}/{} sources successful ({}%), avg weight {:.2}\n",
                    g.gap_type, g.successful_sources, g.total_sources, pct, g.avg_weight,
                ));
                if g.total_sources >= 5 && g.successful_sources == 0 {
                    out.push_str(&format!(
                        "→ WARNING: \"{}\" strategy has 0% success rate across {} attempts. Consider avoiding.\n",
                        g.gap_type, g.total_sources,
                    ));
                }
            }
            out.push('\n');
        }

        // Extraction yield
        if !self.extraction_yield.is_empty() {
            out.push_str("## EXTRACTION YIELD\n");
            for y in &self.extraction_yield {
                let survival_pct = if y.extracted > 0 {
                    (y.survived as f64 / y.extracted as f64 * 100.0) as u32
                } else {
                    0
                };
                out.push_str(&format!(
                    "- {}: {} extracted, {} survived ({}%), {} corroborated, {} contradicted\n",
                    y.source_label,
                    y.extracted,
                    y.survived,
                    survival_pct,
                    y.corroborated,
                    y.contradicted,
                ));
                if y.extracted >= 10 && survival_pct < 50 {
                    out.push_str(&format!(
                        "→ {} survival rate below 50% — signals from this source type are frequently reaped.\n",
                        y.source_label,
                    ));
                }
                let contradiction_pct = if y.extracted > 0 {
                    (y.contradicted as f64 / y.extracted as f64 * 100.0) as u32
                } else {
                    0
                };
                if y.extracted >= 10 && contradiction_pct >= 20 {
                    out.push_str(&format!(
                        "→ {} has high contradiction rate ({}%) — evidence frequently disputes these signals.\n",
                        y.source_label, contradiction_pct,
                    ));
                }
            }
            out.push('\n');
        }

        // Existing queries
        if !self.existing_queries.is_empty() {
            out.push_str("## EXISTING QUERIES (do not duplicate)\n");
            for q in &self.existing_queries {
                out.push_str(&format!("- {}\n", q));
            }
            out.push('\n');
        }

        out
    }
}

// --- LLM Structured Output Types ---

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DiscoveryPlan {
    #[serde(default, deserialize_with = "deserialize_queries")]
    pub queries: Vec<DiscoveryQuery>,
    /// Social media discovery topics (hashtags + search terms).
    /// Searched across Instagram, X/Twitter, TikTok, and GoFundMe.
    #[serde(default)]
    pub social_topics: Vec<SocialTopic>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SocialTopic {
    /// A hashtag or search term (plain text, no # prefix)
    pub topic: String,
    /// Why this topic — what gap it fills on social media
    pub reasoning: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DiscoveryQuery {
    /// The web search query
    pub query: String,
    /// Why this query — what gap it fills
    pub reasoning: String,
    /// Gap type: "unmet_tension", "low_type_diversity", "emerging_thread",
    /// "signal_imbalance", "novel_angle"
    pub gap_type: String,
    /// Related tension title, if applicable
    pub related_tension: Option<String>,
}

/// Handle LLM returning queries as either a proper JSON array, a stringified JSON array, or null.
fn deserialize_queries<'de, D>(deserializer: D) -> Result<Vec<DiscoveryQuery>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Array(_) => serde_json::from_value(value).map_err(de::Error::custom),
        serde_json::Value::String(ref s) => serde_json::from_str(s).map_err(de::Error::custom),
        serde_json::Value::Null => Ok(Vec::new()),
        _ => Err(de::Error::custom(
            "queries must be an array, JSON string, or null",
        )),
    }
}

// --- Initial weight by discovery method ---

/// Assign an initial weight to a new WebQuery source based on how it was created.
/// Higher weights mean more frequent scraping. Queries from methods with
/// historically higher yield get more budget.
pub fn initial_weight_for_method(method: DiscoveryMethod, gap_type: Option<&str>) -> f64 {
    match method {
        // Cold start: no signal data to guide us — moderate exploration weight
        DiscoveryMethod::Curated | DiscoveryMethod::HumanSubmission => 0.5,
        // Gap analysis targeting an unmet tension: high-value, community-driven
        DiscoveryMethod::GapAnalysis => match gap_type {
            Some(gt) if gt == "unmet_tension" => 0.4,
            _ => 0.3,
        },
        // Signal expansion: derived from existing signals, lower novelty
        DiscoveryMethod::SignalExpansion => 0.2,
        // Social graph follow: promoted from mentions, unproven
        DiscoveryMethod::SocialGraphFollow => 0.2,
        // Linked from a scraped page: speculative, unproven
        DiscoveryMethod::LinkedFrom => 0.25,
        // Everything else (HashtagDiscovery, SignalReference, etc.)
        _ => 0.3,
    }
}

// --- LLM Prompts ---

pub fn discovery_system_prompt(city_name: &str) -> String {
    format!(
        "You are the curiosity engine for an intelligence scout monitoring {city_name}.\n\
         \n\
         Your job: decide WHERE TO LOOK NEXT to fill gaps in what the scout knows.\n\
         \n\
         You receive a briefing about what the scout has learned — tensions without responses,\n\
         emerging situations, signal imbalances, and the track record of your past suggestions.\n\
         \n\
         Generate 3-7 targeted web search queries. Prioritize:\n\
         1. RESPONSE RESOURCES for unmet tensions (highest priority)\n\
         2. DEMAND SIGNALS — if tensions have few associated Needs, search for \
people expressing needs related to those tensions (GoFundMe campaigns, \
mutual aid requests, community forum posts, volunteer calls)\n\
         3. MISSING SIGNAL TYPES for situations with low type diversity\n\
         4. EMERGING THREADS that deserve deeper investigation\n\
         5. NOVEL ANGLES that existing sources aren't covering\n\
         \n\
         Query quality guidelines:\n\
         - Include \"{city_name}\" or neighborhood names — local results only\n\
         - Target organizations, programs, resources — not news articles about problems\n\
         - Avoid queries similar to ones that previously failed\n\
         - Be specific: \"affordable housing waitlist programs {city_name}\" not \"housing crisis\"\n\
         \n\
         ENGAGEMENT SIGNAL: Tensions with higher corroboration, source diversity, and\n\
         cause_heat are ones the community is actively discussing — prioritize these\n\
         for response-seeking queries. But ALWAYS reserve at least 2 queries for\n\
         low-engagement or novel tensions — early signals matter.\n\
         \n\
         For each query, explain your reasoning — why this search, what gap it fills.\n\
         \n\
         ## SOCIAL MEDIA DISCOVERY TOPICS\n\
         \n\
         Additionally, generate 2-5 social media discovery topics — hashtags and\n\
         search terms for finding INDIVIDUALS posting publicly about community\n\
         tensions on Instagram, X/Twitter, TikTok, and GoFundMe.\n\
         \n\
         Social topics should target:\n\
         - Individuals publicly advocating, volunteering, or organizing\n\
         - GoFundMe campaigns for community causes\n\
         - Hashtags used by local advocacy communities\n\
         - Search terms for mutual aid, donations, volunteer coordination\n\
         \n\
         Include \"{city_name}\" or state abbreviation. Examples:\n\
         - \"MNimmigration\" (Instagram hashtag)\n\
         - \"sanctuary city {city_name} volunteer\" (X/Twitter keyword)\n\
         - \"immigration legal aid Minnesota\" (GoFundMe search)\n\
         \n\
         Focus on PEOPLE, not organizations — individuals who chose to be\n\
         publicly visible. Organizations may be deliberately hidden.\n\
         \n\
         For each topic, explain your reasoning."
    )
}

fn discovery_user_prompt(city_name: &str, briefing: &str) -> String {
    format!(
        "Here is the current state of the {city_name} signal graph. Analyze the gaps \
         and generate discovery queries.\n\n{briefing}"
    )
}

// --- SourceFinder ---

/// Discovers new sources from existing graph data.
pub struct SourceFinder<'a> {
    graph: &'a GraphStore,
    region_slug: String,
    region_name: String,
    claude: Option<Claude>,
    budget: &'a BudgetTracker,
    embedder: Option<&'a dyn TextEmbedder>,
}

/// Cosine similarity threshold for embedding-based query dedup.
/// 0.90 catches near-identical reformulations while preserving
/// queries that target different aspects of the same tension.
const QUERY_DEDUP_SIMILARITY_THRESHOLD: f64 = 0.90;

impl<'a> SourceFinder<'a> {
    pub fn new(
        graph: &'a GraphStore,
        region_slug: &str,
        region_name: &str,
        anthropic_api_key: Option<&str>,
        budget: &'a BudgetTracker,
    ) -> Self {
        let claude = anthropic_api_key
            .filter(|k| !k.is_empty())
            .map(|k| Claude::new(k, HAIKU_MODEL));
        Self {
            graph,
            region_slug: region_slug.to_string(),
            region_name: region_name.to_string(),
            claude,
            budget,
            embedder: None,
        }
    }

    /// Set an embedder for semantic query deduplication.
    /// When set, new queries are embedded and checked against existing query
    /// embeddings before creation. Without an embedder, falls back to
    /// substring-based dedup only.
    pub fn with_embedder(mut self, embedder: &'a dyn TextEmbedder) -> Self {
        self.embedder = Some(embedder);
        self
    }

    /// Run all discovery triggers. Returns stats, social topics, and discovered sources.
    pub async fn run(&self) -> (SourceFinderStats, Vec<String>, Vec<SourceNode>) {
        let mut stats = SourceFinderStats::default();
        let mut social_topics = Vec::new();
        let mut sources = Vec::new();

        // 1. Actor-mentioned sources — actors with domains/URLs that aren't tracked
        self.discover_from_actors(&mut stats, &mut sources).await;

        // 2. LLM-driven curiosity engine (with mechanical fallback)
        self.discover_from_curiosity(&mut stats, &mut social_topics, &mut sources)
            .await;

        if stats.actor_sources + stats.link_sources + stats.gap_sources > 0 {
            info!("{stats}");
        }

        if !social_topics.is_empty() {
            info!(
                count = social_topics.len(),
                "Social discovery topics generated"
            );
        }

        (stats, social_topics, sources)
    }

    /// Find actors with domains/URLs that aren't already tracked as sources.
    async fn discover_from_actors(
        &self,
        stats: &mut SourceFinderStats,
        sources: &mut Vec<SourceNode>,
    ) {
        let actors = match self
            .graph
            .get_actors_with_domains(Some(MAX_DISCOVERY_DEPTH))
            .await
        {
            Ok(a) => a,
            Err(e) => {
                warn!(error = %e, "Failed to get actors for discovery");
                return;
            }
        };

        let existing = match self.graph.get_active_sources().await {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "Failed to get existing sources for dedup");
                return;
            }
        };
        let existing_urls: HashSet<String> = existing
            .iter()
            .filter_map(|s| s.url.as_ref().cloned())
            .collect();
        let existing_keys: HashSet<String> =
            existing.iter().map(|s| s.canonical_key.clone()).collect();

        for (actor_name, domains, social_urls, dominant_role) in &actors {
            let source_role = SourceRole::from_str_loose(dominant_role);

            // Check each domain as a potential web source
            for domain in domains {
                let url = if domain.starts_with("http") {
                    domain.clone()
                } else {
                    format!("https://{domain}")
                };

                if existing_urls.contains(&url) {
                    stats.duplicates_skipped += 1;
                    continue;
                }

                let ck = canonical_value(&url);
                let cv = rootsignal_common::canonical_value(&url);
                if existing_keys.contains(&ck) {
                    stats.duplicates_skipped += 1;
                    continue;
                }

                let source = SourceNode::new(
                    ck,
                    cv,
                    Some(url.clone()),
                    DiscoveryMethod::SignalReference,
                    0.3,
                    source_role,
                    Some(format!("Actor: {actor_name}")),
                );
                stats.actor_sources += 1;
                info!(
                    actor = actor_name.as_str(),
                    url, "Discovered source from actor domain"
                );
                sources.push(source);
            }

            // Check social URLs as potential sources
            for social_url in social_urls {
                if existing_urls.contains(social_url) {
                    stats.duplicates_skipped += 1;
                    continue;
                }

                let ck = canonical_value(social_url);
                let cv = rootsignal_common::canonical_value(social_url);
                if existing_keys.contains(&ck) {
                    stats.duplicates_skipped += 1;
                    continue;
                }

                let source = SourceNode::new(
                    ck,
                    cv,
                    Some(social_url.clone()),
                    DiscoveryMethod::SignalReference,
                    0.3,
                    source_role,
                    Some(format!("Actor: {actor_name}")),
                );
                stats.actor_sources += 1;
                info!(
                    actor = actor_name.as_str(),
                    url = social_url.as_str(),
                    "Discovered source from actor social"
                );
                sources.push(source);
            }
        }
    }

    /// LLM-driven curiosity engine with mechanical fallback.
    async fn discover_from_curiosity(
        &self,
        stats: &mut SourceFinderStats,
        social_topics: &mut Vec<String>,
        sources: &mut Vec<SourceNode>,
    ) {
        // Guard: no Claude client → mechanical fallback
        let claude = match &self.claude {
            Some(c) => c,
            None => {
                self.discover_from_gaps_mechanical(stats, social_topics, sources)
                    .await;
                return;
            }
        };

        // Guard: no budget → mechanical fallback
        if self.budget.is_active()
            && !self
                .budget
                .has_budget(OperationCost::CLAUDE_HAIKU_DISCOVERY)
        {
            info!("Skipping LLM discovery (budget exhausted), falling back to mechanical");
            self.discover_from_gaps_mechanical(stats, social_topics, sources)
                .await;
            return;
        }

        // Build briefing from graph queries
        let briefing = match self.build_briefing().await {
            Ok(b) => b,
            Err(e) => {
                warn!(error = %e, "Failed to build discovery briefing, falling back to mechanical");
                self.discover_from_gaps_mechanical(stats, social_topics, sources)
                    .await;
                return;
            }
        };

        // Cold-start check
        if briefing.is_cold_start() {
            info!("Cold start detected (< 3 tensions, 0 situations), using mechanical discovery");
            self.discover_from_gaps_mechanical(stats, social_topics, sources)
                .await;
            return;
        }

        // LLM call
        let formatted = briefing.format_prompt();
        let system = discovery_system_prompt(&self.region_name);
        let user = discovery_user_prompt(&self.region_name, &formatted);

        let plan: DiscoveryPlan = match claude.extract(&system, &user).await {
            Ok(p) => p,
            Err(e) => {
                warn!(error = %e, "LLM discovery failed, falling back to mechanical");
                self.discover_from_gaps_mechanical(stats, social_topics, sources)
                    .await;
                return;
            }
        };

        // Record budget spend after successful response
        self.budget.spend(OperationCost::CLAUDE_HAIKU_DISCOVERY);

        // Extract social topics from plan (for topic discovery pipeline)
        const MAX_SOCIAL_TOPICS: usize = 8;
        for st in plan.social_topics.iter().take(MAX_SOCIAL_TOPICS) {
            info!(
                topic = st.topic.as_str(),
                reasoning = st.reasoning.as_str(),
                "LLM discovery: social topic"
            );
            social_topics.push(st.topic.clone());
        }

        // Create sources from plan
        let existing_queries: HashSet<String> = briefing
            .existing_queries
            .iter()
            .map(|q| q.to_lowercase())
            .collect();

        for dq in plan.queries.into_iter().take(MAX_CURIOSITY_QUERIES) {
            let query_lower = dq.query.to_lowercase();

            // Dedup layer 1: substring overlap with existing queries
            let is_dup = existing_queries
                .iter()
                .any(|q| q.contains(&query_lower) || query_lower.contains(q.as_str()));
            if is_dup {
                stats.duplicates_skipped += 1;
                continue;
            }

            // Dedup layer 2: embedding similarity against indexed query embeddings
            if let Some(embedder) = self.embedder {
                match embedder.embed(&dq.query).await {
                    Ok(embedding) => {
                        match self
                            .graph
                            .find_similar_query(&embedding, QUERY_DEDUP_SIMILARITY_THRESHOLD)
                            .await
                        {
                            Ok(Some((existing_ck, sim))) => {
                                info!(
                                    query = dq.query.as_str(),
                                    existing_key = existing_ck.as_str(),
                                    similarity = format!("{sim:.3}").as_str(),
                                    "Skipping semantically duplicate query"
                                );
                                stats.duplicates_skipped += 1;
                                continue;
                            }
                            Ok(None) => {
                                // Not a duplicate — we'll store the embedding after creation
                            }
                            Err(e) => {
                                warn!(error = %e, "Query embedding dedup check failed, proceeding");
                            }
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "Query embedding failed, skipping dedup check");
                    }
                }
            }

            let cv = dq.query.clone();
            let ck = canonical_value(&cv);

            let gap_context = format!(
                "Curiosity: {} | Gap: {} | Related: {}",
                dq.reasoning,
                dq.gap_type,
                dq.related_tension.as_deref().unwrap_or("none"),
            );

            // Assign source role based on gap type:
            // - unmet_tension: seeking responses TO a tension → Response
            // - signal_imbalance: depends on what's underrepresented, default Mixed
            // - everything else: Mixed
            let source_role = match dq.gap_type.as_str() {
                "unmet_tension" => SourceRole::Response,
                _ => SourceRole::Mixed,
            };

            let weight =
                initial_weight_for_method(DiscoveryMethod::GapAnalysis, Some(dq.gap_type.as_str()));

            let source = SourceNode::new(
                ck.clone(),
                cv,
                None,
                DiscoveryMethod::GapAnalysis,
                weight,
                source_role,
                Some(gap_context),
            );
            stats.gap_sources += 1;
            info!(
                query = dq.query.as_str(),
                reasoning = dq.reasoning.as_str(),
                gap_type = dq.gap_type.as_str(),
                weight,
                "LLM discovery: created query source"
            );
            sources.push(source);
            // Store query embedding so future runs can dedup against it
            if let Some(embedder) = self.embedder {
                if let Ok(embedding) = embedder.embed(&dq.query).await {
                    if let Err(e) = self.graph.set_query_embedding(&ck, &embedding).await {
                        warn!(error = %e, "Failed to store query embedding (non-fatal)");
                    }
                }
            }
        }
    }

    /// Build a DiscoveryBriefing from graph queries.
    async fn build_briefing(&self) -> anyhow::Result<DiscoveryBriefing> {
        let tensions = self
            .graph
            .get_unmet_tensions(10)
            .await
            .map_err(|e| anyhow::anyhow!("get_unmet_tensions: {e}"))?;

        let situations = self
            .graph
            .get_situation_landscape(10)
            .await
            .map_err(|e| anyhow::anyhow!("get_situation_landscape: {e}"))?;

        let signal_counts = self
            .graph
            .get_signal_type_counts()
            .await
            .map_err(|e| anyhow::anyhow!("get_signal_type_counts: {e}"))?;

        let (successes, failures) = self
            .graph
            .get_discovery_performance()
            .await
            .map_err(|e| anyhow::anyhow!("get_discovery_performance: {e}"))?;

        let gap_type_stats = self
            .graph
            .get_gap_type_stats()
            .await
            .map_err(|e| anyhow::anyhow!("get_gap_type_stats: {e}"))?;

        let extraction_yield = self
            .graph
            .get_extraction_yield()
            .await
            .map_err(|e| anyhow::anyhow!("get_extraction_yield: {e}"))?;

        let response_shapes = self
            .graph
            .get_tension_response_shape(10)
            .await
            .map_err(|e| anyhow::anyhow!("get_tension_response_shape: {e}"))?;

        // Get existing WebQuery sources for dedup
        let existing = self
            .graph
            .get_active_sources()
            .await
            .map_err(|e| anyhow::anyhow!("get_active_sources: {e}"))?;
        let existing_queries: Vec<String> = existing
            .iter()
            .filter(|s| is_web_query(&s.canonical_value))
            .map(|s| s.canonical_value.clone())
            .collect();

        Ok(DiscoveryBriefing {
            tensions,
            situations,
            signal_counts,
            successes,
            failures,
            existing_queries,
            region_name: self.region_name.clone(),
            gap_type_stats,
            extraction_yield,
            response_shapes,
        })
    }

    /// Mechanical template-based gap analysis — the original discovery method.
    /// Used as fallback when LLM is unavailable, budget is exhausted, or on cold start.
    ///
    /// Sorts tensions by engagement score (corroboration + source_diversity + cause_heat)
    /// so high-engagement tensions fill early query slots — but all tensions are eligible.
    async fn discover_from_gaps_mechanical(
        &self,
        stats: &mut SourceFinderStats,
        social_topics: &mut Vec<String>,
        sources: &mut Vec<SourceNode>,
    ) {
        // Get tensions with engagement data, sorted by engagement within unmet-first grouping
        let mut tensions = match self.graph.get_unmet_tensions(20).await {
            Ok(t) => t,
            Err(e) => {
                warn!(error = %e, "Failed to get tensions for gap analysis");
                return;
            }
        };

        if tensions.is_empty() {
            return;
        }

        // Stable-sort by engagement score descending (preserves unmet-first from query)
        tensions.sort_by(|a, b| {
            let score_a =
                a.corroboration_count as f64 + a.source_diversity as f64 + a.cause_heat * 10.0;
            let score_b =
                b.corroboration_count as f64 + b.source_diversity as f64 + b.cause_heat * 10.0;
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let existing = match self.graph.get_active_sources().await {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "Failed to get sources for gap analysis");
                return;
            }
        };

        let existing_queries: HashSet<String> = existing
            .iter()
            .filter(|s| is_web_query(&s.canonical_value))
            .map(|s| s.canonical_value.to_lowercase())
            .collect();

        let mut gap_count = 0u32;
        const MAX_GAP_QUERIES: u32 = 5;

        for t in &tensions {
            if gap_count >= MAX_GAP_QUERIES {
                break;
            }

            let help_text = t.what_would_help.as_deref().unwrap_or(&t.title);
            let query = format!("{} resources services {}", help_text, self.region_slug);
            let query_lower = query.to_lowercase();

            // Skip if we already have a similar query
            if existing_queries
                .iter()
                .any(|q| q.contains(&query_lower) || query_lower.contains(q.as_str()))
            {
                stats.duplicates_skipped += 1;
                continue;
            }

            let cv = query.clone();
            let ck = canonical_value(&cv);

            let weight =
                initial_weight_for_method(DiscoveryMethod::GapAnalysis, Some("unmet_tension"));

            let source = SourceNode::new(
                ck,
                cv,
                None,
                DiscoveryMethod::GapAnalysis,
                weight,
                // Mechanical gap queries seek responses to tensions
                SourceRole::Response,
                Some(format!("Tension: {}", t.title)),
            );
            gap_count += 1;
            stats.gap_sources += 1;
            info!(
                tension = t.title.as_str(),
                query = source.canonical_value.as_str(),
                "Created gap analysis query"
            );
            sources.push(source);
        }

        // Generate social topics from the same tensions — mechanical fallback parity
        const MAX_MECHANICAL_SOCIAL_TOPICS: usize = 3;
        for t in tensions.iter().take(MAX_MECHANICAL_SOCIAL_TOPICS) {
            let help_text = t.what_would_help.as_deref().unwrap_or(&t.title);
            social_topics.push(format!("{} {}", help_text, self.region_name));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Helper builders ---

    fn make_tension(
        title: &str,
        severity: &str,
        what_would_help: Option<&str>,
        unmet: bool,
    ) -> UnmetTension {
        UnmetTension {
            title: title.to_string(),
            severity: severity.to_string(),
            what_would_help: what_would_help.map(|s| s.to_string()),
            category: None,
            unmet,
            corroboration_count: 0,
            source_diversity: 0,
            cause_heat: 0.0,
        }
    }

    fn make_tension_with_engagement(
        title: &str,
        severity: &str,
        what_would_help: Option<&str>,
        unmet: bool,
        corroboration_count: u32,
        source_diversity: u32,
        cause_heat: f64,
    ) -> UnmetTension {
        UnmetTension {
            title: title.to_string(),
            severity: severity.to_string(),
            what_would_help: what_would_help.map(|s| s.to_string()),
            category: None,
            unmet,
            corroboration_count,
            source_diversity,
            cause_heat,
        }
    }

    fn make_source_brief(
        cv: &str,
        signals: u32,
        weight: f64,
        empty_runs: u32,
        context: &str,
        active: bool,
    ) -> SourceBrief {
        SourceBrief {
            canonical_value: cv.to_string(),
            signals_produced: signals,
            weight,
            consecutive_empty_runs: empty_runs,
            gap_context: Some(context.to_string()),
            active,
        }
    }

    fn make_briefing() -> DiscoveryBriefing {
        DiscoveryBriefing {
            tensions: vec![
                make_tension(
                    "Northside food desert growing",
                    "high",
                    Some("grocery co-op, food shelf expansion"),
                    true,
                ),
                make_tension(
                    "Youth mental health crisis",
                    "critical",
                    Some("crisis counselors, peer support"),
                    true,
                ),
                make_tension(
                    "Housing affordability declining",
                    "medium",
                    Some("affordable housing programs"),
                    false,
                ),
            ],
            signal_counts: SignalTypeCounts {
                gatherings: 23,
                aids: 8,
                needs: 15,
                notices: 12,
                tensions: 31,
            },
            successes: vec![make_source_brief(
                "affordable housing programs Minneapolis",
                12,
                0.8,
                0,
                "unmet tension housing",
                true,
            )],
            failures: vec![make_source_brief(
                "youth mentorship programs Minneapolis",
                0,
                0.3,
                10,
                "emerging thread youth services",
                false,
            )],
            existing_queries: vec![
                "affordable housing programs Minneapolis".to_string(),
                "food shelf locations Minneapolis".to_string(),
            ],
            region_name: "Minneapolis".to_string(),
            gap_type_stats: vec![],
            extraction_yield: vec![],
            situations: vec![SituationBrief {
                headline: "Northside food access crisis".to_string(),
                arc: "emerging".to_string(),
                temperature: 0.72,
                clarity: "fuzzy".to_string(),
                signal_count: 8,
                tension_count: 3,
                dispatch_count: 0,
                location_name: Some("North Minneapolis".to_string()),
                sensitivity: "GENERAL".to_string(),
            }],
            response_shapes: vec![],
        }
    }

    // --- A. Briefing Construction & Formatting ---

    #[test]
    fn briefing_format_includes_all_sections() {
        let briefing = make_briefing();
        let prompt = briefing.format_prompt();

        assert!(
            prompt.contains("## UNMET TENSIONS"),
            "Missing UNMET TENSIONS section"
        );
        assert!(
            prompt.contains("## TENSIONS WITH RESPONSES"),
            "Missing TENSIONS WITH RESPONSES section"
        );
        assert!(
            prompt.contains("## SITUATION LANDSCAPE"),
            "Missing SITUATION LANDSCAPE section"
        );
        assert!(
            prompt.contains("## SIGNAL BALANCE"),
            "Missing SIGNAL BALANCE section"
        );
        assert!(
            prompt.contains("## PAST DISCOVERY RESULTS"),
            "Missing PAST DISCOVERY RESULTS section"
        );
        assert!(
            prompt.contains("## EXISTING QUERIES"),
            "Missing EXISTING QUERIES section"
        );

        // Tensions include severity and what_would_help
        assert!(prompt.contains("[HIGH]"), "Missing severity tag");
        assert!(
            prompt.contains("grocery co-op"),
            "Missing what_would_help text"
        );

        // Situations include arc and temperature
        assert!(prompt.contains("arc=emerging"), "Missing situation arc");
        assert!(
            prompt.contains("temp=0.72"),
            "Missing situation temperature"
        );

        // Signal balance line
        assert!(prompt.contains("Gatherings: 23"), "Missing gathering count");
        assert!(prompt.contains("Tensions: 31"), "Missing tension count");

        // Past results
        assert!(prompt.contains("Worked well:"), "Missing successes header");
        assert!(prompt.contains("Didn't work:"), "Missing failures header");
        assert!(
            prompt.contains("12 signals"),
            "Missing success signal count"
        );
        assert!(
            prompt.contains("10 empty runs"),
            "Missing failure empty runs"
        );

        // Existing queries
        assert!(
            prompt.contains("affordable housing programs Minneapolis"),
            "Missing existing query"
        );
    }

    #[test]
    fn briefing_format_under_token_limit() {
        // Build a maximally-full briefing
        let mut briefing = make_briefing();
        for i in 0..10 {
            briefing.tensions.push(make_tension(
                &format!("Tension number {} with a reasonably long title here", i),
                "high",
                Some(&format!(
                    "help text for tension {} that is moderately long",
                    i
                )),
                i % 2 == 0,
            ));
        }
        for i in 0..5 {
            briefing.successes.push(make_source_brief(
                &format!("successful query number {} Minneapolis", i),
                i + 1,
                0.5 + i as f64 * 0.1,
                0,
                &format!("reason for success {}", i),
                true,
            ));
        }
        for i in 0..5 {
            briefing.failures.push(make_source_brief(
                &format!("failed query number {} Minneapolis", i),
                0,
                0.2,
                i + 3,
                &format!("reason for failure {}", i),
                false,
            ));
        }
        for i in 0..15 {
            briefing
                .existing_queries
                .push(format!("existing query {} Minneapolis", i));
        }

        let prompt = briefing.format_prompt();
        // ~4K tokens ≈ 16K chars — ensure we stay within Haiku's comfort zone
        assert!(
            prompt.len() < 16_000,
            "Briefing prompt is {} chars, expected < 16,000",
            prompt.len()
        );
    }

    #[test]
    fn briefing_cold_start_with_no_tensions() {
        let briefing = DiscoveryBriefing {
            tensions: vec![],
            situations: vec![],
            signal_counts: SignalTypeCounts::default(),
            successes: vec![],
            failures: vec![],
            existing_queries: vec![],
            region_name: "Minneapolis".to_string(),
            gap_type_stats: vec![],
            extraction_yield: vec![],
            response_shapes: vec![],
        };
        assert!(briefing.is_cold_start());
    }

    #[test]
    fn briefing_cold_start_with_few_tensions() {
        let briefing = DiscoveryBriefing {
            tensions: vec![
                make_tension("Tension 1", "low", None, true),
                make_tension("Tension 2", "medium", None, true),
            ],
            situations: vec![],
            signal_counts: SignalTypeCounts::default(),
            successes: vec![],
            failures: vec![],
            existing_queries: vec![],
            region_name: "Minneapolis".to_string(),
            gap_type_stats: vec![],
            extraction_yield: vec![],
            response_shapes: vec![],
        };
        assert!(
            briefing.is_cold_start(),
            "2 tensions + 0 situations should be cold start"
        );
    }

    #[test]
    fn briefing_not_cold_start_with_enough_data() {
        let briefing = DiscoveryBriefing {
            tensions: vec![
                make_tension("T1", "low", None, true),
                make_tension("T2", "medium", None, true),
                make_tension("T3", "high", None, true),
                make_tension("T4", "high", None, false),
                make_tension("T5", "critical", None, true),
            ],
            situations: vec![],
            signal_counts: SignalTypeCounts::default(),
            successes: vec![],
            failures: vec![],
            existing_queries: vec![],
            region_name: "Minneapolis".to_string(),
            gap_type_stats: vec![],
            extraction_yield: vec![],
            response_shapes: vec![],
        };
        assert!(!briefing.is_cold_start());
    }

    // --- B. Feedback Loop (Critical) ---

    #[test]
    fn briefing_surfaces_successful_discoveries() {
        let briefing = DiscoveryBriefing {
            tensions: vec![make_tension("T1", "high", None, true); 3],
            situations: vec![],
            signal_counts: SignalTypeCounts::default(),
            successes: vec![
                make_source_brief(
                    "affordable housing programs Minneapolis",
                    12,
                    0.8,
                    0,
                    "Curiosity: unmet tension food insecurity | Gap: unmet_tension | Related: food desert",
                    true,
                ),
                make_source_brief(
                    "community health clinics Northside",
                    7,
                    0.6,
                    0,
                    "Curiosity: gap in health coverage | Gap: unmet_tension | Related: health access",
                    true,
                ),
            ],
            failures: vec![],
            existing_queries: vec![],
            region_name: "Minneapolis".to_string(),
            gap_type_stats: vec![],
            extraction_yield: vec![],
            response_shapes: vec![],
        };
        let prompt = briefing.format_prompt();
        assert!(prompt.contains("affordable housing programs Minneapolis"));
        assert!(prompt.contains("12 signals"));
        assert!(prompt.contains("weight 0.8"));
        assert!(prompt.contains("unmet tension food insecurity"));
    }

    #[test]
    fn briefing_surfaces_failed_discoveries() {
        let briefing = DiscoveryBriefing {
            tensions: vec![make_tension("T1", "high", None, true); 3],
            situations: vec![],
            signal_counts: SignalTypeCounts::default(),
            successes: vec![],
            failures: vec![make_source_brief(
                "youth mentorship programs Minneapolis",
                0,
                0.3,
                10,
                "Curiosity: emerging thread youth services | Gap: emerging_thread",
                false,
            )],
            existing_queries: vec![],
            region_name: "Minneapolis".to_string(),
            gap_type_stats: vec![],
            extraction_yield: vec![],
            response_shapes: vec![],
        };
        let prompt = briefing.format_prompt();
        assert!(prompt.contains("youth mentorship programs Minneapolis"));
        assert!(prompt.contains("0 signals"));
        assert!(prompt.contains("10 empty runs"));
        assert!(prompt.contains("emerging thread youth services"));
    }

    #[test]
    fn briefing_separates_successes_from_failures() {
        let briefing = DiscoveryBriefing {
            tensions: vec![make_tension("T1", "high", None, true); 3],
            situations: vec![],
            signal_counts: SignalTypeCounts::default(),
            successes: vec![make_source_brief("good query", 10, 0.7, 0, "worked", true)],
            failures: vec![make_source_brief("bad query", 0, 0.2, 8, "failed", false)],
            existing_queries: vec![],
            region_name: "Minneapolis".to_string(),
            gap_type_stats: vec![],
            extraction_yield: vec![],
            response_shapes: vec![],
        };
        let prompt = briefing.format_prompt();

        // Find positions of sections
        let worked_pos = prompt.find("Worked well:").expect("Missing Worked well");
        let didnt_pos = prompt.find("Didn't work:").expect("Missing Didn't work");
        let good_pos = prompt.find("good query").expect("Missing good query");
        let bad_pos = prompt.find("bad query").expect("Missing bad query");

        // Good query appears after "Worked well" and before "Didn't work"
        assert!(
            good_pos > worked_pos && good_pos < didnt_pos,
            "Success in wrong section"
        );
        // Bad query appears after "Didn't work"
        assert!(bad_pos > didnt_pos, "Failure in wrong section");
    }

    #[test]
    fn briefing_includes_unmet_vs_met_tension_distinction() {
        let briefing = DiscoveryBriefing {
            tensions: vec![
                make_tension("Unmet tension A", "high", Some("help A"), true),
                make_tension("Met tension B", "medium", Some("help B"), false),
            ],
            situations: vec![],
            signal_counts: SignalTypeCounts::default(),
            successes: vec![],
            failures: vec![],
            existing_queries: vec![],
            region_name: "Minneapolis".to_string(),
            gap_type_stats: vec![],
            extraction_yield: vec![],
            response_shapes: vec![],
        };
        let prompt = briefing.format_prompt();

        assert!(
            prompt.contains("## UNMET TENSIONS"),
            "Missing UNMET section"
        );
        assert!(
            prompt.contains("## TENSIONS WITH RESPONSES"),
            "Missing RESPONDED section"
        );

        // Unmet tensions in the UNMET section
        let unmet_pos = prompt.find("## UNMET TENSIONS").unwrap();
        let met_pos = prompt.find("## TENSIONS WITH RESPONSES").unwrap();
        let unmet_a_pos = prompt.find("Unmet tension A").unwrap();
        let met_b_pos = prompt.find("Met tension B").unwrap();

        assert!(
            unmet_a_pos > unmet_pos && unmet_a_pos < met_pos,
            "Unmet tension in wrong section"
        );
        assert!(met_b_pos > met_pos, "Met tension in wrong section");
    }

    #[test]
    fn briefing_signal_imbalance_annotation() {
        let briefing = DiscoveryBriefing {
            tensions: vec![make_tension("T1", "high", None, true); 3],
            situations: vec![],
            signal_counts: SignalTypeCounts {
                gatherings: 10,
                aids: 3,
                needs: 12,
                notices: 8,
                tensions: 31,
            },
            successes: vec![],
            failures: vec![],
            existing_queries: vec![],
            region_name: "Minneapolis".to_string(),
            gap_type_stats: vec![],
            extraction_yield: vec![],
            response_shapes: vec![],
        };
        let prompt = briefing.format_prompt();
        assert!(
            prompt.contains("Aid signals significantly underrepresented"),
            "Missing imbalance annotation. Prompt:\n{prompt}"
        );
    }

    // --- C. Discovery Plan Deserialization ---

    #[test]
    fn discovery_plan_deserializes_valid_json() {
        let json = r#"{"queries": [
            {"query": "mutual aid Minneapolis", "reasoning": "unmet tension", "gap_type": "unmet_tension", "related_tension": "food desert"},
            {"query": "tenant rights org Minneapolis", "reasoning": "low type diversity", "gap_type": "low_type_diversity", "related_tension": null}
        ]}"#;
        let plan: DiscoveryPlan = serde_json::from_str(json).unwrap();
        assert_eq!(plan.queries.len(), 2);
        assert_eq!(plan.queries[0].query, "mutual aid Minneapolis");
        assert!(plan.queries[0].related_tension.is_some());
        assert!(plan.queries[1].related_tension.is_none());
        assert!(
            plan.social_topics.is_empty(),
            "Missing social_topics should default to empty"
        );
    }

    #[test]
    fn discovery_plan_handles_null_queries() {
        let json = r#"{"queries": null}"#;
        let plan: DiscoveryPlan = serde_json::from_str(json).unwrap();
        assert!(plan.queries.is_empty());
        assert!(plan.social_topics.is_empty());
    }

    #[test]
    fn discovery_plan_deserializes_social_topics() {
        let json = r#"{
            "queries": [],
            "social_topics": [
                {"topic": "MNimmigration", "reasoning": "find immigration advocates"},
                {"topic": "sanctuary city Minneapolis", "reasoning": "find volunteer organizers"}
            ]
        }"#;
        let plan: DiscoveryPlan = serde_json::from_str(json).unwrap();
        assert_eq!(plan.social_topics.len(), 2);
        assert_eq!(plan.social_topics[0].topic, "MNimmigration");
        assert_eq!(plan.social_topics[1].topic, "sanctuary city Minneapolis");
    }

    #[test]
    fn discovery_plan_social_topics_default_when_missing() {
        let json = r#"{"queries": [
            {"query": "test", "reasoning": "r", "gap_type": "unmet_tension", "related_tension": null}
        ]}"#;
        let plan: DiscoveryPlan = serde_json::from_str(json).unwrap();
        assert_eq!(plan.queries.len(), 1);
        assert!(
            plan.social_topics.is_empty(),
            "Missing social_topics field should default to empty vec"
        );
    }

    #[test]
    fn discovery_plan_handles_stringified_array() {
        let json = r#"{"queries": "[{\"query\": \"test\", \"reasoning\": \"r\", \"gap_type\": \"unmet_tension\", \"related_tension\": null}]"}"#;
        let plan: DiscoveryPlan = serde_json::from_str(json).unwrap();
        assert_eq!(plan.queries.len(), 1);
    }

    // --- D. Source Creation from Plan ---

    #[test]
    fn gap_context_captures_llm_reasoning() {
        let reasoning = "unmet tension: food insecurity needs resources";
        let gap_type = "unmet_tension";
        let related = Some("Northside food desert");
        let context = format!(
            "Curiosity: {} | Gap: {} | Related: {}",
            reasoning,
            gap_type,
            related.unwrap_or("none"),
        );
        assert!(context.contains("food insecurity"));
        assert!(context.contains("unmet_tension"));
        assert!(context.contains("Northside food desert"));
    }

    #[test]
    fn max_queries_capped_at_limit() {
        let queries: Vec<DiscoveryQuery> = (0..20)
            .map(|i| DiscoveryQuery {
                query: format!("test query {i}"),
                reasoning: "test".to_string(),
                gap_type: "unmet_tension".to_string(),
                related_tension: None,
            })
            .collect();
        assert_eq!(
            queries.into_iter().take(MAX_CURIOSITY_QUERIES).count(),
            MAX_CURIOSITY_QUERIES
        );
    }

    #[test]
    fn dedup_catches_substring_matches() {
        let existing: HashSet<String> = ["affordable housing minneapolis".to_string()].into();
        let new_query = "affordable housing minneapolis programs";
        let is_dup = existing
            .iter()
            .any(|q| q.contains(new_query) || new_query.contains(q.as_str()));
        assert!(is_dup, "Substring dedup should catch this");
    }

    #[test]
    fn dedup_allows_novel_queries() {
        let existing: HashSet<String> = ["affordable housing minneapolis".to_string()].into();
        let new_query = "tenant rights legal aid minneapolis";
        let is_dup = existing
            .iter()
            .any(|q| q.contains(new_query) || new_query.contains(q.as_str()));
        assert!(!is_dup, "Novel query should pass dedup");
    }

    // --- D2. Engagement-Aware Discovery ---

    #[test]
    fn briefing_engagement_shown_for_tensions() {
        let mut briefing = make_briefing();
        briefing.tensions = vec![
            make_tension_with_engagement(
                "Food desert",
                "high",
                Some("grocery co-op"),
                true,
                3,
                2,
                0.7,
            ),
            make_tension_with_engagement(
                "Housing crisis",
                "medium",
                Some("housing programs"),
                false,
                1,
                1,
                0.3,
            ),
        ];
        let prompt = briefing.format_prompt();
        assert!(
            prompt.contains("community attention: 2 sources, 3 corroborations, heat=0.7"),
            "Missing engagement line for unmet tension. Prompt:\n{prompt}"
        );
        assert!(
            prompt.contains("community attention: 1 sources, 1 corroborations, heat=0.3"),
            "Missing engagement line for met tension. Prompt:\n{prompt}"
        );
    }

    #[test]
    fn briefing_engagement_zero_still_shown() {
        let mut briefing = make_briefing();
        briefing.tensions = vec![make_tension(
            "Novel early signal",
            "low",
            Some("unknown"),
            true,
        )];
        let prompt = briefing.format_prompt();
        assert!(
            prompt.contains("Novel early signal"),
            "Zero-engagement tension should still appear"
        );
        assert!(
            prompt.contains("community attention: 0 sources, 0 corroborations, heat=0.0"),
            "Zero engagement should still show engagement line. Prompt:\n{prompt}"
        );
    }

    #[test]
    fn briefing_high_engagement_tensions_first() {
        let mut briefing = make_briefing();
        briefing.tensions = vec![
            make_tension_with_engagement("Low engagement", "high", Some("help"), true, 0, 0, 0.0),
            make_tension_with_engagement("High engagement", "high", Some("help"), true, 5, 3, 0.8),
            make_tension_with_engagement(
                "Medium engagement",
                "high",
                Some("help"),
                true,
                2,
                1,
                0.4,
            ),
        ];
        let prompt = briefing.format_prompt();
        // All three should appear (no gating)
        assert!(
            prompt.contains("Low engagement"),
            "Low engagement tension missing"
        );
        assert!(
            prompt.contains("High engagement"),
            "High engagement tension missing"
        );
        assert!(
            prompt.contains("Medium engagement"),
            "Medium engagement tension missing"
        );
    }

    #[test]
    fn mechanical_fallback_sorts_by_engagement() {
        // Simulate the sort logic used in discover_from_gaps_mechanical
        let mut tensions = vec![
            make_tension_with_engagement("Low", "high", Some("help"), true, 0, 0, 0.0),
            make_tension_with_engagement("High", "high", Some("help"), true, 5, 3, 0.8),
            make_tension_with_engagement("Medium", "high", Some("help"), true, 2, 1, 0.4),
        ];
        tensions.sort_by(|a, b| {
            let score_a =
                a.corroboration_count as f64 + a.source_diversity as f64 + a.cause_heat * 10.0;
            let score_b =
                b.corroboration_count as f64 + b.source_diversity as f64 + b.cause_heat * 10.0;
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        assert_eq!(
            tensions[0].title, "High",
            "Highest engagement should sort first"
        );
        assert_eq!(
            tensions[1].title, "Medium",
            "Medium engagement should sort second"
        );
        assert_eq!(
            tensions[2].title, "Low",
            "Low engagement should sort last but still present"
        );
    }

    #[test]
    fn cold_start_ignores_engagement() {
        // Cold start: < 3 tensions, no situations — mechanical fallback runs regardless of engagement
        let briefing = DiscoveryBriefing {
            tensions: vec![make_tension_with_engagement(
                "Only tension",
                "high",
                Some("help"),
                true,
                0,
                0,
                0.0,
            )],
            situations: vec![],
            signal_counts: SignalTypeCounts::default(),
            successes: vec![],
            failures: vec![],
            existing_queries: vec![],
            region_name: "Minneapolis".to_string(),
            gap_type_stats: vec![],
            extraction_yield: vec![],
            response_shapes: vec![],
        };
        assert!(
            briefing.is_cold_start(),
            "Should be cold start with 1 tension"
        );
        // Zero-engagement tension is still present — no gating
        assert_eq!(briefing.tensions.len(), 1);
        assert_eq!(briefing.tensions[0].corroboration_count, 0);
    }

    // --- E. Degradation Path ---

    #[test]
    fn budget_constant_exists() {
        assert_eq!(OperationCost::CLAUDE_HAIKU_DISCOVERY, 1);
    }

    #[test]
    fn exhausted_budget_blocks_discovery() {
        let tracker = BudgetTracker::new(1);
        tracker.spend(1);
        assert!(!tracker.has_budget(OperationCost::CLAUDE_HAIKU_DISCOVERY));
    }

    #[test]
    fn unlimited_budget_allows_discovery() {
        let tracker = BudgetTracker::new(0);
        assert!(tracker.has_budget(OperationCost::CLAUDE_HAIKU_DISCOVERY));
    }

    // --- F. Feedback Loop: Strategy Performance ---

    #[test]
    fn briefing_format_includes_strategy_performance() {
        let mut briefing = make_briefing();
        briefing.gap_type_stats = vec![
            GapTypeStats {
                gap_type: "unmet_tension".to_string(),
                total_sources: 10,
                successful_sources: 7,
                avg_weight: 0.65,
            },
            GapTypeStats {
                gap_type: "novel_angle".to_string(),
                total_sources: 5,
                successful_sources: 1,
                avg_weight: 0.20,
            },
        ];
        let prompt = briefing.format_prompt();
        assert!(
            prompt.contains("## STRATEGY PERFORMANCE"),
            "Missing STRATEGY PERFORMANCE section"
        );
        assert!(
            prompt.contains("unmet_tension: 7/10 sources successful (70%)"),
            "Missing unmet_tension stats"
        );
        assert!(
            prompt.contains("novel_angle: 1/5 sources successful (20%)"),
            "Missing novel_angle stats"
        );
    }

    #[test]
    fn briefing_strategy_warns_on_zero_success() {
        let mut briefing = make_briefing();
        briefing.gap_type_stats = vec![GapTypeStats {
            gap_type: "novel_angle".to_string(),
            total_sources: 5,
            successful_sources: 0,
            avg_weight: 0.20,
        }];
        let prompt = briefing.format_prompt();
        assert!(
            prompt.contains(
                "WARNING: \"novel_angle\" strategy has 0% success rate across 5 attempts"
            ),
            "Missing zero-success warning. Prompt:\n{prompt}"
        );
    }

    // --- G. Feedback Loop: Extraction Yield ---

    #[test]
    fn briefing_format_includes_extraction_yield() {
        let mut briefing = make_briefing();
        briefing.extraction_yield = vec![
            ExtractionYield {
                source_label: "web".to_string(),
                extracted: 142,
                survived: 118,
                corroborated: 23,
                contradicted: 2,
            },
            ExtractionYield {
                source_label: "web_query".to_string(),
                extracted: 67,
                survived: 31,
                corroborated: 4,
                contradicted: 8,
            },
        ];
        let prompt = briefing.format_prompt();
        assert!(
            prompt.contains("## EXTRACTION YIELD"),
            "Missing EXTRACTION YIELD section"
        );
        assert!(
            prompt.contains("web: 142 extracted, 118 survived (83%)"),
            "Missing web yield stats"
        );
        assert!(
            prompt.contains("web_query: 67 extracted, 31 survived (46%)"),
            "Missing web_query yield stats"
        );
    }

    #[test]
    fn briefing_extraction_yield_annotates_low_survival() {
        let mut briefing = make_briefing();
        briefing.extraction_yield = vec![ExtractionYield {
            source_label: "web_query".to_string(),
            extracted: 67,
            survived: 31,
            corroborated: 4,
            contradicted: 0,
        }];
        let prompt = briefing.format_prompt();
        assert!(
            prompt.contains("web_query survival rate below 50%"),
            "Missing low survival annotation. Prompt:\n{prompt}"
        );
    }

    #[test]
    fn briefing_extraction_yield_annotates_high_contradiction() {
        let mut briefing = make_briefing();
        briefing.extraction_yield = vec![ExtractionYield {
            source_label: "web_query".to_string(),
            extracted: 50,
            survived: 40,
            corroborated: 4,
            contradicted: 15,
        }];
        let prompt = briefing.format_prompt();
        assert!(
            prompt.contains("web_query has high contradiction rate (30%)"),
            "Missing high contradiction annotation. Prompt:\n{prompt}"
        );
    }

    // --- H. Response Shape ---

    #[test]
    fn briefing_format_includes_response_shape() {
        let mut briefing = make_briefing();
        briefing.response_shapes = vec![TensionResponseShape {
            title: "Immigration Enforcement Fear".to_string(),
            what_would_help: Some(
                "legal defense, emergency housing, mental health support".to_string(),
            ),
            cause_heat: 0.8,
            aid_count: 3,
            gathering_count: 2,
            need_count: 1,
            sample_titles: vec![
                "ILCM Legal Clinic".to_string(),
                "Know Your Rights Workshop".to_string(),
                "ICE Rapid Response Fund".to_string(),
            ],
        }];
        let prompt = briefing.format_prompt();
        assert!(
            prompt.contains("## RESPONSE SHAPE"),
            "Missing RESPONSE SHAPE section"
        );
        assert!(
            prompt.contains("Immigration Enforcement Fear"),
            "Missing tension title"
        );
        assert!(prompt.contains("heat: 0.8"), "Missing cause heat");
        assert!(
            prompt.contains("Aids: 3, Gatherings: 2, Needs: 1"),
            "Missing response counts"
        );
        assert!(prompt.contains("ILCM Legal Clinic"), "Missing sample title");
        assert!(
            prompt.contains("legal defense, emergency housing"),
            "Missing what_would_help"
        );
    }

    #[test]
    fn briefing_empty_response_shapes_omits_section() {
        let briefing = make_briefing();
        let prompt = briefing.format_prompt();
        assert!(
            !prompt.contains("## RESPONSE SHAPE"),
            "Empty response_shapes should not produce RESPONSE SHAPE section"
        );
    }

    // --- I. Method-Based Initial Weight ---

    #[test]
    fn initial_weight_cold_start() {
        let w = initial_weight_for_method(DiscoveryMethod::Curated, None);
        assert!((w - 0.5).abs() < f64::EPSILON, "Curated should be 0.5: {w}");
        let w = initial_weight_for_method(DiscoveryMethod::HumanSubmission, None);
        assert!(
            (w - 0.5).abs() < f64::EPSILON,
            "HumanSubmission should be 0.5: {w}"
        );
    }

    #[test]
    fn initial_weight_gap_analysis_unmet() {
        let w = initial_weight_for_method(DiscoveryMethod::GapAnalysis, Some("unmet_tension"));
        assert!(
            (w - 0.4).abs() < f64::EPSILON,
            "GapAnalysis+unmet should be 0.4: {w}"
        );
    }

    #[test]
    fn initial_weight_gap_analysis_other() {
        let w = initial_weight_for_method(DiscoveryMethod::GapAnalysis, Some("novel_angle"));
        assert!(
            (w - 0.3).abs() < f64::EPSILON,
            "GapAnalysis+other should be 0.3: {w}"
        );
    }

    #[test]
    fn initial_weight_signal_expansion() {
        let w = initial_weight_for_method(DiscoveryMethod::SignalExpansion, None);
        assert!(
            (w - 0.2).abs() < f64::EPSILON,
            "SignalExpansion should be 0.2: {w}"
        );
    }

    #[test]
    fn initial_weight_hashtag_discovery() {
        let w = initial_weight_for_method(DiscoveryMethod::HashtagDiscovery, None);
        assert!(
            (w - 0.3).abs() < f64::EPSILON,
            "HashtagDiscovery should be 0.3: {w}"
        );
    }
}

