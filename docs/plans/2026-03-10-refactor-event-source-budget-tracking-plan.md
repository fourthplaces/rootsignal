---
title: "refactor: Event-source budget tracking"
type: refactor
date: 2026-03-10
---

# Event-Source Budget Tracking

Move budget tracking from `ScoutEngineDeps` runtime object to event-sourced aggregate state on `PipelineState`. Budget limit set via `ScoutRunRequested`, spend tracked via `BudgetSpent` pipeline events, handlers check `state.has_budget()`. Projection reads `state.stats` directly — fully replayable.

## Problem Statement

Budget lives on `deps.budget` as an `Arc<BudgetTracker>` with an `AtomicU64` counter. This creates three problems:

1. **Not replayable.** The `scout_runs_projection` reads `deps.budget.total_spent()` to write stats. During `REPLAY=1`, this projection doesn't exist — Postgres operational tables can't be rebuilt from the event log.
2. **Lost on crash.** `build_engine_deps_for_resume()` passes `spent_cents = 0`. The atomic counter is gone; budget state is not in the event stream.
3. **Projection coupled to engine runtime.** The projection reaches into `deps.budget` — infrastructure state leaking into what should be a pure event-driven read model.

Budget is *state*. It belongs on the aggregate.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Where does BudgetSpent live? | `PipelineEvent` | Pipeline-level bookkeeping, not domain logic |
| What type for budget_cents on ScoutRunRequested? | `u64` with `#[serde(default)]` | 0 = unlimited, matches existing BudgetTracker convention. Old events replay as unlimited — correct, since we can't recover their original limit anyway |
| Emit BudgetSpent at all handler sites? | No — only at existing `spend()` site (source_finder) for now | Preserves current behavior. Adding spend tracking to other handlers is a separate improvement |
| NewsScanner budget? | Keep standalone BudgetTracker | NewsScanner runs on a separate engine with no pipeline aggregators. Out of scope |
| SourceFinder refactoring? | Return `spent_cents: u64` in output, handler emits BudgetSpent | SourceFinder is a pure activity — it can't emit events |
| BudgetSpent context field? | Not now — `BudgetSpent { cents: u64 }` only | Add operation context in a follow-up |
| Concurrency concern? | Non-issue | Seesaw settles sequentially within a dispatch cycle |

## Acceptance Criteria

- [x] `ScoutRunRequested` carries `budget_cents: u64`
- [x] `PipelineEvent::BudgetSpent { cents: u64 }` exists
- [x] `PipelineState` has `budget_limit_cents: u64` field, set by reducer on `ScoutRunRequested`
- [x] `PipelineState` accumulates `stats.spent_cents` from `BudgetSpent` events
- [x] `state.has_budget(cost) -> bool` method on PipelineState (0 = unlimited)
- [x] All handlers check `state.has_budget()` instead of `deps.budget.has_budget()`
- [x] `deps.budget` field removed from `ScoutEngineDeps`
- [x] `scout_runs_projection` reads `state.stats` directly — no deps.budget access
- [x] SourceFinder returns spend amount; handler emits `BudgetSpent`
- [x] NewsScanner reads budget limit via `daily_budget_cents` param (not deps.budget)
- [x] `budget.log_status()` calls replaced with tracing from state
- [x] All existing tests pass (411/411)
- [x] `BudgetTracker` struct stays (for OperationCost constants + NewsScanner) but is no longer on deps

## Implementation

### Phase 1: Add event + state infrastructure

**Files:**

#### `modules/rootsignal-scout/src/domains/lifecycle/events.rs`

Add `budget_cents` to `ScoutRunRequested`:

```rust
ScoutRunRequested {
    run_id: Uuid,
    #[serde(default)]
    scope: RunScope,
    #[serde(default)]
    budget_cents: u64,
},
```

#### `modules/rootsignal-scout/src/core/pipeline_events.rs`

Add `BudgetSpent` variant:

```rust
pub enum PipelineEvent {
    HandlerFailed { ... },
    BudgetSpent { cents: u64 },
}
```

Update `is_projectable()` — `BudgetSpent` is not Neo4j-projectable (no graph mutation):

```rust
pub fn is_projectable(&self) -> bool {
    matches!(self, Self::HandlerFailed { .. })
}
```

