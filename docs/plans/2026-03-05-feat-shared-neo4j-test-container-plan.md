---
title: "feat: Shared Neo4j test container"
type: feat
date: 2026-03-05
---

# feat: Shared Neo4j Test Container

## Overview

Consolidate the 4 Neo4j integration test files (`litmus_test`, `pipeline_test`, `enrich_test`, `source_region_test`) into a single test binary with a shared Neo4j container. The container boots once, migrations run once, and each test gets a clean graph via `MATCH (n) DETACH DELETE n`.

## Problem Statement

Each integration test file compiles to a **separate binary** and spins up its own Neo4j Enterprise container (10-30s boot time). With 4 files, that's 40-120s of container startup overhead. Additionally, only `litmus_test.rs` runs `migrate()` — the other 3 operate against an unmigrated schema, which means vector index queries silently return empty results and NOT NULL constraints aren't enforced.

## Proposed Solution

### Architecture

Merge the 4 container-dependent test files into a **single integration test binary** via a `tests/integration/main.rs` entrypoint with `mod` declarations. A `tokio::sync::OnceCell` holds the shared container handle + `GraphClient`. A `setup()` helper initializes the container on first call, runs `migrate()` once, and returns a cloned `GraphClient` after wiping all node data.

```
tests/
  integration/
    main.rs              # mod declarations, shared setup
    litmus.rs            # renamed from litmus_test.rs
    pipeline.rs          # renamed from pipeline_test.rs
    enrich.rs            # renamed from enrich_test.rs
    source_region.rs     # renamed from source_region_test.rs
  projector_contract_test.rs   # unchanged (no Neo4j)
  severity_inference_test.rs   # unchanged (no Neo4j)
  embedding_store_test.rs      # unchanged (Postgres only)
  bbox_scoping_test.rs         # unchanged (#[ignore])
  cloud_connect.rs             # unchanged (#[ignore])
```

### Implementation Phases

#### Phase 1: Shared test harness

Create the shared infrastructure in `tests/integration/main.rs`:

```rust
// tests/integration/main.rs
#![cfg(feature = "test-utils")]

mod litmus;
mod pipeline;
mod enrich;
mod source_region;

use std::sync::Arc;
use tokio::sync::OnceCell;
use rootsignal_graph::{GraphClient, testutil::neo4j_container};

struct TestContainer {
    _handle: Box<dyn std::any::Any + Send>,
    client: GraphClient,
}

static CONTAINER: OnceCell<TestContainer> = OnceCell::const_new();

/// Acquire the shared Neo4j client. First caller boots the container + migrates.
/// Every caller gets a clean graph (all nodes deleted, schema preserved).
async fn setup() -> GraphClient {
    let tc = CONTAINER.get_or_init(|| async {
        let (handle, client) = neo4j_container().await;
        rootsignal_graph::migrate::migrate(&client)
            .await
            .expect("migration failed");
        TestContainer { _handle: handle, client }
    }).await;

    // Wipe all data (schema survives DETACH DELETE)
    tc.client.run(rootsignal_graph::query(
        "MATCH (n) DETACH DELETE n"
    )).await.expect("cleanup failed");

    tc.client.clone()
}
```

**Key decisions:**
- `tokio::sync::OnceCell` (not `LazyLock`) because initialization is async
- Cleanup runs at the **start** of each test (in `setup()`), so a panicking test doesn't block the next
- `GraphClient` (neo4rs::Graph) is internally `Arc`-wrapped — cloning is cheap
- Container handle held in static — never dropped, testcontainers RYUK reaper handles cleanup
- `NEO4J_TEST_URI` support preserved (already in `neo4j_container()`)

**Files:**
- `modules/rootsignal-graph/tests/integration/main.rs` (new)

#### Phase 2: Move test files

Move and rename the 4 test files into the integration module:

- `tests/litmus_test.rs` → `tests/integration/litmus.rs`
- `tests/pipeline_test.rs` → `tests/integration/pipeline.rs`
- `tests/enrich_test.rs` → `tests/integration/enrich.rs`
- `tests/source_region_test.rs` → `tests/integration/source_region.rs`

Changes per file:
- Remove `#![cfg(feature = "test-utils")]` (already on `main.rs`)
- Replace local `setup()` with `super::setup().await`
- Remove `let (_c, client) = setup().await` → `let client = super::setup().await`
- Remove any `use rootsignal_graph::testutil::neo4j_container` imports

