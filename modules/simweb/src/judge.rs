//! LLM Judge — evaluates Scout's output against ground truth.

use ai_client::Claude;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::prompt;
use crate::world::World;

const SONNET_MODEL: &str = "claude-sonnet-4-20250514";

/// Criteria for judge evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JudgeCriteria {
    pub checks: Vec<String>,
    pub pass_threshold: f32,
    pub critical_categories: Vec<String>,
}

/// The judge's evaluation of agent output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Verdict {
    pub pass: bool,
    pub score: f32,
    pub reasoning: String,
    pub issues: Vec<Issue>,
}

/// A single issue found during evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub severity: Severity,
    pub category: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Severity {
    Critical,
    Warning,
    Info,
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let status = if self.pass { "PASS" } else { "FAIL" };
        writeln!(f, "Verdict: {status} (score: {:.2})", self.score)?;
        writeln!(f, "Reasoning: {}", self.reasoning)?;
        if !self.issues.is_empty() {
            writeln!(f, "Issues:")?;
            for issue in &self.issues {
                writeln!(
                    f,
                    "  [{:?}] {}: {}",
                    issue.severity, issue.category, issue.description
                )?;
            }
        }
        Ok(())
    }
}

/// LLM-based judge that evaluates agent output against ground truth.
pub struct Judge {
    claude: Claude,
}

impl Judge {
    pub fn new(api_key: &str) -> Self {
        Self {
            claude: Claude::new(api_key, SONNET_MODEL),
        }
    }

    /// Evaluate agent output against the world description and criteria.
    pub async fn evaluate(
        &self,
        world: &World,
        criteria: &JudgeCriteria,
        agent_output: &str,
    ) -> Result<Verdict> {
        let system = prompt::judge_system();
        let user = prompt::judge_user(world, &criteria.checks, agent_output);

        info!(world = world.name, checks = criteria.checks.len(), "Judge evaluating");

        let response = self.claude.chat_completion(system, &user).await?;

        let verdict = parse_verdict(&response, criteria)?;

        info!(
            world = world.name,
            pass = verdict.pass,
            score = verdict.score,
            issues = verdict.issues.len(),
            "Judge verdict"
        );

        Ok(verdict)
    }
}

fn parse_verdict(response: &str, criteria: &JudgeCriteria) -> Result<Verdict> {
    let json_str = response.trim();
    let json_str = json_str
        .strip_prefix("```json")
        .or_else(|| json_str.strip_prefix("```"))
        .and_then(|s| s.strip_suffix("```"))
        .unwrap_or(json_str);

    let mut verdict: Verdict = serde_json::from_str(json_str).map_err(|e| {
        warn!(error = %e, response = response, "Failed to parse judge response");
        anyhow!("Failed to parse judge verdict: {e}")
    })?;

    // Override pass/fail based on threshold (don't trust the LLM's boolean)
    verdict.pass = verdict.score >= criteria.pass_threshold;

    // Also fail if any critical category has Critical-severity issues
    if !criteria.critical_categories.is_empty() {
        let has_critical_failure = verdict.issues.iter().any(|issue| {
            issue.severity == Severity::Critical
                && criteria
                    .critical_categories
                    .iter()
                    .any(|cat| issue.category.to_lowercase().contains(&cat.to_lowercase()))
        });
        if has_critical_failure {
            verdict.pass = false;
        }
    }

    Ok(verdict)
}

/// Generate a random World using Sonnet (for Tier 3 random discovery tests).
pub async fn generate_random_world(api_key: &str) -> Result<World> {
    let claude = Claude::new(api_key, SONNET_MODEL);
    let system = prompt::world_gen_system();
    let user = prompt::world_gen_user();

    info!("Generating random world for discovery test");
    let response = claude.chat_completion(system, user).await?;

    let json_str = response.trim();
    let json_str = json_str
        .strip_prefix("```json")
        .or_else(|| json_str.strip_prefix("```"))
        .and_then(|s| s.strip_suffix("```"))
        .unwrap_or(json_str);

    let world: World = serde_json::from_str(json_str)
        .map_err(|e| anyhow!("Failed to parse generated world: {e}"))?;

    info!(name = world.name, sites = world.sites.len(), "Random world generated");
    Ok(world)
}
