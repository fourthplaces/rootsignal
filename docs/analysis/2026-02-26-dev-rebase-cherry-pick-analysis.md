# Dev Rebase Cherry-Pick Analysis

**Date:** 2026-02-26
**Context:** `dev` has been rebased onto `feat/event-sourcing-foundation`. This document identifies all changes from the old `dev` branch (`dev-diverged` @ `84db856`) that need to be selectively applied to the new `dev` (@ `e304c78`).

**Diff summary:** 209 files changed, +24,288 / -21,795

---

## Branch Topology

```
Common ancestor: 91d905c
├── dev (new)        = feat/event-sourcing-foundation (19 commits) → e304c78
└── dev-diverged     = old dev (60 commits)                        → 84db856
```

The two branches diverged from the same ancestor and evolved independently. There are zero shared commits — no cherry-picks or rebases happened between them.

**Files only in dev-diverged (new feature work):** 55 files
**Files only in new dev (event-sourcing foundation):** 43 files

---

## The Core Architectural Divergence

This is the single most important thing to understand before touching anything.

### Foundation built new machinery that dev-diverged never had

The foundation branch created an event-sourcing architecture with new crates and modules:

| New on Foundation | Purpose |
|---|---|
| `rootsignal-events/` crate | Event store (append-only Postgres log) |
| `rootsignal-world/` crate | World event types, value objects |
| `rootsignal-common/src/events.rs` | Unified Event enum with typed variants |
| `rootsignal-common/src/system_events.rs` | SystemDecision enum |
| `rootsignal-common/src/telemetry_events.rs` | TelemetryEvent enum |
| `rootsignal-graph/src/reducer.rs` | GraphProjector — pure event→graph projection |
| `rootsignal-graph/src/pipeline.rs` | Enrichment pipeline orchestrator |
| `rootsignal-graph/src/enrich.rs` | Enrichment passes (diversity, actor stats, cause_heat) |
| `rootsignal-graph/src/embedding_enrichment.rs` | Embedding enrichment pass |
| `rootsignal-graph/src/embedding_store.rs` | Get-or-compute embedding cache |
| `rootsignal-graph/src/synthesizer.rs` | Enrichment synthesizer |
| `rootsignal-scout/src/pipeline/event_sourced_store.rs` | EventSourcedStore — all signal writes go through here |
| `rootsignal-api/migrations/007_unified_events.sql` | Generic `events` table + `embedding_cache` table |

Dev-diverged never had any of these. It diverged before the foundation work began.

### Dev-diverged built features that the foundation never had

| New on Dev-diverged | Purpose |
|---|---|
| `rootsignal-graph/src/severity_inference.rs` | Community alert severity scoring |
| `rootsignal-graph/src/response.rs` | ResponseMapper (relocated from scout) |
| `rootsignal-scout/src/pipeline/signal_lint.rs` | Post-pipeline signal audit |
| `rootsignal-scout/src/pipeline/lint_tools.rs` | LLM tools for lint verdicts |
| `rootsignal-scout/src/enrichment/domain_filter.rs` | Infrastructure URL filtering |
| `rootsignal-scout/src/enrichment/universe_check.rs` | LLM in-universe gating |
| `rootsignal-scout/src/workflows/lint.rs` | SignalLintWorkflow |
| `rootsignal-scout/src/workflows/reaper.rs` | SignalReaper with auto-seeding |
| `rootsignal-scout/src/workflows/scrape_url.rs` | Per-URL scrape workflow |
| 12 new admin-app pages/components | Graph Explorer, Sources, Dangling Signals, etc. |
| 8 new migrations (007–014) | Events table, scrape stats, lint columns, etc. |
| 7 test fixture files | Editorial filtering test data |
| 5 new test files | conversion, firsthand filter, investigation triage, signal lint |

### Both branches independently removed the Story layer

