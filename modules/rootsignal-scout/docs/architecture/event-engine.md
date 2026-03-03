# Event Engine

Scout runs on [seesaw](https://crates.io/crates/seesaw_core), an event-sourced engine with handler-based dispatch. The entire scout pipeline is driven by emitting a single `EngineStarted` event and calling `engine.settle()` — handlers react to events, emit child events, and the engine loops until quiescence.

## Dispatch Loop

Seesaw's settle loop processes events in three phases per iteration:

```
loop {
    // 1. DRAIN EVENT QUEUE
    while event = queue.poll():
        apply_to_aggregators(event)   // REDUCE — fold into PipelineState
        execute_inline_handlers(event) // ROUTE — priority-ordered handlers
        queue_async_handlers(event)    // queue effects for parallel execution

    // 2. DRAIN EFFECT QUEUE
    join_all(queued_handlers)          // execute in parallel
    collect_emitted_events()           // child events re-enter the queue

    // 3. TERMINATE when nothing processed
    if !processed_any: break
}
```

Child events emitted by handlers re-enter the event queue, creating **causal chains**. The loop keeps draining until the system reaches quiescence (settled).

## Engine Setup

```rust
// core/engine.rs
let engine = seesaw_core::Engine::new(deps)
    .with_aggregators(pipeline_aggregators::aggregators())  // PipelineState
    .with_handlers(projection::handlers::handlers())        // persist + neo4j
    .with_handlers(signals::handlers::handlers())           // dedup, create, wire
    .with_handlers(lifecycle::handlers::handlers())         // reap, schedule, finalize
    .with_handlers(scrape::handlers::handlers())            // tension, response
    .with_handlers(discovery::handlers::handlers())         // bootstrap, link promotion
    .with_handlers(enrichment::handlers::handlers())        // actors, metrics
    .with_handlers(expansion::handlers::handlers())         // signal expansion
    .with_handlers(synthesis::handlers::handlers());        // trigger, 6 parallel roles, completion
```

The full engine adds `situation_weaving` and `supervisor` handlers.

## Infrastructure Handlers

Three infrastructure handlers run at priority 0–2 before any domain handler:

| Priority | ID | Trigger | Action |
|----------|-----|---------|--------|
| 0 | `persist` | every event | Persist to Postgres event store. Links causal chains via `parent_event_id`. |
| 1 | *(seesaw built-in)* | aggregator match | Apply event to `PipelineState` singleton aggregate. |
| 2 | `neo4j_projection` | every event | Project to Neo4j graph. Only `WorldEvent`, `SystemEvent`, and `DiscoveryEvent::SourceDiscovered` are projectable. All others are skipped. |

## PipelineState Aggregate

`PipelineState` is a seesaw **singleton aggregate** (`aggregate_type = "ScoutRun"`). It accumulates all mutable state for a run:

```rust
pub struct PipelineState {
    pub url_to_canonical_key: HashMap<String, String>,   // URL resolution
    pub source_signal_counts: HashMap<String, u32>,      // per-source yield
    pub expansion_queries: Vec<String>,                  // discovered queries
    pub social_expansion_topics: Vec<String>,            // social topics
    pub stats: ScoutStats,                               // cumulative metrics
    pub actor_contexts: HashMap<String, ActorContext>,    // per-source actors
    pub pending_nodes: HashMap<Uuid, PendingNode>,       // dedup → create handoff
    pub wiring_contexts: HashMap<Uuid, WiringContext>,   // create → wire handoff
    pub scheduled: Option<ScheduledData>,                // schedule → scrape handoff
    pub social_topics: Vec<String>,                      // mid-run → response handoff
    pub collected_links: Vec<CollectedLink>,              // scrape → promote handoff
    // ...
}
```

Four aggregator functions fold events into state:

```rust
#[aggregators(singleton)]
pub mod pipeline_aggregators {
    fn on_signal(state: &mut PipelineState, event: SignalEvent);
    fn on_scrape(state: &mut PipelineState, event: ScrapeEvent);
    fn on_discovery(state: &mut PipelineState, event: DiscoveryEvent);
    fn on_pipeline(state: &mut PipelineState, event: PipelineEvent);
}
```

`LifecycleEvent` and `EnrichmentEvent` are no-ops for aggregate state.

### Reading State in Handlers

Handlers read aggregate state via `ctx.singleton()`, which returns zero-clone `Arc` references:

```rust
let (_, state) = ctx.singleton::<PipelineState>();  // (prev, current)
// state is Arc<PipelineState> — no cloning, no async lock
```

Workflows read final state from the engine after settle:

```rust
engine.emit(LifecycleEvent::EngineStarted { .. }).await;
engine.settle().await?;
let state: Arc<PipelineState> = engine.singleton::<PipelineState>();
```

### State Handoff via Stashing

Cross-phase data transfer uses the aggregate as a shared blackboard:

1. Handler A emits a `PipelineEvent` carrying accumulated data
2. Aggregator folds that data into `PipelineState` fields
3. Handler B reads those fields via `ctx.singleton()`
4. Handler B emits a clearing event (e.g., `SocialTopicsConsumed`) when done

This ensures all state changes are event-driven and survive crash recovery.

## Projection: Neo4j as Derived View

The Neo4j graph is a **materialized projection** of the event store, not the source of truth. The `neo4j_projection` handler (priority 2) constructs `StoredEvent` structs and passes them to `GraphProjector::project()`, which executes idempotent `MERGE`-based Cypher queries.

Only three event categories are projected:
- **WorldEvent** — always projected (signal creation, citations, resources, provenance)
- **SystemEvent** — always projected (classifications, corrections, relationships)
- **DiscoveryEvent::SourceDiscovered** — the only projectable domain event (creates Source nodes)

Everything else (lifecycle, signal processing, pipeline bookkeeping) stays in the Postgres event store only.

## ScoutEngineDeps

All handler dependencies live in a single struct shared via `Arc`:

```rust
pub struct ScoutEngineDeps {
    pub store: Arc<dyn SignalReader>,          // read-only graph queries
    pub embedder: Arc<dyn TextEmbedder>,      // Voyage AI embeddings
    pub fetcher: Option<Arc<dyn ContentFetcher>>,  // web/feed/social
    pub extractor: Option<Arc<dyn SignalExtractor>>, // LLM extraction
    pub embed_cache: EmbeddingCache,          // in-memory dedup cache
    pub graph_projector: Option<GraphProjector>,    // Neo4j projection
    pub event_store: Option<RsEventStore>,    // Postgres event store
    pub graph_client: Option<GraphClient>,    // Neo4j client (reads)
    pub budget: Option<Arc<BudgetTracker>>,   // cost tracking
    pub cancelled: Option<Arc<AtomicBool>>,   // cancellation flag
    pub run_id: String,                       // current run ID
    // ...
}
```

Optional fields are `None` in tests, allowing handler tests to run without Neo4j, Postgres, or external APIs.

## Workflows (Restate)

Each workflow is a Restate durable function that:

1. Validates phase status via a Neo4j `ScoutTask` node
2. Builds an engine with all per-invocation resources
3. Emits a single entry event and calls `settle()`
4. Reads final state from `engine.singleton::<PipelineState>()`
5. Writes completion status back to the task node

| Workflow | Engine | Entry Event |
|----------|--------|-------------|
| `BootstrapWorkflow` | scrape | `EngineStarted` |
| `ScrapeWorkflow` | scrape | `EngineStarted` |
| `FullScoutRunWorkflow` | full | `EngineStarted` |
| `SynthesisWorkflow` | full | `PhaseCompleted(Expansion)` |
| `SituationWeaverWorkflow` | full | `PhaseCompleted(Synthesis)` |
| `SupervisorWorkflow` | full | `PhaseCompleted(SituationWeaving)` |
