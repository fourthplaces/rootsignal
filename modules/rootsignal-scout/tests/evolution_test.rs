//! Evolution test — autonomous prompt improvement via mutation and selection.
//!
//! Gated behind `EVOLUTION_LOOP=1` (expensive: ~$3-5 per run).

mod harness;
mod scenarios;

use std::path::PathBuf;
use std::sync::Arc;

use harness::audit::AuditConfig;
use harness::queries::serialize_graph_state;
use harness::{scope_for, TestContext};
use simweb::{
    AuditSummary, EvolutionConfig, Evolver, Judge, JudgeCriteria, ScenarioEntry, ScenarioGym,
    ScenarioSource, ScoutGenome, SimulatedWeb, World,
};

/// Directory for evolution artifacts (gitignored).
fn evolution_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/evolution")
}

/// Directory for accumulated adversarial scenarios.
fn evolution_scenarios_dir() -> PathBuf {
    evolution_dir().join("scenarios")
}

/// All hand-written scenarios from sim_integration.
const ALL_SCENARIOS: &[(&str, fn() -> (World, JudgeCriteria))] = &[
    ("stale_minneapolis", || {
        (
            scenarios::stale_minneapolis::world(),
            scenarios::stale_minneapolis::criteria(),
        )
    }),
    ("organizing_portland", || {
        (
            scenarios::organizing_portland::world(),
            scenarios::organizing_portland::criteria(),
        )
    }),
    ("simmering_cedar_riverside", || {
        (
            scenarios::simmering_cedar_riverside::world(),
            scenarios::simmering_cedar_riverside::criteria(),
        )
    }),
    ("rural_minnesota", || {
        (
            scenarios::rural_minnesota::world(),
            scenarios::rural_minnesota::criteria(),
        )
    }),
    ("hidden_community_minneapolis", || {
        (
            scenarios::hidden_community_minneapolis::world(),
            scenarios::hidden_community_minneapolis::criteria(),
        )
    }),
    ("shifting_ground", || {
        (
            scenarios::shifting_ground::world(),
            scenarios::shifting_ground::criteria(),
        )
    }),
    ("tension_response_cycle", || {
        (
            scenarios::tension_response_cycle::world(),
            scenarios::tension_response_cycle::criteria(),
        )
    }),
    ("tension_discovery_bridge", || {
        (
            scenarios::tension_discovery_bridge::world(),
            scenarios::tension_discovery_bridge::criteria(),
        )
    }),
];

/// Run a single scenario with a genome-driven Scout, returning verdict + audit.
async fn run_scenario_with_genome(
    ctx: &TestContext,
    genome: &ScoutGenome,
    world: &World,
    criteria: &JudgeCriteria,
    scenario_name: &str,
) -> anyhow::Result<(simweb::Verdict, AuditSummary)> {
    let api_key = ctx.anthropic_key();

    // Build SimulatedWeb
    let sim = Arc::new(SimulatedWeb::new(world.clone(), api_key));

    // Build and run Scout with genome's extractor prompt
    let scope = scope_for(world);
    let scout = ctx.sim_scout_with_genome(sim.clone(), scope, genome);
    let stats = scout.run().await?;

    eprintln!(
        "=== {scenario_name} (gen {}) stats ===\n{stats}",
        genome.generation
    );

    // Serialize graph state for judge
    let graph_state = serialize_graph_state(ctx.client()).await;

    // Judge evaluates
    let judge = Judge::new(api_key);
    let verdict = judge.evaluate(world, criteria, &graph_state).await?;

    eprintln!(
        "=== {scenario_name} verdict: {} (score: {:.2}) ===",
        if verdict.pass { "PASS" } else { "FAIL" },
        verdict.score
    );

    // Run structural audit
    let audit_config = AuditConfig::for_sim(world);
    let audit = harness::audit::run_audit(ctx.client(), &audit_config).await;

    Ok((
        verdict,
        AuditSummary {
            passed: audit.passed,
            total: audit.checks.len(),
        },
    ))
}

fn save_json<T: serde::Serialize>(path: &std::path::Path, data: &T) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(data) {
        let _ = std::fs::write(path, json);
    }
}

#[tokio::test]
async fn evolution_loop() {
    if std::env::var("EVOLUTION_LOOP").is_err() {
        eprintln!("Skipping evolution_loop: set EVOLUTION_LOOP=1 to enable");
        return;
    }

    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys or Docker not available");
        return;
    };

    // Load gym: hand-written scenarios + any previously promoted ones
    let hand_written: Vec<ScenarioEntry> = ALL_SCENARIOS
        .iter()
        .map(|&(name, factory)| {
            let (world, criteria) = factory();
            ScenarioEntry {
                name: name.to_string(),
                world,
                criteria,
                source: ScenarioSource::HandWritten,
            }
        })
        .collect();
    let mut gym = ScenarioGym::load(hand_written, &evolution_scenarios_dir());

    eprintln!(
        "Gym loaded: {} hand-written, {} generated scenarios",
        gym.hand_written_count(),
        gym.generated_count(),
    );

    // Build baseline genome from current prompts
    let extractor_prompt = TestContext::baseline_extractor_prompt();
    let discovery_prompt = rootsignal_scout::discovery::source_finder::discovery_system_prompt("{city_name}");
    let baseline = ScoutGenome::baseline(extractor_prompt, discovery_prompt);

    // Run evolution
    let evolver = Evolver::new(ctx.anthropic_key());
    let max_generations = std::env::var("EVOLUTION_GENERATIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    let mutations_per_generation = std::env::var("EVOLUTION_MUTATIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);

    let result =
        evolver
            .evolve_from(
                baseline,
                &mut gym,
                EvolutionConfig {
                    max_generations,
                    mutations_per_generation,
                },
                |genome, scenario| {
                    let ctx_ref = &ctx;
                    let genome = genome.clone();
                    let world = scenario.world.clone();
                    let criteria = scenario.criteria.clone();
                    let name = scenario.name.clone();
                    async move {
                        run_scenario_with_genome(ctx_ref, &genome, &world, &criteria, &name).await
                    }
                },
            )
            .await
            .expect("Evolution failed");

    // Persist results
    save_json(&evolution_dir().join("champion.json"), &result.champion);
    save_json(&evolution_dir().join("history.json"), &result.history);

    eprintln!(
        "Evolution complete: champion fitness={:.3} (gen {}), {} scenarios promoted",
        result.champion.fitness.as_ref().unwrap().total,
        result.champion.generation,
        result.scenarios_promoted,
    );

    assert_eq!(
        result.champion.fitness.as_ref().unwrap().regressions,
        0,
        "Champion must have zero regressions"
    );
}