**Files:**
- `modules/rootsignal-graph/tests/integration/litmus.rs`
- `modules/rootsignal-graph/tests/integration/pipeline.rs`
- `modules/rootsignal-graph/tests/integration/enrich.rs`
- `modules/rootsignal-graph/tests/integration/source_region.rs`

#### Phase 3: Fix migration schema conflicts

Now that `migrate()` always runs before any test, fix test helpers that create nodes missing NOT NULL fields:

- `enrich_test.rs` `create_signal_with_embedding` — add `sensitivity: 'medium'` and `confidence: 0.5` to the CREATE Cypher
- Audit `source_region_test.rs` and `pipeline_test.rs` helpers for similar gaps
- These are pre-existing bugs (tests were running against unmigrated schema) — fixing them is correct behavior

**Files:**
- `modules/rootsignal-graph/tests/integration/enrich.rs`
- `modules/rootsignal-graph/tests/integration/source_region.rs`
- `modules/rootsignal-graph/tests/integration/pipeline.rs`

#### Phase 4: Cleanup and verify

- Remove the `memgraph_container` alias from `testutil.rs` (dead code — unused anywhere)
- Add `tokio` as a dev-dependency if not already available for the `OnceCell`
- Run `cargo test -p rootsignal-graph --features test-utils --test integration` — all tests pass with one container
- Verify remaining standalone test files still compile and run independently

**Files:**
- `modules/rootsignal-graph/src/testutil.rs`
- `modules/rootsignal-graph/Cargo.toml`

## Technical Considerations

### Why a single binary, not a shared external process

Cargo compiles each `tests/*.rs` to a separate binary. A `OnceCell` is process-local — it can only share within one binary. To share one container across 4 test files, they must be in the same binary. The `tests/integration/main.rs` + `mod` pattern is idiomatic Cargo for this.

### Parallel execution safety

Tests within a single binary run sequentially by default (`--test-threads=1`). If someone passes `--test-threads=N`, tests would race on the shared graph. This is acceptable because:
- The default is safe
- We can add `#[serial_test::serial]` later if needed
- The existing tests already aren't parallel-safe (they create/query global graph state)

### Container lifecycle

The `ContainerAsync` handle lives in a static `OnceCell` that is never dropped. Testcontainers' RYUK reaper (enabled by default in Docker Desktop and CI) handles orphan cleanup. This is the standard testcontainers pattern for shared fixtures.

### nextest incompatibility

`cargo-nextest` runs each test in its own process, making `OnceCell` sharing ineffective. For nextest environments, use `NEO4J_TEST_URI` pointing at a pre-started container. This is a documentation note, not a code change.

## Acceptance Criteria

- [ ] All 4 integration test files consolidated into `tests/integration/` single binary
- [ ] Neo4j container boots exactly once per `cargo test --test integration` invocation
- [ ] `migrate()` runs once after container boot
- [ ] Each test starts with a clean graph (DETACH DELETE in setup)
- [ ] `NEO4J_TEST_URI` env var still works for external Neo4j
- [ ] All existing tests pass (no behavior changes)
- [ ] Standalone test files (projector_contract, severity_inference, etc.) unaffected
- [ ] Test helpers create nodes compatible with migrated schema (sensitivity, confidence)

## Success Metrics

- Integration test wall-clock time drops from ~60-120s to ~15-30s (one container boot instead of four)
- All tests exercise real Cypher against a properly migrated schema (correctness improvement)

## Dependencies & Risks

- **Docker required**: Tests still need Docker. No change from current behavior.
- **RYUK reaper**: Container cleanup relies on testcontainers' RYUK. If RYUK is disabled, containers may leak. Standard Docker Desktop and CI setups include RYUK.
- **Schema enforcement**: Migrating before all tests may surface pre-existing bugs in test helpers that omit required fields. This is a feature, not a risk — it means tests are now more accurate.

## References

- Brainstorm: `docs/brainstorms/2026-03-05-shared-neo4j-test-container-brainstorm.md`
- Current testutil: `modules/rootsignal-graph/src/testutil.rs`
- Migration: `modules/rootsignal-graph/src/migrate.rs`
- Test architecture plan: `docs/plans/2026-02-24-refactor-test-architecture-plan.md`
