---
title: Demand-Driven Scout Swarm
type: feat
date: 2026-02-20
deepened: 2026-02-20
---

# Demand-Driven Scout Swarm: Killing Regions

## Enhancement Summary

**Deepened on:** 2026-02-20
**Agents used:** architecture-strategist, performance-oracle, security-sentinel,
data-migration-expert, data-integrity-guardian, agent-native-reviewer,
code-simplicity-reviewer, pattern-recognition-specialist, best-practices-researcher,
framework-docs-researcher

### Critical Findings

1. **Canonical key migration is already shipped** — `migrate.rs` line 624
   already strips city slug prefixes. Phase 1b should mark this as done.
2. **Production bug: colon over-match** — the existing migration strips
   any `key CONTAINS ':'` that doesn't start with `http`. This corrupts
   web query sources containing colons (e.g., "housing crisis: rent prices").
   Must fix immediately.
3. **Showstopper: `cleanup_off_geo_signals`** — `migrate.rs` line 1258
   hardcodes a Twin Cities bounding box and DELETES all signals outside
   it on every deploy. Must be removed before any global signals exist.
4. **Plan is over-engineered** for current scale (hundreds of signals,
   single worker). Simplify to 2 phases + defer list.
5. **No API surface for the task queue** — 10 of 16 agent-native
   capabilities have no API. Task queue must be observable from day one.

### Key Simplification

The original plan had 5 phases. At current scale (hundreds of signals,
one scout worker), concurrency infrastructure, Driver A, geographic
cadence tiers, and the heat scrubber are premature. **Build 2 phases,
defer the rest.** Layer sophistication based on real operational data.

## Overview

Eliminate predefined regions. Scout coverage becomes emergent, driven
by two inputs: (A) user search queries and (B) global news/tension
scanning. Signals act as beacons — geographic clusters tell scout
where to go deeper. The concept of "region" dissolves into ephemeral
scout tasks on a priority queue.

**Brainstorm:** `docs/brainstorms/2026-02-20-demand-driven-scout-swarm-brainstorm.md`

## Problem Statement

The current system requires manually creating `RegionNode` entries to
tell scout where to look. This doesn't scale ("add Detroit" is a human
decision), doesn't reflect actual demand, and structurally limits Root
Signal to a set of hand-picked cities.

Meanwhile, the reading side (search-app) already works at any zoom
level via bounding-box queries. The map works at world scale — it's
just empty where scout hasn't run.

## Proposed Solution

Two scout drivers replace the region registry:

- **Driver A (User Query Demand):** User searches get geocoded and
  ranked. Popular search areas get scout attention.
- **Driver B (Global Tension Scanning):** Scout reads the news (RSS,
  wire services, Reddit) with no geographic scope, extracts signals
  with lat/lng, and swarms wherever tension clusters.

A **feedback loop** makes the system self-reinforcing: signals cluster
geographically → clusters become beacons → scout goes deeper → more
signals → repeat until marginal return drops.

Three **user levers** control the display layer:
1. **Where** — map viewport (zoom from world to city)
2. **What** — cause tags (housing, water quality, etc.)
3. **How Loud** — heat scrubber (trending ↔ whispers)

## Technical Approach

### Architecture

```
                    ┌─────────────┐
                    │  Task Queue  │
                    │  (priority)  │
                    └──────┬──────┘
                           │
              ┌────────────┼────────────┐
              ▼            ▼            ▼
         ┌────────┐  ┌────────┐  ┌────────┐
         │Scout W1│  │Scout W2│  │Scout W3│
         └───┬────┘  └───┬────┘  └───┬────┘
             │            │           │
             ▼            ▼           ▼
         ┌────────────────────────────────┐
         │         Signal Graph           │
         │   (lat/lng, heat, tensions)    │
         └───────────┬────────────────────┘
                     │
          ┌──────────┼──────────┐
          ▼          ▼          ▼
    ┌──────────┐ ┌────────┐ ┌────────────┐
    │ Driver B │ │Cluster │ │  Driver A  │
    │  (news)  │ │Detector│ │(user query)│
    └────┬─────┘ └───┬────┘ └─────┬──────┘
         │           │            │
         └───────────┴────────────┘
                     │
                     ▼
              ┌─────────────┐
              │  Task Queue  │ ← feedback loop
              └─────────────┘
```

