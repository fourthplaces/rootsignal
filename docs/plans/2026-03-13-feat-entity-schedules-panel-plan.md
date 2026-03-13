---
title: "feat: Entity SchedulesPanel — per-entity schedule CRUD on detail pages"
type: feat
date: 2026-03-13
---

# Entity SchedulesPanel

## Overview

A reusable `<SchedulesPanel>` React component that surfaces workflow schedules directly on entity detail pages (`/sources/:id`, `/regions/:id`, `/clusters/:id`) with full CRUD: list, add, edit cadence, toggle enable/disable, delete.

The scheduling infrastructure already exists (table, events, projection, polling loop). The gap is **visibility** — schedules are only viewable on the global `/workflows` Schedules tab. This work wires per-entity schedule queries and an update-cadence mutation, then builds the frontend panel.

## Problem Statement

- Users must navigate to `/workflows` → Schedules tab and scan scope JSON to find schedules for a specific entity
- No way to see at a glance whether a source/region/cluster has scheduled workflows
- No way to edit cadence of an existing schedule (only create/toggle/delete)
- `group_feed` schedules are auto-created for clusters but invisible from the cluster page
- Naming mismatch: backend exposes `timeout`, frontend expects `cadenceSeconds` — the existing WorkflowsPage schedules tab and CreateScheduleDialog may be broken

## Proposed Solution

### Phase 1: Backend — Fix naming, add query + mutation

1. **Fix naming mismatch**: Rename `timeout` → `cadence_seconds` on GQL `Schedule` type (so async-graphql generates `cadenceSeconds` matching frontend)
2. **Per-entity schedule query**: Add `schedules_for_entity(entityType, entityId)` resolver that filters by `region_id` column (regions) or JSONB `scope` matching (sources, clusters)
3. **Update cadence mutation**: Add `update_schedule_cadence(scheduleId, cadenceSeconds)` that emits `ScheduleCadenceAdjusted` with reason `"manual_edit"`, updating both `timeout` and `base_timeout`

### Phase 2: Frontend — SchedulesPanel component + integration

4. **`<SchedulesPanel>`**: Reusable component parameterized by entity type + entity ID
5. **Integration**: Mount on SourceDetailPage, RegionDetailPage, ClusterDetailPage

## Technical Approach

### Phase 1: Backend

#### 1a. Fix naming mismatch

Rename fields on the GQL `Schedule` struct so `async_graphql` generates the names the frontend already expects.

**`modules/rootsignal-api/src/graphql/types.rs`** — `Schedule` struct:
```rust
#[derive(SimpleObject)]
pub struct Schedule {
    pub schedule_id: String,
    pub flow_type: String,
    pub scope: String,
    pub cadence_seconds: i32,      // was: timeout
    pub base_cadence_seconds: i32, // was: base_timeout
    pub recurring: bool,
    pub enabled: bool,
    pub last_run_id: Option<String>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub region_id: Option<String>,
}
```

Update `From<ScheduleRow>` impl to map `row.timeout` → `cadence_seconds`, `row.base_timeout` → `base_cadence_seconds`.

**`modules/rootsignal-api/src/graphql/mutations.rs`** — `create_schedule`:
- Rename parameter `timeout` → `cadence_seconds` in the GraphQL signature
- Internal event emission still uses `timeout` field name (no change to event model)

#### 1b. Per-entity schedule query

**`modules/rootsignal-api/src/db/models/schedule.rs`** — add:

```rust
pub async fn list_for_entity(
    pool: &PgPool,
    entity_type: &str,
    entity_id: &str,
) -> Result<Vec<ScheduleRow>, sqlx::Error> {
    match entity_type {
        "region" => {
            // Simple column filter
            sqlx::query_as::<_, ScheduleRow>(
                "SELECT ... FROM schedules \
                 WHERE region_id = $1 AND deleted_at IS NULL \
                 ORDER BY created_at DESC"
            )
            .bind(entity_id)
            .fetch_all(pool)
            .await
        }
        "source" => {
            // JSONB containment: scope->'source_ids' contains the UUID
            sqlx::query_as::<_, ScheduleRow>(
                "SELECT ... FROM schedules \
                 WHERE scope->'source_ids' @> $1::jsonb AND deleted_at IS NULL \
                 ORDER BY created_at DESC"
            )
            .bind(format!("[\"{entity_id}\"]"))
            .fetch_all(pool)
            .await
        }
        "cluster" => {
            // JSONB match: scope->>'group_id' = uuid
            sqlx::query_as::<_, ScheduleRow>(
                "SELECT ... FROM schedules \
                 WHERE scope->>'group_id' = $1 AND deleted_at IS NULL \
                 ORDER BY created_at DESC"
            )
            .bind(entity_id)
            .fetch_all(pool)
            .await
        }
        _ => Ok(vec![]),
    }
}
```

