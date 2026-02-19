//! Fuzzy integration tests using simulated web content and LLM judge.
//!
//! Three tiers:
//! - Tier 1 (Pinned): Snapshot-based, deterministic sim content, fresh judge each run
//! - Tier 2 (City Sim): Fresh sim generation each run, fresh judge
//! - Tier 3 (Random Discovery): Random world, informational only, never blocks CI

mod harness;
mod scenarios;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness::audit::{AuditConfig, AuditReport};
use harness::queries::serialize_graph_state_for_city;
use harness::{city_node_for, seed_sources_from_world, TestContext};
use rootsignal_graph::query;
use simweb::{
    generate_random_world, Improver, Judge, JudgeCriteria, SimulatedWeb, TestFailure, World,
};

/// Snapshot directory (checked into git).
fn snapshots_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")
}

/// Run log directory (gitignored, for debugging).
fn run_logs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/run_logs")
}

/// Load a SimulatedWeb from snapshot if it exists, otherwise generate fresh and save.
async fn load_or_generate_sim(
    world: &World,
    api_key: &str,
    snapshot_path: &Path,
) -> Arc<SimulatedWeb> {
    if snapshot_path.exists() {
        match SimulatedWeb::from_snapshot(world.clone(), api_key, snapshot_path) {
            Ok(sim) => return Arc::new(sim),
            Err(e) => {
                eprintln!(
                    "Warning: failed to load snapshot {}: {e}, regenerating",
                    snapshot_path.display()
                );
            }
        }
    }

    let sim = SimulatedWeb::new(world.clone(), api_key);
    Arc::new(sim)
}

/// Run a full sim → scout → judge → audit pipeline for a scenario.
async fn run_scenario(
    ctx: &TestContext,
    world: World,
    criteria: JudgeCriteria,
    scenario_name: &str,
    use_snapshot: bool,
) -> (simweb::Verdict, AuditReport) {
    let api_key = std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY required");

    // Build SimulatedWeb
    let snapshot_path = snapshots_dir().join(format!("{scenario_name}.json"));
    let sim = if use_snapshot {
        load_or_generate_sim(&world, &api_key, &snapshot_path).await
    } else {
        Arc::new(SimulatedWeb::new(world.clone(), &api_key))
    };

    // Build and run Scout — use scenario name in slug to isolate parallel tests
    let mut city_node = city_node_for(&world);
    city_node.slug = format!("{}_{}", city_node.slug, scenario_name);
    let city_slug = city_node.slug.clone();

    // Clean graph state for this test city (shared Neo4j may have leftover data)
    let writer = ctx.writer();
    let slug = &city_slug;

    // Collect URLs belonging to this city's sources
    let url_q = query("MATCH (s:Source {city: $slug}) WHERE s.url IS NOT NULL RETURN s.url AS url")
        .param("slug", slug.as_str());
    let mut url_stream = ctx
        .client()
        .inner()
        .execute(url_q)
        .await
        .expect("Failed to query source URLs");
    let mut city_urls: Vec<String> = Vec::new();
    while let Some(row) = url_stream.next().await.expect("row failed") {
        city_urls.push(row.get::<String>("url").unwrap_or_default());
    }

    // Delete signals + evidence whose source_url matches this city's sources
    if !city_urls.is_empty() {
        let clean_signals = query(
            "MATCH (n)-[:SOURCED_FROM]->(ev:Evidence) \
             WHERE (n:Event OR n:Give OR n:Ask OR n:Notice OR n:Tension) \
             AND ev.source_url IN $urls \
             DETACH DELETE n, ev",
        )
        .param("urls", city_urls.clone());
        ctx.client()
            .inner()
            .run(clean_signals)
            .await
            .expect("Failed to clean signals");
    }

    // Delete sources for this city
    let clean_sources =
        query("MATCH (s:Source {city: $slug}) DETACH DELETE s").param("slug", slug.as_str());
    ctx.client()
        .inner()
        .run(clean_sources)
        .await
        .expect("Failed to clean sources");

    // Seed sources into Neo4j so the scout has something to schedule
    seed_sources_from_world(&writer, &world, &city_node.slug).await;

    let scout = ctx.sim_scout(sim.clone(), city_node);
    let stats = scout.run().await.expect("Scout run failed");

    eprintln!("=== {scenario_name} stats ===\n{stats}");

    // Save run log for debugging
    let log_path = run_logs_dir().join(format!("{scenario_name}.json"));
    if let Err(e) = sim.save_snapshot(&log_path).await {
        eprintln!("Warning: failed to save run log: {e}");
    }

    // Save snapshot for pinned replay (only if we generated fresh)
    if use_snapshot && !snapshot_path.exists() {
        if let Err(e) = sim.save_snapshot(&snapshot_path).await {
            eprintln!("Warning: failed to save snapshot: {e}");
        }
    }

    // Serialize graph state for judge (scoped to this test city)
    let graph_state = serialize_graph_state_for_city(ctx.client(), Some(&city_slug)).await;

    // Judge evaluates
    let judge = Judge::new(&api_key);
    let verdict = judge
        .evaluate(&world, &criteria, &graph_state)
        .await
        .expect("Judge evaluation failed");

    eprintln!("=== {scenario_name} verdict ===\n{verdict}");

    // Run structural audit
    let audit_config = AuditConfig::for_sim(&world);
    let audit = harness::audit::run_audit(ctx.client(), &audit_config).await;
    eprintln!(
        "=== {scenario_name} audit: {}/{} passed ===",
        audit.passed,
        audit.checks.len()
    );
    for check in audit.checks.iter().filter(|c| !c.passed) {
        eprintln!("  FAIL: {} — {}", check.name, check.detail);
    }

    (verdict, audit)
}

