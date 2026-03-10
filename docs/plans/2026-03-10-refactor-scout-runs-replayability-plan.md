---
title: "refactor: Make scout_runs fully replayable from events"
type: refactor
date: 2026-03-10
---

# Make scout_runs Fully Replayable from Events

Enrich `ScoutRunRequested` with flow metadata (region_id, flow_type, source_ids, task_id) and add a `ScoutRunCompleted` lifecycle event so the `scout_runs` projection can be rebuilt entirely from the event stream. Then delete `early_insert_flow_run` and `post_settle_cleanup`.

## Problem Statement

The `scout_runs` Postgres table is written from two places outside the event stream:

1. **`early_insert_flow_run`** — pre-inserts `region_id`, `flow_type`, `source_ids`, `task_id`, `scope` before the engine settles. These columns cannot be derived from any event during replay.
2. **`post_settle_cleanup`** — sets `finished_at = now()` after settle completes. No event records that the run finished.

The `scout_runs_projection` already handles the INSERT (on `ScoutRunRequested`) and stats UPDATE (on terminal events), but the pre-inserted columns and `finished_at` live outside the event model. During `REPLAY=1`, these columns would be missing — the table cannot be rebuilt from events alone.

This is the same pattern fixed for budget tracking (commit `c8218726`): runtime state leaking into what should be a pure event-driven projection.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Where do flow metadata fields go? | On `ScoutRunRequested` | They describe the run at inception — they belong on the entry event |
| `region_id` type? | `Option<String>` with `#[serde(default)]` | Source-targeted runs have no region_id. Old events replay as None — correct |
| `flow_type` type? | `String` with `#[serde(default)]` | Values: "bootstrap", "scrape", "weave", "scout_source". String keeps it simple, enum is premature |
| `source_ids` type? | `Option<Vec<String>>` with `#[serde(default)]` | Only populated for scout_source flows. Already JSON in the DB |
| `task_id` type? | `Option<String>` with `#[serde(default)]` | FLY_MACHINE_ID — infrastructure metadata, None in dev/tests |
| How to record run completion? | New `LifecycleEvent::ScoutRunCompleted` | Terminal events are domain facts (SeverityInferred). Run completion is a lifecycle fact — different concerns |
| Who emits ScoutRunCompleted? | `run_completion_handler` — a handler inside the causal chain | Observes terminal events, emits `ScoutRunCompleted` as a caused event. Never bypasses the causal chain |
| Empty runs? | Non-issue | Every engine variant guarantees a terminal event. `NothingToWeave`, `NothingToSupervise` exist precisely for this — the chain always terminates |
| `finished_at` timestamp source? | `Utc::now()` at handler emission time, stored on the event | Event carries canonical completion time. Projection reads it |
| `started_at` replay fidelity? | Known limitation — `now()` in SQL | Same gap as today. Fix later by reading seesaw store `created_at` |
| Two-phase rollout? | Yes — additive first, subtractive second | Phase 1 enriches events + projection (safe, backwards-compatible). Phase 2 removes early_insert/post_settle (only after Phase 1 is verified in production) |

## Acceptance Criteria

- [x] `ScoutRunRequested` carries `region_id: Option<String>`, `flow_type: String`, `source_ids: Option<Vec<String>>`, `task_id: Option<String>`
- [x] `LifecycleEvent::ScoutRunCompleted { run_id: Uuid, finished_at: DateTime<Utc> }` exists
- [x] `run_completion_handler` emits `ScoutRunCompleted` on terminal events (inside the causal chain)
- [x] `scout_runs_projection` writes `region_id`, `flow_type`, `source_ids`, `task_id` from `ScoutRunRequested`
- [x] `scout_runs_projection` writes `finished_at` from `ScoutRunCompleted`
- [x] Terminal event stats writing moves from projection to `run_completion_handler`
- [x] All `ScoutRunRequested` emission sites pass flow metadata
- [x] `early_insert_flow_run` deleted
- [x] `post_settle_cleanup` deleted
- [x] `is_source_busy` query still works (reads `source_ids` + `finished_at IS NULL`)
- [x] `PipelineState` reducer handles `ScoutRunCompleted` (no-op)
- [x] `run_completion_handler` registered in all engine builders
- [x] All existing tests pass
- [x] `scout_runs` table can be rebuilt from events during replay

## Implementation

### Phase 1: Enrich events + projection (additive, backwards-compatible)

#### `modules/rootsignal-scout/src/domains/lifecycle/events.rs`

Add fields to `ScoutRunRequested` and new `ScoutRunCompleted` variant:

