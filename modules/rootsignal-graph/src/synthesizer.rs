use ai_client::claude::Claude;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::info;

use rootsignal_common::{ActionGuidance, StoryArc, StoryCategory, StorySynthesis};

/// LLM response schema for story synthesis.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StorySynthesisResponse {
    /// A newspaper-quality headline (max 100 chars)
    pub headline: String,
    /// 2-4 sentence lede summarizing the story
    pub lede: String,
    /// 3-6 sentence narrative giving full context
    pub narrative: String,
    /// Guidance for specific audience roles
    pub action_guidance: Vec<ActionGuidanceResponse>,
    /// Key organizations/groups involved
    pub key_entities: Vec<String>,
    /// Story category
    pub category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ActionGuidanceResponse {
    /// 1-2 sentences of what someone can do
    pub guidance: String,
    /// URLs for taking action
    pub action_urls: Vec<String>,
}

/// Compute story arc from velocity, age, and previous arc. Pure function, no LLM.
///
/// When `was_fading` is true and new activity arrives (velocity > 0), the story
/// gets the `Resurgent` arc — editorially interesting because something that
/// died came back.
pub fn compute_arc(velocity: f64, age_days: f64, was_fading: bool) -> StoryArc {
    if was_fading && velocity > 0.0 {
        StoryArc::Resurgent
    } else if age_days < 7.0 {
        StoryArc::Emerging
    } else if velocity > 0.5 {
        StoryArc::Growing
    } else if velocity <= -0.3 {
        StoryArc::Fading
    } else {
        StoryArc::Stable
    }
}

fn parse_category(s: &str) -> StoryCategory {
    match s.to_lowercase().as_str() {
        "resource" => StoryCategory::Resource,
        "gathering" => StoryCategory::Gathering,
        "crisis" => StoryCategory::Crisis,
        "governance" => StoryCategory::Governance,
        "stewardship" => StoryCategory::Stewardship,
        _ => StoryCategory::Community,
    }
}

/// Signal metadata passed to the synthesizer.
pub struct SynthesisInput {
    pub title: String,
    pub summary: String,
    pub node_type: String,
    pub source_url: String,
    pub action_url: Option<String>,
}

pub struct Synthesizer {
    anthropic_api_key: String,
}

impl Synthesizer {
    pub fn new(anthropic_api_key: &str) -> Self {
        Self {
            anthropic_api_key: anthropic_api_key.to_string(),
        }
    }

    /// Synthesize a newspaper-quality story from its constituent signals.
    pub async fn synthesize(
        &self,
        headline: &str,
        signals: &[SynthesisInput],
        velocity: f64,
        age_days: f64,
    ) -> Result<StorySynthesis, Box<dyn std::error::Error + Send + Sync>> {
        self.synthesize_with_context(headline, signals, velocity, age_days, false, None)
            .await
    }

