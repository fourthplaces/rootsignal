---
title: "refactor: Enrichment Pipeline with project/apply/enrich Architecture"
type: refactor
date: 2026-02-25
brainstorm: docs/brainstorms/2026-02-25-enrichment-pipeline-design-brainstorm.md
---

# Enrichment Pipeline with project/apply/enrich Architecture

## Overview

Add an EmbeddingStore (get-or-compute Postgres cache), enrichment event variants, and a pipeline orchestrator on top of the existing GraphReducer. The reducer keeps its current Cypher logic. Enrichment passes (diversity, cause_heat, actor stats) read the graph and emit typed events that the reducer handles like any other event.

This builds on the event data model redesign (committed) and precedes Phase 3 (wiring GraphWriter through events).

> **Deferred**: The project/apply split (separating event interpretation from Cypher execution) was pressure-tested and deferred. See brainstorm doc "Resolved Questions" for rationale. Revisit when batching becomes a bottleneck or the reducer exceeds ~1500 lines.

## Problem Statement

The current GraphReducer handles event interpretation and execution in one step, which works but has two gaps:

1. **Embeddings** exist in the scout pipeline but have no path to the graph in the event-sourced world
2. **Derived values** (diversity, cause_heat) have no clear computation home
3. **`ActorIdentified` at `reducer.rs:651`** uses `a.signal_count = a.signal_count + 1` which is not idempotent — replaying the same event increments twice

## Proposed Solution

### Keep the reducer, add enrichment on top

```
reducer.reduce(events, &embeddings)             // existing Cypher logic, unchanged
enrich(&graph, &embeddings) → Vec<Event>        // reads graph, emits enrichment events
reducer.reduce(enrichment_events, &embeddings)  // reducer handles enrichment events too
```

- **`reducer`** keeps its current Cypher logic. No rewrite. It gains an `EmbeddingStore` dependency for writing embeddings to nodes, and new `match` arms for enrichment event variants.
- **`enrich`** reads the graph that the reducer just wrote and produces new events (diversity counts, cause_heat, actor stats). These events flow back through the reducer once (depth limit = 1).

### EmbeddingStore

Get-or-compute cache backed by Postgres. Keyed by `hash(model_version + input_text)`. The scout pipeline writes to it during extraction (it already computes embeddings for dedup). The reducer reads from it to include embeddings in graph operations.

### Enrichment passes

Each enricher reads the graph and emits typed events:

1. **DiversityEnricher** — reads Evidence edges per entity, emits `DiversityComputed { entity_id, node_type, source_diversity, channel_diversity, external_ratio }`
2. **CauseHeatEnricher** — reads embeddings + diversity in a bbox, emits `CauseHeatComputed { entity_id, heat: f64 }`
3. **ActorStatsEnricher** — counts ACTED_IN edges per actor, emits `ActorStatsComputed { actor_id, signal_count }`

These events are persisted to the EventStore (actor = "enricher"). On replay, enrichment events are filtered out and recomputed fresh — ensuring determinism.

## Technical Approach

### DEFERRED: project/apply split

> The project/apply split was pressure-tested against the actual Cypher in `reducer.rs` and deferred. The reducer's Cypher uses 7 patterns that don't fit a typed CRUD enum (conditional CASE WHEN, OPTIONAL MATCH coalesce, FOREACH edge repointing, branching queries, relative updates, MERGE on non-id, nullable datetime). Building a GraphOp would mean building a Cypher AST. Returning `Vec<neo4rs::Query>` doesn't improve testability over current string-matching tests.
>
> **Revisit when**: batching becomes a performance bottleneck, a second graph backend appears, or the reducer exceeds ~1500 lines.
>
> See: `docs/brainstorms/2026-02-25-enrichment-pipeline-design-brainstorm.md` → "Resolved Questions"

### EmbeddingLookup trait

```rust
/// modules/rootsignal-graph/src/embedding_store.rs (or shared types)

pub trait EmbeddingLookup: Send + Sync {
    /// Get an embedding for the given text. Cache hit or compute.
    fn get(&self, text: &str) -> Result<Vec<f64>>;
}
```

The reducer gains this as a dependency. Discovery handlers call `embeddings.get(text)` to fetch/compute embeddings and include them in the Cypher SET clause.

### EmbeddingStore