Convergent evolution — both killed `StoryWeaver`, `StoryBrief`, `StoryGrowth`, `StoriesPage.tsx`, `StoryDetailPage.tsx`, `StoryCard.tsx`, `StoryDetail.tsx`. No conflict here; the story layer is dead on both sides.

### The SignalStore Trait: 43 methods vs 102 methods

This is the most dangerous merge point.

**Foundation (43 async methods):** Lean trait shaped around event-sourcing. Writes go through `EventSourcedStore` which emits events and projects to Neo4j.

**Dev-diverged (102 async methods):** Expanded massively with direct graph operations:
- `suppressed_urls`, `cached_domain_verdicts`, `cache_domain_verdicts`, `record_url_scrape` — URL backoff/filtering
- `create_citation` (renamed from `create_evidence`) — citation lifecycle
- `create_schedule`, `link_schedule_to_signal` — schedule nodes
- `set_review_status`, `set_signal_corrected` — review workflow
- `set_in_universe` — universe gating
- `create_evidence_of_edge`, `create_offers_edge`, `create_prefers_edge`, `create_requires_edge`, `create_response_edge` — relationship types
- `find_tension_linker_targets`, `mark_tension_linker_investigated`, `get_tension_landscape`, `get_situation_landscape` — discovery queries
- `upsert_actor`, `find_actor_by_name`, `find_actor_by_entity_id`, `link_actor_to_signal`, `link_actor_to_source`, `get_signals_for_actor`, `list_all_actors`, `update_actor_location` — actor CRUD
- `upsert_source`, `get_active_sources`, `link_signal_to_source`, `link_source_discovered_from` — source CRUD
- `staged_signals_in_region`, `promote_ready_situations`, `promote_ready_stories` — promotion pipeline
- `batch_tag_signals`, `update_signal_fields` — signal mutation
- `find_or_create_resource` — resource lifecycle

**Merge implication:** Every new method from dev-diverged must be added to the foundation's trait AND implemented in `EventSourcedStore` so writes continue to go through the event-sourcing layer. This is not a copy-paste job — each method needs an event type and a projection.

### ResponseMapper Relocation

- Foundation: `modules/rootsignal-scout/src/discovery/response_mapper.rs`
- Dev-diverged: `modules/rootsignal-graph/src/response.rs`

Dev-diverged moved ResponseMapper from scout to graph. Need to decide which location is canonical going forward.

### Graph lib.rs Public API

Must **keep** foundation's exports AND **add** dev-diverged's new exports:

| Keep from Foundation | Add from Dev-diverged |
|---|---|
| `GraphProjector`, `ApplyResult` | `DiscoveryTreeNode` |
| `EmbeddingStore` | `FieldCorrection` |
| `Pipeline`, `BBox`, `PipelineStats` | `SignalBrief` |
| `Synthesizer` | `StagedSignal` |
| `enrich`, `EnrichStats` | `response` module |
| `enrich_embeddings`, `EmbeddingEnrichStats` | `severity_inference` module |
| `StoryWeaver` (may still be needed by foundation) | |

### Cargo.toml: Foundation Dependencies Must Stay

The foundation added workspace dependencies that dev-diverged doesn't have:

```
rootsignal-events = { path = "modules/rootsignal-events" }
rootsignal-world = { path = "modules/rootsignal-world" }
```

These are used by `rootsignal-graph` and `rootsignal-scout`. They MUST remain. Dev-diverged adds:

```
rrule = "0.14"  # RFC 5545 recurrence rules (for ScheduleNode)
```

This must be added. Individual crate Cargo.tomls need the same treatment — keep foundation deps, add dev-diverged's new ones.

---

## Migration Reconciliation

Foundation and dev-diverged have **different 007 migrations** that create **different tables**:

| Branch | Migration 007 | Creates |
|---|---|---|
| Foundation | `007_unified_events.sql` | `events` table (generic event-sourcing) + `embedding_cache` table |
| Dev-diverged | `007_source_fetch_count.sql` | `ALTER TABLE sources ADD COLUMN fetch_count` |

