---
title: "refactor: Enrichment Pipeline with project/apply/enrich Architecture"
type: refactor
date: 2026-02-25
brainstorm: docs/brainstorms/2026-02-25-enrichment-pipeline-design-brainstorm.md
---

# Enrichment Pipeline with project/apply/enrich Architecture

## Overview

Add an EmbeddingStore (get-or-compute Postgres cache), enrichment passes that write derived properties directly to Neo4j, and a pipeline orchestrator on top of the existing GraphReducer. The reducer keeps its current Cypher logic and owns factual properties. Enrichment passes own derived properties (diversity, cause_heat, actor stats) and write them directly to the graph after the reducer runs.

**Replay guarantee**: `reducer(events) → enrich(graph)` always produces the same graph. Enrichment is a deterministic function of graph state — it's recomputed fresh on every replay, never stored in the event log.

**Key design decision**: Enrichment results are NOT events. Events are facts about what happened (the Event enum header: "No embeddings, no derived metrics, no infrastructure artifacts"). Derived metrics are computed by enrichment and written directly to Neo4j. This keeps the event log pure and avoids the complexity of filtering enrichment events on replay.

> **Deferred**: The project/apply split was pressure-tested and deferred. See brainstorm doc "Resolved Questions" for rationale.

## Problem Statement

The current GraphReducer handles event interpretation and execution in one step, which works but has two gaps:

1. **Embeddings** exist in the scout pipeline but have no path to the graph in the event-sourced world
2. **Derived values** (diversity, cause_heat) have no clear computation home
3. **`ActorIdentified` at `reducer.rs:651`** uses `a.signal_count = a.signal_count + 1` which is not idempotent — replaying the same event increments twice

## Proposed Solution

### Keep the reducer, add enrichment on top

```
reducer.apply(events)       // existing Cypher logic, writes factual properties
enrich(&graph)              // reads graph, writes derived properties directly to Neo4j
```

- **`reducer`** keeps its current Cypher logic. No rewrite. Owns all factual properties.
- **`enrich`** reads the graph that the reducer built and writes derived properties (diversity counts, cause_heat, actor stats) directly to Neo4j via SET queries. No events involved.

**Clear ownership boundary**:
- Reducer: factual properties (title, summary, confidence, corroboration_count, etc.)
- Enrichment: derived properties (source_diversity, channel_diversity, external_ratio, cause_heat, signal_count)
- Already enforced by contract tests (`reducer_source_has_no_diversity_writes`, `reducer_source_has_no_cause_heat_writes`)

### EmbeddingStore

Get-or-compute cache backed by Postgres. Keyed by `hash(model_version + input_text)`. The scout pipeline writes to it during extraction (it already computes embeddings for dedup). The reducer reads from it to include embeddings in graph operations.

## Technical Approach

### DEFERRED: project/apply split

> The project/apply split was pressure-tested against the actual Cypher in `reducer.rs` and deferred. The reducer's Cypher uses 7 patterns that don't fit a typed CRUD enum (conditional CASE WHEN, OPTIONAL MATCH coalesce, FOREACH edge repointing, branching queries, relative updates, MERGE on non-id, nullable datetime). Building a GraphOp would mean building a Cypher AST. Returning `Vec<neo4rs::Query>` doesn't improve testability over current string-matching tests.
>
> **Revisit when**: batching becomes a performance bottleneck, a second graph backend appears, or the reducer exceeds ~1500 lines.
>
> See: `docs/brainstorms/2026-02-25-enrichment-pipeline-design-brainstorm.md` → "Resolved Questions"

### EmbeddingStore (Phase 1 — DONE)

```rust
/// modules/rootsignal-common/src/types.rs
pub trait EmbeddingLookup: Send + Sync {
    async fn get(&self, text: &str) -> Result<Vec<f32>>;
}

/// modules/rootsignal-graph/src/embedding_store.rs
pub struct EmbeddingStore {
    pool: PgPool,
    embedder: Arc<dyn TextEmbedder>,
    model_version: String,
}
// Implements EmbeddingLookup with get-or-compute semantics.
// SHA-256 of (model_version + text) as cache key.
// warm() for batch pre-computation.
```

### Enrichment passes

