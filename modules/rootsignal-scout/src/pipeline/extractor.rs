use ai_client::claude::Claude;
use anyhow::Result;
use chrono::Utc;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{
    AidNode, GatheringNode, GeoPoint, GeoPrecision, NeedNode, Node, NodeMeta, NoticeNode,
    SensitivityLevel, Severity, TensionNode, Urgency,
};

/// What the LLM returns for each extracted signal.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExtractedSignal {
    /// Signal type: "gathering", "aid", "need", or "notice"
    pub signal_type: String,
    pub title: String,
    pub summary: String,
    /// "general", "elevated", or "sensitive"
    pub sensitivity: String,
    /// Latitude if location can be determined
    pub latitude: Option<f64>,
    /// Longitude if location can be determined
    pub longitude: Option<f64>,
    /// Geo precision: "exact", "neighborhood"
    pub geo_precision: Option<String>,
    /// Human-readable location name (e.g. "YWCA Midtown", "Lake Nokomis")
    pub location_name: Option<String>,
    /// ISO datetime string for event start time
    pub starts_at: Option<String>,
    /// ISO datetime string for event end time
    pub ends_at: Option<String>,
    /// URL where the user can take action
    pub action_url: Option<String>,
    /// Organizer name (for gatherings)
    pub organizer: Option<String>,
    /// Whether this is recurring
    pub is_recurring: Option<bool>,
    /// Availability schedule (for Aid signals)
    pub availability: Option<String>,
    /// Whether this is an ongoing opportunity
    pub is_ongoing: Option<bool>,
    /// Urgency level for Need signals: "low", "medium", "high", "critical"
    pub urgency: Option<String>,
    /// What is needed (for Need signals)
    pub what_needed: Option<String>,
    /// Goal description (for Need signals)
    pub goal: Option<String>,
    /// Severity for Tension signals: "low", "medium", "high", "critical"
    pub severity: Option<String>,
    /// Category for Notice signals: "psa", "policy", "advisory", "enforcement", "health"
    pub category: Option<String>,
    /// Effective date for Notice signals (ISO 8601)
    pub effective_date: Option<String>,
    /// Source authority for Notice signals (e.g. "City of Minneapolis")
    pub source_authority: Option<String>,
    /// Best-guess date when this content was published or last updated (ISO 8601).
    /// Used for staleness filtering — signals older than 1 year are dropped.
    pub content_date: Option<String>,
    /// Organizations, groups, or individuals mentioned in the signal
    pub mentioned_actors: Option<Vec<String>>,
    /// The specific source URL this signal was extracted from (e.g. a specific post URL).
    /// When extracting from multiple posts, return the URL of the post this signal came from.
    pub source_url: Option<String>,
    /// What response would address this tension (for Tension signals)
    pub what_would_help: Option<String>,
    /// Up to 3 search queries this signal implies — expand outward from this
    /// signal to discover related signals from different perspectives.
    #[serde(default)]
    pub implied_queries: Vec<String>,
    /// Resource capabilities this signal requires, prefers, or offers.
    #[serde(default)]
    pub resources: Vec<ResourceTag>,
    /// 3-5 thematic tags as lowercase-with-hyphens slugs (e.g. "ice-enforcement", "housing-displacement").
    #[serde(default)]
    pub tags: Vec<String>,
    /// Whether this signal is first-hand (from someone directly affected/involved).
    /// Only populated for non-entity search/feed sources. None = not assessed.
    pub is_firsthand: Option<bool>,
    /// The person, organization, or account that authored/published this content.
    /// For social posts: the account holder. For org pages: the organization.
    /// For news: the journalist or publication.
    pub author_actor: Option<String>,
}

/// A resource capability extracted from a signal.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ResourceTag {
    /// Canonical slug (e.g. "vehicle", "bilingual-spanish", "legal-expertise").
    /// Use the seed vocabulary when it fits; otherwise propose a concise noun-phrase slug.
    pub slug: String,
    /// "requires", "prefers", or "offers"
    pub role: String,
    /// 0.0–1.0 confidence that this resource is relevant
    #[serde(default = "default_confidence")]
    pub confidence: f64,
    /// Optional context (e.g. "10 people", "Saturday mornings", "500 lbs shelf-stable protein")
    pub context: Option<String>,
}

