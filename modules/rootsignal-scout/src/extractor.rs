use ai_client::claude::Claude;
use anyhow::Result;
use chrono::Utc;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use rootsignal_common::{
    detect_pii, AskNode, AudienceRole, EventNode, GeoPoint, GeoPrecision, GiveNode, Node,
    NodeMeta, SensitivityLevel, Severity, TensionNode, Urgency,
};

/// What the LLM returns for each extracted signal.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExtractedSignal {
    /// Signal type: "event", "give", "ask", or "tension"
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
    /// Geo precision: "exact", "neighborhood", "city"
    pub geo_precision: Option<String>,
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
}

/// The full extraction response from the LLM.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExtractionResponse {
    #[serde(default)]
    pub signals: Vec<ExtractedSignal>,
}

// StructuredOutput is auto-implemented via blanket impl for JsonSchema + DeserializeOwned

const EXTRACTION_SYSTEM_PROMPT: &str = r#"You are a civic signal extractor for the Twin Cities (Minneapolis-St. Paul, Minnesota).

Your job: extract actionable civic signals from web page content. Each signal should be one of these types:

- **Event**: A time-bound gathering. "Show up at a time/place." Examples: park cleanup Saturday, city council hearing, community meeting.
- **Give**: An ongoing resource or offering available to people. "This is available to you." Examples: food shelf hours, tool library, free tax prep, repair cafe.
- **Ask**: A community need — someone needs something. "We need something from you." Examples: volunteers needed, GoFundMe, donation drive, sandbag volunteers.
- **Tension**: Civic context or stress — not a call to action, but important to know. "This is happening." Examples: zoning fight, school closure debate, infrastructure dispute, policy change.

## Classification Rules
- If people show up at a specific time/place → Event
- If something is available/offered to the community → Give
- If the community is asked for help/support/resources → Ask
- If it's civic context, conflict, or stress with no clear action → Tension

## Sensitivity Classification
- **sensitive**: Mentions enforcement (ICE, police operations, raids), vulnerable populations, sanctuary networks
- **elevated**: Mentions organizing, advocacy, protest, boycott, political action
- **general**: Everything else (volunteer events, cleanups, public meetings, food shelves)

## Audience Roles
Assign one or more: volunteer, donor, neighbor, parent, youth, senior, immigrant, steward, civic_participant, skill_provider

## PII Scrubbing — CRITICAL
- STRIP all personal names (unless the person is a public figure or elected official)
- STRIP phone numbers, email addresses, home addresses
- STRIP medical details, immigration status, financial details
- RETAIN organization names, public venue names, event dates/times

## Location
- Extract the most specific location possible (venue address, intersection, neighborhood, city)
- For Twin Cities signals, default to Minneapolis (44.9778, -93.2650) or St. Paul (44.9537, -93.0900) if only the city is known
- Set geo_precision: "exact" for specific addresses, "neighborhood" for neighborhoods/zip codes, "city" for city-level

## Timing
- Extract start/end times as ISO 8601 datetime strings
- For ongoing services, set is_ongoing: true instead of specific times
- For recurring events, set is_recurring: true

## Action URLs
- Include the most relevant action URL (registration link, donation link, event page)
- If no specific action URL exists, use the source page URL
- Tension signals typically have no action_url

Return ALL civic signals found on the page. Do not filter by quality — extract everything civic."#;

pub struct Extractor {
    claude: Claude,
}

impl Extractor {
    pub fn new(anthropic_api_key: &str) -> Self {
        let claude = Claude::new(anthropic_api_key, "claude-haiku-4-5-20251001");
        Self { claude }
    }

    /// Extract civic signals from page content.
    pub async fn extract(
        &self,
        content: &str,
        source_url: &str,
        source_trust: f32,
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
            .extract("claude-haiku-4-5-20251001", EXTRACTION_SYSTEM_PROMPT, &user_prompt)
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

            // PII check on title + summary
            let combined = format!("{} {}", signal.title, signal.summary);
            let pii = detect_pii(&combined);
            if !pii.is_empty() {
                warn!(
                    source_url,
                    title = signal.title,
                    pii_findings = ?pii,
                    "PII detected in extraction, attempting re-scrub"
                );
                // Try scrubbing by just skipping this signal
                // In production, we'd re-extract with a stronger prompt
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
                source_trust,
                freshness_score: 1.0, // Fresh at extraction time
                corroboration_count: 0,
                location,
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
                "tension" => {
                    let severity = match signal.severity.as_deref() {
                        Some("high") => Severity::High,
                        Some("critical") => Severity::Critical,
                        Some("low") => Severity::Low,
                        _ => Severity::Medium,
                    };
                    Node::Tension(TensionNode { meta, severity })
                }
                other => {
                    warn!(signal_type = other, "Unknown signal type, defaulting to Tension");
                    Node::Tension(TensionNode {
                        meta,
                        severity: Severity::Medium,
                    })
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