```rust
/// modules/rootsignal-graph/src/embedding_store.rs

use async_trait::async_trait;

pub struct EmbeddingStore {
    pool: sqlx::PgPool,
    embedder: Arc<dyn TextEmbedder>,
    model_version: String,
}

#[async_trait]
impl EmbeddingLookup for EmbeddingStore {
    fn get(&self, text: &str) -> Result<Vec<f64>> {
        let hash = self.hash_key(text);
        // 1. Check Postgres cache
        // 2. Hit → return
        // 3. Miss → compute via self.embedder, store, return
    }
}

impl EmbeddingStore {
    /// Pre-warm cache for a batch of texts. Single API call.
    pub async fn warm(&self, texts: &[&str]) -> Result<()>;

    fn hash_key(&self, text: &str) -> String {
        // SHA-256 of (model_version + text)
    }
}
```

Postgres table (migration):

```sql
CREATE TABLE embedding_cache (
    input_hash    TEXT PRIMARY KEY,
    model_version TEXT NOT NULL,
    embedding     FLOAT4[] NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_embedding_cache_model ON embedding_cache (model_version);
```

### Enrichment passes

```rust
/// modules/rootsignal-graph/src/enrich.rs

/// Read the graph and emit enrichment events.
pub async fn enrich(
    client: &GraphClient,
    bbox: &BBox,
) -> Result<Vec<Event>> {
    let mut events = Vec::new();

    // 1. Diversity enrichment
    events.extend(compute_diversity(client).await?);

    // 2. cause_heat enrichment
    events.extend(compute_cause_heat_events(client, bbox).await?);

    events
}

/// Compute diversity for all entities that need it.
/// "Need it" = entities with Evidence edges whose diversity
/// hasn't been computed since last corroboration.
async fn compute_diversity(client: &GraphClient) -> Result<Vec<Event>> {
    // For each entity type label:
    //   Query entities with SOURCED_FROM edges
    //   Count distinct source URLs → source_diversity
    //   Count distinct channel types from external sources → channel_diversity
    //   Compute external_ratio
    //   Emit DiversityComputed event
}

/// Compute cause_heat for all signals in bbox.
/// Uses the existing compute_heats() pure function.
async fn compute_cause_heat_events(
    client: &GraphClient,
    bbox: &BBox,
) -> Result<Vec<Event>> {
    // Load signals with embeddings + diversity from graph
    // Call compute_heats() (existing pure function in cause_heat.rs)
    // Emit CauseHeatComputed per signal
}
```

New event variants:

```rust
// In modules/rootsignal-common/src/events.rs

DiversityComputed {
    entity_id: Uuid,
    node_type: NodeType,
    source_diversity: u32,
    channel_diversity: u32,
    external_ratio: f32,
},

CauseHeatComputed {
    entity_id: Uuid,
    node_type: NodeType,
    heat: f64,
},
```

### Pipeline orchestrator

```rust
/// modules/rootsignal-graph/src/pipeline.rs

pub struct Pipeline {
    reducer: GraphReducer,
    embeddings: Arc<EmbeddingStore>,
}

impl Pipeline {
    /// Process a batch of events through the full pipeline.
    pub async fn process(&self, events: &[StoredEvent], bbox: &BBox) -> Result<PipelineStats> {
        // Phase 1: reduce observation events (existing reducer logic)
        let reduce_stats = self.reducer.reduce(events, &self.embeddings).await?;

        // Phase 2: enrichment reads graph, emits new events
        let enrichment_events = enrich(&self.reducer.client, bbox).await?;

        // Phase 3: reduce enrichment events (reducer handles them like any other event)
        let enrich_stats = self.reducer.reduce(&enrichment_events, &self.embeddings).await?;

        Ok(PipelineStats { reduce_stats, enrich_stats })
    }

    /// Full rebuild: wipe graph, replay all events, enrich.
    pub async fn rebuild(&self, store: &EventStore, bbox: &BBox) -> Result<PipelineStats>;

    /// Replay from a specific sequence number.
    pub async fn replay_from(&self, store: &EventStore, seq: i64, bbox: &BBox) -> Result<PipelineStats>;
}
```

The reducer gains new `match` arms for enrichment events (`DiversityComputed`, `CauseHeatComputed`, `ActorStatsComputed`). These are simple SET operations — no complex Cypher needed.

### Fix: ActorIdentified idempotency

The current `ON MATCH SET a.signal_count = a.signal_count + 1` is not idempotent. Fix:

```rust
// Instead of incrementing, the project function emits a MergeNode
// with signal_count = 1 on CREATE. On subsequent ActorIdentified events
// for the same actor, the ON MATCH clause does NOT increment signal_count.
// signal_count becomes an enrichment-computed value: count of ACTED_IN edges.
```

