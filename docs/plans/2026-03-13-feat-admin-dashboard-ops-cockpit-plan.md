---
title: "feat: Admin Dashboard Ops Cockpit"
type: feat
date: 2026-03-13
---

# Admin Dashboard Ops Cockpit

## Enhancement Summary

**Deepened on:** 2026-03-13
**Agents used:** 11 (performance oracle, architecture strategist, TypeScript reviewer, frontend races reviewer, security sentinel, simplicity reviewer, agent-native reviewer, best practices researcher, Neo4j docs researcher, frontend design skill, pattern recognition specialist)

### Critical Corrections Discovered
1. **No `:Signal` label in Neo4j** — all pseudocode Cypher was wrong. Must UNION across Gathering|Resource|HelpRequest|Announcement|Concern|Condition
2. **Column name mismatches** — events table uses `ts` not `created_at`; use `region` column not `scope->>'region'`; use `extracted_at` not `created_at`
3. **AdminGuard missing** — resolver must have `#[graphql(guard = "AdminGuard")]`
4. **Neo4j anti-patterns** — `WHERE NOT (n)-[:REL]->()` is expensive; `IS NULL` can't use indexes

### Key Simplifications Applied
- Cut from 6 scorecards to 4 (defer missing confidence, dead sources)
- Defer drill-down filter support (Phase 2c) to fast-follow PR
- Defer weekly deltas — use existing `count_by_type()` for v1
- Modify `admin_dashboard` in place instead of creating new resolver
- 3 flow-status cards instead of regions x flows grid
- Defer chart migration (Phase 3) — separate future PR

### Key Optimizations Applied
- Serve quality counts from in-memory CacheStore — eliminates 3 Neo4j queries
- Use `size()` / GetDegree for orphan count (O(1) per node vs anti-join scan)
- Use inversion (total - IS NOT NULL) for null property counts

---

## Overview

Replace the current admin dashboard (a data dump of vanity metrics and charts) with a global operational cockpit that answers three questions at a glance: Is the system healthy? Is the data quality good? What does the graph look like right now?

The dashboard becomes **global** (all regions) instead of region-scoped.

**Brainstorm:** `docs/brainstorms/2026-03-13-admin-dashboard-ops-cockpit-brainstorm.md`

## Problem Statement

The current dashboard shows total signal counts, stacked area charts, top/bottom source tables, extraction yield bars, and a pie chart. None of these answer an operational question or drive action. The dashboard also has stale terminology ("Tensions" instead of "Concerns") and uses the old 5-type taxonomy (missing Condition). Several fields are returned by the backend but never displayed.

## Proposed Solution

Three-section cockpit with global scope:

### Section 1: System Health

**Three flow-status cards** (scout, coalesce, weave):
- Each card: status icon (shape + color) + relative timestamp ("2h ago") + link to last run
- Status encoding: filled circle = OK, triangle = warning, square = error, ring = unknown
- Click card → `/workflows/<runId>` for that run's detail
- Use `<Link>` for navigation (right-click, screen reader, status bar support)

**Error summary**: single count of errors in last 24h. Displayed as a left-border-accented card (`border-l-2 border-l-red-500/60` when > 0). Click → `/events`

**Budget card**: Global spend today / daily limit with progress bar. Color thresholds: green < 60%, amber 60-85%, red > 85%. Use `<div role="progressbar">` with `aria-valuenow`. Click → `/budget`

### Section 2: Data Quality

**Four scorecards in a 2x2 grid** (v1 — expand to 6 later):

| Card | Data Source | Notes |
|------|-----------|-------|
| Signals missing category | CacheStore (in-memory) | Iterate `snap.signals`, filter `category.is_none()` |
| Signals without location | CacheStore (in-memory) | Signals not in `snap.location_edges` |
| Orphaned signals | Neo4j (one query) | `size((n)-[:MEMBER_OF]->()) = 0` per label |
| Validation issues (open) | Neo4j (`validation_issue_summary`) | Needs global variant (no region filter) |

