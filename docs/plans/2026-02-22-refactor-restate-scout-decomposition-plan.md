---
title: Decompose Scout Pipeline into Restate Durable Workflows
type: refactor
date: 2026-02-22
---

# Decompose Scout Pipeline into Restate Durable Workflows

## Overview

Decompose `Scout::run_inner()` — a 500+ line monolith orchestrating 10 sequential phases — into independently invocable Restate durable workflows. Each phase becomes its own workflow, composable via a thin orchestrator. Follows the proven single-binary Restate pattern from mntogether.

## Problem Statement

The scout pipeline has three compounding problems:

1. **No independent invocability.** Re-weaving situations requires running the entire pipeline (reap → bootstrap → scrape → synthesize → weave). Adding actor discovery as a new job adds more complexity to an already-creaky orchestrator.

2. **No resumability.** If step 7 (synthesis) crashes, everything restarts from step 1. Expensive LLM calls and web scrapes are re-executed.

3. **Hand-rolled orchestration.** Manual Neo4j locks, manual task queue (ScoutTask nodes), `std::thread::spawn` + `Runtime::new()`, `AtomicBool` cancellation — all code that Restate provides for free.

## Proposed Solution

Seven Restate workflows in the scout module, a `ScoutDeps` dependency container, and a phased migration that keeps existing paths working.

### Workflow Boundaries

| Workflow | Scope | Independent Use Case |
|---|---|---|
| `BootstrapWorkflow` | Cold-start seed queries, platform sources, RSS/subreddit discovery | New region setup |
| `ActorDiscoveryWorkflow` | Web search → page fetch → LLM extraction → actor creation | "Discover actors for Portland" |
| `ScrapeWorkflow` | Phase A + mid-run discovery + Phase B + topic discovery + expansion | Re-scrape without synthesis |
| `SynthesisWorkflow` | Similarity edges, response mapping, tension linker, response finder, gathering finder, investigation (6 parallel tasks) | Re-run after algorithm changes |
| `SituationWeaverWorkflow` | Weave signals into situations, source boost, curiosity triggers | Re-weave after code changes |
| `SupervisorWorkflow` | Validation pass + merge duplicate tensions + cause heat | QA pass |
| `FullScoutRunWorkflow` | Orchestrator calling the above in sequence | The "run everything" option |

**Why ScrapeWorkflow is one unit:** RunContext contains an EmbeddingCache (megabytes of f32 vectors), URL→canonical_key maps, expansion queries, and signal counts that flow between Phase A and Phase B. Serializing this between separate workflow invocations is impractical. Mid-run discovery runs between phases and feeds Phase B's source list. Keeping them together avoids the serialization boundary entirely.

**Why SituationWeaver is separate from Synthesis:** Situation weaving depends on SIMILAR_TO edges built during synthesis. The ordering constraint is enforced by `FullScoutRunWorkflow` calling them sequentially. When invoked independently, the weaver operates on whatever edges exist in the graph.

### State Flow Between Workflows

```
FullScoutRunWorkflow
  │
  ├─ BootstrapWorkflow(region) → { sources_created: u32 }
  │
  ├─ ActorDiscoveryWorkflow(region) → { actors_discovered: u32 }
  │
  ├─ ScrapeWorkflow(region) → { stats: ScoutStats, spent_cents: u64 }
  │                              ↑ RunContext lives entirely inside here
  │
  ├─ SynthesisWorkflow(region, spent_cents) → { spent_cents: u64 }
  │                                            ↑ uses budget to gate expensive tasks
  │
  ├─ SituationWeaverWorkflow(region, spent_cents) → { spent_cents: u64 }
  │
  └─ SupervisorWorkflow(region) → { stats: SupervisorStats }
```

Budget flows as a running `spent_cents` total between workflows. Each workflow receives the cumulative spend, adds its own, and outputs the new total. This replaces the in-memory `AtomicU64` for cross-workflow tracking. Within a single workflow (e.g., ScrapeWorkflow), the existing `BudgetTracker` works as-is.

### Module Structure

```
modules/rootsignal-scout/src/
├── workflows/
│   ├── mod.rs                  # re-exports, ScoutDeps, impl_restate_serde macros
│   ├── types.rs                # shared request/response types
│   ├── bootstrap.rs            # BootstrapWorkflow
│   ├── actor_discovery.rs      # ActorDiscoveryWorkflow
│   ├── scrape.rs               # ScrapeWorkflow
│   ├── synthesis.rs            # SynthesisWorkflow
│   ├── situation_weaver.rs     # SituationWeaverWorkflow
│   ├── supervisor.rs           # SupervisorWorkflow
│   └── full_run.rs             # FullScoutRunWorkflow (orchestrator)
├── scheduling/                 # existing — bootstrap.rs, scheduler.rs, etc.
├── pipeline/                   # existing — scrape_phase.rs, extractor.rs, etc.
├── discovery/                  # existing — source_finder.rs, tension_linker.rs, etc.
├── enrichment/                 # existing — expansion.rs, actor_discovery.rs, etc.
└── ...
```

