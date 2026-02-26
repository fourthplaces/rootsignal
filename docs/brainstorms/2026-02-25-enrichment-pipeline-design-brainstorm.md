---
date: 2026-02-25
topic: enrichment-pipeline-design
---

# Enrichment Pipeline Design

## What We're Building

A pipeline architecture for computing derived values (embeddings, diversity, cause_heat) in the event-sourced system. The core question: how do non-factual computed values get onto graph nodes when the event log only carries facts?

## Why This Approach

We evaluated several tensions:

1. **Embeddings on events vs separate store** — Embeddings aren't facts about the world, so they don't belong on events. But the pipeline already computes them for dedup before emitting discovery events. An EmbeddingStore (get-or-compute cache) lets both the pipeline and reducer access embeddings without polluting the event stream.

2. **Producer-computed snapshots vs reducer graph reads** — We initially planned to put `new_source_diversity` on corroboration events (producer-computed snapshots). But this bends the "events carry facts" rule. Since the reducer is the only graph writer, anything it reads from the graph, it put there itself. Reads from its own output are deterministic on replay. The reducer can compute diversity by reading Evidence edges it previously wrote.

3. **Inline enrichment vs post-reduce passes** — Most derived values (diversity, corroboration count) are local to a single entity and can be computed inline by the reducer. cause_heat is the exception — it's a global computation requiring all signals in a bbox. It stays as a separate phase.

4. **Reducer as interpreter+executor vs separated steps** — The original reducer conflated interpreting events, building queries, and executing them. Separating into `project` (pure function: events → operations) and `apply` (mechanical execution) makes each step independently testable and enables batching/optimization at the execution layer.

## Key Decisions

### 1. EmbeddingStore: get-or-compute cache

A standalone cache keyed by `hash(model_version + input_text)`. Interface:

```rust
impl EmbeddingStore {
    /// Cache hit = instant. Miss = API call + store.
    async fn get(&self, text: &str) -> Result<Vec<f32>>;
}
```

Backed by Postgres:

```sql
CREATE TABLE embedding_cache (
    input_hash    TEXT PRIMARY KEY,
    model_version TEXT NOT NULL,
    embedding     FLOAT4[] NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

- Model version is configuration, not a parameter — change the model, all lookups miss, recompute naturally.
- The pipeline writes to it during extraction (it already has the embedding for dedup).
- The reducer reads from it during projection via `embeddings.get(text)`.
- On replay: cache hits are instant; misses recompute. No special replay logic.

### 2. Reducer can read the graph

The purity contract was "no graph reads" to prevent the reducer depending on external state. But the reducer is the only graph writer. Anything it reads, it wrote from a previous event. This is deterministic on replay — same events, same order, same reads, same writes. The graph is the accumulator.

The real rule is: **nothing else writes domain data to the graph.** The reducer's reads are safe because they're reads of its own output.

This means the reducer computes derived values inline:

- **source_diversity**: query Evidence edges for the entity, count distinct source URLs
- **channel_diversity**: query Evidence edges, count distinct channel types from external sources
- **corroboration_count**: count Evidence edges

These values no longer need to be on events. `ObservationCorroborated` carries only facts: entity_id, source_url, similarity. The reducer derives the rest from the graph.

### 3. project / apply / enrich pipeline

Separate interpretation from execution:

```rust
// Phase 1: interpret facts into graph operations (pure, testable)
let ops = project(events, &embeddings);

// Phase 2: execute operations against Neo4j (mechanical, batchable)
apply(ops, &graph);

// Phase 3: enrichment reads graph, emits new events
let enrichment_events = enrich(&graph, &embeddings);