#### `modules/rootsignal-scout/src/core/aggregate.rs`

Add field to `PipelineState`:

```rust
#[serde(default)]
pub budget_limit_cents: u64,
```

Add method:

```rust
pub fn has_budget(&self, cost: u64) -> bool {
    self.budget_limit_cents == 0 || self.stats.spent_cents + cost <= self.budget_limit_cents
}
```

Update `apply_lifecycle`:

```rust
LifecycleEvent::ScoutRunRequested { scope, budget_cents, .. } => {
    self.run_scope = scope.clone();
    self.budget_limit_cents = *budget_cents;
}
```

Update `apply_pipeline`:

```rust
pub fn apply_pipeline(&mut self, event: &PipelineEvent) {
    match event {
        PipelineEvent::HandlerFailed { .. } => {
            self.stats.handler_failures += 1;
        }
        PipelineEvent::BudgetSpent { cents } => {
            self.stats.spent_cents += cents;
        }
    }
}
```

Initialize `budget_limit_cents: 0` in `PipelineState::new()`.

#### `modules/rootsignal-scout/src/core/aggregate.rs` (tests)

```rust
#[test]
fn budget_spent_accumulates_on_state() {
    let mut state = PipelineState::default();
    state.budget_limit_cents = 100;
    assert!(state.has_budget(50));

    state.apply_pipeline(&PipelineEvent::BudgetSpent { cents: 80 });
    assert_eq!(state.stats.spent_cents, 80);
    assert!(!state.has_budget(30));
}

#[test]
fn unlimited_budget_always_has_budget() {
    let state = PipelineState::default(); // budget_limit_cents = 0
    assert!(state.has_budget(1_000_000));
}
```

### Phase 2: Remove deps.budget, update handlers

#### `modules/rootsignal-scout/src/core/engine.rs`

Remove `budget` field from `ScoutEngineDeps`:

```rust
// DELETE: pub budget: Option<Arc<...BudgetTracker>>,
```

Remove `budget: None` from `ScoutEngineDeps::new()`.

#### `modules/rootsignal-scout/src/workflows/mod.rs`

Remove BudgetTracker construction from `build_base_deps()`. The `budget_cents` value flows through `ScoutRunRequested` instead. Callers that currently pass `budget_cents` to engine builders now pass it when emitting `ScoutRunRequested`.

Add `budget_cents` as a return value or store it for the caller to put on the event:

```rust
// build_base_deps no longer creates BudgetTracker
// budget_cents is passed through to the ScoutRunRequested emission site
```

#### Handler updates (all follow the same pattern)

**Before:**
```rust
let (graph, budget) = match (deps.graph.as_deref(), deps.budget.as_ref()) {
    (Some(g), Some(b)) => (g, b),
    _ => return Ok(events![...skipped...]),
};
if !budget.has_budget(OperationCost::CLAUDE_HAIKU_SYNTHESIS * 10) { ... }
```

**After:**
```rust
let graph = match deps.graph.as_deref() {
    Some(g) => g,
    None => return Ok(events![...skipped...]),
};
if !state.has_budget(OperationCost::CLAUDE_HAIKU_SYNTHESIS * 10) { ... }
```

**Files to update:**

| File | Lines | Change |
|------|-------|--------|
| `domains/discovery/mod.rs` | 222 | Remove budget from destructure, use `state.has_budget()` |
| `domains/expansion/mod.rs` | 35, 70 | Remove budget from destructure, remove `log_status()` |
| `domains/synthesis/mod.rs` | 112 | Remove budget from 3-tuple match |
| `domains/curiosity/mod.rs` | 121, 207, 404, 593 | Remove budget from 4-tuple matches |
| `domains/situation_weaving/activities/mod.rs` | 22 | Remove budget from 4-tuple match |
| `domains/news_scanning/activities.rs` | 17 | Read `state.budget_limit_cents` instead of `deps.budget.daily_limit()` |

#### `budget.log_status()` replacement

The two `budget.log_status()` calls (projection.rs:228, expansion/mod.rs:71) become:

```rust
tracing::info!(
    spent_cents = state.stats.spent_cents,
    budget_limit = state.budget_limit_cents,
    "Budget status"
);
```

### Phase 3: SourceFinder refactoring

#### `modules/rootsignal-scout/src/domains/discovery/activities/source_finder.rs`

