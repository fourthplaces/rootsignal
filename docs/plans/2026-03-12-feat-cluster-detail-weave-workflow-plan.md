---
title: Cluster Detail Page and Weave Workflow
type: feat
date: 2026-03-12
---

# Cluster Detail Page and Weave Workflow

## Overview

Build a `/clusters/:id` page that displays a `SignalGroup` node from Neo4j with its member signals, and a "Weave" button that kicks off a cluster-scoped weave workflow to upsert a Situation from the group.

### User Flow

1. Navigate to a coalesce run at `/workflows/:id`
2. Click a group card → navigate to `/clusters/:groupId`
3. See the cluster detail: label, queries, member signals with type/confidence/source
4. Click "Weave" → kicks off a new run scoped to that cluster's group ID
5. Weave workflow reads the group from Neo4j, creates a Situation linked via `WOVEN_INTO`
6. Re-weave detects new signals added to the group since last weave and creates a Dispatch for the delta

## Problem Statement

Coalesced groups exist as `SignalGroup` nodes in Neo4j but have no dedicated view or actionable workflow. The existing weaving code (`situation_weaving/`) does full-region discovery of unassigned signals — we need a lighter path that takes a single group and produces a Situation directly.

## Proposed Solution

### Frontend: Cluster Detail Page

New page at `modules/admin-app/src/pages/ClusterDetailPage.tsx` following `SignalDetailPage.tsx` patterns:

- `useParams<{ id: string }>()` for group ID
- `useQuery(ADMIN_CLUSTER_DETAIL)` to fetch group + members
- Metadata cards: label, signal count, created_at, woven status
- Member signals table (reuse `OutcomeTable` or similar): title, type, confidence, source URL
- "Weave" button triggers `WEAVE_CLUSTER` mutation
- If already woven: show link to the Situation, button text changes to "Re-weave"

### Frontend: Navigation + Routing

- `App.tsx`: add `<Route path="clusters/:id" element={<ClusterDetailPage />} />`
- `ScoutRunDetailPage.tsx` coalesce outcomes: group cards become `<Link to={/clusters/${group.id}}>`

### Backend: GraphQL Query

New query `adminClusterDetail(groupId: String!)` in `schema.rs`:

```graphql
type ClusterDetail {
  id: String!
  label: String!
  queries: [String!]!
  createdAt: String!
  memberCount: Int!
  members: [ClusterMember!]!
  wovenSituationId: String  # null if not yet woven
}

type ClusterMember {
  id: String!
  title: String!
  signalType: String!
  confidence: Float!
  sourceUrl: String
  summary: String
}
```

Implementation: Cypher query against Neo4j reading `SignalGroup` node + `MEMBER_OF` edges + optional `WOVEN_INTO` edge.

### Backend: GraphQL Mutation

New mutation `weaveCluster(groupId: String!)` in `mutations.rs`:

```graphql
mutation WeaveCluster($groupId: String!) {
  weaveCluster(groupId: $groupId) {
    success
    message
  }
}
```

Pattern follows `run_weave()` — calls `runner.run_cluster_weave(group_id)`.

### Backend: Graph Reader Methods

New methods on `GraphQueries` trait in `queries.rs`:

- [ ] `get_cluster_detail(&self, group_id: Uuid) -> Result<Option<ClusterDetail>>` — reads SignalGroup node, MEMBER_OF members, optional WOVEN_INTO→Situation
- [ ] `get_cluster_members(&self, group_id: Uuid) -> Result<Vec<WeaveSignal>>` — reads member signals as `WeaveSignal` structs for the weaver
- [ ] `get_cluster_delta_signals(&self, group_id: Uuid) -> Result<Vec<WeaveSignal>>` — signals added to the group after the last weave

Delta detection Cypher (signals with MEMBER_OF created after the WOVEN_INTO edge):
```cypher
MATCH (g:SignalGroup {id: $group_id})-[w:WOVEN_INTO]->(sit:Situation)
MATCH (sig)-[m:MEMBER_OF]->(g)
WHERE m.added_at > w.woven_at
RETURN sig
```

### Backend: Cluster Weave Workflow

New module `modules/rootsignal-scout/src/domains/cluster_weaving/` (or a new activity in `situation_weaving/`):

