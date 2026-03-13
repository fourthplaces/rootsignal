---
date: 2026-03-01
topic: pipeline-state-to-seesaw-aggregate
---

# PipelineState → Seesaw Aggregate

## The Problem

PipelineState has two mutation paths:

1. **Event-driven** (sound): `apply_scrape()`, `apply_signal()`, `apply_discovery()` — called by the priority-1 `apply_to_aggregate_handler` in response to events flowing through seesaw's settle loop. These are persisted, replayable, traceable.

2. **Direct handler writes** (unsound): `apply_scrape_output()`, `apply_expansion_output()`, `apply_schedule_output()`, and a direct `social_topics` assignment — handlers acquire the `Arc<RwLock>` write lock and stuff results into state. These bypass the event system entirely.

Path 2 breaks on crash. If the workflow fails after a handler writes state but before the next event persists, the state is lost. PipelineState lives in `Arc<RwLock>` — purely in-memory, no persistence, no replay. On Restate retry, PipelineState starts fresh and downstream handlers operate on incomplete state.

Concretely, what vanishes:

- `apply_schedule_output()`: scheduled_data (which sources to scrape), actor_contexts, url_mappings. **If lost, scrape handlers don't know what to scrape.**
- `apply_scrape_output()`: url_mappings, source_signal_counts, pub_dates, collected_links, expansion_queries, query_api_errors, stats deltas.
- `apply_expansion_output()`: social_expansion_topics, expansion stats.
- `social_topics` direct write: topics for response scrape phase.

This is fundamentally unsound. All state mutations must flow through events.

## Why Not Just "Emit Events" Without Aggregates?

We could turn the `apply_*_output()` calls into Pipeline events and keep PipelineState as `Arc<RwLock>`. The priority-1 handler would apply them. Events get persisted. Problem solved?

Partially. The state is still ephemeral in-memory. On cold start, there's no hydration path — you'd need to manually replay events into PipelineState, which is what seesaw's aggregate machinery already does (auto-persist, auto-hydrate, snapshots). Building that ourselves would be reimplementing seesaw.

## The Design

PipelineState becomes a seesaw Aggregate. All mutations flow through events. Seesaw handles persistence, hydration, and snapshots automatically.

### Key Decisions

**EmbeddingCache is not state — it's a service.**

`embed_cache` holds `Vec<f32>` LLM embeddings from Voyage AI. It's a lookup cache, not event-derived state. It doesn't serialize, can't replay, and doesn't need to survive restarts (Restate re-runs the handlers that populate it).

Move to `deps.embed_cache`. Dedup handler calls `deps.embed_cache.add(node_id, embedding)`. Create handler calls `deps.embed_cache.get(node_id)`. PipelineState drops the field entirely, enabling `Serialize + Deserialize`.

**Keying: `run_id` on every event.**

Seesaw aggregators extract an ID from event payloads: `|e| e.order_id`. PipelineState is a singleton per run — every event maps to the same instance. Add `run_id: Uuid` to each event enum. This is natural — events should know which run produced them. The run_id is already stored alongside events in Postgres via `AppendEvent::with_run_id()`, just not in the payload itself.

```rust
#[aggregators]
mod pipeline_aggregators {
    use super::*;

    #[aggregator(id_fn = "run_id")]
    fn on_signal(state: &mut PipelineState, event: SignalEvent) {
        state.apply_signal(&event);
    }

    #[aggregator(id_fn = "run_id")]
    fn on_scrape(state: &mut PipelineState, event: ScrapeEvent) {
        state.apply_scrape(&event);
    }

    #[aggregator(id_fn = "run_id")]
    fn on_discovery(state: &mut PipelineState, event: DiscoveryEvent) {
        state.apply_discovery(&event);
    }

    #[aggregator(id_fn = "run_id")]
    fn on_pipeline(state: &mut PipelineState, event: PipelineEvent) {
        state.apply_pipeline(&event);
    }
}

// Registration:
engine.with_aggregators(pipeline_aggregators::aggregators())
```

**Output stashing becomes Pipeline events.**

Instead of handlers acquiring the write lock:

```rust
// Before (unsound):
let mut state = deps.state.write().await;
state.apply_scrape_output(output);

// After (event-sourced):
Ok(events![
    PipelineEvent::ScrapeAccumulated {
        url_mappings, source_signal_counts, pub_dates,
        collected_links, expansion_queries, query_api_errors, stats_delta,
    },
    LifecycleEvent::PhaseCompleted { phase: TensionScrape },
])
```

Four new PipelineEvent variants:

- `ScrapeAccumulated` — replaces `apply_scrape_output()`
- `ScheduleResolved` — replaces `apply_schedule_output()`
- `ExpansionAccumulated` — replaces `apply_expansion_output()`
- `SocialTopicsCollected` — replaces direct `social_topics` assignment

**Handlers read via `get_transition_arc` — zero-clone.**

```rust
let registry = ctx.aggregator_registry().unwrap();
let (_, state) = registry.get_transition_arc::<PipelineState>(run_id);
// state is Arc<PipelineState> — read through the Arc, no cloning HashMaps
```

**Consume/drain patterns emit clearing events.**

