---
title: "refactor: Replace ScoutTask with Region model and decoupled flows"
type: refactor
date: 2026-03-04
---

# Replace ScoutTask with Region Model and Decoupled Flows

## Overview

Replace the ephemeral, one-shot `ScoutTask` with a persistent **Region** model and decouple the monolithic scout pipeline into independent, composable flows. Each flow has a clear input and output, can be triggered independently, and produces its own `Run` in Postgres.

### Problems with the current model

1. **ScoutTask is one-shot** — if you want to watch Portland, you keep creating new tasks. There's no persistent "area to watch."
2. **Source-specific runs have no home** — "scrape this Instagram account" must be shoehorned through a geographic task.
3. **Situation weaving is bolted on as a phase** — it's a fundamentally different kind of work (cross-signal synthesis vs. scraping) but shares a pipeline and task lifecycle.
4. **Two stores for one concept** — ScoutTask lives in Neo4j, ScoutRun lives in Postgres, with phase_status on the task tracking run progress (blurring the boundary).

### Design decisions

- **Regions are persistent, geo-scoped, and can nest** via `CONTAINS` edges. A parent region (e.g. "US") is just a larger circle — the hierarchy makes containment explicit for scheduling and UI, but the signal query is always center + radius.
- **Sources are independent** — regions point to sources via `WATCHES` edges, but sources can exist without a region and can belong to multiple regions.
- **Flows are decoupled** — Bootstrap, Scrape, Weave, and Scout Source are independent operations, each producing a Run.
- **ScoutTask goes away entirely** — replaced by Region + direct source runs.

## Region Model (Neo4j)

```
(:Region {
  id: UUID,
  name: String,            // "Portland", "Pacific Northwest", "United States"
  center_lat: f64,
  center_lng: f64,
  radius_km: f64,
  geo_terms: [String],     // search terms for bootstrap
  is_leaf: Boolean,        // leaf regions have sources; parent regions weave only
  created_at: DateTime,
})
```

### Edges

```
(parent:Region)-[:CONTAINS]->(child:Region)
(region:Region)-[:WATCHES]->(source:Source)
```

### Rust type

```rust
pub struct Region {
    pub id: Uuid,
    pub name: String,
    pub center_lat: f64,
    pub center_lng: f64,
    pub radius_km: f64,
    pub geo_terms: Vec<String>,
    pub is_leaf: bool,
    pub created_at: DateTime<Utc>,
}

impl From<&Region> for ScoutScope {
    fn from(region: &Region) -> Self {
        ScoutScope {
            center_lat: region.center_lat,
            center_lng: region.center_lng,
            radius_km: region.radius_km,
            name: region.name.clone(),
        }
    }
}
```

## Decoupled Flows

| Flow | Input | What it does | Output |
|---|---|---|---|
| **Bootstrap** | Region | Discover sources for the region (web search, news, social) | New `Source` nodes + `WATCHES` edges |
| **Scrape** | Region | Auto-bootstraps if no sources, then scrapes all watched sources, extracts + classifies signals inline | Signals (with category, confidence, tone, severity) |
| **Weave** | Region (any level) | Cross-signal synthesis: concern↔response linking, actor dedup, situation building, gathering gravity | Situations + relationship edges |
| **Scout Source** | Source(s) | Scrape specific source(s), extract + classify signals | Signals |

### Flow enum

```rust
pub enum FlowType {
    Bootstrap,
    Scrape,
    Weave,
    ScoutSource,
}
```

### Enrichment split

**Inline during Scrape** (context is hot, per-signal):
- Signal extraction
- Category / confidence / tone / severity classification
- Per-signal enrichment

**Moved to Weave** (cross-signal, needs the full picture):
- Concern ↔ response linking
- Actor deduplication / merging
- Situation building + assignment
- Gathering ↔ concern gravity
- Investigation (evidence collection)
- Response mapping

Rule of thumb: if it needs one source's context, it's Scrape. If it needs to see across signals/sources, it's Weave.

### Weave at any altitude

Weave takes a region and queries signals within its geo bounds. The radius determines the altitude:

