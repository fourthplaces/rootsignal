//! Integration test: sim → archive seed → Postgres replay → scout.
//!
//! Verifies that Scout can run against archived content replayed from Postgres,
//! producing the same kind of signals as when running against SimulatedWeb directly.
//!
//! Requirements:
//!   - Docker (for Neo4j via testcontainers)
//!   - ANTHROPIC_API_KEY, VOYAGE_API_KEY env vars
//!   - DATABASE_URL env var (Postgres)

mod harness;
mod scenarios;

use std::sync::Arc;

use harness::archive_seed::seed_from_sim;
use harness::{scope_for, seed_sources_from_world, TestContext};
use simweb::SimulatedWeb;

#[tokio::test]
async fn archive_replay_produces_signals() {
    let Some(ctx) = TestContext::try_new_with_pg().await else {
        eprintln!("Skipping: API keys, Docker, or DATABASE_URL not available");
        return;
    };

    let world = scenarios::stale_minneapolis::world();
    let scope = scope_for(&world);

    // 1. Run scout through SimArchive to populate sim logs
    let sim = Arc::new(SimulatedWeb::new(
        world.clone(),
        ctx.anthropic_key(),
    ));

    let writer = ctx.writer();
    seed_sources_from_world(&writer, &world, &scope.name).await;

    let sim_stats = ctx.sim_scout(sim.clone(), scope.clone()).run().await.expect("Sim scout run failed");
    eprintln!("=== Sim run: {} signals extracted ===", sim_stats.signals_extracted);
    assert!(
        sim_stats.signals_extracted >= 1,
        "Sim run should extract at least 1 signal, got {}",
        sim_stats.signals_extracted
    );

    // 2. Seed Postgres from sim's logged interactions
    let (seeder, run_id) = ctx.seeder(&scope.name).await;
    seed_from_sim(&seeder, &sim).await.expect("Failed to seed from sim");

    // 3. Replay from Postgres — same data, different backend
    let replay = ctx.replay(run_id);

    // Clean graph so replay run starts fresh
    let clean = rootsignal_graph::query("MATCH (n) DETACH DELETE n");
    ctx.client().inner().run(clean).await.expect("Failed to clean graph");

    rootsignal_graph::migrate::migrate(ctx.client())
        .await
        .expect("Re-migration failed");

    seed_sources_from_world(&writer, &world, &scope.name).await;

    let replay_stats = ctx.scout().with_archive(replay).with_city(scope).run().await;
    eprintln!("=== Replay run: {} signals extracted ===", replay_stats.signals_extracted);

    assert!(
        replay_stats.signals_extracted >= 1,
        "Replay run should extract at least 1 signal, got {}",
        replay_stats.signals_extracted
    );
}
