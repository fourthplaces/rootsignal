//! Self-improvement loop — analyzes test failures and generates adversarial scenarios.

use ai_client::Claude;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::world::World;
use crate::judge::JudgeCriteria;

const SONNET_MODEL: &str = "claude-sonnet-4-6-20250514";

/// A test failure capturing verdict and audit details.
#[derive(Debug, Clone, Serialize)]
pub struct TestFailure {
    pub scenario_name: String,
    pub verdict_pass: bool,
    pub verdict_score: f32,
    pub verdict_reasoning: String,
    pub audit_failures: Vec<String>,
}

/// A blind spot identified from test failures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlindSpot {
    pub category: String,
    pub description: String,
    pub source: String,
    pub severity: BlindSpotSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BlindSpotSeverity {
    High,
    Medium,
    Low,
}

/// A suggestion for fixing a prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptFix {
    pub target: String,
    pub issue: String,
    pub suggestion: String,
}

/// Complete improvement report from analyzing test failures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImprovementReport {
    pub blind_spots: Vec<BlindSpot>,
    pub suggested_scenarios: Vec<World>,
    pub prompt_suggestions: Vec<PromptFix>,
}

/// Analyzes test failures and generates adversarial scenarios targeting blind spots.
pub struct Improver {
    claude: Claude,
}

impl Improver {
    pub fn new(api_key: &str) -> Self {
        Self {
            claude: Claude::new(api_key, SONNET_MODEL),
        }
    }

    /// Analyze test failures to identify blind spots and generate new scenarios.
    pub async fn analyze(&self, failures: Vec<TestFailure>) -> Result<ImprovementReport> {
        if failures.is_empty() {
            return Ok(ImprovementReport {
                blind_spots: vec![],
                suggested_scenarios: vec![],
                prompt_suggestions: vec![],
            });
        }

        let failures_json =
            serde_json::to_string_pretty(&failures).map_err(|e| anyhow!("serialize: {e}"))?;

        // Step 1: Identify blind spots
        let analysis = self.identify_blind_spots(&failures_json).await?;

        // Step 2: Generate adversarial scenarios for each blind spot
        let mut scenarios = Vec::new();
        for spot in &analysis.blind_spots {
            match self.generate_scenario(spot).await {
                Ok(world) => scenarios.push(world),
                Err(e) => {
                    tracing::warn!(category = %spot.category, error = %e, "Failed to generate scenario for blind spot");
                }
            }
        }

        Ok(ImprovementReport {
            blind_spots: analysis.blind_spots,
            suggested_scenarios: scenarios,
            prompt_suggestions: analysis.prompt_suggestions,
        })
    }

    /// Generate judge criteria for a generated scenario.
    pub async fn criteria_for(&self, world: &World) -> Result<JudgeCriteria> {
        let world_json =
            serde_json::to_string_pretty(world).map_err(|e| anyhow!("serialize: {e}"))?;

        let system = "You generate evaluation criteria for scout, a civic signal agent whose core job \
                      is finding tensions (real problems) and the responses (gives/asks/events) that \
                      address them. Given a World scenario, return JSON with checks that scout should \
                      pass. Focus on tension-response completeness and what would expose the blind spot.";

        let user = format!(
            "Generate JudgeCriteria for this scenario:\n\n{world_json}\n\n\
             Return JSON: {{\"checks\": [\"string\", ...], \"pass_threshold\": 0.6, \
             \"critical_categories\": [\"string\", ...]}}"
        );

        let response = self.claude.chat_completion(system, &user).await?;
        let json_str = strip_code_fence(&response);

        #[derive(Deserialize)]
        struct CriteriaResponse {
            checks: Vec<String>,
            pass_threshold: f32,
            critical_categories: Vec<String>,
        }

        let parsed: CriteriaResponse = serde_json::from_str(json_str)
            .map_err(|e| anyhow!("Failed to parse criteria: {e}"))?;

        Ok(JudgeCriteria {
            checks: parsed.checks,
            pass_threshold: parsed.pass_threshold,
            critical_categories: parsed.critical_categories,
        })
    }

