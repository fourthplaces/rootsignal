---
title: "feat: Per-Group Gravity Feed"
type: feat
date: 2026-03-13
---

# Per-Group Gravity Feed

## Overview

Signal groups are gravitational wells. Each group stores search queries that define its gravity — when fed, the coalescer runs those queries to find new matching signals, asks the LLM whether they belong, and adds them. Groups are fed on a self-adjusting schedule with exponential backoff: frequent when active, slowing when they stop attracting signals. A successful feed auto-chains a re-weave of the group's situation.

## Problem Statement

Today `feed_single_group()` exists but is private, only callable as part of a full region-wide coalesce run (`feed_mode()` iterates up to 5 groups). There's no way to feed a specific group on demand or on its own schedule. Groups created during coalescing are static — they don't grow unless a full coalesce happens to pick them up.

## Proposed Solution

1. **Manual feed**: "Feed" button on Cluster Detail page → runs `feed_single_group()` for just that group
2. **Auto-chain**: Successful feed (new signals found) → auto-chains `run_cluster_weave()` to re-weave the situation
3. **Auto-schedule**: New groups automatically get a feed schedule at base interval (1 hour)
4. **Exponential backoff**: Empty feeds double `cadence_seconds`. Successful feeds reset to `base_cadence_seconds`. Cap at 7 days.

## Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Backoff storage | `timeout` / `base_timeout` / `recurring` on `schedules` | Rename `cadence_seconds` → `timeout`, add `base_timeout` for reset, `recurring` for one-shot vs repeating |
| Feed completion event | `GroupFeedCompleted` on `CoalescingEvent` | Drives backoff logic, chain decision, and audit trail |
| Backoff cap | 604800s (7 days) | ~13 consecutive empties from 1h base. Functionally dead but still polls weekly |
| Feed + weave mutual exclusion | Yes | Busy check looks for both `group_feed` AND `cluster_weave` for same group |
| Manual feed resets backoff | Yes (if successful) | Evidence the group is alive should reset the schedule |
| `flow_type` string | `"group_feed"` | Consistent with `"cluster_weave"` naming |
| Schedule ID format | `"group_feed_{group_id}"` | Deterministic, prevents duplicates on replay |
| Base interval | 3600s (1h), hardcoded constant | Simple, can be made configurable later |
| `parent_run_id` on chain | Add to `ClusterWeaveRequested` | Enables causal chain tracking in workflows UI |

## Technical Approach

### Phase 1: Backend Foundation

#### Migration

- [x] Rename `cadence_seconds` → `timeout`, add `base_timeout` and `recurring` columns

`modules/rootsignal-migrate/migrations/XXX_schedule_backoff.sql`:

```sql
ALTER TABLE schedules RENAME COLUMN cadence_seconds TO timeout;
ALTER TABLE schedules ADD COLUMN base_timeout INTEGER;
UPDATE schedules SET base_timeout = timeout;
ALTER TABLE schedules ALTER COLUMN base_timeout SET NOT NULL;
ALTER TABLE schedules ADD COLUMN recurring BOOLEAN NOT NULL DEFAULT true;
```

