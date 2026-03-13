---
date: 2026-03-11
topic: unified-workflows
---

# Unified Workflows Page

## What We're Building

Replace the Scout page with a generalized Workflows page that manages all run types (scout, coalesce, weave) in one place. Runs are the single execution primitive. Schedules are recurring triggers that create runs. The pipeline chain (scout → coalesce → weave) is modeled via parent→child run relationships, not a separate orchestration concept.

## Why This Approach

The current Scout page already manages multiple flow types (bootstrap, scrape, weave, scout_source) and has a separate "Scheduled" tab. Coalescing needs a UI too, which would mean yet another page. Instead of fragmenting, unify everything under Workflows — the `scout_runs` table is already most of the way there with `flow_type` and `scope` fields.

## Key Decisions

- **Runs are the single execution primitive**: Every execution — scout, coalesce, weave — is a row in the same table with `flow_type`, `scope` (JSONB), `parent_run_id`, `run_at`, `started_at`, `finished_at`.
- **`parent_run_id` models the pipeline chain**: Scout run completes → creates coalesce run (parent_run_id = scout's id) → creates weave run (parent_run_id = coalesce's id). Chain orchestration is emergent from parent→child, not a separate concept.
- **Schedules are recurring triggers**: A schedule has `flow_type`, `scope`, `cadence`, `enabled`. It creates runs on its cadence. Schedules are for top-level recurring work (e.g. "scrape Minneapolis every 6h"). One-shot future work is just a run with a future `run_at`.
- **Status is derived**: `scheduled` (future run_at, no started_at), `running` (started_at, no finished_at), `completed`, `failed`. No explicit status column needed.
- **App-level concern, not causal library**: `parent_run_id` is a simple FK on the runs table. The causal library tracks event-level causation; run-level causation is coarser and doesn't need library support.
- **Scope is polymorphic via JSONB**: Scout scope has region + sources. Coalesce scope has region + seed signal or group. Weave scope has region + groups. Each flow type interprets its own scope.

## Data Model

### runs (rename from scout_runs)

| Column | Type | Notes |
|--------|------|-------|
| run_id | TEXT PK | UUID |
| flow_type | TEXT | scout, coalesce, weave, bootstrap |
| scope | JSONB | Flow-specific parameters |
| parent_run_id | TEXT FK nullable | Chain to parent run |
| schedule_id | TEXT FK nullable | Which schedule spawned this |
| run_at | TIMESTAMPTZ | When it should start |
| started_at | TIMESTAMPTZ nullable | Null = not yet started |
| finished_at | TIMESTAMPTZ nullable | Null = still running |
| stats | JSONB | Flow-specific outcome stats |
| region_id | TEXT nullable | Denormalized for filtering |

### schedules

| Column | Type | Notes |
|--------|------|-------|
| schedule_id | TEXT PK | UUID |
| flow_type | TEXT | What kind of run to create |
| scope | JSONB | Parameters for each spawned run |
| cadence | TEXT | "6h", "daily", etc. |
| enabled | BOOLEAN | Toggle without deleting |
| last_run_id | TEXT FK nullable | Most recent spawned run |
| next_run_at | TIMESTAMPTZ nullable | Computed from cadence |

## UI Structure

### Workflows Page — Two Tabs

**Runs tab:**
- Unified table of all runs across all flow types
- Columns: Status (derived), Flow Type, Region, Run At, Duration, Parent Run (link)
- Filterable by flow type, region, status
- Click row → drill into run detail (events, outputs, child runs)
- For coalesce runs: show groups produced, member signals
- For scout runs: show signals produced, stats
- For weave runs: show situations produced

**Schedules tab:**
- List of recurring templates
- Columns: Flow Type, Scope (human-readable), Cadence, Enabled, Last Run, Next Run
- Toggle enabled/disabled
- Create new schedule
- Click → see history of runs spawned by this schedule

## Pressure Test Findings

### Critical Issues

**1. Weave/coalesce runs are invisible today.**
Only `ScoutRunRequested` creates a `scout_runs` row via the projection. `GenerateSituationsRequested` and `CoalesceRequested` bypass the table entirely. The projection must handle all three entry events before unification makes sense. This is independently valuable — fix first.

**2. "Failed" status has no signal.**
Status derived from timestamps can't distinguish "running" from "crashed." If a run panics, `finished_at` stays NULL forever. The 30-minute staleness guard in `resume_incomplete_runs` is a recovery mechanism, not a status signal. Add an `error TEXT nullable` column — when non-null, status = failed.

**3. Chain orchestration doesn't exist yet.**
No `parent_run_id`, no child-spawning logic. Decision: ScoutRunner spawns children imperatively after `settle()` returns successfully. Simpler than event-driven (avoids correlation_id conflicts). Children only created on successful parent completion, so no orphan problem.

