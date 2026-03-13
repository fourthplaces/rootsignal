# Seesaw 0.26.0 Migration Plan

## What Changed (0.25.0 → 0.26.0)

Three breaking changes on top of the Store split (EventLog + HandlerQueue):

### 1. Event trait + `#[event]` macro (required)
All event types used with seesaw must implement the `Event` trait via `#[event]` macro. This provides:
- **`durable_name()`** — stable per-variant name for storage (e.g. `"scrape:web_scrape_completed"`)
- **`event_prefix()`** — type-level prefix for codec/aggregator lookup (e.g. `"scrape"`)
- **`is_ephemeral()`** — whether event skips persistence

`on::<E>()`, `engine.emit()`, `Aggregator::new::<E, A, F>()`, `Events::push()`, `on_failure()` all now require `E: Event`.

### 2. Durable names replace type_name everywhere
- `EmittedEvent.event_type` → `EmittedEvent.durable_name` + `event_prefix` + `persistent`
- Aggregator registration: `event_type` → `event_prefix` with prefix-based matching
- EventCodec: prefix-based lookup instead of TypeId-only

### 3. Ephemeral events (two-tier persistence)
`#[event(ephemeral)]` marks coordination signals that route through handlers but:
- Skip aggregator apply
- Skip projections
- Get `persistent: false` in `NewEvent` and `PersistedEvent`
- Persist to operational store (Postgres) for causal chain, but skip permanent store (KurrentDB)

## Ephemeral Analysis

Ephemeral events skip aggregators. Any event with meaningful `Apply` impl MUST stay persistent.

**13 persistent event types** (all mutate PipelineState or curiosity aggregates):

| Event | Why persistent |
|---|---|
| WorldEvent | Domain fact. `apply_world`: signals_awaiting_review++ |
| SystemEvent | Editorial decision. `apply_system`: signals_review_completed++ |
| TelemetryEvent | Operational audit trail. No-op apply but essential for observability |
| ScrapeEvent | `apply_scrape`: done flags, stats, url mappings, links, queries |
| SignalEvent | `apply_signal`: stats, review counters, dedup verdicts |
| DiscoveryEvent | `apply_discovery`: stats, sources, expansion topics |
| SynthesisEvent | `apply_synthesis`: completion flags (similarity, responses, severity) |
| ExpansionEvent | `apply_expansion`: stats, social topics |
| EnrichmentEvent | `apply_enrichment`: enrichment_ready flag |
| LifecycleEvent | `apply_lifecycle`: run_scope, source_plan |
| PipelineEvent | `apply_pipeline`: handler_failures counter |
| CuriosityEvent | SignalLifecycle + ConcernLifecycle aggregate mutations |
| SchedulingEvent | Projected to scheduled_scrapes table |

**2 ephemeral event types** (no-op aggregator, no projection, coordination-only):

| Event | Why ephemeral |
|---|---|
| SituationWeavingEvent | `on_situation_weaving` is no-op. Not projected. Completion marker only. |
| SupervisorEvent | `on_supervisor` is no-op. Not projected. Completion marker only. |

## Pressure Test Findings

### Critical: `events.event_type` stores Rust TYPE names, not variant names

The `events.event_type` column currently stores Rust type names from `std::any::type_name`:
- Layer events: `"WorldEvent"`, `"SystemEvent"`, `"TelemetryEvent"`
- Domain events: `"ScrapeEvent"`, `"SignalEvent"`, `"DiscoveryEvent"`, etc.

After 0.26, seesaw stores `durable_name()` per variant: `"world:gathering_announced"`, `"scrape:web_scrape_completed"`.

The original plan assumed variant names were stored — they're not. Migration requires extracting the variant from `payload->>'type'` JSON field and combining with the prefix.

### Critical: Admin API functions match on old type names

`rootsignal-api/src/db/models/scout_run.rs:498-528`:
- `event_layer()` matches `"WorldEvent"` → `"world"`, etc.
- `event_domain_prefix()` matches `"WorldEvent"` → `"world"`, etc.

Both break completely after event_type format changes. Must be rewritten to use prefix extraction from the new `"prefix:variant"` format.

### High: `EventDomain::from_event_type()` needs new prefixes

`rootsignal-common/src/events.rs:461-478` already uses `split_once(':')` prefix matching for domain events, but routes unprefixed events (no colon) to `Self::Fact`. After migration, World/System/Telemetry events gain prefixes (`world:`, `system:`, `telemetry:`). Need to add these to the prefix match arm.

### Medium: `classify_event()` uses three different APIs

`rootsignal-scout/src/core/projection.rs:111-172` uses:
- `e.event_type()` — Eventlike trait (World, System, Telemetry)
- `e.event_type_str()` — domain event method (Discovery, Signal, etc.)
- `Event::World(e.clone()).to_payload()` — wrapper enum serialization
- `e.to_persist_payload()` — domain event method