All existing schedules become `recurring = true` with `base_timeout = timeout`. The `timeout` field gets mutated by backoff; `base_timeout` is the reset value. `recurring = false` enables one-shot runs (e.g., a manual feed that doesn't repeat).

#### Lifecycle Event

- [x] Add `FeedGroupRequested` to `LifecycleEvent`

`modules/rootsignal-scout/src/domains/lifecycle/events.rs`:

```rust
FeedGroupRequested {
    run_id: Uuid,
    group_id: Uuid,
    budget_cents: u32,
},
```

- [x] Add `parent_run_id: Option<String>` to `ClusterWeaveRequested`

#### Domain Events

- [x] Add `GroupFeedCompleted` to `CoalescingEvent`

`modules/rootsignal-scout/src/domains/coalescing/events.rs`:

```rust
GroupFeedCompleted {
    group_id: Uuid,
    signals_added: u32,
    queries_refined: bool,
},
```

- [x] Add `ScheduleCadenceAdjusted` to `SchedulingEvent`

`modules/rootsignal-scout/src/domains/scheduling/events.rs`:

```rust
ScheduleCadenceAdjusted {
    schedule_id: String,
    new_timeout: i32,
    reason: String,
},
```

#### Graph Queries

- [x] Add `get_group_brief(id: Uuid) -> Result<Option<GroupBrief>>` to `GraphQueries` trait

`modules/rootsignal-graph/src/queries.rs` — new trait method. Wraps existing group landscape query with `WHERE g.id = $id` filter.

- [x] Implement in `GraphReader` (`modules/rootsignal-graph/src/reader.rs`)
- [x] Add default (returns `Ok(None)`) in `MockGraphQueries`

#### Coalescer

- [x] Make `feed_single_group` `pub` on `Coalescer`

`modules/rootsignal-scout/src/domains/coalescing/activities/coalescer.rs:332` — change `async fn` to `pub async fn`.

#### Reactor

- [x] New reactor `feed_group` in `modules/rootsignal-scout/src/domains/coalescing/mod.rs`

Triggered by `FeedGroupRequested`. Logic:
1. Fetch `GroupBrief` via `graph.get_group_brief(group_id)`
2. Construct `Coalescer`, call `feed_single_group(&group)`
3. Emit `SignalAddedToGroup` for each fed signal
4. Emit `GroupQueriesRefined` if queries changed
5. Emit `GroupFeedCompleted { group_id, signals_added, queries_refined }`

#### Engine Builder

- [x] Add `build_feed_group_engine()` in `modules/rootsignal-scout/src/core/engine.rs`

Registers: `feed_group` reactor from coalescing + standard projections (runs, system_log, graph). Follows `build_coalesce_engine` pattern.

- [x] Add `build_feed_group_engine()` on `ScoutDeps` in `modules/rootsignal-scout/src/workflows/mod.rs`

#### Runs Projection

- [x] Handle `FeedGroupRequested` in `runs_projection`

`modules/rootsignal-scout/src/core/projection.rs`:

```rust
LifecycleEvent::FeedGroupRequested { run_id, group_id, .. } => {
    // INSERT INTO runs (run_id, region, flow_type, started_at)
    // VALUES ($1, $2, 'group_feed', now())
    // region = group_id.to_string()
}
```

- [x] Handle `parent_run_id` in `ClusterWeaveRequested` projection

#### Runner

- [x] Add `run_feed_group(group_id: Uuid)` to `ScoutRunner`

`modules/rootsignal-api/src/scout_runner.rs` — follows `run_cluster_weave` pattern:
1. Clone deps, generate run_id, compute budget
2. `tokio::spawn` → build feed engine → emit `FeedGroupRequested` → settle
3. Post-settle: check if signals were added (query runs stats or events)
4. If signals added AND group has woven situation → chain `run_cluster_weave(group_id)` with `parent_run_id`

- [x] Add `run_feed_group` variant with `ChainOpts` for scheduled path

#### Concurrency

- [x] Busy check: look for both `group_feed` AND `cluster_weave` active runs for same group_id

`modules/rootsignal-api/src/graphql/mutations.rs` — shared helper or inline in mutation.

### Phase 2: Schedule Backoff

#### Projection Updates

- [x] Handle `ScheduleCadenceAdjusted` in `schedules_projection`

`modules/rootsignal-scout/src/core/projection.rs`:

```sql
UPDATE schedules
SET timeout = $new_timeout,
    next_run_at = now() + make_interval(secs => $new_timeout)
WHERE schedule_id = $id
```

- [x] Handle `ScheduleCreated` with `base_timeout`

Ensure `base_timeout` is written on creation (equal to `timeout`).

#### Backoff Logic in Feed Reactor

After `GroupFeedCompleted`, emit backoff adjustment:

```rust
if signals_added == 0 {
    let schedule_id = format!("group_feed_{}", group_id);
    let new_timeout = (current_timeout * 2).min(MAX_BACKOFF_SECONDS);
    emit ScheduleCadenceAdjusted { schedule_id, new_timeout, reason: "empty feed" }
} else {
    let new_timeout = base_timeout; // reset to base
    emit ScheduleCadenceAdjusted { schedule_id, new_timeout, reason: "signals found" }
}
```

Constants: `BASE_FEED_INTERVAL: i32 = 3600` (1h), `MAX_BACKOFF_SECONDS: i32 = 604_800` (7 days).

#### Auto-Schedule Creation

- [x] In `result_to_events()` (`coalescing/mod.rs`), emit `ScheduleCreated` for each new group

```rust
for group in &result.new_groups {
    events.push(SchedulingEvent::ScheduleCreated {
        schedule_id: format!("group_feed_{}", group.group_id),
        flow_type: "group_feed".into(),
        scope: json!({ "group_id": group.group_id.to_string() }),
        timeout: BASE_FEED_INTERVAL,
        base_timeout: BASE_FEED_INTERVAL,
        recurring: true,
        region_id: None,
    });
}
```

#### Process Schedules

- [x] Add `"group_feed"` arm to `process_schedules()` in `scout_runner.rs`

Parse `group_id` from `scope` JSONB, call `run_feed_group(group_uuid)`.

#### Resume Incomplete Runs

- [x] Add `"group_feed"` arm to `resume_incomplete_runs()` in `scout_runner.rs`

### Phase 3: GraphQL + Frontend

#### GraphQL Mutation

- [x] Add `feed_group(group_id: String!) -> ScoutResult` mutation

`modules/rootsignal-api/src/graphql/mutations.rs` — follows `weave_cluster` pattern:
1. Parse group_id UUID
2. Busy check: no active `group_feed` OR `cluster_weave` for this group
3. Call `runner.run_feed_group(group_uuid).await`
4. Return `ScoutResult { success: true, message: "Feed started" }`

#### Frontend Mutation

- [x] Add `FEED_GROUP` mutation to `modules/admin-app/src/graphql/mutations.ts`

```typescript
export const FEED_GROUP = gql`
  mutation FeedGroup($groupId: String!) {
    feedGroup(groupId: $groupId) {
      success
      message
    }
  }
`;
```

#### Cluster Detail Page

- [x] Add "Feed" button alongside existing "Weave" button

`modules/admin-app/src/pages/ClusterDetailPage.tsx`:

```typescript
const [feedGroup, { loading: feeding }] = useMutation(FEED_GROUP);
// ...
<button onClick={async () => {
  const { data } = await feedGroup({ variables: { groupId: id } });
  // Show message, refetch after delay
}} disabled={feeding}>
  {feeding ? "Feeding..." : "Feed"}
</button>
```

After feed completes, refetch cluster detail to show new members.

## Acceptance Criteria

### Backend
- [x] `feedGroup` mutation triggers a per-group feed run
- [x] Feed runs `feed_single_group()` for the targeted group only
- [x] Successful feed auto-chains a re-weave (if group has a woven situation)
- [x] New groups automatically get a `group_feed` schedule at 1h base interval
- [x] Empty feeds double the schedule interval (capped at 7 days)
- [x] Successful feeds (manual or scheduled) reset interval to base
- [x] Feed and weave are mutually exclusive for the same group (busy check)
- [x] `process_schedules` handles `group_feed` flow type
- [x] `resume_incomplete_runs` handles `group_feed` flow type
- [x] Feed runs appear in the Workflows page with `flow_type = 'group_feed'`

### Frontend
- [x] "Feed" button on Cluster Detail page
- [x] Button disabled while feed is running
- [x] Shows feedback message ("Feed started for group X")
- [x] Member list refreshes after feed

## File Changes

| File | Change |
|------|--------|
| `modules/rootsignal-migrate/migrations/XXX_schedule_backoff.sql` | NEW: rename `cadence_seconds` → `timeout`, add `base_timeout`, `recurring` |
| `modules/rootsignal-scout/src/domains/lifecycle/events.rs` | Add `FeedGroupRequested`, add `parent_run_id` to `ClusterWeaveRequested` |
| `modules/rootsignal-scout/src/domains/coalescing/events.rs` | Add `GroupFeedCompleted` |
| `modules/rootsignal-scout/src/domains/scheduling/events.rs` | Add `ScheduleCadenceAdjusted` |
| `modules/rootsignal-scout/src/domains/coalescing/activities/coalescer.rs` | Make `feed_single_group` pub |
| `modules/rootsignal-scout/src/domains/coalescing/mod.rs` | New `feed_group` reactor, emit `ScheduleCreated` in `result_to_events()` |
| `modules/rootsignal-scout/src/core/engine.rs` | Add `build_feed_group_engine()` |
| `modules/rootsignal-scout/src/workflows/mod.rs` | Add `build_feed_group_engine()` on `ScoutDeps` |
| `modules/rootsignal-scout/src/core/projection.rs` | Handle `FeedGroupRequested` in runs, `ScheduleCadenceAdjusted` in schedules |
| `modules/rootsignal-graph/src/queries.rs` | Add `get_group_brief(id)` to trait |
| `modules/rootsignal-graph/src/reader.rs` | Implement `get_group_brief` |
| `modules/rootsignal-api/src/scout_runner.rs` | Add `run_feed_group()`, `"group_feed"` in `process_schedules` + `resume_incomplete_runs` |
| `modules/rootsignal-api/src/graphql/mutations.rs` | Add `feed_group` mutation |
| `modules/admin-app/src/graphql/mutations.ts` | Add `FEED_GROUP` |
| `modules/admin-app/src/pages/ClusterDetailPage.tsx` | Add Feed button |

## References

- Brainstorm: `docs/brainstorms/2026-03-13-group-gravity-feed-brainstorm.md`
- Closest pattern: `weaveCluster` mutation + `run_cluster_weave()` in `scout_runner.rs`
- Coalescer feed logic: `modules/rootsignal-scout/src/domains/coalescing/activities/coalescer.rs:296-420`
- Chain orchestration: `scout_runner.rs:320-384` (`maybe_chain_coalesce`, `maybe_chain_weave`)
- Schedule schema: `modules/rootsignal-migrate/migrations/043_schedules.sql`
- Unified workflows plan: `docs/plans/2026-03-11-feat-unified-workflows-plan.md`
