---
date: 2026-03-05
topic: shared-neo4j-test-container
---

# Shared Neo4j Test Container

## What We're Building
A shared Neo4j testcontainer that boots once for the entire `rootsignal-graph` integration test suite, instead of once per test file. Each test gets a clean graph via `MATCH (n) DETACH DELETE n` between tests, avoiding the 10-30s container startup penalty per file.

## Why This Approach
- **Current pain:** Each test file calls `neo4j_container()` → spins up its own Neo4j Enterprise container. With 4+ integration test files, that's 40-120s of container startup alone.
- **Considered Memgraph:** Bolt-compatible and boots in 1-2s, but lacks Neo4j's `graph-data-science` plugin needed for vector indexes (`db.index.vector.queryNodes`). Would require skipping or adapting vector-dependent tests.
- **Considered Kuzu:** Embedded Cypher DB, but not query-compatible (different vector API, no `FOREACH`, requires predefined schemas, different pattern matching semantics).
- **Shared container wins:** Zero compatibility risk, real Cypher coverage, and the startup cost is amortized across all tests.

## Key Decisions
- **Shared via `once_cell::sync::Lazy` or `std::sync::OnceLock`**: A global static holds the container handle + `GraphClient`. First test to access it triggers the boot; all others reuse.
- **Per-test cleanup via Cypher**: `MATCH (n) DETACH DELETE n` + drop indexes/constraints before each test. Fast (~50ms) and ensures isolation without container restart.
- **Migration runs once**: After container boot, run `migrate()` once. Tests that need indexes (vector, fulltext) get them from the shared migration.
- **`NEO4J_TEST_URI` still works**: External Neo4j support preserved for CI environments with a pre-running instance.
- **Parallel test isolation**: Cargo runs integration tests within a single test binary sequentially by default (`--test-threads=1` for integration tests). If parallel execution is needed later, can namespace with unique labels or use Neo4j's multi-database feature.

## Open Questions
- Should we add `#[serial]` (from `serial_test` crate) to integration tests to prevent accidental parallel execution corrupting shared state?
- Do we need to re-run migrations between tests, or is `DETACH DELETE` + re-creating vector indexes sufficient?

## Next Steps
-> `/workflows:plan` for implementation details
