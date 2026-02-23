---
title: "feat: Admin Individual Scout Phase Runner"
type: feat
date: 2026-02-22
---

# Admin: Run Individual Scout Workflow Phases

## Overview

Add the ability to run any individual scout workflow phase against a region from the admin UI's Tasks tab. Each phase is already a standalone Restate workflow — the gap is in the API + UI layer. This enables debugging and validating changes to specific phases without running the full pipeline.

## Problem Statement / Motivation

When iterating on a single phase (e.g. updating SituationWeaver logic), re-running the entire 6-phase pipeline is wasteful and slow. The underlying Restate infrastructure already supports standalone phase invocation — we just need to expose it through the API and admin UI.

## Proposed Solution

Add a phase dropdown to the existing "Run" button on the Tasks tab. The dropdown defaults to "Full Run" (preserving current behavior) and lists each individual phase. A new `runScoutPhase` GraphQL mutation dispatches individual phases. The binary `ScoutLock` Neo4j node is replaced with a `RegionScoutRun` status node that tracks the last completed phase per region, gating which phases are available in the UI.

## Architecture Decisions

### 1. Status write-back: workflows update Neo4j on completion

The spec-flow analysis identified a critical gap: Restate workflows complete asynchronously, so the API server cannot update the status node after dispatch. **Each workflow must write to the `RegionScoutRun` Neo4j node on completion.** This is a small change — a single `GraphWriter` call at the end of each workflow's `run()` method.

The `FullScoutRunWorkflow` orchestrator updates status as it progresses through phases (it already sets Restate state per phase — we add a parallel Neo4j write).

### 2. Restate keying: use `{region}-{timestamp}` for individual runs

Restate workflows keyed by `region_key` can only have one active invocation per key. Running `BootstrapWorkflow/austin-tx/run` individually, then a Full Run (which calls `BootstrapWorkflow/austin-tx/run` internally), would collide.

**Individual phase runs use `{region_key}-{unix_timestamp}` as the Restate key** to avoid collisions. Full Runs continue using the bare `region_key` since the orchestrator internally manages sub-workflow keys.

### 3. Fresh regions: Full Run is always available

The dropdown always enables "Full Run" regardless of region status. For individual phases on a fresh region, only Bootstrap is enabled. This avoids a regression from the current behavior.

### 4. Budget: `spent_cents` defaults to 0

Standalone phase runs are for debugging, not production budget accounting. Synthesis and SituationWeaver receive `BudgetedRegionRequest { scope, spent_cents: 0 }`.

### 5. Stale status cleanup: reset to `idle`

If a `running_*` status is older than 30 minutes (matching current lock timeout), reset to `idle`. This matches the current behavior of deleting the `ScoutLock` node and is the simplest approach.

### 6. No polling for MVP

The UI shows the `RegionScoutRun` status from Neo4j, refreshed on page load and after mutation. No real-time polling of Restate `get_status()` for this iteration.

## Implementation Plan

### Phase 1: Neo4j — Replace ScoutLock with RegionScoutRun

**Files:**
- `modules/rootsignal-graph/src/writer.rs`

**Changes:**

Replace the three `ScoutLock` methods with `RegionScoutRun` equivalents:

```rust
// New node: RegionScoutRun {region, status, started_at, updated_at}
//
// status values: "idle", "running_bootstrap", "bootstrap_complete",
//   "running_actor_discovery", "actor_discovery_complete",
//   "running_scrape", "scrape_complete", "running_synthesis",
//   "synthesis_complete", "running_situation_weaver",
//   "situation_weaver_complete", "running_supervisor",
//   "supervisor_complete", "complete"

/// Atomically transition the region status. Returns false if the current status
/// is not in the allowed set (acts as a lock — rejects if already running).
pub async fn transition_region_status(
    &self,
    region: &str,
    allowed_from: &[&str],
    new_status: &str,
) -> Result<bool, neo4rs::Error>

/// Read the current region run status. Returns "idle" if no node exists.
pub async fn get_region_run_status(
    &self,
    region: &str,
) -> Result<String, neo4rs::Error>

/// Reset a stuck region status to "idle". Replaces release_scout_lock.
pub async fn reset_region_run_status(
    &self,
    region: &str,
) -> Result<(), neo4rs::Error>

/// Clean up stale running statuses (>30 min). Called periodically or on read.
pub async fn cleanup_stale_region_statuses(&self) -> Result<(), neo4rs::Error>
```