Add to the diversity enricher: after computing per-entity diversity, also count ACTED_IN edges per actor and emit `ActorStatsComputed { actor_id, signal_count }`.

New event variant:

```rust
ActorStatsComputed {
    actor_id: Uuid,
    signal_count: u32,
    last_active: DateTime<Utc>,
},
```

This replaces the current `ActorStatsUpdated` producer-computed event with an enrichment-computed one. The `ActorIdentified` handler no longer touches `signal_count` on MATCH — it just updates `name` and `last_active`.

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

### Phase 2: Enrichment events + enrich()

Add enrichment event variants and the enrich function.

**Files:**
- `modules/rootsignal-common/src/events.rs` — ADD: `DiversityComputed`, `CauseHeatComputed`, `ActorStatsComputed`
- `modules/rootsignal-graph/src/enrich.rs` — NEW: `enrich()`, diversity computation, cause_heat event emission
- `modules/rootsignal-graph/src/reducer.rs` — ADD: match arms for enrichment event variants
- `modules/rootsignal-graph/tests/enrich_test.rs` — NEW: tests

**Tasks:**
- [ ] Add `DiversityComputed`, `CauseHeatComputed`, `ActorStatsComputed` event variants
- [ ] Update `event_type()` for new variants
- [ ] Implement `compute_diversity()` — reads Evidence edges, emits events
- [ ] Implement `compute_cause_heat_events()` — wraps existing `compute_heats()` pure function
- [ ] Implement `compute_actor_stats()` — counts ACTED_IN edges per actor
- [ ] Implement `enrich()` orchestrator
- [ ] Add reducer match arms for enrichment events (simple SET operations)
- [ ] Fix `ActorIdentified` idempotency: stop incrementing `signal_count` on MATCH
- [ ] Write test: diversity computed correctly from Evidence edges
- [ ] Write test: cause_heat events emitted for signals in bbox
- [ ] Write test: actor stats computed from edge counts
- [ ] Update reducer contract test: add new event types to APPLIED list
- [ ] Update boundary test: classify new event types

### Phase 3: Pipeline orchestrator

Wire reducer + enrich into a Pipeline struct that orchestrates the two-pass flow.

**Files:**
- `modules/rootsignal-graph/src/pipeline.rs` — NEW: Pipeline struct with process(), rebuild(), replay_from()
- `modules/rootsignal-graph/src/lib.rs` — UPDATE: export pipeline module
- `modules/rootsignal-graph/tests/pipeline_test.rs` — NEW: end-to-end tests

**Tasks:**
- [ ] Implement `Pipeline::process()` — reducer.reduce → enrich → reducer.reduce
- [ ] Implement `Pipeline::rebuild()` — wipe + replay + enrich
- [ ] Implement `Pipeline::replay_from()` — incremental replay + enrich
- [ ] Implement enrichment event persistence (append to EventStore with actor="enricher")
- [ ] Implement replay filtering (skip enrichment events, recompute fresh)
- [ ] Migrate call sites to use `Pipeline` (wraps existing `GraphReducer`)
- [ ] Write end-to-end test: events → pipeline → graph state matches expectations
- [ ] Write test: replay produces identical graph
- [ ] Write test: enrichment events are persisted and filtered on replay

## Acceptance Criteria

### Functional
- [ ] `EmbeddingStore` caches embeddings in Postgres with get-or-compute semantics
- [ ] Reducer uses `EmbeddingStore` to write embeddings to graph nodes
- [ ] `enrich()` reads graph and emits `DiversityComputed`, `CauseHeatComputed`, `ActorStatsComputed` events
- [ ] Enrichment events flow through the reducer (depth = 1)
- [ ] `ActorIdentified` no longer increments `signal_count` — computed by enrichment
- [ ] Pipeline orchestrates the full cycle: reducer → enrich → reducer

### Quality Gates
- [ ] Replay test: same events produce identical graph
- [ ] All tests follow MOCK → FUNCTION → OUTPUT pattern
- [ ] Test names describe behavior, not implementation

### Non-Goals
- GraphWriter migration (Phase 3 of foundation plan — separate work)
- SIMILAR_TO edge rebuilding (separate enrichment pass, not in scope)
- EmbeddingStore backfill from existing Neo4j data (migration script, separate)
- Real-time NOTIFY subscription for enrichment triggering (future)

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