**Relationship model:** Signals belong to groups, groups weave into situations. No direct signal→situation edges. The graph path is `(Signal)-[:MEMBER_OF]->(SignalGroup)-[:WOVEN_INTO]->(Situation)`. Situations and dispatches *pull* from the group's signals to build narrative, but don't own them.

**First weave (no WOVEN_INTO edge exists):**
1. Read group + all member signals from Neo4j via `get_cluster_members()`
2. Lightweight LLM call: given group label + member signal summaries → generate headline, lede, structured_state
3. Emit `SituationIdentified` event (reuse existing event type)
4. Emit `GroupWovenIntoSituation` event (new) → projector creates `WOVEN_INTO` edge with `woven_at` timestamp

**Re-weave (WOVEN_INTO edge exists):**
1. Read delta signals via `get_cluster_delta_signals()` (members added after `woven_at`)
2. If no delta → return early, report "no new signals"
3. Lightweight LLM call: generate dispatch body from delta signals
4. Emit `DispatchCreated` for the delta (references signal IDs for context, no graph edges)
5. Update `woven_at` timestamp on WOVEN_INTO edge

**Post-weave (reuse from existing weaving):**
- Temperature recomputation (`compute_situation_temperature`)
- Dispatch verification (`unverified_dispatches` check)
- Source boost
- Curiosity triggers

### Backend: New Event + Projection

New system event in `system_events.rs`:

```rust
GroupWovenIntoSituation {
    group_id: Uuid,
    situation_id: Uuid,
}
```

Projector in `projector.rs`:

```cypher
MATCH (g:SignalGroup {id: $group_id})
MATCH (s:Situation {id: $situation_id})
MERGE (g)-[w:WOVEN_INTO]->(s)
ON CREATE SET w.woven_at = datetime($ts)
ON MATCH SET w.woven_at = datetime($ts)
```

### Backend: Engine + Runner Wiring

- New `build_cluster_weave_engine()` in `engine.rs` — registers cluster_weaving reactor
- New entry event: `ClusterWeaveRequested { group_id: Uuid }` (pipeline event)
- New `runner.run_cluster_weave(group_id)` method
- `scout_runs` row created with task_id = "cluster_weave", scope includes group_id

## Acceptance Criteria

### Frontend
- [x] `/clusters/:id` route renders cluster detail page
- [x] Page shows group label, queries, created_at, member count
- [x] Member signals listed with title, type, confidence, source URL
- [x] "Weave" button triggers mutation and shows loading state
- [x] After weave: shows link to created Situation
- [x] Re-weave shows "Re-weave" button text when WOVEN_INTO exists
- [x] Coalesce run group cards link to `/clusters/:groupId`
- [x] Breadcrumb navigation back to workflows

### Backend
- [x] `adminClusterDetail` query returns group + members + woven status
- [x] `weaveCluster` mutation triggers cluster weave run
- [x] First weave creates Situation + WOVEN_INTO edge (no direct signal→situation edges)
- [x] Re-weave detects delta signals (added after last woven_at) and creates Dispatch
- [x] `GroupWovenIntoSituation` event projected as `WOVEN_INTO` edge in Neo4j
- [x] Temperature recomputation runs after weave
- [x] Handles missing group (returns error, not panic)

## Technical Considerations

### What to Reuse from Existing Weaving

| Keep | Drop |
|------|------|
| `SituationIdentified` event + projection | `discover_unassigned_signals()` (replaced by group membership) |
| `DispatchCreated` event + projection | `SignalAssignedToSituation` (signals belong to groups, not situations) |
| Temperature recomputation | Heavy LLM weaving prompt (replace with lightweight headline/lede gen) |
| Dispatch verification | Batch processing with `temp_id_map` |
| Source boost | Full-region signal discovery |
| Curiosity triggers | `PART_OF` edges (replaced by `MEMBER_OF→WOVEN_INTO` traversal) |
| `WeaveSignal` type | `WeaveCandidate` type (no candidate matching needed) |

### LLM Prompt (Lightweight)

Instead of the full weaving prompt that discovers relationships between signals and situations, the cluster weave prompt just needs:
- Input: group label, list of member signal titles + summaries
- Output: headline (one sentence), lede (2-3 sentences), structured_state (JSON)
- Much cheaper/faster than full weave

### Concurrency