**Feed → Queue → Scout → Graph → Detect → Queue** (repeat)

### The Task Primitive: `ScoutTask`

Replaces `RegionNode` as the unit of work. Start minimal:

```rust
ScoutTask {
    id: Uuid,
    center_lat: f64,
    center_lng: f64,
    radius_km: f64,           // min 2km, max 100km
    context: String,           // LLM-friendly description ("housing crisis near Flint, MI")
    geo_terms: Vec<String>,    // for geo-filtering during extraction
    priority: f64,             // heat × recency × coverage_gap
    source: DriverB | Beacon | Manual,  // add DriverA when Phase 3 ships
    created_at: DateTime,
}
```

#### Research Insights

**Simplicity:** Start with 6 core fields. Add `status`, `claimed_at`,
`ttl_hours` only when you have multiple workers and actually need
concurrency control. At single-worker scale, a task is either done or
it isn't.

**Agent-native (critical):** The task queue must be observable and
controllable from day one. Define these GraphQL operations in Phase 1,
not as a future dashboard:
- `list_scout_tasks(status, geohash, source_type, limit)` — query
- `create_scout_task(center_lat, center_lng, radius_km, context)` — mutation
- `cancel_scout_task(id)` — mutation

Without these, no agent or admin tool can observe or steer the system.

**Storage:** Neo4j `:ScoutTask` node. At current scale, Neo4j is fine
for task storage. **Known scaling boundary:** Neo4j's page-level locking
makes CAS-based claiming unreliable above ~10 concurrent workers. If
you grow beyond that, migrate the queue to Postgres `SELECT FOR UPDATE
SKIP LOCKED` or Redis. Note this, don't build for it now.

**Indexes required:**
```cypher
CREATE INDEX scouttask_status IF NOT EXISTS FOR (t:ScoutTask) ON (t.status)
CREATE INDEX scouttask_priority IF NOT EXISTS FOR (t:ScoutTask) ON (t.priority)
CREATE CONSTRAINT scouttask_id IF NOT EXISTS FOR (t:ScoutTask) REQUIRE t.id IS UNIQUE
```

### The ScoutScope Bridge

**Architecture insight:** Introduce a `ScoutScope` struct as an
intermediate abstraction. This lets Phase 1 refactor all pipeline
stages from `&RegionNode` to `&ScoutScope`, and Phase 2 simply adds
`From<&ScoutTask>` for `ScoutScope`. No double-refactor needed.

```rust
pub struct ScoutScope {
    pub center_lat: f64,
    pub center_lng: f64,
    pub radius_km: f64,
    pub context: String,      // LLM-friendly name (replaces region.name)
    pub geo_terms: Vec<String>,
}

impl From<&RegionNode> for ScoutScope { ... }
// Phase 2 adds: impl From<&ScoutTask> for ScoutScope { ... }
```

This is the minimal interface that scout pipeline stages need from
their geographic context. 16 files reference `RegionNode` — refactoring
them to `ScoutScope` once is cleaner than refactoring them twice.

### Clustering: Signals → Hotspots

**Simplified approach:** Skip a clustering subsystem. Group signals by
rounding lat/lng to ~0.05 degree precision (~5km). If a grid cell has
3+ signals, create a task centered there. This is 10 lines of code.

Use the `geohash` crate (v0.13.1) for bucketing. **Gotcha:** Coord
ordering is `(x=longitude, y=latitude)`, not `(lat, lng)`. Common bug.

#### Research Insights

**Performance:** Make clustering incremental. Track a watermark. On
each pass, only process signals with `extracted_at > last_clustered_at`.
Store cell aggregates as Neo4j nodes:

```
(:GeohashCell {
    geohash: "9zvg5",
    signal_count: 47,
    sum_heat: 12.3,
    most_recent_signal: datetime("..."),
    source_diversity: 5,
    last_computed: datetime("...")
})
```

This avoids loading all signals for clustering (O(total) → O(new)).