// =============================================================================
// Tier 2: City Simulation Tests (fresh sim each run)
// =============================================================================

#[tokio::test]
async fn sim_stale_minneapolis() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys or Docker not available");
        return;
    };

    let (world, criteria) = (
        scenarios::stale_minneapolis::world(),
        scenarios::stale_minneapolis::criteria(),
    );

    let (verdict, audit) = run_scenario(&ctx, world, criteria, "stale_minneapolis", false).await;
    assert!(
        verdict.pass,
        "stale_minneapolis failed: {}",
        verdict.reasoning
    );
    assert!(
        audit.failed == 0,
        "stale_minneapolis: {} audit checks failed",
        audit.failed
    );
}

#[tokio::test]
async fn sim_organizing_portland() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys or Docker not available");
        return;
    };

    let (world, criteria) = (
        scenarios::organizing_portland::world(),
        scenarios::organizing_portland::criteria(),
    );

    let (verdict, audit) = run_scenario(&ctx, world, criteria, "organizing_portland", false).await;
    assert!(
        verdict.pass,
        "organizing_portland failed: {}",
        verdict.reasoning
    );
    assert!(
        audit.failed == 0,
        "organizing_portland: {} audit checks failed",
        audit.failed
    );
}

#[tokio::test]
async fn sim_simmering_cedar_riverside() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys or Docker not available");
        return;
    };

    let (world, criteria) = (
        scenarios::simmering_cedar_riverside::world(),
        scenarios::simmering_cedar_riverside::criteria(),
    );

    let (verdict, audit) =
        run_scenario(&ctx, world, criteria, "simmering_cedar_riverside", false).await;
    assert!(
        verdict.pass,
        "simmering_cedar_riverside failed: {}",
        verdict.reasoning
    );
    assert!(
        audit.failed == 0,
        "simmering_cedar_riverside: {} audit checks failed",
        audit.failed
    );
}

#[tokio::test]
async fn sim_rural_minnesota() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys or Docker not available");
        return;
    };

    let (world, criteria) = (
        scenarios::rural_minnesota::world(),
        scenarios::rural_minnesota::criteria(),
    );

    let (verdict, audit) = run_scenario(&ctx, world, criteria, "rural_minnesota", false).await;
    assert!(
        verdict.pass,
        "rural_minnesota failed: {}",
        verdict.reasoning
    );
    assert!(
        audit.failed == 0,
        "rural_minnesota: {} audit checks failed",
        audit.failed
    );
}

