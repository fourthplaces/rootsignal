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

use rootsignal_common::{
    canonical_value, AidNode, DiscoveryMethod, GatheringNode, GeoPoint, GeoPrecision, NeedNode, Node,
    NodeMeta, NodeType, ScoutScope, SensitivityLevel, SourceNode, SourceRole, Urgency,
};
use rootsignal_graph::{GatheringFinderTarget, GraphWriter, ResponseHeuristic};

use rootsignal_archive::Archive;

use crate::infra::embedder::TextEmbedder;
use crate::discovery::agent_tools::{ReadPageTool, WebSearchTool};

const HAIKU_MODEL: &str = "claude-haiku-4-5-20251001";
const MAX_GRAVITY_TARGETS_PER_RUN: usize = 5;
const MAX_TOOL_TURNS: usize = 10;
const MAX_GATHERINGS_PER_TENSION: usize = 8;
const MAX_FUTURE_QUERIES_PER_TENSION: usize = 3;

// =============================================================================
// Structured output types
// =============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GravityFinding {
    /// True if investigation found no evidence of gatherings
    #[serde(default)]
    pub no_gravity: bool,
    /// Why the LLM stopped early (if no_gravity is true)
    pub no_gravity_reason: Option<String>,
    #[serde(default)]
    pub gatherings: Vec<DiscoveredGathering>,
    #[serde(default)]
    pub future_queries: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DiscoveredGathering {
    pub title: String,
    pub summary: String,
    /// "gathering", "aid", or "need"
    pub signal_type: String,
    /// Must be a URL the agent actually read via read_page
    pub url: String,
    /// Freeform: "vigil", "singing", "solidarity meal", "cleanup", etc.
    pub gathering_type: String,
    /// Where people gather (church name, park, community center, etc.)
    pub venue: Option<String>,
    /// Recurring gatherings signal sustained community formation
    #[serde(default)]
    pub is_recurring: bool,
    /// Who is creating the gravitational center
    pub organizer: Option<String>,
    /// How this tension creates this gathering
    pub explanation: String,
    /// 0.0-1.0 how directly this relates to the tension
    pub match_strength: f64,
    /// Titles of OTHER tensions this gathering also addresses
    #[serde(default)]
    pub also_addresses: Vec<String>,
    /// ISO date for events (null if not an event or date unknown)
    pub event_date: Option<String>,
}

// =============================================================================
// Stats
// =============================================================================

#[derive(Debug, Default)]
pub struct GatheringFinderStats {
    pub targets_found: u32,
    pub targets_investigated: u32,
    pub targets_no_gravity: u32,
    pub gatherings_discovered: u32,
    pub gatherings_deduped: u32,
    pub signals_created: u32,
    pub edges_created: u32,
    pub future_sources_created: u32,
}

impl std::fmt::Display for GatheringFinderStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Gathering finder: {} targets found, {} investigated, \
             {} no-gravity, {} gatherings discovered ({} deduped), \
             {} signals created, {} edges, {} future sources",
            self.targets_found,
            self.targets_investigated,
            self.targets_no_gravity,
            self.gatherings_discovered,
            self.gatherings_deduped,
            self.signals_created,
            self.edges_created,
            self.future_sources_created,
        )
    }
}

// =============================================================================
// Prompts
// =============================================================================