Each card: `<button>` (not `<div onClick>`) with count, status dot (shape + color), hover-reveal "View details →". Color: 0 = emerald, 1-10 = amber, 10+ = red.

**Deferred to v2**: missing confidence (likely always 0 due to NOT NULL constraint), dead sources (supervisor handles), drill-down links to filtered views.

### Section 3: Graph Overview

**Six domain count cards** — Situations, Concerns, Help Requests, Resources, Announcements, Conditions:
- Use existing `count_by_type()` (already cached) + `situation_count()` for Situations
- Each: label, count with `tabular-nums`, click → `/data?tab=signals&type=Concern` (or `tab=situations`)
- Defer weekly deltas to v2 (need new backend queries + trend arrow UI)

**Hottest Concerns** — top 5 by `cause_heat`:
- Title, category, cause_heat (with mini bar visualization), corroboration count
- Click title → `/signals/<id>` via `<Link>` (requires adding `id` to `UnmetTension` struct)

---

## Technical Approach

### Phase 1: Backend — Modify Resolver + Add Queries

**1a. Quality counts from CacheStore** (`rootsignal-graph/src/cached_reader.rs`)

Serve missing-category and without-location from the in-memory cache (reloaded hourly). No Neo4j round-trip:

```rust
// On CachedReader — admin-only, not on GraphQueries trait
pub fn signals_missing_category(&self) -> u64 {
    let snap = self.cache.load_full();
    snap.signals.iter()
        .filter(|n| n.meta().map(|m| m.category.is_none()).unwrap_or(true))
        .count() as u64
}

pub fn signals_without_location(&self) -> u64 {
    let snap = self.cache.load_full();
    let with_loc: HashSet<Uuid> = snap.location_edges.iter().map(|e| e.signal_id).collect();
    snap.signals.iter().filter(|n| !with_loc.contains(&n.id())).count() as u64
}
```

**1b. Orphaned signal count** (`rootsignal-graph/src/writer.rs`)

Use `size()` with GetDegree (O(1) per node) instead of anti-join pattern. Admin-only method on `GraphReader` directly — not on `GraphQueries` trait:

```cypher
// Per label, combined via UNION ALL:
MATCH (n:Gathering)
WHERE size((n)-[:MEMBER_OF]->()) = 0
  AND n.extracted_at < datetime() - duration('P1D')
RETURN count(n) AS orphaned
UNION ALL
MATCH (n:Resource)
WHERE size((n)-[:MEMBER_OF]->()) = 0
  AND n.extracted_at < datetime() - duration('P1D')
RETURN count(n) AS orphaned
// ... repeat for HelpRequest, Announcement, Concern, Condition
```

The `extracted_at < 1 day ago` filter excludes freshly extracted signals that haven't been coalesced yet.

**1c. Global validation issue summary** (`rootsignal-graph/src/reader.rs`)

Add `validation_issue_summary_global()` variant that drops the `WHERE v.region = $region` clause from the existing `validation_issue_summary()`. Reuse existing `SupervisorSummary` GQL type.

**1d. Pipeline status query** (`rootsignal-api/src/db/models/scout_run.rs`)

```rust
pub async fn last_run_per_flow(pool: &PgPool) -> Result<Vec<AdminPipelineStatus>> {
    sqlx::query_as!(AdminPipelineStatus,
        r#"SELECT DISTINCT ON (flow_type)
            id as run_id, region, flow_type,
            started_at, finished_at, error,
            CASE
                WHEN cancelled_at IS NOT NULL THEN 'cancelled'
                WHEN error IS NOT NULL THEN 'failed'
                WHEN finished_at IS NOT NULL THEN 'completed'
                WHEN started_at > now() - interval '30 minutes' THEN 'running'
                ELSE 'stale'
            END as "status!"
        FROM runs
        WHERE flow_type IS NOT NULL
        ORDER BY flow_type, started_at DESC"#
    ).fetch_all(pool).await
}
```