### ScoutDeps Container

Following mntogether's `ServerDeps` pattern:

```rust
#[derive(Clone)]
pub struct ScoutDeps {
    pub graph_client: GraphClient,          // Neo4j (Arc internally)
    pub pg_pool: PgPool,                    // Postgres for archive (Arc internally)
    pub anthropic_api_key: String,
    pub voyage_api_key: String,
    pub serper_api_key: String,
    pub apify_api_key: String,
    pub daily_budget_cents: u64,
    pub region_config: RegionConfig,        // default region for fallback
}
```

Each workflow impl holds `Arc<ScoutDeps>` and constructs per-invocation resources (Archive, Embedder, Extractor) from the deps. This mirrors how mntogether workflows hold `Arc<ServerDeps>` and build transient resources inside `ctx.run()` blocks.

**Key change from current code:** `ScrapePhase<'a>` borrows `&'a GraphWriter`, `&'a dyn SignalExtractor`, `&'a dyn TextEmbedder`. Restate handlers must be `'static`. The deps must become `Arc`-wrapped. `ScrapePhase` will take `Arc<GraphWriter>`, `Arc<dyn SignalExtractor>`, `Arc<dyn TextEmbedder>` — constructed at the start of each workflow invocation from ScoutDeps, then passed by reference into ScrapePhase's methods.

### Workflow Anatomy (Example: BootstrapWorkflow)

```rust
#[restate_sdk::workflow]
#[name = "BootstrapWorkflow"]
pub trait BootstrapWorkflow {
    async fn run(req: BootstrapRequest) -> Result<BootstrapResult, HandlerError>;
    #[shared]
    async fn get_status(req: EmptyRequest) -> Result<String, HandlerError>;
}

pub struct BootstrapWorkflowImpl {
    deps: Arc<ScoutDeps>,
}

impl BootstrapWorkflow for BootstrapWorkflowImpl {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        req: BootstrapRequest,
    ) -> Result<BootstrapResult, HandlerError> {
        let writer = GraphWriter::new(self.deps.graph_client.clone());
        let archive = create_archive(&self.deps).await?;

        ctx.set("status", "Generating seed queries...".to_string());
        let bootstrapper = Bootstrapper::new(
            &writer, archive.clone(), &self.deps.anthropic_api_key, req.scope.clone(),
        );

        let sources_created = ctx.run(|| async {
            bootstrapper.run().await
                .map_err(|e| TerminalError::new(e.to_string()))
        }).await??;

        ctx.set("status", format!("Bootstrap complete: {} sources", sources_created));
        Ok(BootstrapResult { sources_created })
    }
}
```

### Restate Keying & Concurrency

- **Workflow key = region slug.** Restate's single-writer guarantee prevents concurrent runs for the same region.
- **During migration:** Keep Neo4j locks as primary guard. CLI and pre-Restate API paths still use them. Restate-keyed workflows provide an additional layer.
- **Post-migration:** Neo4j locks can be removed once all invocation paths go through Restate.

### Cancellation

- Restate SDK 0.4.0 supports cancellation via the admin API (`DELETE /restate/workflow/{name}/{key}/cancel`).
- The `stopScout` GraphQL mutation maps to this admin API call.
- Within a workflow, cancellation is checked between `ctx.run()` blocks (Restate raises an error if the workflow is cancelled).

### Retryability Classification

| Operation | Retryable? | Strategy |
|---|---|---|
| Neo4j writes (MERGE) | Yes | Idempotent, safe to retry |
| LLM extraction | Yes, but expensive | Wrap in `ctx.run()` so result is journaled; replayed on retry, not re-called |
| Web scrape | Yes | `content_already_processed` hash provides natural idempotency |
| Embedding API | Yes | Wrap in `ctx.run()` to avoid re-billing |
| Nominatim geocode | Yes | Rate-limited externally, retry with backoff |
| Apify social scrape | Yes, with limits | Wrap in `ctx.run()`; expensive, journal the result |
| Archive Postgres writes | Yes | Idempotent upserts |

**Key principle:** Every external call that costs money or time goes inside `ctx.run()`. Restate journals the result; retries replay from journal, not re-execute.

### Post-Run Steps Distribution

| Step | Current Location | New Home |
|---|---|---|
| `merge_duplicate_tensions` | CLI main.rs | SupervisorWorkflow |
| `run_actor_extraction` | CLI main.rs | ScrapeWorkflow (end of run) |
| `compute_cause_heat` | CLI main.rs + API | SupervisorWorkflow |
| `run_supervisor` | API spawn_scout_run | SupervisorWorkflow (already planned) |
| `cache_store.reload` | API spawn_scout_run + interval | FullScoutRunWorkflow (final step) |
| `detect_beacons` | API interval loop | FullScoutRunWorkflow (after supervisor) |