**Geohash neighbor checking:** Check the 8 adjacent cells when
computing hotspot metrics. Signals near cell boundaries can split across
two cells, causing neither to exceed the threshold individually. The
`geohash` crate's `neighbors()` function handles this.

**Source loading:** Add `(:Source)-[:ACTIVE_IN]->(:GeohashCell)`
relationships. This enables fast geographic source loading:
```cypher
MATCH (c:GeohashCell)<-[:ACTIVE_IN]-(s:Source {active: true})
WHERE c.geohash IN $cell_hashes
RETURN DISTINCT s
```

### Marginal Return / Termination

**One rule:** If a task produced zero new signals, double the wait
before creating a new task for that area. Start at 6h, cap at 7 days.
This is the exponential backoff pattern already used in
`SourceScheduler`, applied at the geographic level. No need for three
separate throttling mechanisms at current scale.

Plus a global budget ceiling: max N tasks per cycle.

### Driver B: Global Tension Scanning

Pipeline:

1. **News sources:** Curated seed list of 20-30 national/global RSS
   feeds (AP, Reuters, major national outlets). Discovered local feeds
   via existing RSS enrichment plan.
2. **Scan phase:** Scrape feeds on a fixed cadence (e.g., every 2h).
   No geographic scope — read everything.
3. **Extract:** Run standard signal extraction on articles. LLM
   geocodes based on content (already implemented — `GeoPoint` +
   `GeoPrecision`).
4. **Cluster:** Extracted signals land in the graph. Clustering detects
   new hotspots.
5. **Enqueue:** New hotspots become `ScoutTask` entries with
   `source: Beacon`, priority based on signal heat + novelty.

Driver B is the cold-start engine. Day one, zero users: news feeds
produce signals → signals cluster → tasks enqueue → scout deepens →
map has content when users arrive.

#### Research Insights

**RSS efficiency:** Use conditional GET (`If-None-Match` /
`If-Modified-Since`) to skip re-downloading unchanged feeds. Cuts
bandwidth by 60-80%. The `feed-rs` crate (already in deps) handles
RSS/Atom parsing.

**LLM cost control:** At 20 feeds × ~50 articles × 12 runs/day =
12,000 potential LLM calls/day. Mitigate:
- Article URL dedup (hash set, skip if seen)
- Cheap pre-filter (keyword matching) before LLM extraction
- **Separate Driver B budget** from task-based scouting budget.
  Prevents news scanning from starving demand-driven coverage.

**Security — prompt injection:** Scraped web content can contain text
designed to manipulate LLM extraction. Mitigate:
- Output validation: verify extracted lat/lng is geographically
  plausible given the article's context
- Anomaly detection: flag sources producing unusually many signals or
  signals far from their historical pattern
- Quarantine: signals from newly discovered (non-curated) sources
  require 2+ independent source corroboration before triggering beacon
  tasks

### Source Independence

Sources become region-independent. The `:SCOUTS` relationship and
`:City` nodes are deleted entirely. No migration — clean break.

#### Notes

**Canonical keys:** `make_canonical_key()` already produces
region-independent keys. Fix the colon over-match bug (Phase 1a),
then strip any remaining city prefixes from source canonical_keys
by running a one-time Cypher cleanup.

**Source geographic loading:** Sources loaded by bounding box query
on their signals' lat/lng (or source lat/lng where available).
Sources with no geographic signals become orphans — acceptable at
current scale (few hundred sources). If needed later, add
`discovered_in_geohash` field.

### Frontend Changes

Minimal changes needed on the read path. These are independent of the
backend work and can ship whenever:

- **Default view:** Change initial map center from Twin Cities to a
  world view (zoom 2-3) or user's detected location.
- **Empty state:** When viewport has no signals, show "No signals in
  this area yet" with a subtle prompt.

### Admin / API Changes

**Agent-native requirement:** The task queue must be observable:
- `list_scout_tasks(status, geohash, limit)` — query
- `scout_task(id)` — query (with task history)
- `create_scout_task(...)` — mutation (replaces `create_city`)
- `cancel_scout_task(id)` — mutation
- `budget_status` — query (spent, remaining)
- `list_hotspot_cells(min_heat, limit)` — query (clustering state)
- `record_demand(lat, lng, context)` — mutation (programmatic demand
  injection, not just from the search UI)