These are not in conflict — they touch different tables. But they can't both be numbered 007.

Dev-diverged's `008_scout_run_events.sql` creates `scout_run_events` — a structured telemetry table (typed columns, not JSONB). This is **different from** foundation's generic `events` table. Both tables serve different purposes and both are needed.

**Recommended migration order on new dev:**

| Number | Content | Origin |
|---|---|---|
| 007 | `events` + `embedding_cache` (unified event log) | Foundation (keep as-is) |
| 008 | `ALTER TABLE sources ADD COLUMN fetch_count` | Dev-diverged's old 007 |
| 009 | `scout_run_events` table + DROP events JSONB from scout_runs | Dev-diverged's old 008 |
| 010 | Drop parent_id FK | Dev-diverged's old 009 |
| 011 | `validation_issues` table | Dev-diverged's old 010 |
| 012 | `url_scrape_stats` | Dev-diverged's old 011 |
| 013 | Lint event columns | Dev-diverged's old 012 |
| 014 | Event summary column | Dev-diverged's old 013 |
| 015 | Node event indexes | Dev-diverged's old 014 |

---

## All 60 Commits from dev-diverged, Categorized

### Category 1: Admin App — SAFE (foundation never touched admin-app)

| Commit | Description |
|---|---|
| `0e6649d` | Graph Explorer with map, filter sidebar, inspector |
| `af43f53` | Live Restate invocation status + stop workflow button |
| `cc74222` | Scout events table + tree view (SourceTrace, event-colors) |
| `58fa6b4` | Schedule display + link previews on signal detail |
| `c28157b` | Stats summary + URL filter on trace tab |
| `8af3bf3` | Dangling signals page |
| `2182387` | Review status badges + filter on signals page |
| `5f6f125` | Purge area operation |
| `64ca610` | Event type filter, simplify task status |
| `5d73014` | Remove run selector dropdown from task trace |
| `0649c7a` | Reload cache after purge area |
| `28e0575` | Memoize schedule variables (fix infinite refetch) |
| `e15f159` | Admin pages, CLI refactor (large — also touches scout, CLI) |

Also includes: `SourceDetailPage.tsx`, `SourcesPage.tsx`, `AdminLayout.tsx` nav changes, `LinkPreview.tsx`, `useLinkPreview.ts`, `ReviewStatusBadge.tsx`, `GraphExplorerPage.tsx`, `DanglingSignalsPage.tsx`, all graph/ components.

### Category 2: Scout Pipeline Features — NEEDS ADAPTATION for event-sourcing

These are feature work but they call `SignalStore` methods that don't exist on the foundation. Each one needs its trait methods wired through `EventSourcedStore`.

| Commit | Description | Trait Impact |
|---|---|---|
| `66f184a` | Signal lint module | `set_review_status`, `set_signal_corrected` |
| `c32bd2e` | Wire SignalLintWorkflow into full run | Adds workflow orchestration |
| `0572328` | Remove batch_review, lint handles promotion | Changes supervisor flow |
| `a598d5e` | SignalReaper workflow with auto-seeding | `reap_expired` changes |
| `6fc8331` | Per-URL backoff with Postgres scrape stats | `suppressed_urls`, `record_url_scrape` |
| `36c5208` | Gate link following with LLM filtering | `set_in_universe`, `cached_domain_verdicts` |
| `2fd4752` | Filter infrastructure URLs + junk extensions | `domain_filter.rs` (new module) |
| `a49fb88` | ScheduleNode for event recurrence | `create_schedule`, `link_schedule_to_signal` |
| `46e0d5e` | 7 code quality fixes across scout (20 files!) | Broad touch |
| `5067a45` | Fire-and-forget event logging | `run_log.rs` perf changes |
| `6accaf8` | Wire real RunLogger through Restate | `run_log.rs`, workflow wiring |
| `b79e484` | Create scout_runs row eagerly for FK | `run_log.rs`, `scrape_pipeline.rs` |
| `cf00c20` | Nest signal events under parents | `run_log.rs`, `scrape_phase.rs` |
| `5ef47b7` | Wire severity inference into synthesis | `synthesis.rs` |
| `24c1a33` | Remove run_id scoping from lint, refine editorial | `pipeline/mod.rs` |
| `73aa3f1` | Logging for outbound link collection | Minor |
| `7f51c62` | Detailed logging for bootstrap discovery | Minor |