fn investigation_system_prompt(city_name: &str) -> String {
    format!(
        "You are investigating where people are GATHERING around a community tension \
in {city_name}. Tension creates gravity — it pulls people together. Your job \
is to find where that gravitational pull is manifesting.

You have two tools: web_search and read_page.

WHAT YOU'RE LOOKING FOR:
You are NOT looking for organizations that solve the problem. You are looking \
for places where PEOPLE ARE SHOWING UP — physically, emotionally, creatively — \
in response to this tension.

Tension creates many kinds of gravity. Examples across different domains:

SOLIDARITY & IDENTITY:
- Singing events, vigils, marches where people show up for each other
- Community meetings where people process what's happening together
- Cultural events responding to the tension (art, music, theater, murals)

ENVIRONMENTAL & SAFETY:
- Community cleanups after a pollution spill or environmental disaster
- Neighborhood watch formations after a safety crisis
- Town halls where residents organize around contamination, flooding, noise
- Volunteer mobilizations after storms, fires, infrastructure failures

ECONOMIC & HOUSING:
- Tenant meetups, renter solidarity gatherings
- Community swap meets, mutual aid distribution events
- Neighborhood potlucks organized because people are struggling

HEALTH & WELLBEING:
- Support circles, healing spaces, grief groups
- Community fitness or wellness events in response to mental health crises
- Peer support gatherings for affected populations

CIVIC & DEMOCRATIC:
- Packed school board meetings, city council overflows
- Petition drives, signature-gathering events
- Citizen journalism meetups, community reporting efforts

DIGITAL:
- Instagram accounts documenting community response, organizing events
- Facebook groups, Reddit threads forming around the tension
- Crowdfunding and donation campaigns (GoFundMe, GiveSendGo, org donation pages, \
Venmo/CashApp mutual aid funds) that become community rallying points
- Nextdoor threads where neighbors are organizing

THE KEY DISTINCTION: An instrumental response SOLVES a problem. Gravity \
is where people COME TOGETHER because of a problem. A legal clinic solves \
a housing issue. A tenant potluck where neighbors share stories IS the \
community forming around the pressure. You want the second kind.

HOW TO INVESTIGATE:
1. Search for the tension + gathering language: \"community meeting\", \
\"solidarity\", \"volunteer\", \"gathering\", \"coming together\", \"rally\", \
\"town hall\", \"cleanup\", \"meetup\", \"support group\"
2. Search for venues: community centers, parks, schools, libraries, \
houses of worship, rec centers + the tension topic
3. Check Instagram, Facebook events, Nextdoor, local news, community boards
4. Read articles about community response — they often mention specific gatherings
5. Follow threads: one gathering often links to others in the same movement
6. Search for recurring gatherings (weekly meetups, monthly dinners, standing events)

IMPORTANT CONSTRAINTS:
- URLs must be pages you actually read via read_page
- For events: verify dates. Only extract current/future gatherings.
- Note whether gatherings are one-time or recurring — recurring ones are \
especially valuable as they indicate sustained community formation.
- If you find a gathering that responds to MULTIPLE tensions, note all of them.

EARLY TERMINATION:
After your first 2-3 searches, if you find NO evidence of gatherings, \
community events, vigils, solidarity actions, or people coming together \
around this tension — stop. Not every tension creates gravity. Report \
no_gravity=true with your reasoning. This saves budget for tensions \
that DO pull people together.

SIGNAL TYPE SEMANTICS:
- If the gathering is a physical or virtual GATHERING: provide venue, event_date, \
is_recurring, organizer.
- If the gravity manifests as Aid (e.g., a solidarity fund) or Need \
(e.g., a call to action): leave venue and event_date null. These are \
gravity expressions that don't have a physical location or date.

For each gathering, note: the URL, what type it is (gathering/aid/need), \
the venue or location if known, whether it's recurring, the organizer if known, \
and how it relates to the tension (what kind of gravity it represents)."
    )
}

fn investigation_user_prompt(
    target: &GatheringFinderTarget,
    existing: &[ResponseHeuristic],
) -> String {
    let mut prompt = format!(
        "TENSION: {}\nSeverity: {}\nSummary: {}\nCause heat: {:.2}",
        target.title, target.severity, target.summary, target.cause_heat,
    );

    if let Some(ref wwh) = target.what_would_help {
        prompt.push_str(&format!("\nWhat would help: {wwh}"));
    }
    if let Some(ref cat) = target.category {
        prompt.push_str(&format!("\nCategory: {cat}"));
    }

    if !existing.is_empty() {
        prompt.push_str(
            "\n\nKNOWN GATHERINGS (already discovered — look for NEW ones not in this list):",
        );
        for g in existing {
            prompt.push_str(&format!(
                "\n- [{}] {}: {}",
                g.signal_type, g.title, g.summary,
            ));
        }
    }

    prompt
}

const STRUCTURING_SYSTEM: &str = "\
Based on your investigation, extract your findings as JSON.