**`modules/rootsignal-api/src/graphql/schema.rs`** — add resolver:

```rust
#[graphql(guard = "AdminGuard")]
async fn schedules_for_entity(
    &self,
    ctx: &Context<'_>,
    entity_type: String,
    entity_id: String,
) -> Result<Vec<Schedule>> {
    let pool = ctx.data_unchecked::<PgPool>();
    let rows = schedule::list_for_entity(pool, &entity_type, &entity_id).await?;
    Ok(rows.into_iter().map(Schedule::from).collect())
}
```

#### 1c. Update cadence mutation

**`modules/rootsignal-api/src/graphql/mutations.rs`** — add:

```rust
#[graphql(guard = "AdminGuard")]
async fn update_schedule_cadence(
    &self,
    ctx: &Context<'_>,
    schedule_id: String,
    cadence_seconds: i32,
) -> Result<ScoutResult> {
    // Emit ScheduleCadenceAdjusted with reason "manual_edit"
    // Updates both timeout AND base_timeout (resets backoff baseline)
}
```

**`modules/rootsignal-scout/src/core/projection.rs`** — update `ScheduleCadenceAdjusted` handler to also set `base_timeout` when reason is `"manual_edit"`.

### Phase 2: Frontend

#### 2a. GraphQL definitions

**`modules/admin-app/src/graphql/queries.ts`** — add:

```graphql
query SchedulesForEntity($entityType: String!, $entityId: String!) {
  schedulesForEntity(entityType: $entityType, entityId: $entityId) {
    scheduleId
    flowType
    scope
    cadenceSeconds
    baseCadenceSeconds
    recurring
    enabled
    lastRunId
    nextRunAt
    createdAt
    regionId
  }
}
```

**`modules/admin-app/src/graphql/mutations.ts`** — add:

```graphql
mutation UpdateScheduleCadence($scheduleId: String!, $cadenceSeconds: Int!) {
  updateScheduleCadence(scheduleId: $scheduleId, cadenceSeconds: $cadenceSeconds) {
    success
    message
  }
}
```

#### 2b. SchedulesPanel component

**`modules/admin-app/src/components/SchedulesPanel.tsx`**

Props:
```typescript
type SchedulesPanelProps = {
  entityType: "source" | "region" | "cluster";
  entityId: string;
  regionId?: string; // for region-scoped schedule creation
};
```

Flow type compatibility map (static):
```typescript
const COMPATIBLE_FLOWS: Record<string, { key: string; label: string }[]> = {
  source: [{ key: "scout_source", label: "Scout Source" }],
  region: [
    { key: "scrape", label: "Scrape" },
    { key: "bootstrap", label: "Bootstrap" },
    { key: "weave", label: "Weave" },
    { key: "coalesce", label: "Coalesce" },
  ],
  cluster: [{ key: "group_feed", label: "Group Feed" }],
};
```

Scope construction (per entity type):
```typescript
function buildScope(entityType: string, entityId: string): string {
  switch (entityType) {
    case "source": return JSON.stringify({ source_ids: [entityId] });
    case "cluster": return JSON.stringify({ group_id: entityId });
    default: return "{}";
  }
}
```

Layout:
- Section header: `<h3>` "Schedules" with `+ Add` button (right-aligned)
- Table columns: Flow Type, Cadence (editable), Status (toggle), Next Run, Created, Actions (delete)
- Cadence cell: click to edit → number + unit picker inline, save on blur/enter
- Status cell: toggle button (enabled/disabled)
- Empty state: "No schedules" with contextual help text per entity type
- Add flow: dropdown of compatible flow types (filtered to exclude already-present types to prevent duplicates), cadence picker, create button
- Chain checkbox shown only for region entity type

Uses `DataTable<Schedule>` matching existing WorkflowsPage pattern. Reuses `formatCadence()` helper (extract from WorkflowsPage to shared util).

#### 2c. Integration on detail pages

**`modules/admin-app/src/pages/SourceDetailPage.tsx`**:
- Add `<SchedulesPanel entityType="source" entityId={source.id} />` section
- Coexists with existing cadence metadata card (which shows source-level stats)

