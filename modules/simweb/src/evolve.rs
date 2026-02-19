//! Evolver — autonomous prompt evolution via mutation and selection.
//!
//! Goal: improve scout's ability to complete the tension-response cycle —
//! find tensions in the world, then find the asks/gives/events that address them.

use std::future::Future;

use anyhow::{anyhow, Result};
use serde::Deserialize;
use tracing::{info, warn};

use ai_client::Claude;

use crate::fitness::{is_improvement, score_genome};
use crate::genome::{ScenarioScore, ScoutGenome};
use crate::improve::Improver;
use crate::judge::Verdict;
use crate::scenario_gym::ScenarioGym;

const SONNET_MODEL: &str = "claude-sonnet-4-20250514";

/// Configuration for an evolution run.
pub struct EvolutionConfig {
    pub max_generations: u32,
    pub mutations_per_generation: u32,
}

impl Default for EvolutionConfig {
    fn default() -> Self {
        Self {
            max_generations: 3,
            mutations_per_generation: 2,
        }
    }
}

/// Result of an evolution run.
pub struct EvolutionResult {
    pub champion: ScoutGenome,
    pub history: Vec<ScoutGenome>,
    pub scenarios_promoted: usize,
}

/// Audit report summary (passed to the evolver from the test harness).
pub struct AuditSummary {
    pub passed: usize,
    pub total: usize,
}

/// The evolver: mutates prompts, evaluates against scenarios, keeps winners.
pub struct Evolver {
    claude: Claude,
    improver: Improver,
}

impl Evolver {
    pub fn new(api_key: &str) -> Self {
        Self {
            claude: Claude::new(api_key, SONNET_MODEL),
            improver: Improver::new(api_key),
        }
    }

    /// Run the evolution loop starting from a baseline genome.
    pub async fn evolve_from<F, Fut>(
        &self,
        baseline: ScoutGenome,
        gym: &mut ScenarioGym,
        config: EvolutionConfig,
        mut run_fn: F,
    ) -> Result<EvolutionResult>
    where
        F: FnMut(&ScoutGenome, &crate::scenario_gym::ScenarioEntry) -> Fut,
        Fut: Future<Output = Result<(Verdict, AuditSummary)>>,
    {
        let mut history: Vec<ScoutGenome> = Vec::new();
        let mut scenarios_promoted = 0usize;

        // Evaluate baseline
        info!(generation = 0, "Evaluating baseline genome");
        let baseline_scores = self.evaluate_genome(&baseline, gym, &mut run_fn).await?;
        let baseline_fitness = score_genome(&baseline_scores, None);
        let mut champion = baseline.clone();
        champion.fitness = Some(baseline_fitness.clone());
        history.push(champion.clone());

        info!(
            fitness = baseline_fitness.total,
            "Baseline fitness established"
        );

        // Evolution generations
        for gen in 1..=config.max_generations {
            info!(generation = gen, "Starting generation");

            // Collect failure details for mutation
            let failures = collect_failures(&champion);

            if failures.is_empty() {
                info!(
                    generation = gen,
                    "No failures to target — champion is perfect"
                );
                break;
            }

            // Generate mutations
            let mutations = self
                .generate_mutations(
                    &champion.extractor_prompt,
                    &failures,
                    config.mutations_per_generation,
                )
                .await;

            // Clone champion scores to avoid borrow conflict with champion reassignment
            let champion_scores: Option<Vec<ScenarioScore>> =
                champion.fitness.as_ref().map(|f| f.scenario_scores.clone());

            for mutation in mutations {
                let mutant = champion.child_extractor(mutation.prompt, mutation.reasoning);

                info!(mutant_id = mutant.id.as_str(), "Evaluating mutant");

                let scores = match self.evaluate_genome(&mutant, gym, &mut run_fn).await {
                    Ok(s) => s,
                    Err(e) => {
                        warn!(error = %e, "Failed to evaluate mutant, skipping");
                        continue;
                    }
                };

                let fitness = score_genome(&scores, champion_scores.as_deref());
                let mut evaluated_mutant = mutant;
                evaluated_mutant.fitness = Some(fitness.clone());
                history.push(evaluated_mutant.clone());

                info!(
                    mutant_fitness = fitness.total,
                    champion_fitness = champion.fitness.as_ref().unwrap().total,
                    regressions = fitness.regressions,
                    "Mutant evaluation complete"
                );

                if is_improvement(&fitness, champion.fitness.as_ref().unwrap()) {
                    info!(
                        old_fitness = champion.fitness.as_ref().unwrap().total,
                        new_fitness = fitness.total,
                        "New champion!"
                    );
                    champion = evaluated_mutant;
                }
            }

            // Generate adversarial scenarios from failures via Improver
            let test_failures: Vec<crate::improve::TestFailure> = collect_test_failures(&champion);
            if !test_failures.is_empty() {
                match self.improver.analyze(test_failures).await {
                    Ok(report) => {
                        for (world, blind_spot) in report
                            .suggested_scenarios
                            .into_iter()
                            .zip(report.blind_spots.iter())
                        {
                            // Check if baseline would fail this scenario
                            let criteria = match self.improver.criteria_for(&world).await {
                                Ok(c) => c,
                                Err(e) => {
                                    warn!(error = %e, "Failed to generate criteria for adversarial scenario");
                                    continue;
                                }
                            };

                            if let Err(e) = gym.promote(
                                world.name.clone(),
                                world,
                                criteria,
                                blind_spot.description.clone(),
                            ) {
                                warn!(error = %e, "Failed to promote scenario");
                            } else {
                                scenarios_promoted += 1;
                            }
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "Improvement analysis failed");
                    }
                }
            }
        }

        Ok(EvolutionResult {
            champion,
            history,
            scenarios_promoted,
        })
    }