These replace the old region-based admin flow and make the system
steerable by both humans and agents.

## Implementation Phases (Simplified)

### Phase 0: Pre-Flight Snapshot (Optional — for awareness only)

App hasn't launched. No migration path needed. These queries are useful
for understanding what's in the graph but do NOT block code changes.

```cypher
// Quick inventory
MATCH (n) WHERE n:Gathering OR n:Aid OR n:Need OR n:Notice OR n:Tension
RETURN labels(n)[0] AS type, count(n) AS count ORDER BY count DESC;

MATCH (s:Source) RETURN count(s) AS total_sources,
  count(CASE WHEN s.active THEN 1 END) AS active_sources;

MATCH (c:City) RETURN c.slug, c.name, c.center_lat, c.center_lng, c.radius_km;

MATCH ()-[r:SCOUTS]->() RETURN count(r) AS scouts_edges;
```

### Phase 1: Aggressive Refactor — Delete RegionNode

**No migration path.** App hasn't launched. Delete RegionNode, delete
`:SCOUTS`, delete `cleanup_off_geo_signals`, replace all region
references with `ScoutScope`. Clean break.

**1a. Critical fixes (do first)**
- Fix colon over-match bug in `migrate_region_relationships`
  (`migrate.rs` line 626): add `AND s.canonical_key <> s.canonical_value`
- Delete `cleanup_off_geo_signals` function entirely
  (`migrate.rs` line 1252-1358) and its call site (line 465)
