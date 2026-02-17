use ai_client::claude::Claude;
use anyhow::Result;
use chrono::Utc;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{
    AskNode, AudienceRole, EventNode, GeoPoint, GeoPrecision, GiveNode, Node, NodeMeta,
    NoticeNode, SensitivityLevel, Severity, Urgency,
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
    /// Audience roles this signal is relevant to
    pub audience_roles: Vec<String>,
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

    /// Extract civic signals from page content.
    pub async fn extract(
        &self,
        content: &str,
        source_url: &str,
    ) -> Result<Vec<Node>> {
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
            "Extract all civic signals from this web page.\n\nSource URL: {source_url}\n\n---\n\n{content}"
        );

        let response: ExtractionResponse = self
            .claude
            .extract("claude-haiku-4-5-20251001", &self.system_prompt, &user_prompt)
            .await?;

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

            let audience_roles: Vec<AudienceRole> = signal
                .audience_roles
                .iter()
                .filter_map(|s| match s.as_str() {
                    "volunteer" => Some(AudienceRole::Volunteer),
                    "donor" => Some(AudienceRole::Donor),
                    "neighbor" => Some(AudienceRole::Neighbor),
                    "parent" => Some(AudienceRole::Parent),
                    "youth" => Some(AudienceRole::Youth),
                    "senior" => Some(AudienceRole::Senior),
                    "immigrant" => Some(AudienceRole::Immigrant),
                    "steward" => Some(AudienceRole::Steward),
                    "civic_participant" => Some(AudienceRole::CivicParticipant),
                    "skill_provider" => Some(AudienceRole::SkillProvider),
                    _ => None,
                })
                .collect();

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
                source_url: source_url.to_string(),
                extracted_at: now,
                last_confirmed_active: now,
                audience_roles,
            };

            let node = match signal.signal_type.as_str() {
                "event" => {
                    let starts_at = signal
                        .starts_at
                        .as_deref()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or(now);
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
                            .unwrap_or_else(|| source_url.to_string()),
                        organizer: signal.organizer,
                        is_recurring: signal.is_recurring.unwrap_or(false),
                    })
                }
                "give" => Node::Give(GiveNode {
                    meta,
                    action_url: signal
                        .action_url
                        .unwrap_or_else(|| source_url.to_string()),
                    availability: signal.availability.unwrap_or_else(|| "Contact for details".to_string()),
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
                        what_needed: signal.what_needed.unwrap_or_else(|| "Support needed".to_string()),
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
                // Tension signals are not extracted individually — they emerge from
                // signal clustering in the graph (Phase 2). Skip if LLM produces one.
                "tension" => {
                    warn!(
                        source_url,
                        title = signal.title,
                        "LLM produced tension signal, skipping (tensions emerge from clustering)"
                    );
                    continue;
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
            "Extracted civic signals"
        );
        Ok(nodes)
    }
}

fn build_system_prompt(city_name: &str, _default_lat: f64, _default_lng: f64) -> String {
    format!(
        r#"You are a civic signal extractor for {city_name}.

Extract civic signals from web page content. Each signal is something a community member can act on or should be aware of.

## Signal Types

- **Event**: A time-bound gathering. Has start time and location.
- **Give**: An available resource, service, or offering. Has availability and contact info.
- **Ask**: A community need requesting help. Has what's needed and how to help.
- **Notice**: An official advisory or policy change. Has source authority and effective date.

If content doesn't map to one of these types, return an empty signals array.

## Extracting from News and Crisis Content
When a page describes a crisis, conflict, or problem, extract the COMMUNITY RESPONSES — not the narrative itself. The responses are the signals:
- Legal aid hotlines, know-your-rights resources → Give
- Community meetings, workshops, public hearings → Event
- Volunteer calls, donation drives, petitions → Ask
- Official advisories, policy changes → Notice

If a page describes only a problem with no actionable response, return an empty signals array.

## Sensitivity
- **sensitive**: Enforcement activity, vulnerable populations, sanctuary networks
- **elevated**: Organizing, advocacy, political action
- **general**: Everything else

## Audience Roles
Assign one or more: volunteer, donor, neighbor, parent, youth, senior, immigrant, steward, civic_participant, skill_provider

## Location
- Extract the most specific location possible from the content
- Only provide latitude/longitude if you can identify a SPECIFIC place (building, park, intersection, venue)
- If the signal is city-wide or you can't determine a specific location, omit latitude/longitude entirely (null)
- Also provide location_name: the place name as text (e.g. "YWCA Midtown", "Lake Nokomis", "City Hall")
- geo_precision: "exact" for specific addresses/buildings, "neighborhood" for areas

## Timing
- ISO 8601 datetime strings for start/end times
- is_ongoing: true for ongoing services
- is_recurring: true for recurring events

## Notice Fields
- severity: "low", "medium", "high", "critical"
- category: "psa", "policy", "advisory", "enforcement", "health"
- effective_date: ISO 8601 when the notice takes effect
- source_authority: The official body issuing it

## Action URLs
- Include the most relevant action URL (registration, donation, event page)
- If none exists, use the source page URL

## Contact Information
Preserve organization phone numbers, emails, and addresses — these are public broadcast information, not private data. Strip only genuinely private individual information (personal cell phones, home addresses, SSNs)."#
    )
}