`std::mem::take(&mut state.social_topics)` becomes: read from aggregate, emit `PipelineEvent::SocialTopicsConsumed`. Settle loop is sequential — no double-read risk.

**PendingNode embedding field.**

`PendingNode.embedding: Vec<f32>` is `#[serde(skip)]`. Two options:

1. Serialize it (it's just floats — verbose but correct, survives snapshots).
2. Move to `deps.embed_cache` keyed by `node_id` (cleaner separation — embedding is a computed artifact, not a domain fact).

Option 2 is preferred. Dedup stores the embedding in the cache, create reads it from the cache. PendingNode drops the embedding field.

### Aggregate Implementation

```rust
impl Aggregate for PipelineState {
    fn aggregate_type() -> &'static str { "ScoutRun" }
}

impl Apply<SignalEvent> for PipelineState {
    fn apply(&mut self, event: SignalEvent) {
        self.apply_signal(&event); // existing pure method
    }
}

impl Apply<ScrapeEvent> for PipelineState { /* delegates to apply_scrape() */ }
impl Apply<DiscoveryEvent> for PipelineState { /* delegates to apply_discovery() */ }

impl Apply<PipelineEvent> for PipelineState {
    fn apply(&mut self, event: PipelineEvent) {
        match event {
            PipelineEvent::ScrapeAccumulated { url_mappings, .. } => {
                self.url_to_canonical_key.extend(url_mappings);
                // ... same logic as current apply_scrape_output()
            }
            PipelineEvent::ScheduleResolved { scheduled_data, actor_contexts, url_mappings } => {
                self.actor_contexts.extend(actor_contexts);
                self.url_to_canonical_key.extend(url_mappings);
                self.scheduled = Some(scheduled_data);
            }
            PipelineEvent::ExpansionAccumulated { social_topics, .. } => {
                self.social_expansion_topics.extend(social_topics);
                // ... stats
            }
            PipelineEvent::SocialTopicsCollected { topics } => {
                self.social_topics = topics;
            }
            PipelineEvent::SocialTopicsConsumed => {
                self.social_topics.clear();
            }
        }
    }
}
```

### What Gets Deleted

- `apply_to_aggregate_handler()` — seesaw's `apply_to_aggregators()` does this automatically
- `deps.state: Arc<RwLock<PipelineState>>` — state lives in seesaw's DashMap
- All `deps.state.read().await` / `deps.state.write().await` — replaced with `get_transition_arc`
- `apply_scrape_output()`, `apply_expansion_output()`, `apply_schedule_output()` — logic moves to `Apply<PipelineEvent>` impls

### Engine Setup

```rust
pub fn build_engine(deps: ScoutEngineDeps) -> SeesawEngine {
    seesaw_core::Engine::new(deps)
        .with_event_store(event_store)           // auto-persist + auto-hydrate
        .with_snapshot_store(snapshot_store)      // optional perf optimization
        // Register PipelineState aggregate (run_id extracted from each event)
        .with_aggregators(pipeline_aggregators::aggregators())
        // Infrastructure handlers (persist to rootsignal event store, project to Neo4j)
        .with_handlers(projection::handlers::handlers())
        // Domain handlers
        .with_handlers(/* ... */)
}
```

### What This Gives Us

- **Crash-safe**: every state mutation is an event, persisted by seesaw's auto-persist
- **Cold-start hydration**: seesaw auto-loads aggregate from EventStore on first access
- **Snapshots**: `snapshot_every(N)` accelerates hydration for long runs
- **No manual lock management**: `Arc<RwLock>` gone, no deadlock risk
- **Less code**: delete the priority-1 handler, the RwLock field, all `state.read()/write()` ceremony
- **Traceable**: every state change is an event in the log — no invisible side-channel mutations

### Resumption: Restate Handles It

Aggregates are for domain modeling (invariants, transitions, state reconstruction). Resumption after crash is handled by Restate's Runtime trait — seesaw wraps each handler invocation via `runtime.run()`, journaling results. On replay, completed handlers return cached results. These are different concerns:

- **Restate**: "don't re-execute handler X, return its cached result"
- **Aggregates**: "reconstruct PipelineState so handler Y can read correct state"

Both are needed. Restate avoids re-doing expensive work (HTTP, LLM). Aggregates ensure state is correct for new work.

## Migration Steps

1. Move `embed_cache` to `ScoutEngineDeps`, remove from PipelineState
2. Add `run_id: Uuid` to each event enum (SignalEvent, ScrapeEvent, DiscoveryEvent, LifecycleEvent, EnrichmentEvent)
3. Derive `Serialize, Deserialize, Clone` on PipelineState
4. Add `PipelineEvent` enum with the four new variants (+ `run_id: Uuid`)
5. Implement `Aggregate` + `Apply<E>` traits via `#[aggregators]` module
6. Update `build_engine` / `build_full_engine` to register aggregators via `with_aggregators()`
7. Convert each handler: replace `deps.state.write().apply_*_output()` with event emission
8. Convert each handler: replace `deps.state.read()` with `get_transition_arc`
9. Delete `apply_to_aggregate_handler`, `Arc<RwLock<PipelineState>>` field, `apply_*_output()` methods
10. Tests: verify aggregate state survives simulated cold start