### Category 3: Graph Module — HIGH CONFLICT with foundation

| Commit | Description | Conflict Level |
|---|---|---|
| `c18e83a` | Severity inference for community alerts | **New file** — clean |
| `dcf03e5` | Review status fields in NodeMeta + GraphQL | Modifies shared writer types |
| `2185b52` | Shorter Notice TTL + evidence-boosted cause heat | Modifies `cause_heat.rs` |
| `c22112c` | Thread content_date through weavers | Modifies `situation_weaver.rs` |
| `2aa5175` | EVIDENCE_OF edge, tension linker relationships | Modifies `tension_linker.rs` |
| `9667f22` | Exclude unproductive sources from region loading | Modifies `reader.rs` |
| `50ed12f` | Rename quarantined→rejected, persist correction | Shared types |
| `8c1b104` | Remove dead StoryWeaver code | Already dead on both — no-op |

### Category 4: API / GraphQL — MODERATE CONFLICT

| Commit | Description |
|---|---|
| `a1838ee` | Review status resolvers for all signal types |
| `dbdff58` | Rename evidence → citations in GraphQL + frontend |
| `4a8b3a9` | GraphQL depth/complexity limits, mask phone |
| `4418343` | Request JSON from Restate admin API |
| `397d678` | Enable gzip decompression for Restate admin |
| `9a86641` | Request uncompressed responses from Restate admin |
| `40d3add` | Handle unknown Restate response shape |
| `ebccb56` | Add missing tracing::warn import |
| `6d80df4` | Move ValidationIssues from Neo4j to Postgres |

### Category 5: Data Model Renames — CROSS-CUTTING

| Commit | Description | Scope |
|---|---|---|
| `d2fdb96` + `dbdff58` | Evidence → Citation rename | Entire codebase (types, GraphQL, frontend) |
| `b66cadc` | Remove Story + ClusterSnapshot layer | Graph, admin, search-app |
| `84db856` | Remove dead signal properties | Types, GraphQL |
| `50ed12f` | Rename quarantined → rejected | Types, lint |

**Warning:** The Evidence→Citation rename touches foundation code that uses `create_evidence`. The foundation's `SignalStore` trait has `create_evidence` — dev-diverged renamed it to `create_citation`. This rename must be applied carefully across the foundation's event types and store implementations too.

### Category 6: Test Infrastructure — SAFE (mostly new files)

| Commit | Description |
|---|---|
| `dd2bff5` | Enforce MOCK→FUNCTION→OUTPUT across test suite |
| `7b9f463` | Rename test functions to describe behavior |
| `1ff2b67` | Signal lint tool and verdict tests |
| New files | `conversion_test.rs`, `firsthand_filter_test.rs`, `investigation_triage_test.rs`, `signal_lint_test.rs` |
| New fixtures | 7 `.txt` files for editorial filtering |

### Category 7: Archive — SAFE (foundation never touched)

| Commit | Description |
|---|---|
| `007839b` | Strip URL fragments, fix TikTok permalink fallback |
| `3bfebee` | Extract only href links, not every URL in HTML |
| `a950a24` | Route Facebook URLs to web scraper |

### Category 8: CLI — SAFE (foundation never touched)

| Commit | Description |
|---|---|
| `e15f159` (partial) | CLI refactor — `dev/cli/src/cmd/test.rs`, main.rs restructure |

### Category 9: Supervisor — SAFE (foundation never touched)