#[tokio::test]
async fn sim_hidden_community_minneapolis() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys or Docker not available");
        return;
    };

    let (world, criteria) = (
        scenarios::hidden_community_minneapolis::world(),
        scenarios::hidden_community_minneapolis::criteria(),
    );

    let (verdict, audit) =
        run_scenario(&ctx, world, criteria, "hidden_community_minneapolis", false).await;
    assert!(
        verdict.pass,
        "hidden_community_minneapolis failed: {}",
        verdict.reasoning
    );
    assert!(
        audit.failed == 0,
        "hidden_community_minneapolis: {} audit checks failed",
        audit.failed
    );
}

#[tokio::test]
async fn sim_shifting_ground() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys or Docker not available");
        return;
    };

    let (world, criteria) = (
        scenarios::shifting_ground::world(),
        scenarios::shifting_ground::criteria(),
    );

    let (verdict, audit) = run_scenario(&ctx, world, criteria, "shifting_ground", false).await;
    assert!(
        verdict.pass,
        "shifting_ground failed: {}",
        verdict.reasoning
    );
    assert!(
        audit.failed == 0,
        "shifting_ground: {} audit checks failed",
        audit.failed
    );
}

// =============================================================================
// Tension-First Pipeline Tests
// =============================================================================

#[tokio::test]
async fn sim_tension_response_cycle() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys or Docker not available");
        return;
    };

    let (world, criteria) = (
        scenarios::tension_response_cycle::world(),
        scenarios::tension_response_cycle::criteria(),
    );

    let (verdict, _audit) =
        run_scenario(&ctx, world, criteria, "tension_response_cycle", false).await;
    assert!(
        verdict.pass,
        "tension_response_cycle failed: {}",
        verdict.reasoning
    );
}

#[tokio::test]
async fn sim_tension_discovery_bridge() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys or Docker not available");
        return;
    };

    let (world, criteria) = (
        scenarios::tension_discovery_bridge::world(),
        scenarios::tension_discovery_bridge::criteria(),
    );

    let (verdict, _audit) =
        run_scenario(&ctx, world, criteria, "tension_discovery_bridge", false).await;
    assert!(
        verdict.pass,
        "tension_discovery_bridge failed: {}",
        verdict.reasoning
    );
}

// =============================================================================
// Tier 1: Pinned Scenarios (snapshot-based, deterministic sim content)
// =============================================================================

#[tokio::test]
async fn pinned_stale_minneapolis() {
    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: API keys or Docker not available");
        return;
    };

    let (world, criteria) = (
        scenarios::stale_minneapolis::world(),
        scenarios::stale_minneapolis::criteria(),
    );

    let (verdict, audit) = run_scenario(&ctx, world, criteria, "stale_minneapolis", true).await;
    assert!(
        verdict.pass,
        "pinned stale_minneapolis failed: {}",
        verdict.reasoning
    );
    assert!(
        audit.failed == 0,
        "pinned stale_minneapolis: {} audit checks failed",
        audit.failed
    );
}

// =============================================================================
// Tier 3: Random Discovery (informational, never blocks CI)
// =============================================================================

#[tokio::test]
async fn discovery_random_world() {
    let api_key = match std::env::var("ANTHROPIC_API_KEY") {
        Ok(k) => k,
        Err(_) => {
            eprintln!("Skipping: ANTHROPIC_API_KEY not set");
            return;
        }
    };

    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: Docker not available");
        return;
    };

    // Generate a random world
    let world = match generate_random_world(&api_key).await {
        Ok(w) => w,
        Err(e) => {
            eprintln!("Warning: failed to generate random world: {e}");
            return;
        }
    };

    eprintln!("=== Random world: {} ===", world.name);
    eprintln!(
        "Sites: {}, Profiles: {}, Facts: {}",
        world.sites.len(),
        world.social_profiles.len(),
        world.facts.len()
    );

    // Use permissive criteria — we're exploring, not asserting
    let criteria = JudgeCriteria {
        checks: vec![
            "The agent should extract at least one signal from the available sources.".to_string(),
            "Signals should be relevant to the geography described in the world.".to_string(),
            "The agent should not hallucinate information not present in any source.".to_string(),
        ],
        pass_threshold: 0.4, // Very permissive
        critical_categories: vec![],
    };

    let (verdict, audit) = run_scenario(&ctx, world, criteria, "random_discovery", false).await;

    // Never assert — just report
    if verdict.pass {
        eprintln!("Random discovery PASSED (score: {:.2})", verdict.score);
    } else {
        eprintln!(
            "Random discovery FAILED (score: {:.2}) — investigate run log for potential scenario promotion",
            verdict.score
        );
        for issue in &verdict.issues {
            eprintln!(
                "  [{:?}] {}: {}",
                issue.severity, issue.category, issue.description
            );
        }
    }
    if audit.failed > 0 {
        eprintln!("Random discovery audit: {} checks failed", audit.failed);
    }
}