If you found NO evidence of gatherings after your initial searches, set:
- no_gravity: true
- no_gravity_reason: brief explanation of why (e.g. \"no community events found after 3 searches\")
- gatherings: [] (empty)

Otherwise, for each gathering you discovered:
- title: short name of the gathering
- summary: 1-2 sentences about what happens there
- signal_type: \"gathering\" (physical/virtual gatherings where people come together), \"aid\" (free resources like solidarity funds, mutual aid), or \"need\" (direct expressions of need — e.g. GoFundMe campaigns, volunteer signups, petition drives)
- url: the EXACT URL you read via read_page (do not reconstruct or guess)
- gathering_type: freeform category (e.g. \"vigil\", \"singing\", \"solidarity meal\", \"tenant meetup\", \"cleanup\")
- venue: where people gather (church name, park, community center) — null if not applicable
- is_recurring: true if this is a recurring gathering (weekly, monthly, etc.)
- organizer: who creates the gravitational center — null if unknown
- explanation: how this tension creates this gathering (the gravity relationship)
- match_strength: 0.0-1.0 how directly this relates to the tension
- also_addresses: titles of OTHER community tensions this gathering also addresses (empty if none)
- event_date: ISO date if this is an event (null if not an event or date unknown)

Also report:
- future_queries: search queries that could find MORE gatherings (threads you couldn't fully explore)

Return valid JSON matching the GravityFinding schema.";

// =============================================================================
// GatheringFinder
// =============================================================================

pub struct GatheringFinder<'a> {
    writer: &'a GraphWriter,
    claude: Claude,
    embedder: &'a dyn TextEmbedder,
    region: ScoutScope,
    region_slug: String,
    min_lat: f64,
    max_lat: f64,
    min_lng: f64,
    max_lng: f64,
    cancelled: Arc<AtomicBool>,
    run_id: String,
}

impl<'a> GatheringFinder<'a> {
    pub fn new(
        writer: &'a GraphWriter,
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
            })
            .tool(ReadPageTool {
                archive: archive.clone(),
                visited_urls: None,
            });

        let lat_delta = region.radius_km / 111.0;
        let lng_delta = region.radius_km / (111.0 * region.center_lat.to_radians().cos());
        let region_slug = region.name.clone();
        Self {
            writer,
            claude,
            embedder,
            min_lat: region.center_lat - lat_delta,
            max_lat: region.center_lat + lat_delta,
            min_lng: region.center_lng - lng_delta,
            max_lng: region.center_lng + lng_delta,
            region,
            region_slug,
            cancelled,
            run_id,
        }
    }

    pub async fn run(&self) -> GatheringFinderStats {
        let mut stats = GatheringFinderStats::default();

        let targets = match self
            .writer
            .find_gathering_finder_targets(
                MAX_GRAVITY_TARGETS_PER_RUN as u32,
                self.min_lat,
                self.max_lat,
                self.min_lng,
                self.max_lng,
            )
            .await
        {
            Ok(t) => t,
            Err(e) => {
                warn!(error = %e, "Failed to find gathering finder targets");
                return stats;
            }
        };

        stats.targets_found = targets.len() as u32;
        if targets.is_empty() {
            info!("No gathering finder targets found");
            return stats;
        }

        info!(count = targets.len(), "Gathering finder targets selected");

        for target in &targets {
            if self.cancelled.load(Ordering::Relaxed) {
                info!("Gathering finder cancelled");
                break;
            }

            let found_gatherings = match self.investigate_tension(target, &mut stats).await {
                Ok(found) => {
                    stats.targets_investigated += 1;
                    found
                }
                Err(e) => {
                    warn!(
                        tension_id = %target.tension_id,
                        title = target.title.as_str(),
                        error = %e,
                        "Gathering finder investigation failed"
                    );
                    false
                }
            };

            // Mark scouted with backoff (success resets miss count, failure increments)
            if let Err(e) = self
                .writer
                .mark_gathering_found(target.tension_id, found_gatherings)
                .await
            {
                warn!(
                    tension_id = %target.tension_id,
                    error = %e,
                    "Failed to mark tension as gravity-scouted"
                );
            }
        }

        stats
    }

    /// Investigate a single tension for gatherings. Returns true if gatherings were found.
    async fn investigate_tension(
        &self,
        target: &GatheringFinderTarget,
        stats: &mut GatheringFinderStats,
    ) -> Result<bool> {
        // Fetch existing gravity signals for context
        let existing = self
            .writer
            .get_existing_gathering_signals(
                target.tension_id,
                self.region.center_lat,
                self.region.center_lng,
                self.region.radius_km,
            )
            .await
            .unwrap_or_default();

        let system = investigation_system_prompt(&self.region.name);
        let user = investigation_user_prompt(target, &existing);

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
            "Tension investigated: {} — {}\n\nInvestigation findings:\n{}",
            target.title, target.summary, reasoning,
        );

        let finding: GravityFinding = self
            .claude
            .extract(HAIKU_MODEL, STRUCTURING_SYSTEM, &structuring_user)
            .await?;

        // Handle no_gravity early termination
        if finding.no_gravity {
            info!(
                tension_id = %target.tension_id,
                title = target.title.as_str(),
                reason = finding.no_gravity_reason.as_deref().unwrap_or("unknown"),
                "No gravity found — early termination"
            );
            stats.targets_no_gravity += 1;

            // Still create future queries even on no_gravity
            for query in finding
                .future_queries
                .iter()
                .take(MAX_FUTURE_QUERIES_PER_TENSION)
            {
                if let Err(e) = self.create_future_query(query, target, stats).await {
                    warn!(query = query.as_str(), error = %e, "Failed to create future query source");
                }
            }

            return Ok(false);
        }

        // Handle contradiction: no_gravity=false but check if gatherings is empty
        if finding.gatherings.is_empty() {
            info!(
                tension_id = %target.tension_id,
                title = target.title.as_str(),
                "Investigation complete but no gatherings extracted"
            );
            return Ok(false);
        }

        stats.gatherings_discovered += finding.gatherings.len() as u32;

        // Process discovered gatherings
        for gathering in finding
            .gatherings
            .into_iter()
            .take(MAX_GATHERINGS_PER_TENSION)
        {
            if let Err(e) = self.process_gathering(target, &gathering, stats).await {
                warn!(
                    tension_id = %target.tension_id,
                    gathering_title = gathering.title.as_str(),
                    error = %e,
                    "Failed to process discovered gathering"
                );
            }
        }

        // Create future query sources
        for query in finding
            .future_queries
            .iter()
            .take(MAX_FUTURE_QUERIES_PER_TENSION)
        {
            if let Err(e) = self.create_future_query(query, target, stats).await {
                warn!(
                    query = query.as_str(),
                    error = %e,
                    "Failed to create future query source"
                );
            }
        }

        info!(
            tension_id = %target.tension_id,
            title = target.title.as_str(),
            gatherings = stats.gatherings_discovered,
            "Tension gravity investigation complete"
        );

        Ok(true)
    }

    async fn process_gathering(
        &self,
        target: &GatheringFinderTarget,
        gathering: &DiscoveredGathering,
        stats: &mut GatheringFinderStats,
    ) -> Result<()> {
        let embed_text = format!("{} {}", gathering.title, gathering.summary);
        let embedding = self.embedder.embed(&embed_text).await?;

        let node_type = match gathering.signal_type.to_lowercase().as_str() {
            "gathering" => NodeType::Gathering,
            "need" => NodeType::Need,
            _ => NodeType::Aid, // Default to Aid for unknown types
        };

        // Check for duplicate (region-scoped)
        let existing = self
            .writer
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
                    title = gathering.title.as_str(),
                    "Matched existing signal for gathering"
                );
                stats.gatherings_deduped += 1;
                was_new = false;

                // Touch the existing signal so it doesn't age out
                if let Err(e) = self.writer.touch_signal_timestamp(dup.id).await {
                    warn!(error = %e, "Failed to touch signal timestamp (non-fatal)");
                }

                dup.id
            }
            _ => {
                if let Err(ref e) = existing {
                    warn!(error = %e, "Gathering dedup check failed, creating new");
                }
                was_new = true;
                self.create_gathering_node(gathering).await?
            }
        };

        // Wire DRAWN_TO edge to the target tension
        self.writer
            .create_drawn_to_edge(
                signal_id,
                target.tension_id,
                gathering.match_strength.clamp(0.0, 1.0),
                &gathering.explanation,
                &gathering.gathering_type,
            )
            .await?;
        stats.edges_created += 1;

        // Wire additional DRAWN_TO edges for also_addresses
        if !gathering.also_addresses.is_empty() {
            if let Err(e) = self
                .wire_also_addresses(
                    signal_id,
                    &gathering.also_addresses,
                    &gathering.explanation,
                    &gathering.gathering_type,
                )
                .await
            {
                warn!(error = %e, "Failed to wire also_addresses (non-fatal)");
            }
        }

        // Place creation: promote venue string to first-class Place node
        if let Some(ref venue) = gathering.venue {
            if !venue.is_empty() {
                match self
                    .writer
                    .find_or_create_place(
                        venue,
                        self.region.center_lat,
                        self.region.center_lng,
                    )
                    .await
                {
                    Ok(place_id) => {
                        if let Err(e) = self
                            .writer
                            .create_gathers_at_edge(signal_id, place_id)
                            .await
                        {
                            warn!(venue, error = %e, "Failed to create GATHERS_AT edge (non-fatal)");
                        }
                    }
                    Err(e) => {
                        warn!(venue, error = %e, "Failed to find_or_create_place (non-fatal)")
                    }
                }

                // Venue seeding: create future source for the venue
                let venue_query = format!("{} {} community events", venue, self.region.name);
                if let Err(e) = self.create_future_query(&venue_query, target, stats).await {
                    warn!(venue, error = %e, "Failed to create venue-seeded future source");
                }
            }
        }

        if was_new {
            stats.signals_created += 1;
        }

        Ok(())
    }

    async fn create_gathering_node(&self, gathering: &DiscoveredGathering) -> Result<Uuid> {
        let now = Utc::now();
        let meta = NodeMeta {
            id: Uuid::new_v4(),
            title: gathering.title.clone(),
            summary: gathering.summary.clone(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.7,
            freshness_score: 1.0,
            corroboration_count: 0,
            location: Some(GeoPoint {
                lat: self.region.center_lat,
                lng: self.region.center_lng,
                precision: GeoPrecision::Approximate,
            }),
            location_name: Some(self.region.name.clone()),
            source_url: gathering.url.clone(),
            extracted_at: now,
            last_confirmed_active: now,
            source_diversity: 1,
            external_ratio: 1.0,
            cause_heat: 0.0,
            channel_diversity: 1,
            mentioned_actors: vec![],
            implied_queries: vec![],
        };

        let node = match gathering.signal_type.to_lowercase().as_str() {
            "gathering" => {
                let starts_at = gathering.event_date.as_deref().and_then(|d| {
                    chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                        .ok()
                        .map(|nd| nd.and_hms_opt(0, 0, 0).unwrap().and_utc())
                });
                Node::Gathering(GatheringNode {
                    meta,
                    starts_at,
                    ends_at: None,
                    action_url: gathering.url.clone(),
                    organizer: gathering.organizer.clone(),
                    is_recurring: gathering.is_recurring,
                })
            }
            "need" => Node::Need(NeedNode {
                meta,
                urgency: Urgency::Medium,
                what_needed: Some(gathering.summary.clone()),
                action_url: Some(gathering.url.clone()),
                goal: None,
            }),
            _ => Node::Aid(AidNode {
                meta,
                action_url: gathering.url.clone(),
                availability: None,
                is_ongoing: gathering.is_recurring,
            }),
        };

        let embed_text = format!("{} {}", gathering.title, gathering.summary);
        let embedding = self.embedder.embed(&embed_text).await?;

        let node_id = self.writer.create_node(&node, &embedding, "gathering_finder", &self.run_id).await?;

        info!(
            node_id = %node_id,
            title = gathering.title.as_str(),
            signal_type = gathering.signal_type.as_str(),
            gathering_type = gathering.gathering_type.as_str(),
            is_recurring = gathering.is_recurring,
            "New gathering signal created"
        );

        Ok(node_id)
    }

    /// Wire gravity edges to additional tensions that this gathering also addresses.
    /// Uses embedding similarity against all active tensions (>0.85 threshold).
    /// Passes gathering_type through so all edges are marked as gravity.
    async fn wire_also_addresses(
        &self,
        signal_id: Uuid,
        also_addresses: &[String],
        explanation: &str,
        gathering_type: &str,
    ) -> Result<()> {
        let lat_delta = self.region.radius_km / 111.0;
        let lng_delta =
            self.region.radius_km / (111.0 * self.region.center_lat.to_radians().cos());
        let active_tensions = self
            .writer
            .get_active_tensions(
                self.region.center_lat - lat_delta,
                self.region.center_lat + lat_delta,
                self.region.center_lng - lng_delta,
                self.region.center_lng + lng_delta,
            )
            .await?;
        if active_tensions.is_empty() {
            return Ok(());
        }

        for tension_title in also_addresses {
            let title_embedding = self.embedder.embed(tension_title).await?;
            let title_emb_f64: Vec<f64> = title_embedding.iter().map(|&v| v as f64).collect();

            let mut best_match: Option<(Uuid, f64)> = None;
            for (tid, temb) in &active_tensions {
                let sim = cosine_sim_f64(&title_emb_f64, temb);
                if sim >= 0.85 {
                    if best_match.as_ref().map_or(true, |b| sim > b.1) {
                        best_match = Some((*tid, sim));
                    }
                }
            }

            if let Some((tension_id, sim)) = best_match {
                info!(
                    signal_id = %signal_id,
                    tension_id = %tension_id,
                    similarity = sim,
                    also_addresses = tension_title.as_str(),
                    "Wiring gravity also_addresses edge"
                );
                self.writer
                    .create_drawn_to_edge(
                        signal_id,
                        tension_id,
                        sim.clamp(0.0, 1.0),
                        explanation,
                        gathering_type,
                    )
                    .await?;
            }
        }

        Ok(())
    }

    async fn create_future_query(
        &self,
        query: &str,
        target: &GatheringFinderTarget,
        stats: &mut GatheringFinderStats,
    ) -> Result<()> {
        let cv = query.to_string();
        let ck = canonical_value(&cv);
        let gap_context = format!(
            "Gathering finder: gathering discovery for \"{}\"",
            target.title,
        );

        let source = SourceNode {
            id: Uuid::new_v4(),
            canonical_key: ck,
            canonical_value: cv,
            url: None,
            discovery_method: DiscoveryMethod::GapAnalysis,
            created_at: Utc::now(),
            last_scraped: None,
            last_produced_signal: None,
            signals_produced: 0,
            signals_corroborated: 0,
            consecutive_empty_runs: 0,
            active: true,
            gap_context: Some(gap_context),
            weight: crate::discovery::source_finder::initial_weight_for_method(
                DiscoveryMethod::GapAnalysis,
                Some("unmet_tension"),
            ),
            cadence_hours: None,
            avg_signals_per_scrape: 0.0,
            quality_penalty: 1.0,
            source_role: SourceRole::Response,
            scrape_count: 0,
        };

        self.writer.upsert_source(&source).await?;
        stats.future_sources_created += 1;

        info!(
            query = query,
            tension = target.title.as_str(),
            "Future query source created by gathering finder"
        );

        Ok(())
    }
}