| Commit | Description |
|---|---|
| `0572328` | Remove batch_review, lint handles promotion |
| Various | Supervisor type changes, source penalty changes |

### Category 10: Docs — SAFE (all new files)

| Commit | Description |
|---|---|
| `6a27e7a` | Community alert surfacing plan + gap analyses |
| `a7b95af` | Mark plan acceptance criteria as complete |
| `5b22520` | Check off lint plan acceptance criteria |
| Various | 5 brainstorms, 7 plans, 2 gap analyses |

---

## Dual-Modified Files (34 files changed on BOTH branches)

Computed programmatically via `comm -12` on `git diff --name-only` from the common ancestor to each branch.

### Tier 1: Heavy conflicts (both branches made large changes)

| File | Foundation Δ | Dev-diverged Δ | Nature |
|---|---|---|---|
| `rootsignal-graph/src/writer.rs` | +rewrite (5,126 lines) | +features (6,198 lines) | Foundation rewrote for event-sourcing; dev-diverged added feature methods. Hardest merge. |
| `rootsignal-scout/src/pipeline/traits.rs` | 43 methods | 102 methods | Foundation shaped for events; dev-diverged tripled with direct graph ops. Every new method needs EventSourcedStore impl. |
| `rootsignal-scout/src/pipeline/scrape_phase.rs` | signature changes | +1856 feature code | Schedule nodes, lint, URL backoff |
| `rootsignal-common/src/types.rs` | event type additions | +750 new types + renames | Evidence→Citation, ScheduleNode, review status |
| `rootsignal-scout/src/workflows/scrape.rs` | event-sourcing wiring | +938 feature code | Heavily modified on both sides |
| `rootsignal-scout/src/testing.rs` | trait mock changes | +588 test helpers | Mock signatures diverged |
| `rootsignal-scout/src/pipeline/boundary_tests.rs` | trait changes | +1508 new tests | Test expectations diverged |
| `rootsignal-graph/tests/litmus_test.rs` | -48 lines | +474/-1159 lines | Major rewrite on dev-diverged |
| `rootsignal-scout/src/discovery/tension_linker.rs` | +5/-1 | +224/-29 | Massive feature additions |
| `rootsignal-scout/src/enrichment/link_promoter.rs` | +6/-1 | +342/-14 | Domain filtering, universe check |
| `rootsignal-api/src/graphql/schema.rs` | +small | +758 new resolvers | New resolvers, renames |
| `rootsignal-api/src/graphql/mutations.rs` | +20/-7 | +366/-76 | Purge, review status, cache mutations |
| `rootsignal-scout/src/pipeline/mod.rs` | +66 (build_signal_store) | +2 (lint modules) | Foundation added factory fn; dev-diverged added module declarations |

### Tier 2: Moderate conflicts (foundation made small signature changes, dev-diverged added features)

Pattern: foundation changed 5–20 lines (plumbing), dev-diverged changed 30–100 lines (features). Strategy: take dev-diverged's feature code, adapt signatures to foundation's event-sourcing plumbing.

| File | Foundation Δ | Dev-diverged Δ |
|---|---|---|
| `rootsignal-scout/src/discovery/bootstrap.rs` | +5/-2 | +66/-13 |
| `rootsignal-scout/src/discovery/gathering_finder.rs` | +10/-7 | +51/-23 |
| `rootsignal-scout/src/discovery/investigator.rs` | +6/-1 | +40/-9 |
| `rootsignal-scout/src/discovery/response_finder.rs` | +14/-11 | +34/-15 |
| `rootsignal-scout/src/discovery/source_finder.rs` | +7/-4 | +14/-100 |
| `rootsignal-scout/src/enrichment/actor_extractor.rs` | +9/-7 | +1/-1 |
| `rootsignal-scout/src/pipeline/scrape_pipeline.rs` | +73/-7 | +24/-19 |
| `rootsignal-scout/src/pipeline/expansion.rs` | +4/-1 | +3/-2 |
| `rootsignal-scout/src/scheduling/metrics.rs` | +20/-17 | +16/-14 |
| `rootsignal-scout/src/workflows/bootstrap.rs` | +4/-2 | +4/-3 |
| `rootsignal-scout/src/workflows/mod.rs` | +9 | +39/-2 |
| `rootsignal-scout/src/workflows/synthesis.rs` | +8/-2 | +33/-4 |
| `rootsignal-api/src/main.rs` | +9 | +32/-10 |
| `rootsignal-graph/src/lib.rs` | +10/-1 | +7/-11 |
| `rootsignal-graph/tests/bbox_scoping_test.rs` | +5/-19 | -74 |