fn default_confidence() -> f64 {
    0.8
}

/// The full extraction response from the LLM.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExtractionResponse {
    #[serde(default, deserialize_with = "deserialize_signals")]
    pub signals: Vec<ExtractedSignal>,
}

/// Handle LLM returning signals as either a proper JSON array or a stringified JSON array.
fn deserialize_signals<'de, D>(
    deserializer: D,
) -> std::result::Result<Vec<ExtractedSignal>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Array(_) => serde_json::from_value(value).map_err(de::Error::custom),
        serde_json::Value::String(ref s) => serde_json::from_str(s).map_err(de::Error::custom),
        serde_json::Value::Null => Ok(Vec::new()),
        _ => Err(de::Error::custom("signals must be an array or JSON string")),
    }
}

// StructuredOutput is auto-implemented via blanket impl for JsonSchema + DeserializeOwned

/// Result of signal extraction — nodes plus any implied discovery queries.
#[derive(Default)]
pub struct ExtractionResult {
    pub nodes: Vec<Node>,
    pub implied_queries: Vec<String>,
    /// Resource tags paired with the signal node UUID they came from.
    pub resource_tags: Vec<(Uuid, Vec<ResourceTag>)>,
    /// Thematic tags paired with the signal node UUID they came from.
    pub signal_tags: Vec<(Uuid, Vec<String>)>,
}

// --- SignalExtractor trait ---

#[async_trait::async_trait]
pub trait SignalExtractor: Send + Sync {
    async fn extract(&self, content: &str, source_url: &str) -> Result<ExtractionResult>;
}

pub struct Extractor {
    claude: Claude,
    system_prompt: String,
}

impl Extractor {
    pub fn new(
        anthropic_api_key: &str,
        city_name: &str,
        default_lat: f64,
        default_lng: f64,
    ) -> Self {
        Self::with_tag_vocabulary(anthropic_api_key, city_name, default_lat, default_lng, &[])
    }

    pub fn with_tag_vocabulary(
        anthropic_api_key: &str,
        city_name: &str,
        default_lat: f64,
        default_lng: f64,
        tag_vocabulary: &[String],
    ) -> Self {
        let claude = Claude::new(anthropic_api_key, "claude-haiku-4-5-20251001");
        let system_prompt =
            build_system_prompt(city_name, default_lat, default_lng, tag_vocabulary);
        Self {
            claude,
            system_prompt,
        }
    }

    /// Create an extractor with a pre-built system prompt (for genome-driven evolution).
    pub fn with_system_prompt(anthropic_api_key: &str, system_prompt: String) -> Self {
        let claude = Claude::new(anthropic_api_key, "claude-haiku-4-5-20251001");
        Self {
            claude,
            system_prompt,
        }
    }