All four APIs converge to `durable_name()` + `serde_json::to_value()` after migration.

### Medium: `scout_run_events` table needs backfill too

The `scout_run_events` projection table has its own `event_type` column that stores the same Rust type names. Needs the same backfill treatment.

## Migration Scope

### Phase 1: Add `#[event]` to all 15 event types

All event enums already have `#[serde(tag = "type", rename_all = "snake_case")]` — the macro reads these to derive durable names. Adding `seesaw_core` (or `seesaw_core_macros`) as a dependency to all three crates.

| Event Type | Crate | Annotation |
|---|---|---|
| WorldEvent | rootsignal-world | `#[event(prefix = "world")]` |
| SystemEvent | rootsignal-common | `#[event(prefix = "system")]` |
| TelemetryEvent | rootsignal-common | `#[event(prefix = "telemetry")]` |
| ScrapeEvent | rootsignal-scout | `#[event(prefix = "scrape")]` |
| SignalEvent | rootsignal-scout | `#[event(prefix = "signal")]` |
| DiscoveryEvent | rootsignal-scout | `#[event(prefix = "discovery")]` |
| LifecycleEvent | rootsignal-scout | `#[event(prefix = "lifecycle")]` |
| EnrichmentEvent | rootsignal-scout | `#[event(prefix = "enrichment")]` |
| ExpansionEvent | rootsignal-scout | `#[event(prefix = "expansion")]` |
| SchedulingEvent | rootsignal-scout | `#[event(prefix = "scheduling")]` |
| SituationWeavingEvent | rootsignal-scout | `#[event(prefix = "situation_weaving", ephemeral)]` |
| SupervisorEvent | rootsignal-scout | `#[event(prefix = "supervisor", ephemeral)]` |
| PipelineEvent | rootsignal-scout | `#[event(prefix = "pipeline")]` |
| SynthesisEvent | rootsignal-scout | `#[event(prefix = "synthesis")]` |
| CuriosityEvent | rootsignal-scout | `#[event(prefix = "curiosity")]` |

### Phase 2: Remove `Eventlike` trait + `event_type_str()` + update all routing

**Delete:**
- `Eventlike` trait from `rootsignal-world/src/eventlike.rs`
- `impl Eventlike` blocks from WorldEvent, SystemEvent, TelemetryEvent
- `fn event_type_str()` methods from all 10 domain event files:
  - `rootsignal-scout/src/domains/signals/events.rs`
  - `rootsignal-scout/src/domains/scheduling/events.rs`
  - `rootsignal-scout/src/domains/enrichment/events.rs`
  - `rootsignal-scout/src/domains/discovery/events.rs`
  - `rootsignal-scout/src/domains/expansion/events.rs`
  - `rootsignal-scout/src/domains/lifecycle/events.rs`
  - `rootsignal-scout/src/core/pipeline_events.rs`
  - `rootsignal-scout/src/domains/supervisor/events.rs`
  - `rootsignal-scout/src/domains/situation_weaving/events.rs`
  - `rootsignal-scout/src/domains/signals/activities/engine_tests.rs` (if it defines one)

**Update `classify_event()`** (`projection.rs:111-172`):
Replace all four APIs with uniform `durable_name()` + `serde_json::to_value()`:
```rust
fn classify_event(event: &AnyEvent) -> (EventDomain, Option<String>, Option<serde_json::Value>) {
    // Try each event type, use durable_name() for event_type and serde for payload
    if let Some(e) = event.downcast_ref::<WorldEvent>() {
        (EventDomain::Fact, Some(e.durable_name().to_string()), Some(serde_json::to_value(e).unwrap()))
    } else if let Some(e) = event.downcast_ref::<SystemEvent>() {
        // ... same pattern for all 15 types
    }
}
```

**Update `EventDomain::from_event_type()`** (`rootsignal-common/src/events.rs:461-478`):
Add `world`, `system`, `telemetry` to the prefix match:
```rust
Some((prefix, _)) => match prefix {
    "world" | "system" | "telemetry" => Some(Self::Fact),
    "discovery" => Some(Self::Discovery),
    "scrape" => Some(Self::Scrape),
    // ... existing arms
},
None => Some(Self::Fact),  // legacy unprefixed events
```

**Update admin API functions** (`rootsignal-api/src/db/models/scout_run.rs:498-528`):
```rust
fn event_layer(event_type: &str) -> &'static str {
    match event_type.split_once(':').map(|(p, _)| p).unwrap_or(event_type) {
        "world" => "world",
        "system" | "enrichment" | "signal" | "synthesis" | "discovery" => "system",
        _ => "telemetry",
    }
}

fn event_domain_prefix(event_type: &str) -> &'static str {
    event_type.split_once(':').map(|(p, _)| p).unwrap_or("unknown")
}
```