Uses `region` column (indexed) not `scope->>'region'`. For v1: one card per flow type (global). v2: expand to per-region grid.

**1e. Error count query** (`rootsignal-api/src/db/models/scout_run.rs`)

```rust
pub async fn error_count_last_24h(pool: &PgPool) -> Result<i64> {
    sqlx::query_scalar!(
        "SELECT count(*) FROM events
         WHERE ts >= now() - interval '24 hours'
           AND event_type IN ('scrape:ContentFetchFailed', 'scrape:ExtractionFailed', 'pipeline:HandlerFailed')"
    ).fetch_one(pool).await
}
```

Uses `ts` column (indexed) and `event_type` column (indexed). Returns single aggregate count for v1; break down by type in v2.

**1f. Add `id` to UnmetTension** (`rootsignal-graph/src/writer.rs`)

Add `id: Uuid` to the `UnmetTension` struct and `t.id` to the Cypher RETURN clause in `get_unmet_tensions()`. GQL type name: `AdminHottestConcern` (follows `Admin*` prefix convention).

**1g. Modify `admin_dashboard` resolver in place** (`rootsignal-api/src/graphql/schema.rs`)

Rewrite the existing `admin_dashboard` resolver. Keep the `#[graphql(guard = "AdminGuard")]` annotation. Remove the `region: String!` parameter. Replace the 18-field `AdminDashboardData` struct with the new shape:

```graphql
type AdminDashboard {
  # System Health
  pipelineStatus: [AdminPipelineStatus!]!
  errorCount: Int!
  budgetSpentCents: Int!
  budgetLimitCents: Int!

  # Data Quality
  signalsMissingCategory: Int!
  signalsWithoutLocation: Int!
  orphanedSignals: Int!
  validationSummary: SupervisorSummary!

  # Graph Overview
  countByType: [TypeCount!]!
  situationCount: Int!
  hottestConcerns: [AdminHottestConcern!]!
}
```

All queries run in parallel via `tokio::join!()`. Reuse existing `TypeCount` and `SupervisorSummary` GQL types.

**1h. Sanitize error strings**

Truncate `AdminPipelineStatus.error` to 200 chars and strip URL/path patterns before exposing via GraphQL. Never expose raw `anyhow` error chains.

### Phase 2: Frontend — Rewrite DashboardPage

**2a. TypeScript types** (`admin-app/src/graphql/queries.ts`)

Define full types — no `any` leakage:

```typescript
type PipelineStatus = {
  runId: string;
  region: string;
  flowType: string;
  status: "completed" | "failed" | "running" | "cancelled" | "stale";
  startedAt: string;
  finishedAt: string | null;
  error: string | null;
};

type AdminHottestConcern = {
  id: string;
  title: string;
  category: string;
  causeHeat: number;
  corroborationCount: number;
};

type DomainCount = { signalType: string; count: number };

interface AdminDashboardData {
  adminDashboard: {
    pipelineStatus: PipelineStatus[];
    errorCount: number;
    budgetSpentCents: number;
    budgetLimitCents: number;
    signalsMissingCategory: number;
    signalsWithoutLocation: number;
    orphanedSignals: number;
    validationSummary: { totalOpen: number; countBySeverity: { label: string; count: number }[] };
    countByType: DomainCount[];
    situationCount: number;
    hottestConcerns: AdminHottestConcern[];
  };
}
```

Use `useQuery<AdminDashboardData>(ADMIN_DASHBOARD)` with discriminated union for status.

**2b. Component structure**

```
pages/DashboardPage.tsx                     — layout orchestrator, query, section headers
components/dashboard/PipelineCards.tsx       — 3 flow-status cards
components/dashboard/BudgetSummary.tsx       — budget progress bar
components/dashboard/QualityScorecard.tsx    — single clickable quality card
components/dashboard/DomainCountCard.tsx     — single domain count card
components/dashboard/HottestConcernsTable.tsx — 5-row table with cause_heat mini bars
```