// Phase 4: enrichment events get projected and applied too
let enrichment_ops = project(enrichment_events, &embeddings);
apply(enrichment_ops, &graph);
```

- **`project`**: pure function, no side effects, no graph connection needed. Fully unit-testable — "given these events, produce these operations."
- **`apply`**: mechanical execution. Handles batching (UNWIND), optimization (merge consecutive same-entity ops), error handling.
- **`enrich`**: reads the graph the reducer built, produces new events. Each enricher is a producer that emits typed events back into the stream.

`GraphOp` is an intermediate representation — typed operations, not raw Cypher strings. The apply step translates to efficient queries.

### 4. cause_heat as an enrichment phase

cause_heat is a global computation: all-pairs cosine similarity within a geographic bbox. It can't be computed inline during a single event because it depends on the full neighborhood of signals.

The enrichment phase:
1. Reads the graph (embeddings, diversity, coordinates)
2. Computes heat values
3. Emits `CauseHeatComputed { entity_id, heat: f64 }` events
4. Those events are projected and applied by the same pipeline

This keeps the reducer as the only graph writer. The enricher is just another producer. The event log captures when and why heat values changed — auditable.

### 5. Two kinds of event producers

The event stream has two kinds of producers:

1. **External observation producers** (scout pipeline) — "we observed a gathering at this location"
2. **Internal computation producers** (cause_heat, future enrichers) — "we computed that this signal has heat 0.73"

Both emit events. Both go through project → apply. The distinction is only about what triggers them — external scrapes vs internal graph analysis.

### 6. Derived values that come OFF events

With the reducer reading the graph, these fields can be removed from events:

- `ObservationCorroborated.new_corroboration_count` — reducer counts Evidence edges
- `ObservationCorroborated.new_source_diversity` — reducer counts distinct sources (never shipped, was proposed)
- `ObservationCorroborated.new_channel_diversity` — reducer counts distinct channels (never shipped, was proposed)

Events get simpler. The reducer computes from its own output.

## Pipeline Flow

After a scrape cycle:

```
Scout pipeline emits observation events
        ↓
project(events, embeddings) → Vec<GraphOp>
        ↓
apply(ops, graph)  [nodes, edges, embeddings, diversity, corroboration counts]
        ↓
enrich(graph, embeddings)  [cause_heat, future enrichers]
        ↓
project(enrichment_events, embeddings) → Vec<GraphOp>
        ↓
apply(enrichment_ops, graph)  [heat values written]
```

On replay, same sequence. Deterministic because the reducer only reads its own output.

## Resolved Questions

### GraphOp doesn't work (2026-02-25)

Pressure-tested the GraphOp typed enum against actual Cypher in `reducer.rs`. Found 7 patterns that break a CRUD-style intermediate representation:

1. **Conditional SET with CASE WHEN** (line 507): `CASE WHEN s.active = false AND $discovery_method = 'curated' THEN true ELSE s.active END`
2. **OPTIONAL MATCH + coalesce across 5 labels** (lines 249-257): find-any-node-by-id pattern
3. **FOREACH conditional edge creation** (lines 428-456): CitationRecorded combines find-any-label + MERGE edge + ON CREATE/ON MATCH SET
4. **FOREACH edge repointing with property copying** (lines 746-759): DuplicateActorsMerged
5. **Relative updates** (line 600): `s.signals_produced = s.signals_produced + $count`
6. **MERGE on non-id field** (line 489): MERGE on canonical_key
7. **Nullable datetime conversion** (line 1355): `CASE WHEN $content_date = '' THEN null ELSE datetime($content_date) END`

Building a GraphOp enum that handles all of these would be building a Cypher AST — the wrong abstraction.

### project() returning Vec<neo4rs::Query> doesn't help (2026-02-25)

Even if `project()` returned `Vec<neo4rs::Query>` instead of `Vec<GraphOp>`, testability doesn't improve. Tests would still be string-matching Cypher queries, which is what the existing reducer contract tests already do. The split adds a function boundary without improving confidence.

### Decision: keep reducer, defer project/apply split

The current reducer works. The Cypher is complex but correct. Adding EmbeddingStore + enrichment passes + a pipeline orchestrator on top of the existing reducer gives us everything we need without a risky rewrite of working query logic.

Revisit project/apply when:
- Batching becomes a performance bottleneck (UNWIND optimization needs a structured IR)
- A second graph backend appears (the IR would be the abstraction boundary)
- The reducer grows beyond ~1500 lines and needs decomposition for maintainability

## Open Questions

- **Enrichment scheduling**: Does cause_heat run after every scrape cycle, or on a timer? How does it know which bboxes to recompute?
- **Should `new_corroboration_count` come off the event now or later?** It works today. Removing it means the reducer must read the graph during corroboration. Could defer this cleanup.

## Next Steps

-> `/workflows:plan` to design the concrete implementation: EmbeddingStore, enrichment events, pipeline orchestrator on top of existing reducer.
