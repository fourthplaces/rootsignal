---
date: 2026-02-22
topic: admin-individual-phase-runner
---

# Admin: Run Individual Scout Workflow Phases

## What We're Building

Add the ability to run any individual scout workflow phase against a region from the admin UI's Tasks tab. This enables debugging and validating changes to specific phases without running the entire pipeline.

Each phase is already a standalone Restate workflow — the gap is purely in the API + UI layer.

## Why This Approach

The scout pipeline has 6 phases (Bootstrap → ActorDiscovery → Scrape → Synthesis → SituationWeaver → Supervisor), each registered as an independent Restate workflow. Today the admin can only trigger a full run. When iterating on a single phase (e.g. updating SituationWeaver logic), re-running the entire pipeline is wasteful and slow.

## Key Decisions

- **Phase dropdown on existing Run flow (Tasks tab):** Default option is "Full Run" to preserve current behavior. Individual phases listed below in pipeline order.
- **Replace binary ScoutLock with a status enum:** Tracks last completed phase per region. Serves double duty as both a prerequisite gate and a running/idle indicator. Replaces the current `ScoutLock` Neo4j node + `is_scout_running` bool.
- **Status enum gates phase availability:** Later phases are disabled in the UI until their prerequisites have completed. Earlier phases are always re-runnable. E.g. you can't run SituationWeaver unless Synthesis has completed.
- **`spent_cents` defaults to 0 for standalone runs:** Standalone phase runs are for debugging, not production budget accounting. Synthesis and SituationWeaver receive `BudgetedRegionRequest` with `spent_cents: 0`.
- **No scout-side workflow changes:** The Restate workflows already support standalone invocation. Changes are limited to RestateClient, GraphQL API, graph schema (lock → status), and admin UI.

## Status Enum Design

Replaces the binary `ScoutLock` node with a `RegionScoutRun` node (or similar) storing a phase status enum:

| Stored status | Meaning | Phases enabled in UI |
|---|---|---|
| `idle` / no node | No run yet or last run fully complete | Bootstrap only |
| `running_bootstrap` | Bootstrap in progress | None (running) |
| `bootstrap_complete` | Bootstrap done | Bootstrap, ActorDiscovery |
| `running_actor_discovery` | ActorDiscovery in progress | None (running) |
| `actor_discovery_complete` | ActorDiscovery done | Bootstrap → Scrape |
| `running_scrape` | Scrape in progress | None (running) |
| `scrape_complete` | Scrape done | Bootstrap → Synthesis |
| `running_synthesis` | Synthesis in progress | None (running) |
| `synthesis_complete` | Synthesis done | Bootstrap → SituationWeaver |
| `running_situation_weaver` | SituationWeaver in progress | None (running) |
| `situation_weaver_complete` | SituationWeaver done | Bootstrap → Supervisor |
| `running_supervisor` | Supervisor in progress | None (running) |
| `complete` | Full run done | All phases |

Earlier phases are always re-runnable. A `running_*` status disables all phases (acts as lock).

## Stack of Changes

1. **Graph (Neo4j):** Replace `ScoutLock` node with `RegionScoutRun {region, status, started_at, updated_at}`. Update `acquire_scout_lock` / `release_scout_lock` / `is_scout_running` to read/write status enum instead.
2. **RestateClient:** Add generic `run_phase(phase, slug, scope)` method that routes to the correct workflow endpoint. Update status node before/after dispatch.
3. **GraphQL API:** Add `runScoutPhase(phase: ScoutPhase!, query: String!)` mutation. Add `ScoutPhase` enum to schema. Update `RegionScoutStatus` to expose the status enum (not just `running: bool`).
4. **Admin UI:** Add phase dropdown to the Tasks tab Run flow. Query `RegionScoutStatus` to determine which phases are enabled. Show current phase status.

## Open Questions

- Should re-running an earlier phase (e.g. Bootstrap when status is `scrape_complete`) reset the status back to `bootstrap_complete`? Probably yes — downstream data may now be stale.
- Stale status cleanup: keep the 30-minute timeout from the current lock, or make it configurable?

## Next Steps

→ `/workflows:plan` for implementation details