`DashboardPage.tsx` reads as a layout of sections. Each section component takes typed props. No 400-line monolith.

**2c. Apollo Client configuration**

```typescript
const { data, loading, error, networkStatus } = useQuery<AdminDashboardData>(
  ADMIN_DASHBOARD,
  {
    pollInterval: 30_000,
    errorPolicy: "all",          // show stale data + warning, not blank screen
    fetchPolicy: "cache-and-network",
    notifyOnNetworkStatusChange: true,
  }
);

const initialLoading = loading && !data;
const refreshing = networkStatus === NetworkStatus.poll;
```

Add `useVisibilityPolling` hook to pause polling when tab is backgrounded:

```typescript
useEffect(() => {
  const handler = () => {
    if (document.visibilityState === "hidden") stopPolling();
    else { refetch(); startPolling(30_000); }
  };
  document.addEventListener("visibilitychange", handler);
  return () => document.removeEventListener("visibilitychange", handler);
}, [startPolling, stopPolling, refetch]);
```

Show stale-data warning banner when `error` is set but `data` exists.

**2d. UI patterns**

- Section headers: vertical bar accent + uppercase tracking-wide (`<div class="w-1 h-4 rounded-full bg-zinc-500">` + `text-sm font-medium tracking-wide uppercase text-muted-foreground`)
- Status indicators: shape + color (circle/triangle/square), never color-only. Pair with text label.
- All numbers: `tabular-nums` for vertical alignment
- Quality scorecards: `<button>` with hover-reveal "View details →", `aria-label="Missing category: 3 issues"`
- Clickable elements: `<Link>` from react-router (not `onClick` + `navigate`)
- Error summary card: `border-l-2 border-l-red-500/60` left accent when count > 0
- Hottest concerns cause_heat: mini horizontal bar (sparkline-style) + numeric value
- Add CSS custom properties for threshold colors: `--color-status-ok`, `--color-status-warn`, `--color-status-critical`
- Focus rings on interactive elements: `focus-visible:ring-2 ring-ring`
- No entrance animations — ops dashboards render instantly

**2e. Wire up FindingsPage**

- Add route in `App.tsx`: `{ path: "findings", element: <FindingsPage /> }`
- Add "Findings" to sidebar nav in `AdminLayout.tsx` with `AlertTriangle` icon, after "Events"
- Fix FindingsPage hardcoded `region = "twincities"` (line 123) — make global or use `useRegion()` with fallback
- Validation issues card links to `/findings`

### Deferred to Follow-up PRs

**v2 — Drill-down filters:**
- Add `adminQualitySignals(quality: QualityDimension!, limit: Int)` resolver (Rust enum, not freeform string)
- Treat quality as a "mode" on SignalsPage — separate query, not composable with type/status filters
- Scorecards become `<Link>` to `/data?tab=signals&quality=missing_category`

**v2 — Weekly deltas:**
- Add `domain_counts_with_delta()` using `extracted_at` (indexed) for the 7-day window
- Split into two queries per label: count store for total + index range seek for recent
- Add trend arrows with `aria-label` including delta value and direction

**v2 — Pipeline grid:**
- Expand flow-status cards to a regions x flow-types grid when more regions are added

**Separate PR — Chart migration:**
- Signal volume chart → DataPage signals tab
- Pie chart → DataPage signals tab
- Source tables → DataPage sources tab
- Extraction yield → WorkflowsPage

**Separate PR — Agent-native parity:**
- Add `get_ops_dashboard` investigate tool so agents can check system health
- Move threshold semantics to server-side (`status: "healthy" | "degraded" | "critical"` alongside counts)

## Acceptance Criteria

### System Health
- [ ] Three flow-status cards show last run status + relative time per flow type (scout, coalesce, weave)
- [ ] Each card links to `/workflows/<runId>` via `<Link>`
- [ ] Error count shows total errors in last 24h, links to `/events`
- [ ] Budget card shows spend/limit with `<div role="progressbar">`, links to `/budget`
- [ ] Resolver has `#[graphql(guard = "AdminGuard")]`