- Delete `migrate_region_relationships` city-prefix strip logic
  (no longer needed — regions don't exist)

**1b. Delete RegionNode + CityNode**
- Delete `RegionNode` struct and `CityNode` type alias from `types.rs`
- Delete all `(:City)` and `:SCOUTS` Cypher queries from `writer.rs`
- Delete `region` field from config
- Delete any `create_city` / `update_city` GraphQL mutations
- Delete region-related admin pages
- Run `MATCH (c:City) DETACH DELETE c` against the graph

**1c. Define ScoutScope — the new geographic context**
- Define `ScoutScope` struct (center, radius, context, geo_terms)
- Extract `BoundingBox::from_center()` utility (inlined in 6+ places)
- Refactor all ~16 files that referenced `RegionNode` to use `ScoutScope`
- Refactor `Scout::new()` to accept `ScoutScope`
- Replace `get_active_sources(slug)` with geographic source loading
  (bounding box query on source lat/lng or signal lat/lng)
- Update `upsert_source` to not require region slug
- Update `submit_source` / `add_source` mutations to accept lat/lng

**1d. ScoutTask primitive**
- Define minimal `ScoutTask` struct (id, center, radius, context,
  geo_terms, priority, source, created_at)
- Create Neo4j node type with indexes
- Implement `From<&ScoutTask> for ScoutScope`
- Add GraphQL CRUD for ScoutTask (list, create, cancel)
- Seed initial ScoutTask for Twin Cities from config/env vars
  (replaces the old RegionNode)

**Success criteria:**
- RegionNode/CityNode completely gone from codebase and graph
- Scout runs on ScoutTask via ScoutScope
- Existing Twin Cities coverage preserved via seed task
- Sources are region-independent
- Task queue is observable via GraphQL

### Phase 2: Driver B + Simple Clustering

The cold-start engine. Scout discovers the world from news.

**2a. News feed scanner**
- Curate 20-30 national/global RSS feeds
- Scrape on fixed cadence (2h) with conditional GET for efficiency
- Article URL dedup (skip if already seen)
- Extract signals via standard pipeline (LLM geocoding)
- Separate budget pool for Driver B

**2b. Simple clustering**
- Add `geohash` crate dependency
- Stamp geohash-5 on signals at extraction time
- Incremental cell aggregation: update GeohashCell nodes on signal
  creation, not via periodic full scan
- Include geohash neighbor checking for boundary signals
- Cells with 3+ signals become ScoutTask candidates

**2c. Beacon → Task pipeline**
- Cells above threshold become `ScoutTask` entries (source: Beacon)
- One backoff rule: zero new signals → double wait (cap at 7 days)
- Global budget ceiling: max N tasks per cycle
- Add `list_hotspot_cells` GraphQL query

**Success criteria:**
- With no user input and no predefined regions, scout discovers tension
  hotspots from news alone
- Signals appear in 3+ distinct geographic areas
- Feedback loop works: clusters trigger deeper scouting
- Admin can observe hotspot cells and task queue via API

### Deferred (Build When Needed)

**Driver A (user query demand):** Defer until there's meaningful search
traffic. Just log raw searches for now. Build the demand aggregation
pipeline when you have data to analyze.

**Heat scrubber:** Independent read-path feature. Ship whenever. Add
`minHeat`/`maxHeat` to GraphQL queries and a frontend slider.

**Concurrency model:** Defer multi-worker infrastructure until you need
a second worker. Design concurrency for actual contention patterns you
observe, not imagined ones. The single-worker model works fine at
current scale.

**Geographic cadence tiers (hot/warm/cool/dead):** Defer until enough
geographic spread to observe differences. One exponential backoff rule
per area suffices.

**Runtime-tunable parameters:** Store clustering thresholds, cadence
values, and budget allocations as config values. Expose via API when
operational data shows which knobs need turning.

## Security Considerations

From security review:

| Risk | Severity | Mitigation |
|------|----------|------------|
| LLM prompt injection via scraped content | HIGH | Output validation, anomaly detection, quarantine for non-curated sources, 2-source corroboration for beacon tasks |
| Demand manipulation (future Driver A) | HIGH | Per-IP rate limiting, per-cycle cap on DriverA tasks, demand channel separate from search UI |
| Budget exhaustion | MEDIUM-HIGH | Separate Driver B budget, global ceiling, per-source budget cap |
| RSS feed injection / XML attacks | MEDIUM | Hardened XML parser (no XXE), per-feed size limits (5MB), feed validation |
| Canonical key migration data integrity | MEDIUM | Collision detection query, dual-read phase, maintenance window with backup |

## Performance Notes

From performance review:

**In-memory cache is the biggest scalability blocker.** `SignalCache::load()`
loads the entire graph into memory. At 100K signals this becomes 200-400MB
and takes 30-60s to reload.

**Near-term fix:** Add a geohash index to the in-memory cache. Store signals
in `HashMap<String, Vec<usize>>` keyed by geohash-5. Bounding-box queries
compute overlapping cells and scan only those buckets. Turns O(n) into O(k).

**Budget counter:** Use `AtomicU64` in process memory (already have
`BudgetTracker`), not Neo4j property. Periodically flush to Neo4j.

**RSS dedup:** Hash article URLs before LLM extraction. Conditional GET
on feeds. Cheap pre-filter. Can cut LLM calls by 70-80%.

## Acceptance Criteria

### Phase 1

- [ ] RegionNode/CityNode completely deleted from codebase
- [ ] No `:City` nodes or `:SCOUTS` edges in the graph
- [ ] `cleanup_off_geo_signals` deleted
- [ ] Colon over-match bug fixed
- [ ] Scout runs on ScoutTask via ScoutScope
- [ ] Sources are region-independent
- [ ] Existing Twin Cities coverage preserved via seed ScoutTask
- [ ] Task queue CRUD available via GraphQL

### Phase 2

- [ ] Driver B produces signals from news in 3+ geographic areas
- [ ] Feedback loop: signal clusters trigger deeper scouting
- [ ] Budget ceiling prevents runaway consumption
- [ ] Hotspot cells observable via `list_hotspot_cells` query
- [ ] No regression in signal quality for existing coverage

## Risk Analysis & Mitigation

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Colon over-match corrupts existing sources | HIGH (active bug) | High | Fix immediately: add `canonical_key <> canonical_value` guard |
| `cleanup_off_geo_signals` deletes global signals | HIGH (active bug) | Critical | Remove before Phase 2 ships |
| Source cold-start gap (new sources have no signals) | Medium | Medium | `discovered_in_geohash` field on SourceNode |
| Feedback loop creates runaway tasks | Medium | High | Global budget ceiling + exponential backoff per cell |
| News feeds produce low-quality signals | Medium | Medium | Supervisor quality gate + quarantine for non-curated sources |
| Clustering parameters need tuning | High | Low | Start conservative, geohash-5 + 3 signal threshold |
| In-memory cache won't scale to 100K+ | Medium | High | Add geohash index to cache (near-term), move to Neo4j spatial queries (later) |

## Dependencies & Prerequisites

- Scout pipeline modularization: `docs/plans/2026-02-20-refactor-scout-pipeline-stages-plan.md`
- RSS feed enrichment: `docs/plans/2026-02-20-feat-rss-feed-cold-start-enrichment-plan.md`
- `unwrap_or` data quality pattern: `docs/solutions/2026-02-17-unwrap-or-masks-data-quality.md`

## Rust Crates

| Crate | Purpose | Notes |
|-------|---------|-------|
| `geohash` 0.13.1 | Encode/decode lat/lng to geohash | Coord is (lng, lat) not (lat, lng) |
| `feed-rs` 2.3.1 | RSS/Atom parsing | Already in deps |
| `geo` | Geospatial primitives | BoundingBox, distance |
| `geocoding` 0.4.0 | Place name → coordinates | OSM Nominatim, 1 req/sec rate limit |

## Future Considerations

- **Driver A (user query demand):** Build when you have search traffic.
- **Heat scrubber:** Independent feature, ship whenever.
- **Multi-worker concurrency:** Design when you add a second worker.
  Known boundary: Neo4j task claiming breaks above ~10 workers →
  migrate to Postgres `SKIP LOCKED` or Redis at that point.
- **Tag-driven scouting:** Use tag popularity as a Driver A signal.
- **User subscriptions:** Subscription density = demand signal.
- **Radius auto-tuning:** Dense city → small radius. Rural → large.
- **International expansion:** Driver B with international news feeds.
- **Runtime parameter tuning:** Store thresholds in graph, expose via API.

## References

### Internal References

- Brainstorm: `docs/brainstorms/2026-02-20-demand-driven-scout-swarm-brainstorm.md`
- Region-based scout brainstorm: `docs/brainstorms/2026-02-20-region-based-scout-brainstorm.md`
- Scout pipeline refactor plan: `docs/plans/2026-02-20-refactor-scout-pipeline-stages-plan.md`
- RSS feed plan: `docs/plans/2026-02-20-feat-rss-feed-cold-start-enrichment-plan.md`
- Scout heuristics analysis: `docs/analysis/scout-heuristics.md`
- Data quality gotcha: `docs/solutions/2026-02-17-unwrap-or-masks-data-quality.md`
- Gravity multi-city: `docs/architecture/gravity-multi-city.md`

### Key Files

- `modules/rootsignal-common/src/types.rs` — RegionNode, GeoPoint, SourceNode
- `modules/rootsignal-scout/src/scout.rs` — Scout pipeline orchestrator
- `modules/rootsignal-scout/src/scheduling/bootstrap.rs` — Cold start bootstrapper
- `modules/rootsignal-scout/src/scheduling/scheduler.rs` — Source scheduler
- `modules/rootsignal-graph/src/writer.rs` — Neo4j CRUD (~15 region-scoped queries)
- `modules/rootsignal-graph/src/cached_reader.rs` — Geographic signal queries
- `modules/rootsignal-graph/src/migrate.rs` — Graph migrations (contains active bugs)
- `modules/rootsignal-api/src/graphql/schema.rs` — GraphQL API
- `modules/search-app/src/pages/SearchPage.tsx` — Frontend search page
- `modules/search-app/src/components/MapView.tsx` — Map component

### External References

- [Neo4j concurrent data access](https://neo4j.com/docs/operations-manual/current/database-internals/concurrent-data-access/)
- [geohash crate docs](https://docs.rs/geohash/latest/geohash/)
- [Geospatial anomaly detection with geohashes](https://www.instaclustr.com/blog/geospatial-anomaly-detection-terra-locus-anomalia-machina-part-2-geohashes-2d/)
- [Microsoft optimal freshness crawl scheduling](https://microsoft.github.io/Optimal-Freshness-Crawl-Scheduling/)
- [Scalable crawling with noisy signals (2025)](https://arxiv.org/pdf/2502.02430)
