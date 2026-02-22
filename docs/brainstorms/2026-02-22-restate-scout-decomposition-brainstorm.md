---
date: 2026-02-22
topic: restate-scout-decomposition
---

# Decompose Scout Pipeline into Restate Durable Workflows

## What We're Building

Decompose rootsignal's monolithic `Scout::run_inner()` into independently invocable Restate durable workflows. The scout pipeline is a 500+ line method orchestrating ~10 sequential phases with manual cancellation, no resumability, and no way to run phases independently. Adding new jobs (actor discovery) keeps making the hand-rolled orchestrator more fragile.

Each phase becomes its own workflow, independently invocable. A thin orchestrator workflow composes them for a full scout run. Follows the same single-binary domain-driven pattern already proven in mntogether.

## Why This Approach

- **Independent invocability** — re-weave situations without re-scraping, run actor discovery on its own, re-run synthesis after code changes
- **Resumability** — if step 7 crashes, resume from step 7 instead of re-running from step 1
- **Proven pattern** — mntogether already runs 15+ services, 8+ virtual objects, 8+ workflows in a single Rust binary using Restate
- **Natural fit** — the phases already communicate through Neo4j, not in-memory state passing

## Key Decisions

- **Pure Rust** — Restate Rust SDK, no TypeScript orchestration layer
- **Single binary** — same as mntogether, Restate endpoint embedded in the API process
- **Workflows live in scout module** — including the full-run orchestrator. API just `.bind()`s them
- **Multiple workflows, not one monolith** — each phase is its own workflow, composable via a thin orchestrator
- **Manual triggers for now** — scheduling (cron/self-rescheduling) deferred to later
- **Neo4j GraphWriter/GraphClient** — goes into a ServerDeps-style container shared across workflows

## Workflow Boundaries

| Workflow | What it does | Independent use case |
|----------|-------------|---------------------|
| BootstrapWorkflow | Cold-start seed queries, platform sources, RSS discovery | New region setup |
| ActorDiscoveryWorkflow | Web search → page fetch → LLM extraction → actor creation | "Find more actors for Portland" |
| ScrapeWorkflow | Phase A (tension) + Phase B (response) + topic discovery | Re-scrape without re-synthesizing |
| SynthesisWorkflow | Similarity edges, response mapping, tension linker, finders | Re-run after algorithm changes |
| SituationWeaverWorkflow | Weave signals into situations, source boost, curiosity triggers | Re-weave after code changes |
| SupervisorWorkflow | Validation pass | Run independently for QA |
| FullScoutRunWorkflow | Orchestrator — calls the above in sequence | The "run everything" option |

## Module Structure

```
modules/rootsignal-scout/src/
├── workflows/
│   ├── mod.rs
│   ├── bootstrap.rs
│   ├── scrape.rs
│   ├── synthesis.rs
│   ├── situation_weaver.rs
│   ├── supervisor.rs
│   ├── actor_discovery.rs
│   └── full_run.rs
├── enrichment/          # existing
├── discovery/           # existing
├── scheduling/          # existing
├── pipeline/            # existing
└── ...
```

Existing non-Restate code (extractors, pipeline, embedder, etc.) stays in place. Workflows wrap existing logic inside `ctx.run()` blocks for durability.

## Open Questions

- Exact shape of the ServerDeps container (what gets shared, what's per-workflow)
- How much state passes between workflows vs. read-from-graph (leaning toward graph)
- Migration strategy — incremental extraction vs. rewrite
- Whether GraphQL mutations (runScout, discoverActors) become thin Restate clients or get absorbed

## Next Steps

→ `/workflows:plan` for implementation details