**`modules/admin-app/src/pages/RegionDetailPage.tsx`**:
- Add `<SchedulesPanel entityType="region" entityId={regionId} regionId={regionId} />`
- Placed below region metadata, above the tabs

**`modules/admin-app/src/pages/ClusterDetailPage.tsx`**:
- Add `<SchedulesPanel entityType="cluster" entityId={clusterId} />`
- `group_feed` schedules auto-created by coalescing will appear here automatically

## Acceptance Criteria

### Backend
- [x] GQL `Schedule` type exposes `cadenceSeconds` and `baseCadenceSeconds` (not `timeout`/`baseTimeout`)
- [x] `createSchedule` mutation accepts `cadenceSeconds` parameter
- [x] `schedulesForEntity(entityType, entityId)` resolver returns schedules filtered by entity
- [x] Region filtering uses `region_id` column
- [x] Source filtering uses JSONB `scope->'source_ids' @> '["uuid"]'`
- [x] Cluster filtering uses JSONB `scope->>'group_id' = 'uuid'`
- [x] `updateScheduleCadence(scheduleId, cadenceSeconds)` mutation works, updates both timeout and base_timeout
- [x] Manual cadence edit resets backoff baseline

### Frontend
- [x] `<SchedulesPanel>` renders on source, region, and cluster detail pages
- [x] Lists existing schedules with flow type, cadence, enabled/disabled, next run, created
- [x] "+ Add" dropdown filtered to compatible flow types for entity type
- [x] Adding a schedule creates with correct scope JSON and region_id
- [x] Cadence editable inline (number + unit picker)
- [x] Enable/disable toggle works
- [x] Delete works (with soft-delete)
- [x] Already-present flow types excluded from Add dropdown (prevents duplicates)
- [x] Chain checkbox shown only for region schedules
- [x] Backoff indicator when `cadenceSeconds != baseCadenceSeconds`
- [x] Empty state with contextual help text

## Files to Change

| File | Change |
|------|--------|
| `modules/rootsignal-api/src/graphql/types.rs` | Rename `timeout`→`cadence_seconds`, `base_timeout`→`base_cadence_seconds` on `Schedule` struct |
| `modules/rootsignal-api/src/graphql/mutations.rs` | Rename `timeout`→`cadence_seconds` param on `create_schedule`; add `update_schedule_cadence` mutation |
| `modules/rootsignal-api/src/graphql/schema.rs` | Add `schedules_for_entity` resolver |
| `modules/rootsignal-api/src/db/models/schedule.rs` | Add `list_for_entity()` with JSONB matching |
| `modules/rootsignal-scout/src/core/projection.rs` | Update `ScheduleCadenceAdjusted` to also set `base_timeout` on manual edit |
| `modules/admin-app/src/graphql/queries.ts` | Add `SCHEDULES_FOR_ENTITY` query |
| `modules/admin-app/src/graphql/mutations.ts` | Add `UPDATE_SCHEDULE_CADENCE` mutation |
| `modules/admin-app/src/components/SchedulesPanel.tsx` | **New** — reusable panel component |
| `modules/admin-app/src/pages/SourceDetailPage.tsx` | Mount SchedulesPanel |
| `modules/admin-app/src/pages/RegionDetailPage.tsx` | Mount SchedulesPanel |
| `modules/admin-app/src/pages/ClusterDetailPage.tsx` | Mount SchedulesPanel |

## Edge Cases

- **Cluster with no coalesce yet**: Empty SchedulesPanel, user can manually add `group_feed`
- **Source already has scout_source schedule**: "+ Add" dropdown disables/hides `scout_source` option
- **Backoff-inflated cadence**: Panel shows current cadence with indicator that backoff is active; editing resets baseline
- **Deleted schedule re-creation**: Soft-deleted schedules are excluded from duplicate check, allowing re-creation
- **Schedule for deleted entity**: Not addressed — schedules will remain but triggers may fail gracefully at runtime

## References

- Existing schedule infrastructure: `modules/rootsignal-scout/src/domains/scheduling/events.rs`
- Schedule polling loop: `modules/rootsignal-api/src/scout_runner.rs` (lines 772-963)
- Existing CreateScheduleDialog: `modules/admin-app/src/components/CreateScheduleDialog.tsx`
- WorkflowsPage Schedules tab: `modules/admin-app/src/pages/WorkflowsPage.tsx`