- **Portland (r=20km)** — local concerns, who's responding, where are people gathering
- **Pacific NW (r=500km)** — regional patterns, cross-city themes
- **US (r=2500km)** — nationwide patterns that no city-level weave would see

No special logic needed — bigger radius = more signals in scope = higher-altitude insight.

## Run Table Changes

### Migration: extend `scout_runs`

```sql
-- Replace task_id with region_id and add flow metadata
ALTER TABLE scout_runs ADD COLUMN IF NOT EXISTS region_id TEXT;
ALTER TABLE scout_runs ADD COLUMN IF NOT EXISTS flow_type TEXT;
ALTER TABLE scout_runs ADD COLUMN IF NOT EXISTS source_ids JSONB;

-- task_id kept temporarily for backward compat, dropped in a later migration
```

### ScoutRunner changes

Replace:
- `run_scout(task_id, scope)` → `run_scrape(region_id)`
- `run_phase(phase, task_id, scope)` → `run_flow(flow_type, region_id)` or `run_scout_source(source_ids)`
- `cancel(task_id)` → `cancel(run_id)` (already works by run_id internally)

Each method:
1. Loads the Region (or Sources) from Neo4j
2. Derives a `ScoutScope` from the Region
3. Builds the appropriate engine variant
4. Creates a Run in Postgres with `flow_type` and `region_id`/`source_ids`

## Engine Changes

Current engine variants map cleanly to flows:

| Current | New flow | Engine |
|---|---|---|
| `build_scrape_engine` (bootstrap + scrape + enrichment) | **Bootstrap** or **Scrape** | Reuse, minus cross-signal enrichment handlers |
| `build_full_engine` (scrape + synthesis + situation_weaver + supervisor) | **Weave** | New: only cross-signal handlers (synthesis domain becomes weave domain) |
| N/A | **Scout Source** | New: targeted scrape engine that takes source_ids instead of region scope |

The phase FSM on ScoutTask (`phase_status`) goes away — each flow is its own engine, no gating needed.

## GraphQL Mutations

Replace:
```graphql
# Old
runScout(taskId: String!): ScoutResult!
runScoutPhase(phase: ScoutPhase!, taskId: String!): ScoutResult!

# New
runBootstrap(regionId: String!): ScoutResult!
runScrape(regionId: String!): ScoutResult!
runWeave(regionId: String!): ScoutResult!
runScoutSource(sourceIds: [String!]!): ScoutResult!
cancelRun(runId: String!): ScoutResult!
```

Region CRUD:
```graphql
createRegion(input: CreateRegionInput!): Region!
updateRegion(id: String!, input: UpdateRegionInput!): Region!
deleteRegion(id: String!): Boolean!
addRegionSource(regionId: String!, sourceId: String!): Boolean!
removeRegionSource(regionId: String!, sourceId: String!): Boolean!
nestRegion(parentId: String!, childId: String!): Boolean!
```

## Migration Path from ScoutTask

### Step 1: Add Region model
- Add `Region` struct to `rootsignal-common/src/types.rs`
- Add Neo4j operations to `writer.rs`: `upsert_region`, `get_region`, `list_regions`, `add_region_source`, `nest_region`
- Add `FlowType` enum

### Step 2: Extend scout_runs
- Migration: add `region_id`, `flow_type`, `source_ids` columns
- Update `ScoutRunRow` and queries

### Step 3: Build flow-specific engines
- Extract weave-only engine (cross-signal handlers only)
- Extract scout-source engine (targeted scrape, no region)
- Adjust scrape engine to auto-bootstrap when region has no sources

### Step 4: Update ScoutRunner
- New methods: `run_bootstrap`, `run_scrape`, `run_weave`, `run_scout_source`
- Each loads Region/Sources, builds appropriate engine, creates Run
- Keep old `run_scout`/`run_phase` temporarily as wrappers

### Step 5: Update GraphQL + Admin UI
- New mutations for flows and region CRUD
- Admin UI: Region management page (create, nest, assign sources)
- Admin UI: Flow trigger buttons per region (Bootstrap, Scrape, Weave)
- Admin UI: Scout Source button on source detail page