**Update `rootsignal-common/src/events.rs`:**
- Remove `Eventlike` re-export
- Remove `Event` wrapper enum's `.event_type()` and `.to_payload()` methods that delegate to Eventlike
- Keep `EventDomain` (updated as above)

### Phase 3: Data migration — migration binary + SQL schema

#### SQL schema migration (`024_seesaw_026_schema.sql`)

```sql
-- New tables for checkpoint-based processing
CREATE TABLE seesaw_checkpoints (
    correlation_id UUID PRIMARY KEY,
    position BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE seesaw_handler_journal (
    handler_id TEXT NOT NULL,
    event_id UUID NOT NULL,
    seq INT NOT NULL,
    value JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (handler_id, event_id, seq)
);

-- Add persistent flag to events
ALTER TABLE events ADD COLUMN persistent BOOLEAN NOT NULL DEFAULT true;

-- Drop batch columns from effect executions
ALTER TABLE seesaw_effect_executions DROP COLUMN IF EXISTS batch_id;
ALTER TABLE seesaw_effect_executions DROP COLUMN IF EXISTS batch_index;
ALTER TABLE seesaw_effect_executions DROP COLUMN IF EXISTS batch_size;
```

#### SQL archive (`025_seesaw_026_archive.sql`)

```sql
-- Archive old event queue (verify nothing reads it first)
ALTER TABLE seesaw_events RENAME TO seesaw_events_archive;
```

#### Migration binary (`bin/migrate_event_types.rs`)

Batched, idempotent backfill for `events.event_type` and `scout_run_events.event_type`.

**Current state:** `events.event_type` stores Rust type names (`"WorldEvent"`, `"ScrapeEvent"`, etc.). The variant name lives in `payload->>'type'` (from `#[serde(tag = "type")]`).

**Target state:** `events.event_type` stores durable names (`"world:gathering_announced"`, `"scrape:web_scrape_completed"`).

```rust
// Pseudocode for the migration binary
const BATCH_SIZE: i64 = 10_000;
const TYPE_TO_PREFIX: &[(&str, &str)] = &[
    ("WorldEvent", "world"),
    ("SystemEvent", "system"),
    ("TelemetryEvent", "telemetry"),
    ("ScrapeEvent", "scrape"),
    ("SignalEvent", "signal"),
    ("DiscoveryEvent", "discovery"),
    ("LifecycleEvent", "lifecycle"),
    ("EnrichmentEvent", "enrichment"),
    ("ExpansionEvent", "expansion"),
    ("SchedulingEvent", "scheduling"),
    ("SituationWeavingEvent", "situation_weaving"),
    ("SupervisorEvent", "supervisor"),
    ("PipelineEvent", "pipeline"),
    ("SynthesisEvent", "synthesis"),
    ("CuriosityEvent", "curiosity"),
];

// For each batch:
// UPDATE events
// SET event_type = CASE event_type
//   WHEN 'WorldEvent'     THEN 'world:' || (payload->>'type')
//   WHEN 'SystemEvent'    THEN 'system:' || (payload->>'type')
//   WHEN 'TelemetryEvent' THEN 'telemetry:' || (payload->>'type')
//   -- ... all 15 types
// END
// WHERE event_type NOT LIKE '%:%'   -- idempotency guard
//   AND seq BETWEEN $batch_start AND $batch_end;
//
// Same for scout_run_events (using its own id/seq column).
```

Properties:
- **Idempotent:** `WHERE event_type NOT LIKE '%:%'` skips already-migrated rows
- **Batched:** 10K rows at a time to avoid long locks
- **Progress logging:** `"Migrated 50000/1200000 events (4.2%)"`
- **Verification:** Final count query `WHERE event_type NOT LIKE '%:%'` should return 0
- **Safe to re-run:** Interrupted migration picks up where it left off

### Phase 4: PostgresStore → EventLog + HandlerQueue

#### EventLog impl (5 methods — mostly renames)
| Method | Current | Change |
|---|---|---|
| `append` | `append_event()` | Rename. Add `persistent` column write. |
| `load_from` | `load_global_from()` | Rename. Add `persistent` to SELECT. |
| `load_stream` | `load_stream()` | Param rename: `after_position` → `after_version`. |
| `load_snapshot` | `load_snapshot()` | Identical. |
| `save_snapshot` | `save_snapshot()` | Identical. |