    /// Extract signals from page content (internal implementation).
    async fn extract_impl(&self, content: &str, source_url: &str) -> Result<ExtractionResult> {
        // Truncate content to avoid token limits
        let content = if content.len() > 30_000 {
            let mut end = 30_000;
            while !content.is_char_boundary(end) {
                end -= 1;
            }
            &content[..end]
        } else {
            content
        };

        let user_prompt = format!(
            "Extract all signals from this web page.\n\nSource URL: {source_url}\n\n---\n\n{content}"
        );

        let response: ExtractionResponse = self
            .claude
            .extract(
                "claude-haiku-4-5-20251001",
                &self.system_prompt,
                &user_prompt,
            )
            .await?;

        // Collect implied queries before converting to nodes
        let implied_queries: Vec<String> = response
            .signals
            .iter()
            .flat_map(|s| s.implied_queries.iter().cloned())
            .collect();

        let now = Utc::now();
        let mut nodes = Vec::new();
        let mut resource_tags: Vec<(Uuid, Vec<ResourceTag>)> = Vec::new();
        let mut signal_tags: Vec<(Uuid, Vec<String>)> = Vec::new();

        for signal in response.signals {
            // Skip junk signals from extraction failures
            let title_lower = signal.title.to_lowercase();
            if ["unable to extract", "page not found", "error loading"]
                .iter()
                .any(|junk| title_lower.contains(junk))
            {
                warn!(
                    source_url,
                    title = signal.title,
                    "Filtered junk signal from extraction"
                );
                continue;
            }

            // Drop signals flagged as not first-hand (political commentary, not personally affected)
            if signal.is_firsthand == Some(false) {
                info!(
                    source_url,
                    title = signal.title,
                    "Dropped non-first-hand signal"
                );
                continue;
            }

            let sensitivity = match signal.sensitivity.as_str() {
                "sensitive" => SensitivityLevel::Sensitive,
                "elevated" => SensitivityLevel::Elevated,
                _ => SensitivityLevel::General,
            };

            let location = match (signal.latitude, signal.longitude) {
                (Some(lat), Some(lng)) => {
                    let precision = match signal.geo_precision.as_deref() {
                        Some("exact") => GeoPrecision::Exact,
                        Some("neighborhood") => GeoPrecision::Neighborhood,
                        _ => GeoPrecision::Approximate,
                    };
                    Some(GeoPoint {
                        lat,
                        lng,
                        precision,
                    })
                }
                _ => None,
            };

            let mentioned_actors = signal.mentioned_actors.unwrap_or_default();

            // Use the LLM-returned source_url when present (specific post URL),
            // falling back to the page-level source_url.
            let effective_source_url = signal
                .source_url
                .as_deref()
                .filter(|u| !u.is_empty())
                .unwrap_or(source_url)
                .to_string();

            let content_date = signal
                .content_date
                .as_deref()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc));

            let node_id = Uuid::new_v4();
            let meta = NodeMeta {
                id: node_id,
                title: signal.title.clone(),
                summary: signal.summary.clone(),
                sensitivity,
                confidence: 0.0,      // Will be computed by QualityScorer
                freshness_score: 1.0, // Fresh at extraction time
                corroboration_count: 0,
                location,
                location_name: signal.location_name.clone(),
                source_url: effective_source_url.clone(),
                extracted_at: now,
                content_date,
                last_confirmed_active: now,
                source_diversity: 1,
                external_ratio: 0.0,
                cause_heat: 0.0,
                channel_diversity: 1,
                mentioned_actors,
                implied_queries: signal.implied_queries.clone(),
                author_actor: signal.author_actor.clone(),
            };

            let node = match signal.signal_type.as_str() {
                "gathering" => {
                    let starts_at = signal
                        .starts_at
                        .as_deref()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.with_timezone(&Utc));
                    let ends_at = signal
                        .ends_at
                        .as_deref()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.with_timezone(&Utc));