    /// Synthesize with additional context about story state.
    pub async fn synthesize_with_context(
        &self,
        headline: &str,
        signals: &[SynthesisInput],
        velocity: f64,
        age_days: f64,
        was_fading: bool,
        extra_context: Option<&str>,
    ) -> Result<StorySynthesis, Box<dyn std::error::Error + Send + Sync>> {
        let arc = compute_arc(velocity, age_days, was_fading);

        let signal_descriptions: Vec<String> = signals
            .iter()
            .take(20)
            .map(|s| {
                let source_part = if s.source_url.is_empty() {
                    String::new()
                } else {
                    format!(" (source: {})", s.source_url)
                };
                let action_part = s
                    .action_url
                    .as_deref()
                    .map(|u| format!(" (action: {u})"))
                    .unwrap_or_default();
                format!(
                    "- [{}] {}: {}{}{}",
                    s.node_type, s.title, s.summary, source_part, action_part
                )
            })
            .collect();

        let context_section = extra_context
            .map(|c| format!("\nEditorial context:\n{c}\n"))
            .unwrap_or_default();

        let prompt = format!(
            r#"You are writing for a community newspaper. This story cluster was originally headlined: "{headline}"
{context_section}
Constituent signals:
{signals}

Write a story synthesis as structured JSON. The synthesis should:
1. headline: A compelling, specific headline (max 100 chars). Avoid generic labels.
2. lede: 2-4 sentences capturing the essence — who, what, where, why it matters to community members.
3. narrative: 3-6 sentences giving fuller context. Connect the signals into a coherent story.
4. action_guidance: A list of specific actions someone can take. Include action_urls from the signals where applicable.
5. key_entities: Names of organizations, groups, or individuals mentioned across the signals.
6. category: One of: resource, gathering, crisis, governance, stewardship, community

Write for community members, not journalists. Be specific, not generic.
GROUNDING: Only include facts present in the signals above. Do not invent details, organizations, or events not mentioned. Use source URLs to verify claims."#,
            signals = signal_descriptions.join("\n"),
        );

        let claude = Claude::new(&self.anthropic_api_key, "claude-haiku-4-5-20251001");
        let response: StorySynthesisResponse = claude
            .extract(
                "claude-haiku-4-5-20251001",
                "You are a community newspaper editor. Produce structured JSON for story synthesis. Respond only with valid JSON matching the schema.",
                &prompt,
            )
            .await?;

        let category = parse_category(&response.category);

        let action_guidance: Vec<ActionGuidance> = response
            .action_guidance
            .into_iter()
            .map(|ag| ActionGuidance {
                guidance: ag.guidance,
                action_urls: ag.action_urls,
            })
            .collect();

        info!(
            headline = response.headline,
            category = %category,
            arc = %arc,
            entities = response.key_entities.len(),
            guidance_roles = action_guidance.len(),
            "Story synthesis complete"
        );

        Ok(StorySynthesis {
            headline: response.headline,
            lede: response.lede,
            narrative: response.narrative,
            action_guidance,
            key_entities: response.key_entities,
            category,
            arc,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_arc_emerging() {
        assert_eq!(compute_arc(0.0, 3.0, false), StoryArc::Emerging);
        assert_eq!(compute_arc(1.0, 6.0, false), StoryArc::Emerging);
    }

    #[test]
    fn test_compute_arc_growing() {
        assert_eq!(compute_arc(0.6, 10.0, false), StoryArc::Growing);
        assert_eq!(compute_arc(1.0, 30.0, false), StoryArc::Growing);
    }

    #[test]
    fn test_compute_arc_fading() {
        assert_eq!(compute_arc(-0.3, 10.0, false), StoryArc::Fading);
        assert_eq!(compute_arc(-1.0, 20.0, false), StoryArc::Fading);
    }

    #[test]
    fn test_compute_arc_stable() {
        assert_eq!(compute_arc(0.0, 10.0, false), StoryArc::Stable);
        assert_eq!(compute_arc(0.3, 14.0, false), StoryArc::Stable);
        assert_eq!(compute_arc(-0.2, 14.0, false), StoryArc::Stable);
    }

    #[test]
    fn test_compute_arc_resurgent() {
        // Was fading + positive velocity = resurgent
        assert_eq!(compute_arc(0.1, 20.0, true), StoryArc::Resurgent);
        assert_eq!(compute_arc(1.0, 30.0, true), StoryArc::Resurgent);
        // Was fading but no new activity = still fading
        assert_eq!(compute_arc(-0.5, 20.0, true), StoryArc::Fading);
        // Was fading, zero velocity = stable (not resurgent)
        assert_eq!(compute_arc(0.0, 20.0, true), StoryArc::Stable);
    }

    #[test]
    fn test_parse_category() {
        assert_eq!(parse_category("resource"), StoryCategory::Resource);
        assert_eq!(parse_category("CRISIS"), StoryCategory::Crisis);
        assert_eq!(parse_category("unknown"), StoryCategory::Community);
    }
}