// =============================================================================
// Self-Improvement Loop (gated behind IMPROVEMENT_LOOP=1)
// =============================================================================

/// Scenario loader: name + (world, criteria) factory.
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

#[tokio::test]
async fn improvement_loop() {
    if std::env::var("IMPROVEMENT_LOOP").is_err() {
        eprintln!("Skipping improvement_loop: set IMPROVEMENT_LOOP=1 to enable");
        return;
    }

    let api_key = match std::env::var("ANTHROPIC_API_KEY") {
        Ok(k) => k,
        Err(_) => {
            eprintln!("Skipping: ANTHROPIC_API_KEY not set");
            return;
        }
    };

    let Some(ctx) = TestContext::try_new().await else {
        eprintln!("Skipping: Docker not available");
        return;
    };

    // Run all scenarios, collect failures
    let mut failures = vec![];
    for &(name, factory) in ALL_SCENARIOS {
        let (world, criteria) = factory();
        let (verdict, audit) = run_scenario(&ctx, world, criteria, name, false).await;

        if !verdict.pass || audit.failed > 0 {
            let audit_failures: Vec<String> = audit
                .checks
                .iter()
                .filter(|c| !c.passed)
                .map(|c| format!("{}: {}", c.name, c.detail))
                .collect();

            failures.push(TestFailure {
                scenario_name: name.to_string(),
                verdict_pass: verdict.pass,
                verdict_score: verdict.score,
                verdict_reasoning: verdict.reasoning.clone(),
                audit_failures,
            });
        }
    }

    eprintln!(
        "=== Improvement loop: {}/{} scenarios failed ===",
        failures.len(),
        ALL_SCENARIOS.len()
    );

    if failures.is_empty() {
        eprintln!("All scenarios passed — no blind spots to analyze");
        return;
    }

    let improver = Improver::new(&api_key);
    let report = match improver.analyze(failures).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Improvement analysis failed: {e}");
            return;
        }
    };

    // Save report to run_logs/
    let report_path = run_logs_dir().join("improvement_report.json");
    if let Ok(json) = serde_json::to_string_pretty(&report) {
        let _ = std::fs::create_dir_all(run_logs_dir());
        let _ = std::fs::write(&report_path, json);
        eprintln!("Saved improvement report to {}", report_path.display());
    }

    eprintln!("=== Blind spots: {} ===", report.blind_spots.len());
    for spot in &report.blind_spots {
        eprintln!(
            "  [{:?}] {}: {} (from: {})",
            spot.severity, spot.category, spot.description, spot.source
        );
    }

    eprintln!(
        "=== Prompt suggestions: {} ===",
        report.prompt_suggestions.len()
    );
    for fix in &report.prompt_suggestions {
        eprintln!("  {}: {} → {}", fix.target, fix.issue, fix.suggestion);
    }

    // Run suggested scenarios to validate they expose the weakness
    eprintln!(
        "=== Running {} suggested scenarios ===",
        report.suggested_scenarios.len()
    );
    for world in &report.suggested_scenarios {
        let criteria = match improver.criteria_for(world).await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("  Failed to generate criteria for {}: {e}", world.name);
                continue;
            }
        };

        let scenario_name = format!("improve_{}", world.name.to_lowercase().replace(' ', "_"));
        let (verdict, _audit) =
            run_scenario(&ctx, world.clone(), criteria, &scenario_name, false).await;
        eprintln!(
            "  {} → {} (score: {:.2})",
            world.name,
            if verdict.pass {
                "PASS"
            } else {
                "FAIL (confirms blind spot)"
            },
            verdict.score,
        );
    }
}