```rust
/// modules/rootsignal-graph/src/enrich.rs

/// Run all enrichment passes. Reads the graph, writes derived properties directly.
pub async fn enrich(client: &GraphClient) -> Result<EnrichStats> {
    let mut stats = EnrichStats::default();

    // 1. Diversity: count citation edges per entity → SET diversity properties
    stats.diversity = compute_diversity(client).await?;

    // 2. Actor stats: count ACTED_IN edges per actor → SET signal_count
    stats.actor_stats = compute_actor_stats(client).await?;

    // 3. Cause heat: read embeddings + diversity, compute heats → SET cause_heat
    // (depends on diversity being written first)
    stats.cause_heat = compute_cause_heat(client).await?;

    Ok(stats)
}
```

Each enrichment pass reads graph state and writes derived properties directly:

```cypher
-- Diversity: single Cypher query per entity type
MATCH (n:Gathering)
OPTIONAL MATCH (n)<-[:CITES]-(c:Citation)
WITH n, count(DISTINCT c.url) AS src_div, count(DISTINCT c.channel_type) AS ch_div
SET n.source_diversity = src_div, n.channel_diversity = ch_div,
    n.external_ratio = CASE WHEN src_div = 0 THEN 0.0 ELSE ... END

-- Actor stats: count edges
MATCH (a:Actor)-[r:ACTED_IN]->()
WITH a, count(r) AS cnt, max(r.ts) AS last
SET a.signal_count = cnt

-- Cause heat: wraps existing compute_heats() pure function
-- Reads embeddings + diversity from graph, computes, writes back
```

### Pipeline orchestrator

```rust
/// modules/rootsignal-graph/src/pipeline.rs

pub struct Pipeline {
    reducer: GraphReducer,
    client: GraphClient,
}

impl Pipeline {
    /// Process events through the full pipeline.
    pub async fn process(&self, events: &[StoredEvent]) -> Result<PipelineStats> {
        // Step 1: reduce factual events → graph has factual properties
        let reduce_stats = self.reducer.apply_batch(events).await?;

        // Step 2: enrich → graph gets derived properties
        let enrich_stats = enrich(&self.client).await?;

        Ok(PipelineStats { reduce_stats, enrich_stats })
    }

    /// Full rebuild: wipe graph, replay all events, enrich.
    pub async fn rebuild(&self, store: &EventStore) -> Result<PipelineStats>;

    /// Replay from a specific sequence number.
    pub async fn replay_from(&self, store: &EventStore, seq: i64) -> Result<PipelineStats>;
}
```

### Fix: ActorIdentified idempotency

The current `ON MATCH SET a.signal_count = a.signal_count + 1` is not idempotent. Fix: remove the increment from the reducer. `signal_count` becomes an enrichment-computed value (count of ACTED_IN edges). The `ActorIdentified` handler keeps `signal_count = 1` on CREATE but no longer touches it on MATCH.

## Implementation Phases

### ~~Phase 1-2: project/apply split~~ — DEFERRED

> Pressure-tested and deferred. GraphOp doesn't fit the actual Cypher patterns. `Vec<neo4rs::Query>` doesn't improve testability. Keep the reducer as-is.
>
> See: brainstorm doc "Resolved Questions" for the 7 Cypher patterns that break GraphOp.

### Phase 1: EmbeddingStore

Build the get-or-compute embedding cache.

**Files:**
- `modules/rootsignal-graph/src/embedding_store.rs` — NEW: EmbeddingStore, implements EmbeddingLookup
- `modules/rootsignal-api/migrations/0XX_embedding_cache.sql` — NEW: Postgres table
- `modules/rootsignal-graph/tests/embedding_store_test.rs` — NEW: tests

**Tasks:**
- [x] Create Postgres migration for `embedding_cache` table (already in 007_unified_events.sql)
- [x] Implement `EmbeddingStore` with get-or-compute logic
- [x] Implement `warm()` for batch pre-computation
- [x] Implement SHA-256 hashing with model_version prefix
- [x] Implement `EmbeddingLookup` trait for `EmbeddingStore`
- [x] Write test: cache hit returns stored embedding
- [x] Write test: cache miss computes, stores, and returns
- [x] Write test: model version change causes cache miss
- [x] Write test: warm() batch-computes and stores

### Phase 2: Enrichment passes + enrich()

Build enrichment functions that read the graph and write derived properties directly to Neo4j. No event variants needed — enrichment results are not facts, they're derived metrics.

**Files:**
- `modules/rootsignal-graph/src/enrich.rs` — NEW: `enrich()`, `compute_diversity()`, `compute_actor_stats()`, `compute_cause_heat()`
- `modules/rootsignal-graph/src/reducer.rs` — FIX: `ActorIdentified` idempotency
- `modules/rootsignal-graph/src/lib.rs` — UPDATE: export enrich module
- `modules/rootsignal-graph/tests/enrich_test.rs` — NEW: tests

