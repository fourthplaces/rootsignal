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
use rootsignal_graph::{GatheringFinderTarget, GraphQueries, ResponseHeuristic};
use rootsignal_archive::Archive;
use crate::domains::curiosity::util::{
    self, region_bounds, MAX_TOOL_TURNS,
};
use crate::infra::agent_tools::{ReadPageTool, WebSearchTool};
use crate::infra::embedder::TextEmbedder;

/// Result of dedup classification for a discovered gathering.
#[derive(Debug)]
pub enum GatheringClassification {
    /// Genuinely new — no existing match. Contains the pre-assigned ID.
    New { signal_id: Uuid },
    /// Matched an existing signal in the graph.
    Duplicate { existing_id: Uuid },
}

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
    /// "gathering", "resource", or "help_request"
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

    if let Some(ref wwh) = target.opposing {
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
// GatheringFinder — deps struct + free functions
// =============================================================================

pub struct GatheringFinderDeps<'a> {
    pub graph: &'a dyn GraphQueries,
    pub ai: &'a dyn Agent,
    pub tool_agent: Box<dyn Agent>,
    pub embedder: &'a dyn TextEmbedder,
    pub region: ScoutScope,
    pub min_lat: f64,
    pub max_lat: f64,
    pub min_lng: f64,
    pub max_lng: f64,
    pub run_id: String,
}

impl<'a> GatheringFinderDeps<'a> {
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

        let (min_lat, max_lat, min_lng, max_lng) = region_bounds(&region);
        Self {
            graph,
            ai,
            tool_agent,
            embedder,
            min_lat,
            max_lat,
            min_lng,
            max_lng,
            region,
            run_id,
        }
    }
}

/// LLM investigation + structuring. Returns findings without creating nodes or events.
pub async fn investigate_target(
    deps: &GatheringFinderDeps<'_>,
    target: &GatheringFinderTarget,
) -> Result<GravityFinding> {
    let existing = deps
        .graph
        .get_existing_gathering_signals(
            target.concern_id,
            deps.region.center_lat,
            deps.region.center_lng,
            deps.region.radius_km,
        )
        .await
        .unwrap_or_default();

    let system = investigation_system_prompt(&deps.region.name);
    let user = investigation_user_prompt(target, &existing);

    let reasoning = deps
        .tool_agent
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

    let finding: GravityFinding = ai_extract(deps.ai, STRUCTURING_SYSTEM, &structuring_user)
        .await?;

    Ok(finding)
}

/// Embed + dedup a discovered gathering. Returns whether it's new or a duplicate.
pub async fn classify_gathering(
    deps: &GatheringFinderDeps<'_>,
    gathering: &DiscoveredGathering,
) -> Result<GatheringClassification> {
    let node_type = match gathering.signal_type.to_lowercase().as_str() {
        "gathering" => NodeType::Gathering,
        "help_request" => NodeType::HelpRequest,
        _ => NodeType::Resource,
    };

    let embed_text = format!("{} {}", gathering.title, gathering.summary);
    let embedding = deps.embedder.embed(&embed_text).await?;

    let existing = deps
        .graph
        .find_duplicate(
            &embedding,
            node_type,
            0.85,
            deps.min_lat,
            deps.max_lat,
            deps.min_lng,
            deps.max_lng,
        )
        .await;

    match existing {
        Ok(Some(dup)) => {
            info!(
                existing_id = %dup.id,
                similarity = dup.similarity,
                title = gathering.title.as_str(),
                "Matched existing signal for gathering"
            );
            Ok(GatheringClassification::Duplicate {
                existing_id: dup.id,
            })
        }
        _ => {
            if let Err(ref e) = existing {
                warn!(error = %e, "Gathering dedup check failed, creating new");
            }
            Ok(GatheringClassification::New {
                signal_id: Uuid::new_v4(),
            })
        }
    }
}

/// Resolve raw tension titles to concern_ids via embedding similarity.
pub async fn resolve_also_addresses(
    deps: &GatheringFinderDeps<'_>,
    also_addresses: &[String],
) -> Vec<(Uuid, f64)> {
    let mut edges = Vec::new();
    for tension_title in also_addresses {
        match util::find_best_tension_match(
            deps.embedder, deps.graph, &deps.region, tension_title, 0.85,
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

#[cfg(test)]
mod tests {
    use rootsignal_common::{GatheringNode, Node};
    use crate::domains::curiosity::util::build_node_meta;
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
        };

        let meta = build_node_meta(
            "Singing Rebellion".to_string(),
            "Community singing event".to_string(),
            "https://example.com/singing".to_string(),
            &region,
            0.7,
        );

        let node = Node::Gathering(GatheringNode {
            meta,
            starts_at: None,
            ends_at: None,
            action_url: "https://example.com/singing".to_string(),
            organizer: Some("Solidarity Singers".to_string()),
            is_recurring: true,
        });

        let loc = node.meta().unwrap().about_location.as_ref().unwrap();
        assert!((loc.lat - 44.9778).abs() < 0.001);
        assert!((loc.lng - (-93.2650)).abs() < 0.001);
        assert_eq!(loc.precision, rootsignal_common::GeoPrecision::Approximate);
    }
}

