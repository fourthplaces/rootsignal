use ai_client::claude::Claude;
use anyhow::Result;
use chrono::Utc;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{
    AskNode, EventNode, GeoPoint, GeoPrecision, GiveNode, Node, NodeMeta,
    NoticeNode, SensitivityLevel, Severity, TensionNode, Urgency,
};

/// What the LLM returns for each extracted signal.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExtractedSignal {
    /// Signal type: "event", "give", "ask", or "notice"
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
    /// Organizer name (for events)
    pub organizer: Option<String>,
    /// Whether this is recurring
    pub is_recurring: Option<bool>,
    /// Availability schedule (for Give signals)
    pub availability: Option<String>,
    /// Whether this is an ongoing opportunity
    pub is_ongoing: Option<bool>,
    /// Urgency level for Ask signals: "low", "medium", "high", "critical"
    pub urgency: Option<String>,
    /// What is needed (for Ask signals)
    pub what_needed: Option<String>,
    /// Goal description (for Ask signals)
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
}

/// The full extraction response from the LLM.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExtractionResponse {
    #[serde(default, deserialize_with = "deserialize_signals")]
    pub signals: Vec<ExtractedSignal>,
}

/// Handle LLM returning signals as either a proper JSON array or a stringified JSON array.
fn deserialize_signals<'de, D>(deserializer: D) -> std::result::Result<Vec<ExtractedSignal>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Array(_) => {
            serde_json::from_value(value).map_err(de::Error::custom)
        }
        serde_json::Value::String(ref s) => {
            serde_json::from_str(s).map_err(de::Error::custom)
        }
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
    pub fn new(anthropic_api_key: &str, city_name: &str, default_lat: f64, default_lng: f64) -> Self {
        let claude = Claude::new(anthropic_api_key, "claude-haiku-4-5-20251001");
        let system_prompt = build_system_prompt(city_name, default_lat, default_lng);
        Self { claude, system_prompt }
    }

    /// Create an extractor with a pre-built system prompt (for genome-driven evolution).
    pub fn with_system_prompt(anthropic_api_key: &str, system_prompt: String) -> Self {
        let claude = Claude::new(anthropic_api_key, "claude-haiku-4-5-20251001");
        Self { claude, system_prompt }
    }

    /// Extract signals from page content (internal implementation).
    async fn extract_impl(
        &self,
        content: &str,
        source_url: &str,
    ) -> Result<ExtractionResult> {
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
            .extract("claude-haiku-4-5-20251001", &self.system_prompt, &user_prompt)
            .await?;

        // Collect implied queries before converting to nodes
        let implied_queries: Vec<String> = response
            .signals
            .iter()
            .flat_map(|s| s.implied_queries.iter().cloned())
            .collect();

        let now = Utc::now();
        let mut nodes = Vec::new();

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
                        _ => GeoPrecision::City,
                    };
                    Some(GeoPoint { lat, lng, precision })
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

            let meta = NodeMeta {
                id: Uuid::new_v4(),
                title: signal.title.clone(),
                summary: signal.summary.clone(),
                sensitivity,
                confidence: 0.0, // Will be computed by QualityScorer
                freshness_score: 1.0, // Fresh at extraction time
                corroboration_count: 0,
                location,
                location_name: signal.location_name.clone(),
                source_url: effective_source_url.clone(),
                extracted_at: now,
                last_confirmed_active: now,
                source_diversity: 1,
                external_ratio: 0.0,
                cause_heat: 0.0,
                mentioned_actors,
                implied_queries: signal.implied_queries.clone(),
            };

            let node = match signal.signal_type.as_str() {
                "event" => {
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

                    Node::Event(EventNode {
                        meta,
                        starts_at,
                        ends_at,
                        action_url: signal
                            .action_url
                            .unwrap_or(effective_source_url),
                        organizer: signal.organizer,
                        is_recurring: signal.is_recurring.unwrap_or(false),
                    })
                }
                "give" => Node::Give(GiveNode {
                    meta,
                    action_url: signal
                        .action_url
                        .unwrap_or(effective_source_url),
                    availability: signal.availability,
                    is_ongoing: signal.is_ongoing.unwrap_or(true),
                }),
                "ask" => {
                    let urgency = match signal.urgency.as_deref() {
                        Some("high") => Urgency::High,
                        Some("critical") => Urgency::Critical,
                        Some("low") => Urgency::Low,
                        _ => Urgency::Medium,
                    };
                    Node::Ask(AskNode {
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
                    warn!(signal_type = other, title = signal.title, "Unknown signal type, skipping");
                    continue;
                }
            };

            nodes.push(node);
        }

        info!(
            source_url,
            count = nodes.len(),
            implied_queries = implied_queries.len(),
            "Extracted signals"
        );
        Ok(ExtractionResult { nodes, implied_queries })
    }
}