**Tasks:**
- [x] Fix `ActorIdentified` idempotency: stop incrementing `signal_count` on MATCH
- [x] Implement `compute_diversity()` — reads SOURCED_FROM→Evidence edges, writes diversity properties via SET
- [x] Implement `compute_actor_stats()` — counts ACTED_IN edges, writes signal_count via SET
- [x] Implement `enrich()` orchestrator — runs diversity → actor stats → cause_heat (wraps existing `compute_cause_heat`)
- [x] Write test: diversity properties set correctly from Evidence edges (4 integration tests)
- [x] Write test: actor signal_count matches ACTED_IN edge count (2 integration tests)
- [x] Write test: cause_heat computed for signals with embeddings (2 integration tests)

### Phase 3: Pipeline orchestrator

Wire reducer + enrich into a Pipeline struct that sequences the two steps.

**Files:**
- `modules/rootsignal-graph/src/pipeline.rs` — NEW: Pipeline struct with process(), rebuild(), replay_from()
- `modules/rootsignal-graph/src/lib.rs` — UPDATE: export pipeline module
- `modules/rootsignal-graph/tests/pipeline_test.rs` — NEW: end-to-end tests

**Tasks:**
- [x] Implement `Pipeline::process()` — apply events → enrich
- [x] Implement `Pipeline::rebuild()` — wipe + replay + enrich
- [x] Implement `Pipeline::replay_from()` — incremental replay + enrich
- [x] Migrate call sites to use `Pipeline` (no external callers — GraphReducer is only used internally by Pipeline)
- [x] Write end-to-end test: events → pipeline → graph state matches expectations (3 tests)
- [x] Write test: replay produces identical graph (same events → same factual + derived properties)

## Acceptance Criteria

### Functional
- [x] `EmbeddingStore` caches embeddings in Postgres with get-or-compute semantics
- [x] `enrich()` reads graph and writes diversity, cause_heat, signal_count directly to Neo4j
- [x] `ActorIdentified` no longer increments `signal_count` — computed by enrichment
- [x] Pipeline orchestrates: reducer(events) → enrich(graph)
- [x] Enrichment properties are never set by the reducer (enforced by contract tests: `reducer_source_has_no_diversity_writes`, `reducer_source_has_no_cause_heat_writes`)

### Quality Gates
- [x] Replay test: same events produce identical graph (factual + derived properties)
- [x] All tests follow MOCK → FUNCTION → OUTPUT pattern
- [x] Test names describe behavior, not implementation

### Non-Goals
- GraphStore migration (Phase 3 of foundation plan — separate work)
- SIMILAR_TO edge rebuilding (separate enrichment pass, not in scope)
- EmbeddingStore backfill from existing Neo4j data (migration script, separate)
- Real-time NOTIFY subscription for enrichment triggering (future)
- Enrichment results as Event variants (derived metrics don't belong in the event log)

## Dependencies & Risks

**Dependencies:**
- Event data model redesign (committed: `5d16990`)
- rootsignal-events crate (committed: `ecbb680`)
- GraphReducer (committed: `b577dfd`)

**Risks:**
- **Enrichment performance** — Diversity enrichment queries Evidence edges for every entity on every cycle. Mitigate: only recompute for entities whose Evidence edges changed since last enrichment (track via a marker property or separate cursor).
- **EmbeddingStore latency** — Cache miss during reduce means an API call, blocking the pipeline. Mitigate: `warm()` pre-computes embeddings in batch before reduce runs.

## References

- Brainstorm: `docs/brainstorms/2026-02-25-enrichment-pipeline-design-brainstorm.md`
- Foundation plan: `docs/plans/2026-02-25-refactor-event-sourcing-foundation-plan.md`
- Data model plan: `docs/plans/2026-02-25-refactor-event-data-model-plan.md`
- Current reducer: `modules/rootsignal-graph/src/reducer.rs`
- Current cause_heat: `modules/rootsignal-graph/src/cause_heat.rs` (pure core pattern)
- Current embedder: `modules/rootsignal-scout/src/infra/embedder.rs`
- TextEmbedder trait: `modules/rootsignal-common/src/types.rs:1472`
- Learning: `docs/solutions/2026-02-17-unwrap-or-masks-data-quality.md`