**4. `is_region_busy` would self-block the chain.**
Currently any active run for a region blocks all new runs. If scout completes and spawns coalesce, the region is immediately "busy." Fix: make busy check flow-type-scoped. A running scout blocks another scout for that region but not a coalesce.

**5. No deferred execution mechanism.**
Runs with future `run_at` need a polling loop that queries `WHERE run_at <= now() AND started_at IS NULL` and spawns engines. This replaces the existing `scheduled_scrapes` loop.

### Data Model Gaps

- **`StatsJson` is scout-specific.** Fields like `urls_scraped`, `signals_extracted` don't apply to coalesce/weave. Stats must be polymorphic JSONB, with Rust deserialization handling flow-type-specific shapes.
- **`source_ids` denormalization.** Keep `source_ids` as a dedicated column alongside `scope` for the `is_source_busy` containment query. Moving it into nested JSONB requires different index strategy.
- **`scheduled_scrapes` table.** Migrate one-shot scheduled scrapes into the new model (runs with future `run_at`), then delete the table and its polling loop.
- **`news_scan` doesn't fit.** No region, no scope, no flow_type. Keep as special case outside the workflows model for now.

### Migration Surface

26+ files hardcode `scout_runs` by name: projection.rs, scout_run.rs (~15 queries), scout_runner.rs, schema.rs, pg_projector.rs, engine.rs, state.rs, budget.rs, 7 migration files. Strategy: `ALTER TABLE scout_runs RENAME TO runs` + find-and-replace across codebase. Own PR.

### UI Gaps

- Run detail page needs completely different rendering per flow type (one component with flow_type switch)
- No cancel mechanism for scheduled-but-not-started runs (delete the row or mark cancelled)
- No retry mechanism (admin triggers same scope manually; new run, no lineage connection)
- Pagination needed as runs accumulate (cursor-based on `run_at`)

## Resolved Questions

- **Migration path**: ALTER TABLE RENAME + find-and-replace. Simpler than dual-table.
- **Cadence format**: Simple interval ("6h", "12h", "24h", "7d") with Rust parser. Cron is overkill.
- **Run detail**: One component with flow_type switch, different sections per type.
- **Chain spawning**: Imperative after settle(), not event-driven. Children only on success.
- **Busy checks**: Flow-type-scoped. Scout blocks scout, not coalesce.
- **`source_ids`**: Keep as denormalized column alongside scope JSONB.
- **`news_scan`**: Stays outside the model for now.
- **Scheduled-but-not-started cancel**: Delete the row (no engine to cancel).
- **Future-dated manual runs**: Not in initial UI. Manual runs start immediately.

## Updated Data Model

### runs (rename from scout_runs)

| Column | Type | Notes |
|--------|------|-------|
| run_id | TEXT PK | UUID |
| flow_type | TEXT | scout, coalesce, weave, bootstrap |
| scope | JSONB | Flow-specific parameters |
| parent_run_id | TEXT FK nullable | Chain to parent run |
| schedule_id | TEXT FK nullable | Which schedule spawned this |
| run_at | TIMESTAMPTZ | When it should start (backfill = started_at for existing rows) |
| started_at | TIMESTAMPTZ nullable | Null = not yet started |
| finished_at | TIMESTAMPTZ nullable | Null = still running |
| error | TEXT nullable | Non-null = failed, contains reason |
| stats | JSONB | Flow-specific outcome stats |
| region_id | TEXT nullable | Denormalized for filtering |
| source_ids | JSONB nullable | Denormalized for is_source_busy queries |

### schedules

| Column | Type | Notes |
|--------|------|-------|
| schedule_id | TEXT PK | UUID |
| flow_type | TEXT | What kind of run to create |
| scope | JSONB | Parameters for each spawned run |
| cadence | TEXT | Interval format: "6h", "12h", "24h", "7d" |
| enabled | BOOLEAN | Toggle without deleting |
| last_run_id | TEXT FK nullable | Most recent spawned run |
| next_run_at | TIMESTAMPTZ nullable | Computed from cadence + last run |
| created_at | TIMESTAMPTZ | For audit |

## Build Order

1. **Fix projection gap** — make weave/coalesce visible in runs table (independently valuable)
2. **Table rename** — `scout_runs` → `runs`, find-and-replace across 26+ files (own PR)
3. **Add columns** — `parent_run_id`, `schedule_id`, `run_at`, `error`; backfill `run_at` from `started_at`
4. **Create schedules table** — migrate `scheduled_scrapes` data, replace polling loop
5. **Flow-type-scoped busy checks** — unblock chain orchestration
6. **Chain orchestration** — ScoutRunner spawns children after settle()
7. **Workflows UI** — unified runs table, schedules tab, flow-type-aware detail pages

## Next Steps

→ `/workflows:plan` for implementation details per build step