    /// Evaluate a genome against all scenarios in the gym.
    async fn evaluate_genome<F, Fut>(
        &self,
        genome: &ScoutGenome,
        gym: &ScenarioGym,
        run_fn: &mut F,
    ) -> Result<Vec<ScenarioScore>>
    where
        F: FnMut(&ScoutGenome, &crate::scenario_gym::ScenarioEntry) -> Fut,
        Fut: Future<Output = Result<(Verdict, AuditSummary)>>,
    {
        let mut scores = Vec::new();

        for scenario in gym.scenarios() {
            let (verdict, audit) = run_fn(genome, scenario)
                .await
                .map_err(|e| anyhow!("Failed to evaluate scenario '{}': {}", scenario.name, e))?;

            scores.push(ScenarioScore {
                name: scenario.name.clone(),
                verdict_pass: verdict.pass,
                verdict_score: verdict.score,
                audit_passed: audit.passed,
                audit_total: audit.total,
            });
        }

        Ok(scores)
    }

    /// Generate targeted mutations using Sonnet.
    async fn generate_mutations(
        &self,
        current_prompt: &str,
        failures: &str,
        count: u32,
    ) -> Vec<Mutation> {
        let system = "\
You improve the system prompt for a signal extraction agent.

The agent's core mission is the TENSION-RESPONSE CYCLE:
1. Find TENSIONS — things out of alignment in community or ecological life
   (housing crisis, food desert, river pollution, declining habitat, safety concerns)
2. Find RESPONSES that address those tensions:
   - Give: resources, services, mutual aid (food shelves, legal aid, habitat restoration programs)
   - Ask: calls for help that mobilize action (volunteer drives, donation needs, citizen science)
   - Event: gatherings where people organize around tensions (town halls, cleanups, restoration days)
   - Notice: official advisories or policy changes related to tensions

A prompt is better if it extracts more tension-response pairs from the same content.
A prompt is worse if it misses tensions, misses the responses to them, or hallucinates signals.

Return JSON array of mutations.";

        let user = format!(
            "## Current Prompt\n{current_prompt}\n\n\
             ## Test Failures\n{failures}\n\n\
             The failures above show scenarios where the agent missed tensions, missed responses \
             to tensions (gives/asks/events that address a community problem), or extracted signals \
             that don't connect to real community needs.\n\n\
             Generate {count} targeted modifications that improve the agent's ability to:\n\
             - Recognize structural tensions in community content\n\
             - Extract the specific resources, services, events, and asks that RESPOND to those tensions\n\
             - Distinguish genuine community responses from noise, spam, or off-topic content\n\n\
             Each mutation must be a COMPLETE modified prompt (not a diff).\n\
             Keep {{city_name}} and {{today}} as template variables.\n\n\
             Return JSON array: [{{\"reasoning\": \"why this change improves tension-response extraction\", \
             \"prompt\": \"full prompt text\"}}]"
        );

        let response = match self.claude.chat_completion(system, &user).await {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "Mutation generation failed");
                return vec![];
            }
        };

        let json_str = strip_code_fence(&response);
        match serde_json::from_str::<Vec<Mutation>>(json_str) {
            Ok(mutations) => mutations,
            Err(e) => {
                warn!(error = %e, "Failed to parse mutations");
                vec![]
            }
        }
    }
}

#[derive(Deserialize)]
struct Mutation {
    reasoning: String,
    prompt: String,
}

/// Collect failure descriptions from champion's fitness for the mutation prompt.
///
/// Scenario names encode what aspect of the tension-response cycle they test,
/// so we include them verbatim — Sonnet can infer what went wrong from names like
/// "tension_response_cycle" or "simmering_cedar_riverside".
fn collect_failures(genome: &ScoutGenome) -> String {
    let fitness = match &genome.fitness {
        Some(f) => f,
        None => return String::new(),
    };

    let failures: Vec<String> = fitness
        .scenario_scores
        .iter()
        .filter(|s| !s.verdict_pass || s.audit_passed < s.audit_total)
        .map(|s| {
            let gap_hint = if s.name.contains("tension") {
                " [tension extraction or tension-response linking]"
            } else if s.name.contains("rural") || s.name.contains("hidden") {
                " [subtle signals in sparse content]"
            } else if s.name.contains("organizing") || s.name.contains("shifting") {
                " [community organizing / response extraction]"
            } else {
                ""
            };
            format!(
                "- {}{}: verdict={} (score={:.2}), audit={}/{} passed",
                s.name, gap_hint, s.verdict_pass, s.verdict_score, s.audit_passed, s.audit_total,
            )
        })
        .collect();

    failures.join("\n")
}

/// Convert champion failures into TestFailure structs for the Improver.
fn collect_test_failures(genome: &ScoutGenome) -> Vec<crate::improve::TestFailure> {
    let fitness = match &genome.fitness {
        Some(f) => f,
        None => return vec![],
    };

    fitness
        .scenario_scores
        .iter()
        .filter(|s| !s.verdict_pass)
        .map(|s| crate::improve::TestFailure {
            scenario_name: s.name.clone(),
            verdict_pass: s.verdict_pass,
            verdict_score: s.verdict_score,
            verdict_reasoning: format!("score={:.2}", s.verdict_score),
            audit_failures: if s.audit_passed < s.audit_total {
                vec![format!(
                    "{}/{} audit checks passed",
                    s.audit_passed, s.audit_total
                )]
            } else {
                vec![]
            },
        })
        .collect()
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