    async fn identify_blind_spots(&self, failures_json: &str) -> Result<AnalysisResponse> {
        let system = "You analyze test failures from scout, a civic signal agent whose core job is \
                      the TENSION-RESPONSE CYCLE: find tensions (problems in community or ecological \
                      life) and the responses (gives/asks/events) that address them. \
                      Blind spots fall into: missed tensions, missed responses to known tensions, \
                      broken tension-response linking, or hallucinated signals. Return structured JSON.";

        let user = format!(
            "Given these test failures from scout:\n\n\
             {failures_json}\n\n\
             For each failure, identify which part of the tension-response cycle broke:\n\
             1. Did scout miss a TENSION (a real problem in the community)?\n\
             2. Did it miss a RESPONSE (give/ask/event) that addresses a tension?\n\
             3. Did it fail to LINK a response to its underlying tension?\n\
             4. Did it hallucinate signals not grounded in the source material?\n\
             5. What adversarial scenario would stress-test this weakness?\n\
             6. Any prompt improvements that could help?\n\n\
             Return JSON: {{\n  \
               \"blind_spots\": [{{\"category\": \"string\", \"description\": \"string\", \
               \"source\": \"scenario name\", \"severity\": \"High|Medium|Low\"}}],\n  \
               \"prompt_suggestions\": [{{\"target\": \"extractor|judge|etc\", \
               \"issue\": \"string\", \"suggestion\": \"string\"}}]\n}}"
        );

        let response = self.claude.chat_completion(system, &user).await?;
        let json_str = strip_code_fence(&response);

        serde_json::from_str(json_str)
            .map_err(|e| anyhow!("Failed to parse blind spot analysis: {e}"))
    }

    async fn generate_scenario(&self, blind_spot: &BlindSpot) -> Result<World> {
        let spot_json =
            serde_json::to_string_pretty(blind_spot).map_err(|e| anyhow!("serialize: {e}"))?;

        let system = "You generate World definitions for testing scout, a civic signal agent. \
                      A World describes a simulated city with websites, social profiles, and \
                      ground-truth facts. The scenario must target a specific blind spot in scout's \
                      tension-response extraction so that a system with this weakness WILL fail.";

        let user = format!(
            "Generate a World definition targeting this blind spot:\n\n{spot_json}\n\n\
             The scenario must be designed so a system with this weakness WILL fail.\n\
             Constraints: 3-8 sites, 1-3 social profiles, 3-7 facts, realistic US geography.\n\n\
             Return JSON matching this schema:\n\
             {{\n  \"name\": \"string\",\n  \"description\": \"string\",\n  \
             \"facts\": [{{\"text\": \"string\", \"referenced_by\": [\"url\"], \"category\": \"string\"}}],\n  \
             \"sites\": [{{\"url\": \"https://...\", \"kind\": \"string\", \
             \"content_description\": \"string\", \"published\": \"YYYY-MM-DD or null\", \
             \"links_to\": [\"url\"]}}],\n  \
             \"social_profiles\": [{{\"platform\": \"Instagram|Reddit|Facebook\", \
             \"identifier\": \"string\", \"persona\": \"string\", \"post_count\": number}}],\n  \
             \"topics\": [\"string\"],\n  \
             \"geography\": {{\"city\": \"string\", \"state_or_region\": \"string\", \
             \"country\": \"US\", \"local_terms\": [\"string\"], \
             \"center_lat\": number, \"center_lng\": number}}\n}}"
        );

        let response = self.claude.chat_completion(system, &user).await?;
        let json_str = strip_code_fence(&response);

        serde_json::from_str(json_str)
            .map_err(|e| anyhow!("Failed to parse generated world: {e}"))
    }
}

#[derive(Deserialize)]
struct AnalysisResponse {
    blind_spots: Vec<BlindSpot>,
    prompt_suggestions: Vec<PromptFix>,
}

fn strip_code_fence(s: &str) -> &str {
    let s = s.trim();
    let s = s
        .strip_prefix("```json")
        .or_else(|| s.strip_prefix("```"))
        .and_then(|s| s.strip_suffix("```"))
        .unwrap_or(s);
    s.trim()
}