                    Node::Gathering(GatheringNode {
                        meta,
                        starts_at,
                        ends_at,
                        action_url: signal.action_url.unwrap_or(effective_source_url),
                        organizer: signal.organizer,
                        is_recurring: signal.is_recurring.unwrap_or(false),
                    })
                }
                "aid" => Node::Aid(AidNode {
                    meta,
                    action_url: signal.action_url.unwrap_or(effective_source_url),
                    availability: signal.availability,
                    is_ongoing: signal.is_ongoing.unwrap_or(true),
                }),
                "need" => {
                    let urgency = match signal.urgency.as_deref() {
                        Some("high") => Urgency::High,
                        Some("critical") => Urgency::Critical,
                        Some("low") => Urgency::Low,
                        _ => Urgency::Medium,
                    };
                    Node::Need(NeedNode {
                        meta,
                        urgency,
                        what_needed: signal.what_needed,
                        action_url: signal.action_url,
                        goal: signal.goal,
                    })
                }
                "notice" => {
                    let severity = match signal.severity.as_deref() {
                        Some("high") => Severity::High,
                        Some("critical") => Severity::Critical,
                        Some("low") => Severity::Low,
                        _ => Severity::Medium,
                    };
                    let effective_date = signal
                        .effective_date
                        .as_deref()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.with_timezone(&Utc));

                    Node::Notice(NoticeNode {
                        meta,
                        severity,
                        category: signal.category,
                        effective_date,
                        source_authority: signal.source_authority,
                    })
                }
                "tension" => {
                    let severity = match signal.severity.as_deref() {
                        Some("high") => Severity::High,
                        Some("critical") => Severity::Critical,
                        Some("low") => Severity::Low,
                        _ => Severity::Medium,
                    };
                    Node::Tension(TensionNode {
                        meta,
                        severity,
                        category: signal.category.clone(),
                        what_would_help: signal.what_would_help.clone(),
                    })
                }
                other => {
                    warn!(
                        signal_type = other,
                        title = signal.title,
                        "Unknown signal type, skipping"
                    );
                    continue;
                }
            };

            // Collect resource tags for this signal
            if !signal.resources.is_empty() {
                resource_tags.push((node_id, signal.resources.clone()));
            }

            // Collect thematic tags for this signal (slugify each tag)
            if !signal.tags.is_empty() {
                let slugified: Vec<String> = signal
                    .tags
                    .iter()
                    .map(|t| rootsignal_common::slugify(t))
                    .filter(|s| !s.is_empty())
                    .collect();
                if !slugified.is_empty() {
                    signal_tags.push((node_id, slugified));
                }
            }

            nodes.push(node);
        }

        info!(
            source_url,
            count = nodes.len(),
            implied_queries = implied_queries.len(),
            "Extracted signals"
        );
        Ok(ExtractionResult {
            nodes,
            implied_queries,
            resource_tags,
            signal_tags,
        })
    }
}

#[async_trait::async_trait]
impl SignalExtractor for Extractor {
    async fn extract(&self, content: &str, source_url: &str) -> Result<ExtractionResult> {
        self.extract_impl(content, source_url).await
    }
}