`transition_region_status` uses an atomic Cypher query (like the current `acquire_scout_lock`) to prevent TOCTOU races:

```cypher
MERGE (r:RegionScoutRun {region: $region})
ON CREATE SET r.status = "idle", r.started_at = datetime(), r.updated_at = datetime()
WITH r
WHERE r.status IN $allowed_from
SET r.status = $new_status, r.updated_at = datetime()
RETURN r.status = $new_status AS transitioned
```

Also update `is_scout_running` to read from `RegionScoutRun` (status starts with `running_`).

Remove: `acquire_scout_lock`, `release_scout_lock`, and the `ScoutLock` Cypher queries.

### Phase 2: Scout workflows — Write status to Neo4j on completion

**Files:**
- `modules/rootsignal-scout/src/workflows/bootstrap.rs`
- `modules/rootsignal-scout/src/workflows/actor_discovery.rs`
- `modules/rootsignal-scout/src/workflows/scrape.rs`
- `modules/rootsignal-scout/src/workflows/synthesis.rs`
- `modules/rootsignal-scout/src/workflows/situation_weaver.rs`
- `modules/rootsignal-scout/src/workflows/supervisor.rs`
- `modules/rootsignal-scout/src/workflows/full_run.rs`
- `modules/rootsignal-scout/src/workflows/mod.rs`

**Changes:**

Add a helper to `mod.rs`:

```rust
/// Write phase completion status to Neo4j. Called at the end of each workflow.
pub async fn write_phase_complete(deps: &ScoutDeps, region: &str, status: &str) {
    let writer = GraphWriter::new(deps.graph_client.clone());
    if let Err(e) = writer.set_region_run_status(region, status).await {
        tracing::warn!(%e, "Failed to write phase status to graph");
    }
}
```

At the end of each workflow's `run()`, after the work completes:

```rust
// e.g. in bootstrap.rs, after spawning the bootstrapper:
super::write_phase_complete(&self.deps, &region_key, "bootstrap_complete").await;
```

For `FullScoutRunWorkflow`, update the Neo4j status as it transitions between phases (alongside the existing Restate `ctx.set("status", ...)` calls):

```rust
// Before each phase:
super::write_phase_status(&self.deps, &region_key, "running_bootstrap").await;
// After each phase:
super::write_phase_complete(&self.deps, &region_key, "bootstrap_complete").await;
```

Also update the `Scout::run()` method in `scout.rs` to use `transition_region_status` instead of `acquire_scout_lock`/`release_scout_lock` (if the non-Restate path is still used).

### Phase 3: RestateClient — Add `run_phase` method

**Files:**
- `modules/rootsignal-api/src/restate_client.rs`

**Changes:**

Add a `ScoutPhase` enum (or reuse from a shared crate) and a `run_phase` method:

```rust
/// Dispatch an individual scout workflow phase via Restate ingress.
pub async fn run_phase(
    &self,
    phase: ScoutPhase,
    slug: &str,
    scope: &ScoutScope,
) -> Result<(), RestateError> {
    let workflow_name = phase.workflow_name();
    // Use timestamped key for individual runs to avoid collision with Full Run
    let key = format!("{slug}-{}", chrono::Utc::now().timestamp());
    let url = format!("{}/{workflow_name}/{key}/run", self.ingress_url);

    let body = match phase {
        ScoutPhase::Synthesis | ScoutPhase::SituationWeaver => {
            serde_json::json!({ "scope": scope, "spent_cents": 0u64 })
        }
        _ => serde_json::json!({ "scope": scope }),
    };

    let resp = self.http.post(&url).json(&body).send().await?;
    // ...
}
```