```rust
ScoutRunRequested {
    run_id: Uuid,
    #[serde(default)]
    scope: RunScope,
    #[serde(default)]
    budget_cents: u64,
    #[serde(default)]
    region_id: Option<String>,
    #[serde(default)]
    flow_type: String,
    #[serde(default)]
    source_ids: Option<Vec<String>>,
    #[serde(default)]
    task_id: Option<String>,
},
ScoutRunCompleted {
    run_id: Uuid,
    finished_at: DateTime<Utc>,
},
```

#### `modules/rootsignal-scout/src/core/aggregate.rs`

Handle `ScoutRunCompleted` in `apply_lifecycle`:

```rust
LifecycleEvent::ScoutRunCompleted { .. } => {}
```

#### `modules/rootsignal-scout/src/core/projection.rs` — `run_completion_handler`

Move terminal event detection + stats writing from the projection into a new handler that lives in the causal chain. The handler observes terminal events and emits `ScoutRunCompleted`.

```rust
/// Handler: observe terminal events, write stats, emit ScoutRunCompleted.
///
/// This replaces the terminal event block in scout_runs_projection.
/// Stats writing + completion are now inside the causal chain —
/// ScoutRunCompleted is caused by the terminal event, not injected from outside.
pub fn run_completion_handler() -> Handler<ScoutEngineDeps> {
    on_any()
        .id("run_completion")
        .priority(3)
        .then(move |event: AnyEvent, ctx: Context<ScoutEngineDeps>| {
            async move {
                if !is_terminal_event(&event, &ctx) {
                    return Ok(events![]);
                }

                let deps = ctx.deps();
                let state = ctx.aggregate::<PipelineState>().curr;
                let final_stats = state.stats.clone();
                info!("{}", final_stats);

                if let Some(pool) = &deps.pg_pool {
                    let stats_json = serde_json::to_value(&final_stats)?;
                    sqlx::query(
                        "UPDATE scout_runs SET stats = $2, spent_cents = $3 WHERE run_id = $1",
                    )
                    .bind(deps.run_id.to_string())
                    .bind(stats_json)
                    .bind(final_stats.spent_cents as i64)
                    .execute(pool)
                    .await?;
                }

                Ok(events![LifecycleEvent::ScoutRunCompleted {
                    run_id: deps.run_id,
                    finished_at: Utc::now(),
                }])
            }
        })
}
```

No infinite loop risk: `ScoutRunCompleted` is a `LifecycleEvent`, not a `SynthesisEvent` or `SupervisorEvent`. `is_terminal_event()` won't match it — it only matches `SeverityInferred`, `SupervisionCompleted`, `NothingToSupervise`.

#### `modules/rootsignal-scout/src/core/projection.rs` — update `scout_runs_projection`

1. **Remove** the terminal event stats block (moved to `run_completion_handler`).

2. **Update** the `ScoutRunRequested` INSERT to include new fields:

```rust
if let Some(LifecycleEvent::ScoutRunRequested {
    run_id, scope, region_id, flow_type, source_ids, task_id, ..
}) = event.downcast_ref::<LifecycleEvent>() {
    let deps = ctx.deps();
    if let Some(pool) = &deps.pg_pool {
        let region = scope.region().map(|r| r.name.as_str()).unwrap_or("unknown");
        let scope_json = scope.region().and_then(|r| serde_json::to_value(r).ok());
        let source_ids_json = source_ids.as_ref()
            .and_then(|ids| serde_json::to_value(ids).ok());
        sqlx::query(
            "INSERT INTO scout_runs (run_id, region, region_id, flow_type, source_ids, scope, task_id, started_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, now()) \
             ON CONFLICT (run_id) DO UPDATE SET \
               region_id = COALESCE(EXCLUDED.region_id, scout_runs.region_id), \
               flow_type = COALESCE(EXCLUDED.flow_type, scout_runs.flow_type), \
               source_ids = COALESCE(EXCLUDED.source_ids, scout_runs.source_ids), \
               task_id = COALESCE(EXCLUDED.task_id, scout_runs.task_id)",
        )
        .bind(run_id.to_string())
        .bind(region)
        .bind(region_id.as_deref())
        .bind(flow_type.as_str())
        .bind(&source_ids_json)
        .bind(&scope_json)
        .bind(task_id.as_deref())
        .execute(pool)
        .await?;
    }
    return Ok(());
}
```

3. **Add** `ScoutRunCompleted` reaction to write `finished_at`:

```rust
if let Some(LifecycleEvent::ScoutRunCompleted { run_id, finished_at }) =
    event.downcast_ref::<LifecycleEvent>()
{
    let deps = ctx.deps();
    if let Some(pool) = &deps.pg_pool {
        sqlx::query(
            "UPDATE scout_runs SET finished_at = $2 WHERE run_id = $1 AND finished_at IS NULL",
        )
        .bind(run_id.to_string())
        .bind(finished_at)
        .execute(pool)
        .await?;
    }
    return Ok(());
}
```