pub fn build_system_prompt(
    city_name: &str,
    _default_lat: f64,
    _default_lng: f64,
    tag_vocabulary: &[String],
) -> String {
    let today = Utc::now().format("%Y-%m-%d").to_string();
    let tension_cats = crate::infra::util::TENSION_CATEGORIES;
    let tag_vocab_section = if tag_vocabulary.is_empty() {
        String::new()
    } else {
        format!(
            "**Existing tag vocabulary** (prefer these when they fit; only invent new tags when no existing tag matches):\n`{}`\n\n",
            tag_vocabulary.join("`, `")
        )
    };
    format!(
        r#"You are a signal extractor for {city_name}.

Your job: find real problems and the people addressing them. The most valuable signal is a TENSION (something out of alignment in community or ecological life) paired with RESPONSES (the gives, needs, events, and notices that address it). A food shelf addressing a food desert, a cleanup responding to pollution, a legal aid hotline responding to enforcement activity — these tension-response pairs are what gets people engaged in real-world problems.

## Signal Types (ranked by value)

**Highest — Tension + Response pairs:**
- **Tension**: A community conflict, systemic problem, or ecological misalignment. Has severity and what would help. NOT the narrative itself — the underlying structural issue.
- **Aid**: A free resource, service, or program that people in need can access — food shelves,
  legal clinics, shelter beds, mutual aid funds, habitat restoration programs. Must be free or
  publicly available. A business offering paid services is NOT Aid. Has availability and contact info.
- **Need**: Someone directly expressing what they need and how you can help — fundraisers, volunteer drives, donation requests, mutual aid calls, petitions. The content must come from or speak for the person/group who has the need. Must include: (1) a specific need, (2) a way to respond (donate link, signup, contact info). A journalist reporting that communities need help is a Tension, not a Need.
- **Gathering**: People coming together in response to a community need or tension — town halls,
  cleanups, vigils, mutual aid distributions, workshops, solidarity actions. Has time, location,
  and who's organizing. A press conference or product launch is NOT a Gathering.

**Also valuable — standalone responses with an implicit tension:**
- A "feed people on Sundays" program implies food insecurity. Extract it as an Aid even without an explicit tension on the page.
- A river cleanup implies pollution. Extract it as a Gathering.

**Lower priority — routine community activity:**
- Community calendar events, recurring worship services, social gatherings. Still extract these, but they matter less than signals that point to a real problem someone can help with.

**Context signals:**
- **Notice**: An official advisory or policy change. Has source authority and effective date.

If content doesn't map to one of these types, return an empty signals array.

## Extracting from News and Crisis Content
When a page describes a crisis, conflict, or problem, extract BOTH the underlying tension AND the community responses:
- The structural problem → Tension (always include what_would_help)
- Legal aid hotlines, know-your-rights resources → Aid
- Community meetings, workshops, public hearings → Gathering
- Volunteer calls, donation drives, petitions → Need
- Official advisories, policy changes → Notice

If a page describes only a problem with no actionable response, still extract the Tension (with what_would_help) — the system will seek responses separately.

## News Articles vs. Needs
A news article that reports on a crisis is NOT a Need, even if the situation is urgent.
Need requires someone to be directly expressing their own need with a way to respond.
If a news article simply reports on a problem, extract it as:
- Tension (if it describes a systemic problem or conflict)
- Notice (if it describes a policy change or official action)
Do NOT classify news reportage as a Need based on the urgency of the topic alone.

## Sensitivity
- **sensitive**: Enforcement activity, vulnerable populations, sanctuary networks
- **elevated**: Organizing, advocacy, political action
- **general**: Everything else

## Location
- Extract the most specific location possible from the content
- Only provide latitude/longitude if you can identify a SPECIFIC place (building, park, intersection, venue)
- If the signal is region-wide or you can't determine a specific location, omit latitude/longitude entirely (null)
- Also provide location_name: the place name as text (e.g. "YWCA Midtown", "Lake Nokomis", "City Hall")
- geo_precision: "exact" for specific addresses/buildings, "neighborhood" for areas

## Timing
- ISO 8601 datetime strings for start/end times (e.g. "2026-03-15T14:00:00Z")
- Today's date is {today}. Resolve relative dates: "next Saturday" → the actual date, "March 15" → "2026-03-15T00:00:00Z"
- If the page has NO parseable date for an event, omit starts_at entirely (null). Do NOT guess.
- is_ongoing: true for ongoing services
- is_recurring: true for recurring events

## Notice Fields
- severity: "low", "medium", "high", "critical"
- category: "psa", "policy", "advisory", "enforcement", "health"
- effective_date: ISO 8601 when the notice takes effect
- source_authority: The official body issuing it

## Tension Fields
- severity: "low", "medium", "high", "critical"
- category: One of: {tension_cats}. These are guidance, not constraints — propose a new category if none fit.
- what_would_help: What response would address this tension (e.g. "affordable housing policy", "community oversight board")

## Source URL
- When extracting from multiple posts (e.g. "--- Post 1 (https://...) ---"), set source_url to the specific post URL the signal came from
- This lets readers navigate directly to the original post, not just the profile

## Action URLs
- Include the most relevant action URL (registration, donation, event page)
- If none exists, use the source page URL

## Author Actor
Set author_actor to the person, organization, or account that authored/published this content.
- For social posts: the account holder (e.g. "@MutualAidMpls")
- For org pages: the organization (e.g. "Simpson Housing Services")
- For news: the journalist or publication (e.g. "Star Tribune")
- If unclear or anonymous, leave null.

## Mentioned Actors
Extract the names of organizations, groups, government bodies, or notable individuals mentioned in each signal. These become Actor nodes in the graph for "who's involved" queries.
- Include: nonprofits, city departments, coalitions, community groups, churches, businesses offering help
- Exclude: generic references like "the city" or "local officials" unless a specific body is named
- Do NOT include the author_actor in mentioned_actors — they are tracked separately

## Contact Information
Preserve organization phone numbers, emails, and addresses — these are public broadcast information, not private data. Strip only genuinely private individual information (personal cell phones, home addresses, SSNs).

## Resource Capabilities

For Need, Gathering, and Aid signals, extract the resource capabilities they require, prefer, or offer.
This enables matching: "I have a car" finds all orgs needing drivers; "I need food" finds all orgs giving food.

**Edge types:**
- **requires**: Must have this capability to help (Need/Gathering only)
- **prefers**: Better if you have it, not required (Need/Gathering only)
- **offers**: This is what the signal provides (Aid only)

**Seed vocabulary** (use these slugs when they fit; otherwise propose a concise noun-phrase slug):
`vehicle`, `bilingual-spanish`, `bilingual-somali`, `bilingual-hmong`, `legal-expertise`,
`food`, `shelter-space`, `clothing`, `childcare`, `medical-professional`, `mental-health`,
`physical-labor`, `kitchen-space`, `event-space`, `storage-space`, `technology`,
`reliable-internet`, `financial-donation`, `skilled-trade`, `administrative`

**Examples:**
- A volunteer driver program → resources: [{{slug: "vehicle", role: "requires", confidence: 0.95}}]
- A bilingual legal clinic → resources: [{{slug: "legal-expertise", role: "offers", confidence: 0.9}}, {{slug: "bilingual-spanish", role: "offers", confidence: 0.85}}]
- Food shelf → resources: [{{slug: "food", role: "offers", confidence: 0.95, context: "emergency groceries, Mon-Fri 9-5"}}]
- Court date transport needing Spanish speakers → resources: [{{slug: "vehicle", role: "requires", confidence: 0.9}}, {{slug: "bilingual-spanish", role: "prefers", confidence: 0.7}}]

Only include resources when the capability is clear from the content. Omit the resources array for signals with no resource semantics (e.g. Notices, Tensions).

## THEMATIC TAGS

For each signal, output 3-5 thematic tags as lowercase-with-hyphens slugs.
Tags describe the themes, issues, and topics the signal relates to.
{tag_vocab_section}
**Examples:**
- An ICE enforcement story → tags: ["ice-enforcement", "immigration", "civil-rights"]
- A food shelf → tags: ["food-insecurity", "mutual-aid", "hunger"]
- A housing town hall → tags: ["housing", "governance", "displacement"]

If no thematic tags apply, return an empty tags array.

## IMPLIED QUERIES (optional — signal quality is always the priority)

For signals with a clear community tension connection, provide up to 3
implied_queries — searches that would discover RELATED community signals
by expanding outward from this one.

- A Need (donations, volunteers, drivers, medical bills, rent help,
  funeral costs, school supplies, legal aid, shelter, food, or any
  other expressed need) → search for others expressing the same kind
  of need nearby (GoFundMe campaigns, mutual aid threads, community
  posts, neighborhood forums). If one person needs it, others do too.
- An Aid (food banks, shelters, legal clinics, free clinics, job
  training, mutual aid networks, or any other service) → search for
  more of the same kind of aid in the area, and for unmet needs in
  the population it serves.
- A Gathering (town halls, protests, rallies, marches, cleanups,
  vigils, mutual aid distributions, political movements, or any
  community event) → search for other gatherings around the same
  issue or cause, who is organizing them, and what tensions are
  driving people to show up.
- A Tension → search for who's responding (aid, organizing, legal
  action), where people are gathering, and what needs people are
  expressing because of it.
- A Notice about policy or institutional action → search for who is
  affected, what they need as a result, and how they're organizing
  in response.

Always include the city name or neighborhood. Target specific organizations,
services, and events — not news articles.

DO NOT provide implied_queries for routine community gatherings (farmers markets,
worship services, recurring social gatherings) that have no tension connection.
Return an empty array for these."#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_prompt_includes_tension() {
        let prompt = build_system_prompt("Minneapolis", 44.9778, -93.2650, &[]);
        assert!(
            prompt.contains("Tension"),
            "system prompt should mention Tension as a signal type"
        );
        assert!(
            prompt.contains("what would help"),
            "system prompt should mention 'what would help' for tensions"
        );
    }

    #[test]
    fn tension_type_constructs_node() {
        // The tension match arm now constructs a TensionNode instead of skipping.
        // This test verifies the ExtractedSignal has the what_would_help field
        // and that TensionNode can be constructed with all fields.
        let signal = ExtractedSignal {
            signal_type: "tension".to_string(),
            title: "Housing crisis".to_string(),
            summary: "Rent increases displacing families".to_string(),
            sensitivity: "elevated".to_string(),
            latitude: None,
            longitude: None,
            geo_precision: None,
            location_name: None,
            starts_at: None,
            ends_at: None,
            action_url: None,
            organizer: None,
            is_recurring: None,
            availability: None,
            is_ongoing: None,
            urgency: None,
            what_needed: None,
            goal: None,
            severity: Some("high".to_string()),
            category: Some("housing".to_string()),
            effective_date: None,
            source_authority: None,
            content_date: None,
            mentioned_actors: None,
            what_would_help: Some("affordable housing policy".to_string()),
            source_url: None,
            implied_queries: vec!["affordable housing programs Minneapolis".to_string()],
            resources: vec![],
            tags: vec![],
            is_firsthand: None,
            author_actor: None,
        };

        assert_eq!(signal.signal_type, "tension");
        assert_eq!(
            signal.what_would_help.as_deref(),
            Some("affordable housing policy")
        );
        assert_eq!(signal.category.as_deref(), Some("housing"));
    }

    #[test]
    fn resource_tag_deserialization() {
        let json = r#"{"slug":"vehicle","role":"requires","confidence":0.9,"context":"Saturday mornings"}"#;
        let tag: ResourceTag = serde_json::from_str(json).unwrap();
        assert_eq!(tag.slug, "vehicle");
        assert_eq!(tag.role, "requires");
        assert!((tag.confidence - 0.9).abs() < f64::EPSILON);
        assert_eq!(tag.context.as_deref(), Some("Saturday mornings"));
    }

    #[test]
    fn resource_tag_default_confidence() {
        let json = r#"{"slug":"food","role":"offers","context":null}"#;
        let tag: ResourceTag = serde_json::from_str(json).unwrap();
        assert!((tag.confidence - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn missing_availability_is_none_not_placeholder() {
        use rootsignal_common::{AidNode, NodeMeta, SensitivityLevel};
        let meta = NodeMeta {
            id: uuid::Uuid::new_v4(),
            title: "Food pantry".to_string(),
            summary: "Weekly groceries".to_string(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.0,
            freshness_score: 1.0,
            corroboration_count: 0,
            location: None,
            location_name: None,
            source_url: "https://example.com".to_string(),
            extracted_at: chrono::Utc::now(),
            content_date: None,
            last_confirmed_active: chrono::Utc::now(),
            source_diversity: 1,
            external_ratio: 0.0,
            cause_heat: 0.0,
            channel_diversity: 1,
            mentioned_actors: vec![],
            implied_queries: vec![],
            author_actor: None,
        };
        let aid = AidNode {
            meta,
            action_url: "https://example.com".to_string(),
            availability: None,
            is_ongoing: true,
        };
        assert!(aid.availability.is_none());
    }

    #[test]
    fn missing_what_needed_is_none_not_placeholder() {
        use rootsignal_common::{NeedNode, NodeMeta, SensitivityLevel, Urgency};
        let meta = NodeMeta {
            id: uuid::Uuid::new_v4(),
            title: "Volunteers needed".to_string(),
            summary: "Help at shelter".to_string(),
            sensitivity: SensitivityLevel::General,
            confidence: 0.0,
            freshness_score: 1.0,
            corroboration_count: 0,
            location: None,
            location_name: None,
            source_url: "https://example.com".to_string(),
            extracted_at: chrono::Utc::now(),
            content_date: None,
            last_confirmed_active: chrono::Utc::now(),
            source_diversity: 1,
            external_ratio: 0.0,
            cause_heat: 0.0,
            channel_diversity: 1,
            mentioned_actors: vec![],
            implied_queries: vec![],
            author_actor: None,
        };
        let need = NeedNode {
            meta,
            urgency: Urgency::Medium,
            what_needed: None,
            action_url: None,
            goal: None,
        };
        assert!(need.what_needed.is_none());
    }

    #[test]
    fn extracted_signal_json_with_implied_queries() {
        let json = r#"{
            "signals": [{
                "signal_type": "tension",
                "title": "Immigration enforcement fear",
                "summary": "ICE raids causing fear",
                "sensitivity": "sensitive",
                "severity": "high",
                "category": "safety",
                "what_would_help": "legal defense, emergency housing",
                "implied_queries": [
                    "immigration legal aid Minneapolis",
                    "emergency housing detained immigrants Minneapolis"
                ]
            }]
        }"#;
        let response: ExtractionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.signals.len(), 1);
        assert_eq!(response.signals[0].implied_queries.len(), 2);
        assert_eq!(
            response.signals[0].implied_queries[0],
            "immigration legal aid Minneapolis"
        );
    }

    #[test]
    fn extracted_signal_json_missing_implied_queries() {
        let json = r#"{
            "signals": [{
                "signal_type": "gathering",
                "title": "Farmers market",
                "summary": "Weekly farmers market",
                "sensitivity": "general"
            }]
        }"#;
        let response: ExtractionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.signals.len(), 1);
        assert!(
            response.signals[0].implied_queries.is_empty(),
            "Missing implied_queries should default to empty vec"
        );
    }

    #[test]
    fn extracted_signal_json_empty_implied_queries() {
        let json = r#"{
            "signals": [{
                "signal_type": "aid",
                "title": "Food shelf",
                "summary": "Free groceries",
                "sensitivity": "general",
                "implied_queries": []
            }]
        }"#;
        let response: ExtractionResponse = serde_json::from_str(json).unwrap();
        assert!(response.signals[0].implied_queries.is_empty());
    }

    #[test]
    fn extraction_result_collects_queries() {
        let result = ExtractionResult {
            nodes: vec![],
            implied_queries: vec!["query 1".to_string(), "query 2".to_string()],
            resource_tags: Vec::new(),
            signal_tags: Vec::new(),
        };
        assert_eq!(result.implied_queries.len(), 2);
    }

    #[test]
    fn extraction_result_default_empty() {
        let result = ExtractionResult::default();
        assert!(result.nodes.is_empty());
        assert!(result.implied_queries.is_empty());
    }

    #[test]
    fn system_prompt_includes_implied_queries_instructions() {
        let prompt = build_system_prompt("Minneapolis", 44.9778, -93.2650, &[]);
        assert!(
            prompt.contains("IMPLIED QUERIES"),
            "system prompt should mention IMPLIED QUERIES section"
        );
        assert!(
            prompt.contains("DO NOT provide implied_queries for routine"),
            "system prompt should warn against routine gathering queries"
        );
    }

    #[test]
    fn system_prompt_includes_resource_instructions() {
        let prompt = build_system_prompt("Minneapolis", 44.9778, -93.2650, &[]);
        assert!(
            prompt.contains("Resource Capabilities"),
            "should have Resource Capabilities section"
        );
        assert!(prompt.contains("vehicle"), "should include seed vocabulary");
        assert!(
            prompt.contains("bilingual-spanish"),
            "should include bilingual seed"
        );
        assert!(prompt.contains("requires"), "should mention requires role");
        assert!(prompt.contains("offers"), "should mention offers role");
    }

    #[test]
    fn is_firsthand_false_deserialization() {
        let json = r#"{
            "signals": [{
                "signal_type": "tension",
                "title": "Political commentary",
                "summary": "Opinion about housing",
                "sensitivity": "general",
                "is_firsthand": false
            }]
        }"#;
        let response: ExtractionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.signals[0].is_firsthand, Some(false));
    }

    #[test]
    fn is_firsthand_true_deserialization() {
        let json = r#"{
            "signals": [{
                "signal_type": "need",
                "title": "My family needs help",
                "summary": "Direct plea for assistance",
                "sensitivity": "sensitive",
                "is_firsthand": true
            }]
        }"#;
        let response: ExtractionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.signals[0].is_firsthand, Some(true));
    }

    #[test]
    fn is_firsthand_missing_is_none() {
        let json = r#"{
            "signals": [{
                "signal_type": "aid",
                "title": "Food shelf",
                "summary": "Free groceries",
                "sensitivity": "general"
            }]
        }"#;
        let response: ExtractionResponse = serde_json::from_str(json).unwrap();
        assert!(
            response.signals[0].is_firsthand.is_none(),
            "Missing is_firsthand should be None, not Some(false)"
        );
    }
}