#### HandlerQueue impl (14 methods — enqueue is the big rewrite)
| Method | Change |
|---|---|
| `enqueue` | **Full rewrite** — insert intents + upsert checkpoint. No event queue to ack. |
| `checkpoint` | **New** — query `seesaw_checkpoints`. |
| `dequeue` | Drop batch columns, rename. |
| `earliest_pending_at` | Rename. |
| `resolve` | Simplify — Complete just marks done + clears journal. No events_to_publish. |
| `reclaim_stale` | Handler reclaim only (no event queue). |
| `load_journal` | **New** — query `seesaw_handler_journal`. |
| `append_journal` | **New** — insert into journal table. |
| `clear_journal` | **New** — delete from journal table. |
| `cancel` | Rename. |
| `is_cancelled` | Rename. |
| `status` | Simplify — handler queue only. `QueueStatus` loses `pending_events`. |
| `set_descriptions` | Rename. |
| `get_descriptions` | Rename. |

### Phase 5: Update engine construction + imports

Update `build_engine()` in `core/engine.rs`:
- `Option<Arc<dyn Store>>` → `Option<Arc<PostgresStore>>`
- `with_store` takes `S: EventLog + HandlerQueue + 'static`

Update `make_store()` in `workflows/mod.rs` and `scout_runner.rs`.

### Phase 6: Update `has_pending_work()` and `resume_incomplete_runs()`

`has_pending_work()`:
- Old: checks `seesaw_events` status + `seesaw_effect_executions` status
- New: check `checkpoint < max(events.seq)` + effect executions pending/running

`resume_incomplete_runs()`:
- Old: reclaim_stale resets both event and handler queues
- New: only reclaim handlers + reset checkpoint if interrupted mid-processing

### Phase 7: Tests

- Boundary tests use MemoryStore (already implements new traits), but event types now need `#[event]`
- Update test event construction in boundary_tests.rs, completion_tests.rs, engine_tests.rs
- Verify projection's classify_event works with durable_name format

## Execution Order

```
1. Bump seesaw_core = "0.26.0", add seesaw_core_macros dep to rootsignal-world + rootsignal-common
2. Add #[event] to all 15 event types
3. Remove Eventlike trait + event_type_str() methods, update ALL call sites:
   - classify_event() in projection.rs
   - EventDomain::from_event_type() in events.rs
   - event_layer() + event_domain_prefix() in scout_run.rs
   - rootsignal-common Event wrapper enum
4. DB schema migration (new tables, persistent column, drop batch columns)
5. Migration binary: backfill events.event_type + scout_run_events.event_type
6. Archive seesaw_events table
7. Rewrite PostgresStore (EventLog + HandlerQueue impls)
8. Update engine construction + imports
9. Update has_pending_work + resume_incomplete_runs
10. Test fixes
11. cargo check && cargo test
```

Steps 1-3 can be done BEFORE the Store split — they're pure type-system changes.
Step 4-6 are the data migration.
Steps 7-9 are the Store split.
Step 10 fixes any fallout.

**Deploy order:** Steps 1-3 deploy first (code reads both formats during transition). Then step 5 (migration binary) runs against production. Then steps 4+6-9 deploy together.

## Key Files

| File | Change |
|---|---|
| `Cargo.toml` | Bump `seesaw_core = "0.26.0"` |
| `rootsignal-world/Cargo.toml` | Add `seesaw_core` dep |
| `rootsignal-common/Cargo.toml` | Add `seesaw_core` dep (if not already) |
| `rootsignal-world/src/events.rs` | Add `#[event(prefix = "world")]` |
| `rootsignal-world/src/eventlike.rs` | Delete file |
| `rootsignal-common/src/system_events.rs` | Add `#[event(prefix = "system")]` |
| `rootsignal-common/src/telemetry_events.rs` | Add `#[event(prefix = "telemetry")]` |
| `rootsignal-common/src/events.rs` | Update `EventDomain::from_event_type()`, remove Eventlike re-export |
| `rootsignal-scout/src/domains/*/events.rs` | Add `#[event(prefix = "...")]` to all domain events, delete `event_type_str()` |
| `rootsignal-scout/src/core/projection.rs` | Rewrite `classify_event()` to use `durable_name()` + `serde_json::to_value()` |
| `rootsignal-scout/src/core/postgres_store.rs` | Full rewrite: `impl Store` → `impl EventLog` + `impl HandlerQueue` |
| `rootsignal-scout/src/core/engine.rs` | Update `build_engine` store types |
| `rootsignal-scout/src/workflows/mod.rs` | Update `make_store` return type |
| `rootsignal-api/src/scout_runner.rs` | Update store import, `has_pending_work`, `resume_incomplete_runs` |
| `rootsignal-api/src/db/models/scout_run.rs` | Rewrite `event_layer()` + `event_domain_prefix()` for prefix-based format |
| `rootsignal-scout/src/testing.rs` | Update test helpers if needed |
| `bin/migrate_event_types.rs` | **New** — batched, idempotent event_type backfill |