- Check `is_region_busy` equivalent for cluster weaves — probably use group_id as the busy key
- Or simpler: just check if a cluster_weave run is already in progress for this group_id in `scout_runs`

## File Changes

### New Files
- `modules/admin-app/src/pages/ClusterDetailPage.tsx`
- `modules/rootsignal-scout/src/domains/cluster_weaving/mod.rs`
- `modules/rootsignal-scout/src/domains/cluster_weaving/activities.rs`

### Modified Files
- `modules/admin-app/src/App.tsx` — add `/clusters/:id` route
- `modules/admin-app/src/graphql/queries.ts` — add `ADMIN_CLUSTER_DETAIL` query
- `modules/admin-app/src/graphql/mutations.ts` — add `WEAVE_CLUSTER` mutation
- `modules/admin-app/src/pages/ScoutRunDetailPage.tsx` — group cards become links
- `modules/rootsignal-api/src/graphql/schema.rs` — add `adminClusterDetail` query + types
- `modules/rootsignal-api/src/graphql/mutations.rs` — add `weaveCluster` mutation
- `modules/rootsignal-common/src/system_events.rs` — add `GroupWovenIntoSituation` + `ClusterWeaveRequested`
- `modules/rootsignal-graph/src/projector.rs` — project `GroupWovenIntoSituation` → WOVEN_INTO edge
- `modules/rootsignal-graph/src/queries.rs` — add cluster query trait methods
- `modules/rootsignal-graph/src/writer.rs` — implement cluster query methods
- `modules/rootsignal-graph/src/reader.rs` — implement public cluster detail reader
- `modules/rootsignal-scout/src/core/engine.rs` — add `build_cluster_weave_engine()`
- `modules/rootsignal-scout/src/workflows/mod.rs` — add `run_cluster_weave()`
- `modules/rootsignal-scout/src/domains/mod.rs` — register `cluster_weaving` module

## Implementation Phases

### Phase 1: Backend Data Access
- [x] Add `get_cluster_detail()` and `get_cluster_members()` to `GraphQueries` trait + `GraphReader` impl
- [x] Add `get_cluster_delta_signals()` to `GraphQueries` trait + `GraphReader` impl
- [x] Add `adminClusterDetail` GraphQL query + types in `schema.rs`
- [x] Add `GroupWovenIntoSituation` event to `system_events.rs`
- [x] Add WOVEN_INTO projection to `projector.rs`

### Phase 2: Cluster Weave Workflow
- [x] Create `cluster_weaving` domain module with activities
- [x] Implement first-weave path: group → LLM → SituationIdentified + GroupWovenIntoSituation
- [x] Implement re-weave path: delta detection → DispatchCreated
- [x] Wire post-weave: temperature recomputation on re-weave
- [x] Add `ClusterWeaveRequested` pipeline event
- [x] Add `build_cluster_weave_engine()` in `engine.rs`
- [x] Add `run_cluster_weave()` in `workflows/mod.rs`
- [x] Add `weaveCluster` GraphQL mutation

### Phase 3: Frontend
- [x] Create `ClusterDetailPage.tsx` with group metadata + member signals table
- [x] Add `ADMIN_CLUSTER_DETAIL` query and `WEAVE_CLUSTER` mutation to graphql files
- [x] Add `/clusters/:id` route in `App.tsx`
- [x] Make coalesce group cards linkable in `ScoutRunDetailPage.tsx`
- [x] Add Weave/Re-weave button with loading state + success feedback
- [x] Add breadcrumb navigation

## References

- Detail page pattern: `modules/admin-app/src/pages/SignalDetailPage.tsx`
- Mutation pattern: `modules/rootsignal-api/src/graphql/mutations.rs:394` (`run_weave`)
- Query pattern: `modules/rootsignal-api/src/graphql/schema.rs:897` (`adminCoalesceRunOutcomes`)
- Group projection: `modules/rootsignal-graph/src/projector.rs:3042` (`GroupCreated`)
- Existing weaving: `modules/rootsignal-scout/src/domains/situation_weaving/activities/mod.rs`
- GraphQueries trait: `modules/rootsignal-graph/src/queries.rs:65`
- System events: `modules/rootsignal-common/src/system_events.rs:270` (`SituationIdentified`)
- WeaveSignal type: `modules/rootsignal-graph/src/writer.rs:3326`