### Tier 3: Auto-mergeable (Cargo files, lock files)

| File | Notes |
|---|---|
| `Cargo.lock` | Regenerate after Cargo.toml merge |
| `Cargo.toml` | Keep foundation deps + add rrule |
| `modules/rootsignal-common/Cargo.toml` | Keep rootsignal-world dep |
| `modules/rootsignal-graph/Cargo.toml` | Keep rootsignal-events + rootsignal-world deps, add dev-diverged deps |
| `modules/rootsignal-scout/Cargo.toml` | Keep rootsignal-events dep, add rrule, remove [[bin]] |
| `modules/rootsignal-scout/src/main.rs` | Foundation: trimmed; dev-diverged: deleted. Delete it. |

### Previously listed but NOT actually dual-modified (CORRECTED)

These files were only modified on ONE branch — no merge conflict:

| File | Modified only on |
|---|---|
| `rootsignal-graph/src/reader.rs` | dev-diverged only |
| `rootsignal-graph/src/cached_reader.rs` | dev-diverged only |
| `rootsignal-api/src/graphql/types.rs` | dev-diverged only |

---

## Files Safe to Take Wholesale from dev-diverged

Foundation never touched these modules — take dev-diverged's version directly:

- All `modules/admin-app/` changes
- All `modules/search-app/` changes
- All `docs/` additions
- All test fixtures (`tests/fixtures/*.txt`)
- `dev/cli/` changes
- `modules/rootsignal-archive/` changes
- `modules/rootsignal-scout-supervisor/` changes
- All new standalone source files (severity_inference.rs, signal_lint.rs, lint_tools.rs, domain_filter.rs, universe_check.rs, reaper.rs, scrape_url.rs, workflows/lint.rs)
- New migration files (renumbered 008–015)

---

## Recommended Strategy

**Do NOT cherry-pick individual commits.** The 60 commits have deep interdependencies and many touch the same files repeatedly.

### Phase 1 — Safe wholesale copies (no conflicts)
Copy entire modules/files that foundation never touched: admin-app, search-app, archive, supervisor, CLI, docs, test fixtures, new standalone source files.

### Phase 2 — Migration renumbering
Keep foundation's 007. Renumber dev-diverged's 007–014 to 008–015.

### Phase 3 — Cargo.toml merge
Keep foundation's `rootsignal-events` and `rootsignal-world` deps. Add dev-diverged's `rrule`. Keep foundation's `sha2`/`hex` in graph crate.

### Phase 4 — The hard part: trait + store reconciliation
1. Add all 59 new trait methods from dev-diverged to `SignalStore`
2. Implement each in `EventSourcedStore` with proper event emission + projection
3. This is where most of the engineering work lives

### Phase 5 — Manual 3-way merge of high-conflict files
`writer.rs`, `scrape_phase.rs`, `types.rs`, `reader.rs`, `cached_reader.rs`, `schema.rs`, `types.rs` (graphql), `scrape.rs`, `testing.rs`, `boundary_tests.rs`

### Phase 6 — Cross-cutting renames
Apply Evidence→Citation rename across foundation code (event types, store, trait).

### Phase 7 — Build & verify
`cargo check` → `cargo test` → admin-app build