#[async_trait::async_trait]
impl SignalExtractor for Extractor {
    async fn extract(&self, content: &str, source_url: &str) -> Result<ExtractionResult> {
        self.extract_impl(content, source_url).await
    }
}

pub fn build_system_prompt(city_name: &str, _default_lat: f64, _default_lng: f64) -> String {
    let today = Utc::now().format("%Y-%m-%d").to_string();
    format!(
        r#"You are a signal extractor for {city_name}.

Your job: find real problems and the people addressing them. The most valuable signal is a TENSION (something out of alignment in community or ecological life) paired with RESPONSES (the gives, asks, events, and notices that address it). A food shelf addressing a food desert, a cleanup responding to pollution, a legal aid hotline responding to enforcement activity — these tension-response pairs are what gets people engaged in real-world problems.

## Signal Types (ranked by value)

**Highest — Tension + Response pairs:**
- **Tension**: A community conflict, systemic problem, or ecological misalignment. Has severity and what would help. NOT the narrative itself — the underlying structural issue.
- **Give**: A resource, service, or offering that addresses a need (food shelf, legal aid, habitat restoration). Has availability and contact info.
- **Ask**: A call for help that mobilizes action (volunteer drives, donation needs, citizen science). Has what's needed and how to help.
- **Event**: A gathering where people organize around a problem (town halls, cleanups, community meetings). Has start time and location.

**Also valuable — standalone responses with an implicit tension:**
- A "feed people on Sundays" program implies food insecurity. Extract it as a Give even without an explicit tension on the page.
- A river cleanup implies pollution. Extract it as an Event.

**Lower priority — routine community activity:**
- Community calendar events, recurring worship services, social gatherings. Still extract these, but they matter less than signals that point to a real problem someone can help with.

**Context signals:**
- **Notice**: An official advisory or policy change. Has source authority and effective date.

If content doesn't map to one of these types, return an empty signals array.

## Extracting from News and Crisis Content
When a page describes a crisis, conflict, or problem, extract BOTH the underlying tension AND the community responses:
- The structural problem → Tension (always include what_would_help)
- Legal aid hotlines, know-your-rights resources → Give
- Community meetings, workshops, public hearings → Event
- Volunteer calls, donation drives, petitions → Ask
- Official advisories, policy changes → Notice

If a page describes only a problem with no actionable response, still extract the Tension (with what_would_help) — the system will seek responses separately.

## Sensitivity
- **sensitive**: Enforcement activity, vulnerable populations, sanctuary networks
- **elevated**: Organizing, advocacy, political action
- **general**: Everything else

## Location
- Extract the most specific location possible from the content
- Only provide latitude/longitude if you can identify a SPECIFIC place (building, park, intersection, venue)
- If the signal is city-wide or you can't determine a specific location, omit latitude/longitude entirely (null)
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
- category: "housing", "safety", "equity", "infrastructure", "environment", "governance", "health"
- what_would_help: What response would address this tension (e.g. "affordable housing policy", "community oversight board")

## Source URL
- When extracting from multiple posts (e.g. "--- Post 1 (https://...) ---"), set source_url to the specific post URL the signal came from
- This lets readers navigate directly to the original post, not just the profile

## Action URLs
- Include the most relevant action URL (registration, donation, event page)
- If none exists, use the source page URL

## Mentioned Actors
Extract the names of organizations, groups, government bodies, or notable individuals mentioned in each signal. These become Actor nodes in the graph for "who's involved" queries.
- Include: nonprofits, city departments, coalitions, community groups, churches, businesses offering help
- Exclude: generic references like "the city" or "local officials" unless a specific body is named

## Contact Information
Preserve organization phone numbers, emails, and addresses — these are public broadcast information, not private data. Strip only genuinely private individual information (personal cell phones, home addresses, SSNs).

## IMPLIED QUERIES (optional — signal quality is always the priority)

For signals with a clear community tension connection, provide up to 3
implied_queries — searches that would discover RELATED community signals
by expanding outward from this one.

- An Ask for donations → search for the service that helps affected people directly
- A Give serving a population → search for what else that population needs
- An Event at a venue → search for other community events at that venue
- A Tension → search for who's responding and where people are gathering
- A Notice about policy → search for who's affected and how they're organizing

Always include the city name or neighborhood. Target specific organizations,
services, and events — not news articles.

DO NOT provide implied_queries for routine community events (farmers markets,
worship services, recurring social gatherings) that have no tension connection.
Return an empty array for these."#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_prompt_includes_tension() {
        let prompt = build_system_prompt("Minneapolis", 44.9778, -93.2650);
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
        };

        assert_eq!(signal.signal_type, "tension");
        assert_eq!(signal.what_would_help.as_deref(), Some("affordable housing policy"));
        assert_eq!(signal.category.as_deref(), Some("housing"));
    }

    #[test]
    fn missing_availability_is_none_not_placeholder() {
        use rootsignal_common::{GiveNode, NodeMeta, SensitivityLevel};
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
            last_confirmed_active: chrono::Utc::now(),
            source_diversity: 1,
            external_ratio: 0.0,
            cause_heat: 0.0,
            mentioned_actors: vec![],
            implied_queries: vec![],
        };
        let give = GiveNode {
            meta,
            action_url: "https://example.com".to_string(),
            availability: None,
            is_ongoing: true,
        };
        assert!(give.availability.is_none());
    }

    #[test]
    fn missing_what_needed_is_none_not_placeholder() {
        use rootsignal_common::{AskNode, NodeMeta, SensitivityLevel, Urgency};
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
            last_confirmed_active: chrono::Utc::now(),
            source_diversity: 1,
            external_ratio: 0.0,
            cause_heat: 0.0,
            mentioned_actors: vec![],
            implied_queries: vec![],
        };
        let ask = AskNode {
            meta,
            urgency: Urgency::Medium,
            what_needed: None,
            action_url: None,
            goal: None,
        };
        assert!(ask.what_needed.is_none());
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
        assert_eq!(response.signals[0].implied_queries[0], "immigration legal aid Minneapolis");
    }

    #[test]
    fn extracted_signal_json_missing_implied_queries() {
        let json = r#"{
            "signals": [{
                "signal_type": "event",
                "title": "Farmers market",
                "summary": "Weekly farmers market",
                "sensitivity": "general"
            }]
        }"#;
        let response: ExtractionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.signals.len(), 1);
        assert!(response.signals[0].implied_queries.is_empty(),
            "Missing implied_queries should default to empty vec");
    }

    #[test]
    fn extracted_signal_json_empty_implied_queries() {
        let json = r#"{
            "signals": [{
                "signal_type": "give",
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
            implied_queries: vec![
                "query 1".to_string(),
                "query 2".to_string(),
            ],
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
        let prompt = build_system_prompt("Minneapolis", 44.9778, -93.2650);
        assert!(
            prompt.contains("IMPLIED QUERIES"),
            "system prompt should mention IMPLIED QUERIES section"
        );
        assert!(
            prompt.contains("DO NOT provide implied_queries for routine"),
            "system prompt should warn against routine event queries"
        );
    }
}