Replace `budget: &'a BudgetTracker` with simple state:

```rust
pub struct SourceFinder<'a> {
    graph: &'a dyn GraphQueries,
    region_slug: Option<String>,
    region_name: Option<String>,
    ai: Option<&'a dyn Agent>,
    budget_exhausted: bool,           // was: budget: &'a BudgetTracker
    embedder: Option<&'a dyn TextEmbedder>,
}
```

The `discover_from_gaps` method (line 629-634) checks `self.budget_exhausted` instead of `self.budget.has_budget()`.

The `spend()` call (line 677) is removed. Instead, track spend count internally:

```rust
// In the struct:
discovery_llm_calls: u32,

// After successful LLM response:
self.discovery_llm_calls += 1;
```

Add a method to report spend:

```rust
pub fn spent_cents(&self) -> u64 {
    self.discovery_llm_calls as u64 * OperationCost::CLAUDE_HAIKU_DISCOVERY
}
```

The calling handler reads `finder.spent_cents()` after the activity completes and emits:

```rust
if finder.spent_cents() > 0 {
    out.push(PipelineEvent::BudgetSpent { cents: finder.spent_cents() });
}
```

#### Constructor call sites

`discovery/activities/mod.rs` and `expansion/activities/mod.rs` change from passing `&budget` to passing `budget_exhausted: bool`:

```rust
let budget_exhausted = !state.has_budget(OperationCost::CLAUDE_HAIKU_DISCOVERY);
let finder = SourceFinder::new(graph, region_slug, region_name, ai, budget_exhausted, embedder);
```

### Phase 4: Simplify projection

#### `modules/rootsignal-scout/src/core/projection.rs`

The `scout_runs_projection` terminal event handler becomes:

```rust
if is_terminal_event(&event, &ctx) {
    let deps = ctx.deps();
    let state = ctx.aggregate::<PipelineState>().curr;
    info!("{}", state.stats);

    if let Some(pool) = &deps.pg_pool {
        let stats_json = serde_json::to_value(&state.stats)?;
        sqlx::query(
            "UPDATE scout_runs SET stats = $2, spent_cents = $3 WHERE run_id = $1",
        )
        .bind(deps.run_id.to_string())
        .bind(stats_json)
        .bind(state.stats.spent_cents as i64)
        .execute(pool)
        .await?;
    }
}
```

No `deps.budget` access. Pure read from aggregate state.

### Phase 5: Update emission sites

Every place that emits `ScoutRunRequested` needs to pass `budget_cents`:

| File | Current | After |
|------|---------|-------|
| `testing.rs` ~line 2367 | `ScoutRunRequested { run_id, scope }` | `ScoutRunRequested { run_id, scope, budget_cents: 0 }` (unlimited in tests) |
| `signals/activities/engine_tests.rs` ~line 952 | Same | Same pattern |
| API/workflow callers | Emit after engine build | Pass `budget_cents` from `ScoutDeps.daily_budget_cents` or caller override |

### Phase 6: Cleanup

- Delete `BudgetTracker` tests that test the atomic counter behavior (replaced by aggregate tests)
- Keep `OperationCost` constants (still used by handlers)
- Keep `BudgetTracker` struct only if NewsScanner still needs it; otherwise delete entirely
- Verify `BudgetPage.tsx` / GraphQL `spent_today_cents` query still works (reads from `scout_runs.spent_cents` column — unchanged)

## References

- `modules/rootsignal-scout/src/core/aggregate.rs` — PipelineState aggregate
- `modules/rootsignal-scout/src/core/engine.rs:59` — `deps.budget` field
- `modules/rootsignal-scout/src/core/projection.rs:224-249` — scout_runs_projection budget read
- `modules/rootsignal-scout/src/core/pipeline_events.rs` — PipelineEvent enum
- `modules/rootsignal-scout/src/domains/lifecycle/events.rs:18` — ScoutRunRequested
- `modules/rootsignal-scout/src/domains/scheduling/activities/budget.rs` — BudgetTracker + OperationCost
- `modules/rootsignal-scout/src/domains/discovery/activities/source_finder.rs:435` — SourceFinder struct
- `modules/rootsignal-scout/src/workflows/mod.rs:60-93` — budget construction in build_base_deps