Where `ScoutPhase::workflow_name()` maps:
| Phase | Workflow Name |
|---|---|
| Bootstrap | `BootstrapWorkflow` |
| ActorDiscovery | `ActorDiscoveryWorkflow` |
| Scrape | `ScrapeWorkflow` |
| Synthesis | `SynthesisWorkflow` |
| SituationWeaver | `SituationWeaverWorkflow` |
| Supervisor | `SupervisorWorkflow` |

### Phase 4: GraphQL — Add `runScoutPhase` mutation and `ScoutPhase` enum

**Files:**
- `modules/rootsignal-api/src/graphql/types.rs` — new `ScoutPhase` enum
- `modules/rootsignal-api/src/graphql/mutations.rs` — new `run_scout_phase` mutation
- `modules/rootsignal-api/src/graphql/schema.rs` — update `RegionScoutStatus` to include `phase_status: String`

**New enum** (in `types.rs`, following existing pattern):

```rust
#[derive(async_graphql::Enum, Copy, Clone, Eq, PartialEq)]
pub enum ScoutPhase {
    Bootstrap,
    ActorDiscovery,
    Scrape,
    Synthesis,
    SituationWeaver,
    Supervisor,
}
```

**New mutation** (in `mutations.rs`):

```rust
#[graphql(guard = "AdminGuard")]
async fn run_scout_phase(
    &self,
    ctx: &Context<'_>,
    phase: ScoutPhase,
    query: String,
) -> Result<ScoutResult> {
    // Same API key checks as run_scout
    // Geocode query → ScoutScope
    // Check region status prerequisites via writer.get_region_run_status()
    // Transition status to running_* via writer.transition_region_status()
    // Dispatch via restate.run_phase(phase, &slug, &scope)
    // Return ScoutResult { success, message }
}
```

**Phase prerequisite check:**

```rust
fn phase_allowed(current_status: &str, target_phase: ScoutPhase) -> bool {
    match target_phase {
        ScoutPhase::Bootstrap => !current_status.starts_with("running_"),
        ScoutPhase::ActorDiscovery => matches!(current_status,
            "bootstrap_complete" | "actor_discovery_complete" | "scrape_complete"
            | "synthesis_complete" | "situation_weaver_complete" | "complete"
        ),
        ScoutPhase::Scrape => matches!(current_status,
            "actor_discovery_complete" | "scrape_complete" | "synthesis_complete"
            | "situation_weaver_complete" | "complete"
        ),
        ScoutPhase::Synthesis => matches!(current_status,
            "scrape_complete" | "synthesis_complete"
            | "situation_weaver_complete" | "complete"
        ),
        ScoutPhase::SituationWeaver => matches!(current_status,
            "synthesis_complete" | "situation_weaver_complete" | "complete"
        ),
        ScoutPhase::Supervisor => matches!(current_status,
            "situation_weaver_complete" | "complete"
        ),
    }
}
```

**Update `RegionScoutStatus`** (in `schema.rs`):

```rust
pub struct RegionScoutStatus {
    pub region_name: String,
    pub region_slug: String,
    pub last_scouted: Option<DateTime<Utc>>,
    pub sources_due: u32,
    pub running: bool,
    pub phase_status: String,  // NEW — "idle", "bootstrap_complete", "running_scrape", etc.
}
```

**Update `reset_scout_lock` mutation** to call `writer.reset_region_run_status()` instead of `writer.release_scout_lock()`. Rename to `reset_scout_status` (keep old name as alias for backwards compat if needed).

### Phase 5: Admin UI — Phase dropdown on Tasks tab

**Files:**
- `modules/admin-app/src/graphql/mutations.ts` — new `RUN_SCOUT_PHASE` mutation
- `modules/admin-app/src/graphql/queries.ts` — update dashboard query for `phaseStatus`
- `modules/admin-app/src/pages/ScoutPage.tsx` — phase dropdown + disabled states

**New mutation** (in `mutations.ts`):

```ts
export const RUN_SCOUT_PHASE = gql`
  mutation RunScoutPhase($phase: ScoutPhase!, $query: String!) {
    runScoutPhase(phase: $phase, query: $query) { success message }
  }