### Out of Scope

- **NewsScanner** — stays as a separate concern, not part of the scout workflow chain. Can become its own Restate workflow later.
- **Scheduling/cron** — manual triggers only for now. Self-rescheduling or external cron added later.
- **Demand aggregation** — stays in interval loop or becomes its own workflow later.
- **RunLog** — kept for domain-specific observability alongside Restate's infrastructure-level journal.

## Implementation Phases

### Phase 1: Foundation (ScoutDeps + Restate SDK)

Add `restate-sdk` dependency. Create `ScoutDeps` container. Create `workflows/mod.rs` with the `impl_restate_serde!` macro (copied from mntogether). Create shared request/response types in `workflows/types.rs`.

Refactor `ScrapePhase<'a>` lifetime parameters: the struct still borrows, but the deps it borrows FROM are now `Arc`-wrapped and owned by the workflow. No functional change to ScrapePhase's API — it still takes `&writer`, `&dyn SignalExtractor`, etc. The change is in who owns the things being borrowed.

Register a minimal Restate endpoint in the API server alongside the existing Axum server.

**Files to create:**
- `modules/rootsignal-scout/src/workflows/mod.rs`
- `modules/rootsignal-scout/src/workflows/types.rs`

**Files to modify:**
- `modules/rootsignal-scout/Cargo.toml` — add `restate-sdk = "0.4"`
- `modules/rootsignal-api/Cargo.toml` — add `restate-sdk = "0.4"`
- `modules/rootsignal-api/src/main.rs` — add Restate endpoint binding
- `modules/rootsignal-scout/src/pipeline/scrape_phase.rs` — deps ownership (if needed)

**Acceptance criteria:**
- [x] Restate endpoint starts alongside Axum server
- [x] ScoutDeps container compiles and holds all necessary deps
- [x] `impl_restate_serde!` macro works for request/response types

### Phase 2: BootstrapWorkflow + ActorDiscoveryWorkflow

Extract the two simplest, most self-contained workflows. These have no RunContext dependency and read/write only through the graph.

BootstrapWorkflow wraps `Bootstrapper::run()`. ActorDiscoveryWorkflow wraps `Bootstrapper::discover_actor_pages()` plus the existing `discoverActors` GraphQL mutation logic.

**Files to create:**
- `modules/rootsignal-scout/src/workflows/bootstrap.rs`
- `modules/rootsignal-scout/src/workflows/actor_discovery.rs`

**Files to modify:**
- `modules/rootsignal-scout/src/workflows/mod.rs` — register new workflows
- `modules/rootsignal-api/src/main.rs` — bind new workflows
- `modules/rootsignal-api/src/graphql/mutations.rs` — optionally update `discoverActors` to call workflow

**Acceptance criteria:**
- [x] `BootstrapWorkflow` can be invoked standalone via Restate for a region
- [x] `ActorDiscoveryWorkflow` can be invoked standalone
- [x] Both show status via `get_status()` shared handler
- [x] Existing `scout.run()` still works unchanged (no regression)

### Phase 3: ScrapeWorkflow

The largest workflow. Encapsulates: reap expired signals, load sources + schedule, Phase A scraping, mid-run discovery, Phase B scraping, topic discovery, signal expansion, source metrics, and end-of-run discovery. RunContext lives entirely within this workflow.

Each major phase becomes a durable step via `ctx.run()`:

```
ctx.run(reap_expired)
ctx.run(load_and_schedule_sources)
ctx.run(phase_a_web_scrape)
ctx.run(phase_a_social_scrape)
ctx.run(mid_run_discovery)
ctx.run(phase_b_web_scrape)
ctx.run(phase_b_social_scrape)
ctx.run(topic_discovery)
ctx.run(expansion)
ctx.run(end_of_run_discovery)
ctx.run(metrics_update)
```

**Note:** The granularity of `ctx.run()` blocks within ScrapeWorkflow is a judgment call. Wrapping each URL scrape individually would give maximum resumability but adds journaling overhead. Wrapping each phase (e.g., all of Phase A as one block) is coarser but simpler. Start with phase-level granularity and refine if needed.

**Files to create:**
- `modules/rootsignal-scout/src/workflows/scrape.rs`

**Files to modify:**
- `modules/rootsignal-scout/src/workflows/mod.rs`
- `modules/rootsignal-api/src/main.rs`

