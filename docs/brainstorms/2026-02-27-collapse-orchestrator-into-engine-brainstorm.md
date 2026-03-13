---
date: 2026-02-27
topic: collapse-orchestrator-into-engine
---

# Collapse the Orchestrator into the Engine

## The Problem

Scout has two systems doing one job. The engine handles causal chains
(extract -> dedup -> create -> wire actors), but a 2,200-line orchestrator
(`scrape_pipeline.rs` + `scrape_phase.rs`) manually sequences everything
around it: fetching content, running discovery, enriching actors, updating
metrics, expanding queries. Each of those steps is a reaction to something
completing — exactly what the engine is designed to do.

The result: you can't understand a scout run by reading events. You have to
read the orchestrator to know what happens after the engine finishes each
phase. The engine does half the work, the orchestrator does the other half.

## Decision: Replace rootsignal-engine with Seesaw

Scout's hand-rolled engine (90 lines) does the job but will never grow
transition guards, event upcasting, snapshots, or replay tooling. Seesaw
(~/Developer/crcn/seesaw-rs) is pivoting from orchestration runtime to
event-sourcing library. Scout is the primary consumer driving the ES API.

Seesaw provides:
- `Aggregate` trait — `apply(&mut self, event)` is the reducer
- `EventStore` — append-only with causal chains (`caused_by`)
- `#[handler]` macro — reactions with extract, filter, transition guards
- `EventUpcast` — schema evolution (closes audit gap)
- `engine.append()` — persist + apply + dispatch handlers + recurse, one call
- Priority-based handler ordering — projections run before reactive handlers
- Transition guards — settlement without manual orchestration

## The Target Architecture

```rust
let engine = Engine::new(deps, PostgresBackend::new(pool))
    .with_event_store(event_store)
    .with_handler(project_to_neo4j())     // priority 0 — read model
    .with_handler(handle_dedup())
    .with_handler(handle_creation())
    .with_handler(handle_fetch())
    .with_handler(handle_extract())
    .with_handler(handle_discovery())
    .with_handler(handle_enrichment())     // transition guard
    .with_handler(handle_metrics())
    .with_handler(handle_expansion());

// The entire orchestrator becomes one line:
engine.append(run_id, 0, vec![PipelineEvent::RunStarted { region }]).await?;
```

`scrape_pipeline.rs` and `scrape_phase.rs` disappear.

## How Each Piece Maps

| Scout today | Seesaw equivalent |
|---|---|
| `PipelineState` | `impl Aggregate` with `apply()` |
| `ScoutReducer::reduce()` | `Aggregate::apply(&mut self, event)` |
| `GraphProjector::project()` | `#[handler(priority = 0)]` → `Result<()>` |
| `ScoutRouter::route()` | Gone — individual `#[handler]` registrations |
| `Engine::dispatch()` loop | `engine.append()` — append + apply + dispatch + recurse |
| `EventStore::append_and_read()` | Seesaw's ES layer inside `engine.append()` |
| Dedup/creation/bootstrap handlers | `#[handler]` with extract |
| Orchestrator sequencing (`run_all()`) | Transition guards on state |
| `scrape_pipeline.rs` (2,200 lines) | One `engine.append()` call |
| `scrape_phase.rs` (2,400 lines) | Fetch/extract handlers |
| `synthesis.rs` orchestration | Discovery handler triggered by `PhaseCompleted` |
| Missing: upcasting | `EventUpcast` trait |
| Missing: replay | `AggregateLoader::load()` |
| Missing: snapshots | Future seesaw feature |

## Handler Design

**Neo4j projection** — observer, no child events, runs first:
```rust
#[handler(on = ScoutEvent, id = "neo4j_projection", priority = 0)]
async fn project_to_neo4j(event: ScoutEvent, ctx: Context<Deps>) -> Result<()> {
    match event {
        ScoutEvent::World(WorldEvent::GatheringDiscovered { id, title, .. }) => {
            ctx.deps().graph.run(merge_gathering(id, title)).await?;
        }
        _ => {}
    }
    Ok(())
}
```

**Reactive handler** — returns child events:
```rust
#[handler(on = ScoutEvent, extract(url, canonical_key, count), id = "dedup")]
async fn handle_signals_extracted(
    url: String,
    canonical_key: String,
    count: u32,
    ctx: Context<Deps>
) -> Result<Vec<ScoutEvent>> {
    let batch = ctx.state().extracted_batches.get(&url);
    // ... dedup logic, return verdicts as child events
}
```