fn cosine_sim_f64(a: &[f64], b: &[f64]) -> f64 {
    crate::infra::util::cosine_similarity(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gravity_finding_parses_empty() {
        let json = r#"{"no_gravity": false, "gatherings": [], "future_queries": []}"#;
        let finding: GravityFinding = serde_json::from_str(json).unwrap();
        assert!(!finding.no_gravity);
        assert!(finding.gatherings.is_empty());
        assert!(finding.future_queries.is_empty());
    }

    #[test]
    fn gravity_finding_parses_no_gravity() {
        let json = r#"{
            "no_gravity": true,
            "no_gravity_reason": "No community events found after 3 searches",
            "gatherings": [],
            "future_queries": ["Minneapolis solidarity events 2026"]
        }"#;
        let finding: GravityFinding = serde_json::from_str(json).unwrap();
        assert!(finding.no_gravity);
        assert_eq!(
            finding.no_gravity_reason.as_deref(),
            Some("No community events found after 3 searches")
        );
        assert!(finding.gatherings.is_empty());
        assert_eq!(finding.future_queries.len(), 1);
    }

    #[test]
    fn gravity_finding_parses_with_gatherings() {
        let json = r#"{
            "no_gravity": false,
            "gatherings": [{
                "title": "Singing Rebellion at Lake Street Church",
                "summary": "Weekly gathering where community sings together in solidarity",
                "signal_type": "gathering",
                "url": "https://example.com/singing",
                "gathering_type": "singing",
                "venue": "Lake Street Church",
                "is_recurring": true,
                "organizer": "Twin Cities Solidarity Singers",
                "explanation": "ICE enforcement fear transforms into collective singing — solidarity through shared vulnerability",
                "match_strength": 0.95,
                "also_addresses": ["Housing instability"],
                "event_date": "2026-03-01"
            }],
            "future_queries": ["Minneapolis community vigils 2026"]
        }"#;
        let finding: GravityFinding = serde_json::from_str(json).unwrap();
        assert!(!finding.no_gravity);
        assert_eq!(finding.gatherings.len(), 1);
        assert_eq!(
            finding.gatherings[0].title,
            "Singing Rebellion at Lake Street Church"
        );
        assert_eq!(finding.gatherings[0].signal_type, "gathering");
        assert_eq!(finding.gatherings[0].gathering_type, "singing");
        assert_eq!(
            finding.gatherings[0].venue.as_deref(),
            Some("Lake Street Church")
        );
        assert!(finding.gatherings[0].is_recurring);
        assert!((finding.gatherings[0].match_strength - 0.95).abs() < 0.001);
        assert_eq!(
            finding.gatherings[0].also_addresses,
            vec!["Housing instability"]
        );
        assert_eq!(finding.future_queries.len(), 1);
    }

    #[test]
    fn gravity_finding_defaults_missing_fields() {
        let json = r#"{}"#;
        let finding: GravityFinding = serde_json::from_str(json).unwrap();
        assert!(!finding.no_gravity);
        assert!(finding.gatherings.is_empty());
        assert!(finding.future_queries.is_empty());
    }

    #[test]
    fn gravity_scout_stats_display() {
        let stats = GatheringFinderStats {
            targets_found: 3,
            targets_investigated: 2,
            targets_no_gravity: 1,
            gatherings_discovered: 5,
            gatherings_deduped: 1,
            signals_created: 4,
            edges_created: 6,
            future_sources_created: 3,
        };
        let display = format!("{stats}");
        assert!(display.contains("3 targets found"));
        assert!(display.contains("2 investigated"));
        assert!(display.contains("1 no-gravity"));
        assert!(display.contains("5 gatherings discovered"));
        assert!(display.contains("4 signals created"));
    }

    #[test]
    fn cosine_similarity_works() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_sim_f64(&a, &b) - 1.0).abs() < 0.001);

        let c = vec![0.0, 1.0, 0.0];
        assert!(cosine_sim_f64(&a, &c).abs() < 0.001);

        let d = vec![0.0, 0.0, 0.0];
        assert!(cosine_sim_f64(&a, &d).abs() < 0.001);
    }

    #[test]
    fn event_date_parsing() {
        let date_str = "2026-03-15";
        let parsed = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d");
        assert!(parsed.is_ok());

        let bad_str = "not-a-date";
        let parsed = chrono::NaiveDate::parse_from_str(bad_str, "%Y-%m-%d");
        assert!(parsed.is_err());
    }

    #[test]
    fn gathering_node_uses_region_center_coordinates() {
        let region = ScoutScope {
            name: "Minneapolis".to_string(),
            center_lat: 44.9778,
            center_lng: -93.2650,
            radius_km: 30.0,
            geo_terms: vec!["Minneapolis".to_string()],
        };

        let now = Utc::now();
        let meta = NodeMeta {
            id: Uuid::new_v4(),
            title: "Singing Rebellion".to_string(),
            summary: "Community singing event".to_string(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.7,
            freshness_score: 1.0,
            corroboration_count: 0,
            location: Some(GeoPoint {
                lat: region.center_lat,
                lng: region.center_lng,
                precision: GeoPrecision::Approximate,
            }),
            location_name: Some(region.name.clone()),
            source_url: "https://example.com/singing".to_string(),
            extracted_at: now,
            last_confirmed_active: now,
            source_diversity: 1,
            external_ratio: 1.0,
            cause_heat: 0.0,
            channel_diversity: 1,
            mentioned_actors: vec![],
            implied_queries: vec![],
        };

        let node = Node::Gathering(GatheringNode {
            meta,
            starts_at: None,
            ends_at: None,
            action_url: "https://example.com/singing".to_string(),
            organizer: Some("Solidarity Singers".to_string()),
            is_recurring: true,
        });

        let loc = node.meta().unwrap().location.as_ref().unwrap();
        assert!((loc.lat - 44.9778).abs() < 0.001);
        assert!((loc.lng - (-93.2650)).abs() < 0.001);
        assert_eq!(loc.precision, GeoPrecision::Approximate);
    }
}