**Acceptance criteria:**
- [x] ScrapeWorkflow produces the same signals as the current `scout.run()` pipeline (`tokio::spawn` escapes Restate's `!Send` context; HRTB issue resolved)
- [x] RunContext (embed_cache, url maps, expansion queries) works within the workflow (lives entirely inside spawned task)
- [x] Budget tracking works within the workflow and outputs `spent_cents`
- [x] Status updates visible via `get_status()` (watch channel bridges spawned task → Restate state)

### Phase 4: SynthesisWorkflow + SituationWeaverWorkflow + SupervisorWorkflow

Three workflows that are already relatively isolated in the current code:

- **SynthesisWorkflow:** Wraps the existing `tokio::join!` block (similarity edges + 5 finders). Receives `spent_cents` to gate expensive tasks.
- **SituationWeaverWorkflow:** Wraps `SituationWeaver::run()` + source boosting + curiosity triggers.
- **SupervisorWorkflow:** Wraps `Supervisor::run()` + `merge_duplicate_tensions` + `compute_cause_heat` + `detect_beacons`.

**Files to create:**
- `modules/rootsignal-scout/src/workflows/synthesis.rs`
- `modules/rootsignal-scout/src/workflows/situation_weaver.rs`
- `modules/rootsignal-scout/src/workflows/supervisor.rs`

**Acceptance criteria:**
- [x] Each workflow invocable independently (all three registered in Restate endpoint; `tokio::spawn` pattern escapes `!Send` context)
- [x] SynthesisWorkflow respects budget gates (`BudgetTracker::new_with_spent` carries prior spend; `has_budget` gates each finder)
- [x] SituationWeaverWorkflow works with whatever edges exist in graph (returns real `SituationWeaverStats`)
- [x] SupervisorWorkflow includes post-run cleanup steps

### Phase 5: FullScoutRunWorkflow + Migration

Create the orchestrator workflow that calls all others in sequence. Update the API's `runScout` mutation to invoke `FullScoutRunWorkflow` instead of `spawn_scout_run`. Update `start_scout_interval` to invoke via Restate.

**Files to create:**
- `modules/rootsignal-scout/src/workflows/full_run.rs`

**Files to modify:**
- `modules/rootsignal-api/src/graphql/mutations.rs` — replace `spawn_scout_run` with Restate invocation
- `modules/rootsignal-api/src/graphql/mutations.rs` — update `start_scout_interval` (or remove in favor of manual triggers)
- `modules/rootsignal-scout/src/main.rs` — CLI can optionally invoke via Restate or keep direct path

**Acceptance criteria:**
- [x] `FullScoutRunWorkflow` runs all phases in sequence
- [x] `runScout` GraphQL mutation triggers the Restate workflow (with fallback to direct spawn)
- [x] Budget flows correctly through the full chain (scrape → synthesis → weaver via `spent_cents`)
- [x] Beacon detection runs after supervisor (moved into SupervisorWorkflow)
- [x] Cache reload runs at the end — N/A for workflow path; `CacheStore` lives in API process memory, handled by hourly background loop (`spawn_reload_loop`) and `spawn_scout_run` fallback
- [x] Neo4j locks — retained for CLI (`Scout::run()`) + admin queries (`is_scout_running`, `resetScoutLock`); API orchestration paths (`spawn_scout_run`, `start_scout_interval`) removed

## Risk Analysis & Mitigation

| Risk | Impact | Mitigation |
|---|---|---|
| Restate Rust SDK 0.4 immaturity | Medium | Already proven in mntogether with similar patterns |
| ScrapeWorkflow too large | Medium | Internally structured with `ctx.run()` checkpoints; can be split later |
| RunContext serialization needed later | Low | Designed so it stays within ScrapeWorkflow; cross-workflow state goes through Neo4j |
| Migration breaks existing CLI | Medium | Phase 1-4 don't touch CLI; Phase 5 keeps CLI as optional direct path |
| Budget tracking drift between workflows | Low | Simple integer passing; easy to verify in logs |

## References

### Internal
- Brainstorm: `docs/brainstorms/2026-02-22-restate-scout-decomposition-brainstorm.md`
- Current pipeline: `modules/rootsignal-scout/src/scout.rs`
- API mutations: `modules/rootsignal-api/src/graphql/mutations.rs`
- ScrapePhase: `modules/rootsignal-scout/src/pipeline/scrape_phase.rs`
- RunContext pattern: `docs/plans/2026-02-20-refactor-scout-pipeline-stages-plan.md`
- Stage boundaries: `docs/brainstorms/2026-02-20-scout-pipeline-stages-brainstorm.md`

### External (mntogether reference)
- ServerDeps: `~/Developer/fourthplaces/mntogether/packages/server/src/kernel/deps.rs`
- Workflow registration: `~/Developer/fourthplaces/mntogether/packages/server/src/bin/server.rs`
- Restate serde macro: `~/Developer/fourthplaces/mntogether/packages/server/src/common/restate_serde.rs`
- Example workflow: `~/Developer/fourthplaces/mntogether/packages/server/src/domains/posts/restate/workflows/extract_posts_from_url.rs`