### Step 6: Migrate existing ScoutTasks → Regions
- One-time script: for each ScoutTask, create a Region with same geo + context
- Link existing sources within radius via `WATCHES` edges
- Update existing scout_runs to reference region_id

### Step 7: Remove ScoutTask
- Delete `ScoutTask`, `ScoutTaskSource`, `ScoutTaskStatus` from types
- Remove all ScoutTask Neo4j operations from writer.rs
- Remove old mutations/queries from GraphQL
- Remove ScoutTask admin pages
- Drop `:ScoutTask` nodes from Neo4j

### Step 8: Supervisor adapts to Regions
- `SupervisorState.is_scout_running()` queries Regions instead of ScoutTasks
- Scheduling logic uses Regions: "run scrape for leaf regions every N hours, weave for parent regions every M hours"
- Beacon detection creates Regions (or adds sources to existing ones) instead of ScoutTasks

## Concurrency: Resource Locking

Overlap prevention happens at the **resource** level, not the run level.

### Principle

- **Sources** are the unit of overlap for scraping. Two scouts can run in the same region concurrently — they just can't scrape the same source simultaneously.
- **Regions** are the unit of overlap for weaving. Only one weaver per region at a time, since weaving synthesizes across all signals in the region.
- **Time is the lock.** A timestamp (`locked_at`) on the resource acts as both the lock and the staleness guard. No explicit unlock needed — if a run crashes, the lock expires after a threshold.

### Implementation: `resource_locks` table

```sql
CREATE TABLE resource_locks (
    resource_type TEXT NOT NULL,  -- 'source' or 'region'
    resource_id TEXT NOT NULL,
    locked_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    run_id TEXT,                  -- which run claimed it
    PRIMARY KEY (resource_type, resource_id)
);
```

### How flows use it

| Flow | Lock type | Behavior |
|---|---|---|
| **Scrape** / **Scout Source** | Per-source | Scheduler claims free sources atomically. Busy sources are skipped. If all sources in a region are busy, the run completes with zero work (emits `HandlerSkipped`). |
| **Bootstrap** | None | Discovering sources doesn't conflict with anything. |
| **Weave** | Per-region | Mutation checks `resource_locks` for the region. If locked, refuse to start. |

### Claiming sources (atomic)

```sql
INSERT INTO resource_locks (resource_type, resource_id, run_id)
SELECT 'source', unnest($1::text[]), $2
WHERE NOT EXISTS (
    SELECT 1 FROM resource_locks
    WHERE resource_type = 'source' AND resource_id = ANY($1::text[])
      AND locked_at >= now() - interval '30 minutes'
)
ON CONFLICT (resource_type, resource_id) DO UPDATE
SET locked_at = now(), run_id = $2
WHERE resource_locks.locked_at < now() - interval '30 minutes';
```

### Releasing (on run completion or via staleness)

```sql
DELETE FROM resource_locks WHERE run_id = $1;
```

Stale locks (older than threshold) are treated as expired and can be overwritten by new claims.

## What doesn't change

- `ScoutScope` — still the value object passed to engine deps, now derived from Region instead of ScoutTask
- Event store — runs still produce events in Postgres with `run_id` and `correlation_id`
- Source model — unchanged, just gains `WATCHES` edges from Regions
- Signal model — unchanged
- News scanner — still produces BeaconDetected, but now creates/updates Regions instead of ScoutTasks

## Acceptance Criteria

- [ ] Region Neo4j node with CONTAINS and WATCHES edges
- [ ] Four independent flows: Bootstrap, Scrape, Weave, Scout Source
- [ ] Each flow produces a Run with flow_type in Postgres
- [ ] Scrape auto-bootstraps when region has no sources
- [ ] Weave works at any region level (leaf or parent)
- [ ] ScoutTask fully removed (types, Neo4j ops, GraphQL, admin UI)
- [ ] Existing data migrated: ScoutTasks → Regions, scout_runs updated
- [ ] Supervisor uses Regions for scheduling and running checks
- [ ] Cross-signal enrichment (concern linking, actor dedup, situations) moved from scrape to weave