### Data Quality
- [ ] Four scorecards: missing category, without location, orphaned signals, validation issues
- [ ] Missing category and without location served from CacheStore (no Neo4j hit)
- [ ] Orphaned count uses `size()` / GetDegree pattern per label
- [ ] `validation_issue_summary_global()` has no region filter
- [ ] FindingsPage routed at `/findings`, in sidebar nav, hardcoded region fixed

### Graph Overview
- [ ] Six domain count cards using existing `count_by_type()` + `situation_count()`
- [ ] Hottest concerns shows top 5 by cause_heat with clickable titles via `<Link to={/signals/${id}}>`
- [ ] `UnmetTension` struct has `id: Uuid` field

### Frontend Quality
- [ ] Query result typed as `AdminDashboardData` — no `any` leakage
- [ ] Section components extracted into `components/dashboard/`
- [ ] Status indicators use shape + color + text (not color-only)
- [ ] All interactive elements use `<button>` or `<Link>`, not bare `<div onClick>`
- [ ] `tabular-nums` on all numeric displays
- [ ] Polling pauses when tab is backgrounded (`useVisibilityPolling` hook)
- [ ] `errorPolicy: "all"` — stale data shown with warning banner on polling errors
- [ ] No stale terminology — current taxonomy names throughout

### Security
- [ ] `AdminGuard` on resolver
- [ ] `PipelineStatus.error` sanitized (200 char truncation, URL/path stripping)
- [ ] Quality filter (when added in v2) implemented as Rust enum, not freeform string

## Neo4j Index Requirements

Add `category` to the signal property index loop in `rootsignal-graph/src/migrate.rs`:

```rust
for prop in &["lat", "lng", "source_diversity", "cause_heat", "extracted_at",
              "review_status", "channel_diversity", "category"] {
```

This enables the inversion approach for null-category counts if needed in v2 (v1 uses CacheStore).

## Existing Bugs Found During Research

1. **FindingsPage hardcodes `region = "twincities"`** (line 123) — fix as part of this work
2. **Potential RegionContext issue** — `useRegion()` in `AdminLayout` function body may resolve against default context since the hook call is above the `<RegionProvider>` in the render tree. Verify and fix if needed.
3. **DataTable column resize leaks event listeners** — `handleResizeStart` attaches mousemove/mouseup to document without cleanup on unmount. Not blocking but worth fixing.

## References

- Brainstorm: `docs/brainstorms/2026-03-13-admin-dashboard-ops-cockpit-brainstorm.md`
- Current dashboard: `modules/admin-app/src/pages/DashboardPage.tsx`
- Current resolver: `modules/rootsignal-api/src/graphql/schema.rs:321`
- GraphQueries trait: `modules/rootsignal-graph/src/queries.rs`
- Graph writer: `modules/rootsignal-graph/src/writer.rs`
- CacheStore: `modules/rootsignal-graph/src/cache.rs`
- CachedReader: `modules/rootsignal-graph/src/cached_reader.rs`
- Run model: `modules/rootsignal-api/src/db/models/scout_run.rs`
- Events table schema: `modules/rootsignal-migrate/migrations/007_unified_events.sql` (column is `ts`, not `created_at`)
- Runs table schema: `modules/rootsignal-migrate/migrations/006_scout_runs.sql` (has `region` column, indexed)
- Neo4j indexes: `modules/rootsignal-graph/src/migrate.rs:54`
- Validation issues: `modules/rootsignal-graph/src/reader.rs:1344`
- FindingsPage: `modules/admin-app/src/pages/FindingsPage.tsx`
- Supervisor summary GQL type: `modules/rootsignal-api/src/graphql/schema.rs:1164`
- UnmetTension struct: `modules/rootsignal-graph/src/writer.rs:4578`
