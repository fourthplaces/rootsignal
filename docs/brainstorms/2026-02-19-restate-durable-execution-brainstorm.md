---
date: 2026-02-19
topic: restate-durable-execution
status: parked
revisit_when: multi-city scaling, long-running scout failures, or human-in-the-loop needs
---

# Restate for Durable Execution

## The Idea

Use [Restate](https://restate.dev/) as a durable execution engine for the scout pipeline. Restate journals every step of a workflow so that if a process crashes mid-run, it replays completed steps from the journal and resumes exactly where it left off.

## Why It's Appealing

The scout pipeline is a multi-step workflow (scrape → extract → dedup → embed → write → link → investigate → cluster) with many LLM calls and external API hits. Restate maps naturally:

| Current Pattern | Restate Equivalent |
|---|---|
| `Scout::run()` orchestrating phases A → B → post-scrape | A **workflow** with durable steps |
| Per-source scrape → extract → dedup → embed → write | A **durable handler** per source, journaled |
| Agentic LLM loops (TensionLinker, Investigator, etc.) | Each tool call journaled — crash on call 4 of 5, replay 1-3, retry 4 |
| `SourceScheduler` cadence logic | Restate **delayed calls** / virtual object timers |
| `BudgetTracker` atomic counters | Restate **virtual object** with keyed state |
| GraphQL `triggerScout` mutation | Restate **RPC invocation** with dedup key |

## Why Not Yet (as of Feb 2026)

- Scout runs aren't failing — reliability isn't an acute problem
- Running one city (Twin Cities) — scale doesn't demand distribution
- LLM waste from failures is negligible or unnoticed
- Rust SDK is less mature than TypeScript/Java SDKs
- Adds operational overhead (another service + persistent volume on Railway)
- Introduces a second state store alongside Neo4j — need to reason about what lives where
- Significant refactor of the scout pipeline to split into Restate handlers

## Cheaper Alternatives for Now

| Motivation | Alternative |
|---|---|
| **Reliability** | Checkpoint sources as `processing`/`done` in Neo4j per run. Skip completed on restart. ~50 lines. |
| **Scale** | Single-process Tokio handles one city fine. Revisit at city 3+. |
| **Observability** | Structured tracing spans per source and per pipeline step. Needed regardless. |
| **Architecture** | Extract pipeline steps into a trait-based chain. Cleaner `Scout::run()`, no new infra. |

## Revisit When

- Running **5+ cities** and need to fan out across workers
- Scout runs take **hours** and mid-run failures waste real money
- Want **independent triggers** ("investigate this tension now" as a standalone call)
- Adding **human-in-the-loop** steps (editorial review before publish) — Restate's suspendable workflows shine here
- Rust SDK has matured (check https://github.com/restatedev/sdk-rust)

## Resources

- https://restate.dev/
- https://docs.restate.dev/concepts/durable_execution/
- https://github.com/restatedev/restate
- https://restate.dev/blog/durable-ai-loops-fault-tolerance-across-frameworks-and-without-handcuffs