**Settlement handler** — fires when phase completes:
```rust
#[handler(
    on = ScoutEvent,
    id = "phase_settled",
    transition(|prev, next| prev.sources_remaining > 0 && next.sources_remaining == 0)
)]
async fn run_enrichment(ctx: Context<Deps>) -> Result<Vec<ScoutEvent>> {
    // All sources done — extract actors, enrich locations
}
```

**Discovery handler** — calls existing modules from within handler:
```rust
#[handler(on = ScoutEvent, extract(phase), id = "discovery")]
async fn handle_discovery(phase: PipelinePhase, ctx: Context<Deps>) -> Result<Vec<ScoutEvent>> {
    if phase != PipelinePhase::Response { return Ok(vec![]); }
    let (stats, sources) = ctx.deps().gathering_finder.run().await;
    // ... emit SourceDiscovered events
}
```

## New Events Needed

Pipeline events (bookkeeping):
- `RunStarted { run_id, region }` (replaces manual reap + schedule + boot)
- `SourcesScheduled { phase, sources }` (replaces manual phase dispatch)
- `SourceQueued { url, canonical_key }` (replaces manual fetch loop)
- `ExtractionCompleted { url }` (ties extraction to engine)
- `EnrichmentCompleted` (triggers metrics)
- `MetricsUpdated` (triggers expansion)
- `RunCompleted { stats }` (terminal event)

Already exist: `ContentFetched`, `ContentUnchanged`, `ContentFetchFailed`,
`PhaseStarted`, `PhaseCompleted`, `SignalsExtracted`.

## State Changes for Settlement

`PipelineState` needs:
- `sources_remaining: u32` — decremented by apply() on each `UrlProcessed`,
  transition guard fires enrichment when it hits zero
- `phase: PipelinePhase` — current phase, set by apply() on `PhaseStarted`

## Implementation Order

### Phase 0: Seesaw ES Primitives (blocked — Craig building)

Seesaw ships: `Aggregate`, `EventStore`, `EventUpcast`, `#[handler]` macro
with transition guards, priority, causal chains (`caused_by`).

### Phase 1: Replace rootsignal-engine with Seesaw

- Add seesaw as dependency
- Rewrite `PipelineState` as `impl Aggregate`
- Rewrite `GraphProjector` as priority-0 handler
- Migrate existing handlers (dedup, creation, bootstrap) to `#[handler]`
- Delete `rootsignal-engine` crate, `ScoutReducer`, `ScoutRouter`

### Phase 2: Content Pipeline into Handlers

Move fetch -> extract chain out of `scrape_phase.rs`:
- `handle_fetch()` reacts to `SourceQueued`
- `handle_extract()` reacts to `ContentFetched`
- `handle_refresh()` reacts to `ContentUnchanged`
- Eliminates ~1,200 lines from `scrape_phase.rs`

### Phase 3: Phase Transitions + Settlement

- `handle_scheduling()` reacts to `RunStarted`
- Settlement via `sources_remaining` counter + transition guard
- Phase completion triggers next phase automatically

### Phase 4: Discovery + Enrichment + Metrics

- `handle_discovery()` reacts to `PhaseCompleted { Response }`
- `handle_enrichment()` fires on settlement (transition guard)
- `handle_metrics()` reacts to `EnrichmentCompleted`
- `handle_expansion()` reacts to `MetricsUpdated`

### Phase 5: Delete Orchestrator

- `scrape_pipeline.rs` → one `engine.append(RunStarted)` call
- `scrape_phase.rs` → deleted, logic in handlers
- `synthesis.rs` → simplified, discovery triggered by handler

## What This Does NOT Change

- The event taxonomy (World, System, Pipeline) — unchanged
- External crate boundaries (rootsignal-common, rootsignal-graph) — unchanged
- Existing handler logic (dedup, creation, bootstrap) — migrated, not rewritten
- Neo4j as derived projection — unchanged, just registered differently
- Restate workflows — still the outer shell, just thinner

## Risks

- **Seesaw readiness** — scout is blocked on seesaw shipping ES primitives
- **scrape_phase.rs is 2,411 lines** — decomposing into handlers is work
- **Concurrency** — `run_web()` uses `buffer_unordered(6)`. Need to handle
  concurrent source fetching within or across handlers
- **Testing** — 8-10 new handler test modules, but pattern is proven

## Open Questions

- Concurrent handler execution: batch sources into one event, or let
  the engine handle parallel dispatch?
- Cancellation: `RunCancelled` event that short-circuits remaining work?
- Should `RunContext` merge into `PipelineState`?