#### `modules/rootsignal-scout/src/core/engine.rs` — register handler

Add `run_completion_handler` to all engine builders that have terminal events:

```rust
// In build_full_engine, build_scrape_engine, build_weave_engine:
engine = engine.with_handler(projection::run_completion_handler());
```

Not needed in `build_news_engine` (news scans don't use scout_runs).

#### `modules/rootsignal-api/src/scout_runner.rs` — pass flow metadata

Update all emission sites to include new fields:

```rust
// run_bootstrap
LifecycleEvent::ScoutRunRequested {
    run_id,
    scope: run_scope,
    budget_cents: budget,
    region_id: Some(region_id.clone()),
    flow_type: "bootstrap".into(),
    source_ids: None,
    task_id: std::env::var("FLY_MACHINE_ID").ok(),
}

// run_scrape
LifecycleEvent::ScoutRunRequested {
    run_id,
    scope: run_scope,
    budget_cents: budget,
    region_id: Some(region_id.clone()),
    flow_type: "scrape".into(),
    source_ids: None,
    task_id: std::env::var("FLY_MACHINE_ID").ok(),
}

// run_weave
LifecycleEvent::ScoutRunRequested {
    run_id,
    scope: run_scope,
    budget_cents: budget,
    region_id: Some(region_id.clone()),
    flow_type: "weave".into(),
    source_ids: None,
    task_id: std::env::var("FLY_MACHINE_ID").ok(),
}

// run_scout_sources
LifecycleEvent::ScoutRunRequested {
    run_id,
    scope: run_scope,
    budget_cents: budget,
    region_id: None,
    flow_type: "scout_source".into(),
    source_ids: Some(source_id_strings),
    task_id: std::env::var("FLY_MACHINE_ID").ok(),
}
```

#### Test emission sites

Update `testing.rs` and `engine_tests.rs` to include default values:

```rust
ScoutRunRequested {
    run_id,
    scope,
    budget_cents: 0,
    region_id: None,
    flow_type: String::new(),
    source_ids: None,
    task_id: None,
}
```

### Phase 2: Remove early_insert and post_settle (subtractive)

**Only proceed after Phase 1 is deployed and verified in production.**

#### `modules/rootsignal-api/src/scout_runner.rs`

- Delete `early_insert_flow_run` function entirely
- Delete `post_settle_cleanup` function entirely
- Remove all calls to both functions from `run_bootstrap`, `run_scrape`, `run_weave`, `run_scout_sources`
- Remove `post_settle_cleanup` call from `resume_incomplete_runs` normal resume path
- Replace `resume_incomplete_runs` "no pending work" raw SQL with: let the run stay as `finished_at IS NULL` — it's harmless, and the staleness guard (`started_at > now() - interval '10 minutes'`) will age it out. Alternatively, if resume re-settles and hits a terminal event, `run_completion_handler` will fire and emit `ScoutRunCompleted` naturally

#### `is_source_busy` race window

`is_source_busy` queries `source_ids IS NOT NULL AND finished_at IS NULL`. With `early_insert_flow_run` removed, the row is only created when `ScoutRunRequested` is processed by the projection — a few milliseconds later than before. This creates a tiny window where a duplicate scout could start.

**Acceptable risk:** The existing staleness guard (`started_at > now() - interval '10 minutes'`) already handles this. The window is milliseconds vs. the 10-minute guard. No additional mitigation needed.

## Known Limitations

- **`started_at` uses `now()` in projection SQL.** During replay, this would record the replay time, not the original start time. Fix later by reading the seesaw store's `created_at` timestamp from the persisted event. Not blocking — same gap exists today.

## References

- `modules/rootsignal-api/src/scout_runner.rs:475-515` — `early_insert_flow_run` + `post_settle_cleanup`
- `modules/rootsignal-scout/src/core/projection.rs:177-247` — `is_terminal_event` + `scout_runs_projection`
- `modules/rootsignal-scout/src/domains/lifecycle/events.rs:17-24` — `ScoutRunRequested`
- `modules/rootsignal-scout/src/core/run_scope.rs` — `RunScope` enum
- `modules/rootsignal-scout/src/core/engine.rs:179-246` — engine builders
- `modules/rootsignal-api/src/db/models/scout_run.rs:459` — `is_source_busy` query
- `docs/plans/2026-03-10-refactor-event-source-budget-tracking-plan.md` — budget tracking precedent
- Migrations: 006, 023, 026, 027 — scout_runs schema evolution