`;
```

**UI changes** (in `ScoutPage.tsx`):

Replace the bare "Run" button with a split button / dropdown:

```tsx
// Per-task row, replacing the current Run button:
<div className="flex gap-1">
  <select
    value={selectedPhase[t.id] || "FULL_RUN"}
    onChange={(e) => setSelectedPhase({ ...selectedPhase, [t.id]: e.target.value })}
    className="text-xs px-1 py-1 rounded border border-border bg-background text-muted-foreground"
  >
    <option value="FULL_RUN">Full Run</option>
    <option value="BOOTSTRAP" disabled={!phaseEnabled("BOOTSTRAP", regionStatus)}>Bootstrap</option>
    <option value="ACTOR_DISCOVERY" disabled={!phaseEnabled("ACTOR_DISCOVERY", regionStatus)}>Actor Discovery</option>
    <option value="SCRAPE" disabled={!phaseEnabled("SCRAPE", regionStatus)}>Scrape</option>
    <option value="SYNTHESIS" disabled={!phaseEnabled("SYNTHESIS", regionStatus)}>Synthesis</option>
    <option value="SITUATION_WEAVER" disabled={!phaseEnabled("SITUATION_WEAVER", regionStatus)}>Situation Weaver</option>
    <option value="SUPERVISOR" disabled={!phaseEnabled("SUPERVISOR", regionStatus)}>Supervisor</option>
  </select>
  <button onClick={() => handleRunPhase(t.context, selectedPhase[t.id] || "FULL_RUN")} ...>
    Run
  </button>
</div>
```

The `handleRunPhase` function calls `runScoutPhase` for individual phases or `runScout` for Full Run.

The `phaseEnabled` function mirrors the backend `phase_allowed` logic — checking the region's `phaseStatus` from the dashboard query.

Add a confirmation dialog when re-running an earlier phase: _"Re-running Bootstrap will reset phase progress. Downstream phases will need to be re-run. Continue?"_

**Update "Reset Lock" button** label to "Reset Status" and have it call the renamed mutation.

Show the current `phaseStatus` as a badge next to each task (or in a new column), so the admin can see at a glance what state the region is in.

## Acceptance Criteria

- [x] Admin can select an individual phase from a dropdown and run it against a region
- [x] "Full Run" remains the default and works as before
- [x] Phases are disabled in the dropdown when prerequisites haven't been met
- [x] Re-running an earlier phase resets status, disabling downstream phases
- [x] Current phase status is visible per-region in the Tasks tab
- [x] Stuck `running_*` statuses are cleaned up after 30 minutes
- [x] "Reset Status" button clears a stuck status to `idle`
- [x] Binary `ScoutLock` is fully replaced by `RegionScoutRun` status node

## Dependencies & Risks

- **Risk: Restate key collision.** Mitigated by using timestamped keys for individual phase runs. Need to verify Restate allows this pattern (different keys invoking the same workflow type concurrently).
- **Risk: Status write-back failure.** If the Neo4j write at the end of a workflow fails, the status will be stuck in `running_*`. Mitigated by the 30-minute stale cleanup + Reset Status button.
- **Risk: Non-Restate `Scout::run()` path.** If this path is still used, it needs to be updated to use `RegionScoutRun`. Verify whether it's still active or can be removed.

## References

- Brainstorm: `docs/brainstorms/2026-02-22-admin-individual-phase-runner-brainstorm.md`
- Current mutations: `modules/rootsignal-api/src/graphql/mutations.rs:258-344`
- RestateClient: `modules/rootsignal-api/src/restate_client.rs`
- ScoutLock (Neo4j): `modules/rootsignal-graph/src/writer.rs:1012-1063`
- Workflow types: `modules/rootsignal-scout/src/workflows/types.rs`
- Full run orchestrator: `modules/rootsignal-scout/src/workflows/full_run.rs`
- GraphQL enums pattern: `modules/rootsignal-api/src/graphql/types.rs:23-31`
- ScoutPage Tasks tab: `modules/admin-app/src/pages/ScoutPage.tsx:307-429`
