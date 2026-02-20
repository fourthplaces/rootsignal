---
title: "refactor: Scout pipeline stage reorganization"
type: refactor
date: 2026-02-20
brainstorm: docs/brainstorms/2026-02-20-scout-pipeline-stages-brainstorm.md
---

# refactor: Scout pipeline stage reorganization

## Overview

Reorganize `scout.rs` (103KB, 2586 lines) from a monolithic orchestrator into stage-based modules with typed boundaries. The goals are maintainability, testability, and future Restate readiness. The design preserves all existing behavior — this is a pure structural refactoring with no behavior changes.

## Problem Statement

`scout.rs` handles too many concerns: URL resolution, scraping orchestration, signal extraction, deduplication, embedding cache management, social media scraping, topic discovery, source metrics, weight computation, expansion query collection, and phase orchestration. This makes the code hard to reason about, impossible to test in isolation, and difficult to evolve.

## Proposed Solution

Extract three new modules (`scrape_phase.rs`, `expansion.rs`, `metrics.rs`), introduce a `RunContext` struct for shared mutable state, and reduce `scout.rs` to a thin orchestrator that wires stages together.

### Pattern to Follow

The existing finder modules (ResponseFinder, TensionLinker, GatheringFinder, Investigator) establish the precedent:

```rust
// Lifetime-parameterized struct with borrowed deps
pub struct ScrapePhase<'a> {
    writer: &'a GraphWriter,
    extractor: &'a dyn SignalExtractor,
    // ...
}

// Constructor takes all deps explicitly
impl<'a> ScrapePhase<'a> {
    pub fn new(writer: &'a GraphWriter, ...) -> Self { ... }
}

// Entry point: &self + mutable context
impl<'a> ScrapePhase<'a> {
    pub async fn run(&self, sources: &[&SourceNode], ctx: &mut RunContext) { ... }
}
```

## Technical Approach

### Full Stage Sequence

The actual `run_inner()` flow has 10 steps (the brainstorm listed 7 — spec-flow analysis caught 3 missing):

```
 1. Reap expired signals                    [inline in scout.rs]
 2. Load sources + Schedule                 [scheduler.rs — exists]
 3. Phase A: scrape tension web + social    [scrape_phase.rs — NEW]
 4. Mid-run Discovery                       [source_finder.rs — exists]
 5. Phase B: scrape response web + social   [scrape_phase.rs — same]
 6. Metrics + weight updates                [metrics.rs — NEW]
 7. Synthesis (parallel finders)            [existing modules — no change]
 8. Story Weaving                           [story_weaver — exists, AFTER synthesis]
 9. Expansion                               [expansion.rs — NEW]
10. End-of-run Discovery                    [inline in scout.rs — 2 lines]
```

Steps 1, 2, 10 remain inline in `run_inner()`. Steps 7-8 are untouched.

### RunContext — shared mutable state

```rust
// scrape_phase.rs (or run_context.rs if preferred)
pub(crate) struct RunContext {
    pub embed_cache: EmbeddingCache,
    pub url_to_canonical_key: HashMap<String, String>,
    pub source_signal_counts: HashMap<String, u32>,
    pub expansion_queries: Vec<String>,
    pub stats: ScoutStats,
    pub query_api_errors: HashSet<String>,
}

impl RunContext {
    pub fn new(sources: &[SourceNode]) -> Self {
        let url_to_canonical_key = sources
            .iter()
            .filter_map(|s| s.url.as_ref().map(|u| (sanitize_url(u), s.canonical_key.clone())))
            .collect();
        Self {
            embed_cache: EmbeddingCache::new(),
            url_to_canonical_key,
            source_signal_counts: HashMap::new(),
            expansion_queries: Vec::new(),
            stats: ScoutStats::default(),
            query_api_errors: HashSet::new(),
        }
    }

    /// Rebuild known_city_urls from current URL map state.
    /// Must be called before each social scrape to capture
    /// URLs resolved during the preceding web scrape.
    pub fn known_city_urls(&self) -> HashSet<String> {
        self.url_to_canonical_key.keys().cloned().collect()
    }
}
```

**What is NOT in RunContext** (stays as local variables in `run_inner()`):
- `all_sources: Vec<SourceNode>` — loaded once, passed to metrics directly
- `fresh_sources: Vec<SourceNode>` — reloaded for Phase B, local scope
- Schedule key sets (`tension_phase_keys`, `response_phase_keys`, etc.) — local to orchestration
- `social_topics: Vec<String>` — output of mid-run discovery, input to Phase B topic discovery

### ScrapePhase module

```rust
// scrape_phase.rs
pub(crate) struct ScrapePhase<'a> {
    writer: &'a GraphWriter,
    extractor: &'a dyn SignalExtractor,
    embedder: &'a dyn TextEmbedder,
    scraper: Arc<dyn PageScraper>,
    searcher: Arc<dyn WebSearcher>,
    social: &'a dyn SocialScraper,
    city_node: &'a CityNode,
    cancelled: Arc<AtomicBool>,
}
```

Methods:
- `pub async fn run_web(&self, sources: &[&SourceNode], ctx: &mut RunContext)` — web + WebQuery scraping (current `scrape_phase()`)
- `pub async fn run_social(&self, sources: &[&SourceNode], ctx: &mut RunContext)` — social media scraping (current `scrape_social_media()`)
- `pub async fn discover_from_topics(&self, topics: &[String], ctx: &mut RunContext)` — topic discovery (current `discover_from_topics()`)
- `async fn store_signals(...)` — private, called by run_web/run_social/discover_from_topics
- `async fn refresh_url_signals(...)` — private helper

Contains (moved from scout.rs):
- `EmbeddingCache` struct + `CacheEntry` + `cosine_similarity()` — `pub(crate)` visibility
- `ScrapeOutcome` enum
- `content_hash()`, `normalize_title()`, `node_meta_mut()` — private helpers

### Expansion module

```rust
// expansion.rs
pub(crate) struct Expansion<'a> {
    writer: &'a GraphWriter,
    embedder: &'a dyn TextEmbedder,
    city_slug: &'a str,
}
```

Methods:
- `pub async fn run(&self, ctx: &mut RunContext)` — dedup, create WebQuery sources

Contains (moved from scout.rs):
- `jaccard_similarity()`, `DEDUP_JACCARD_THRESHOLD`, `MAX_EXPANSION_QUERIES_PER_RUN`
- The deferred expansion query collection (queries graph for recently-linked signals)
- WebQuery source creation logic

### Metrics module

```rust
// metrics.rs
pub(crate) struct Metrics<'a> {
    writer: &'a GraphWriter,
}
```

Methods:
- `pub async fn update(&self, all_sources: &[SourceNode], ctx: &RunContext, now: DateTime<Utc>)` — note: `&RunContext` (immutable), reads signal counts and query errors

Contains (moved from scout.rs):
- `record_source_scrape()` calls
- Weight computation logic
- Cadence calculation
- Dead source deactivation
- Web query exponential backoff

### Cross-module dependency: `sanitize_url()`

Called from both `scout.rs` (RunContext initialization at line 383) and `scrape_phase.rs` (throughout scraping and storage). Two options:

- **(a)** Make it `pub(crate)` in `scrape_phase.rs`, import from `scout.rs` — simple, minimal file creation
- **(b)** Move to `util.rs` — the project already has this module

**Decision: (a)** — `sanitize_url()` is closely tied to scraping logic. Add `pub(crate)` and import it.

### Orchestrator after refactoring

`scout.rs` shrinks to ~200-300 lines:

```rust
pub async fn run_inner(&self) -> Result<ScoutStats> {
    // 1. Reap
    self.writer.reap_expired_signals().await?;

    // 2. Load + Schedule
    let all_sources = self.writer.get_active_sources(&self.city_node.slug).await?;
    let plan = SourceScheduler::new(&all_sources, ...).schedule();
    let mut ctx = RunContext::new(&all_sources);

    let phase = ScrapePhase::new(&self.writer, &*self.extractor, ...);

    // 3. Phase A: Find Problems
    let tension_sources = ...; // filter from plan
    phase.run_web(&tension_sources, &mut ctx).await;
    phase.run_social(&tension_social_sources, &mut ctx).await;

    // 4. Mid-run Discovery
    let discoverer = SourceFinder::new(...).await;
    let (discovery_stats, social_topics) = discoverer.run().await;

    // 5. Phase B: Find Responses
    let fresh_sources = self.writer.get_active_sources(&self.city_node.slug).await?;
    let response_sources = ...; // filter from plan + fresh discovery
    phase.run_web(&response_sources, &mut ctx).await;
    phase.run_social(&response_social_sources, &mut ctx).await;
    phase.discover_from_topics(&social_topics, &mut ctx).await;

    // 6. Metrics
    let metrics = Metrics::new(&self.writer);
    metrics.update(&all_sources, &ctx, Utc::now()).await;

    // 7. Synthesis (parallel finders — unchanged)
    let (rm, tl, rf, gf, inv) = tokio::join!(...);

    // 8. Story Weaving (must run AFTER synthesis)
    story_weaver.run().await;

    // 9. Expansion
    let expansion = Expansion::new(&self.writer, &*self.embedder, &self.city_node.slug);
    expansion.run(&mut ctx).await;

    // 10. End-of-run Discovery
    let discoverer = SourceFinder::new(...).await;
    discoverer.run().await;

    Ok(ctx.stats)
}
```

## Implementation Phases

### Phase 1: RunContext + scrape_phase.rs (~1400 lines moved)

The largest and most impactful extraction. Do this first because it's the core of the problem.

**Tasks:**

1. Create `modules/rootsignal-scout/src/scrape_phase.rs`
2. Define `RunContext` struct with `new()` and `known_city_urls()`
3. Move `EmbeddingCache`, `CacheEntry`, `cosine_similarity()` — make `pub(crate)`
4. Move `ScrapeOutcome` enum
5. Move helper functions: `content_hash()`, `normalize_title()`, `node_meta_mut()`
6. Make `sanitize_url()` `pub(crate)` (stays in scrape_phase.rs, imported by scout.rs)
7. Define `ScrapePhase<'a>` struct with `new()` constructor
8. Move `scrape_phase()` → `ScrapePhase::run_web()`, changing signature from 6 `&mut` params to `&mut RunContext`
9. Move `scrape_social_media()` → `ScrapePhase::run_social()`
10. Move `discover_from_topics()` → `ScrapePhase::discover_from_topics()`
11. Move `store_signals()` → private `ScrapePhase::store_signals()`
12. Move `refresh_url_signals()` → private helper
13. Update `scout.rs`: remove moved code, add `mod scrape_phase`, update `run_inner()` to construct `RunContext` and `ScrapePhase`, call new API
14. Add `pub mod scrape_phase;` to `lib.rs`
15. Verify: `cargo build` passes, `cargo test` passes

**Critical details to preserve:**
- `known_city_urls` must be rebuilt from `ctx.url_to_canonical_key` before each social scrape call (not at ScrapePhase construction)
- `query_api_errors` accumulates across Phase A and Phase B (both extend `ctx.query_api_errors`)
- `url_to_canonical_key` is mutated by `run_web()` during WebQuery resolution and read during signal storage attribution
- `fresh_sources` URL entries must be added to `ctx.url_to_canonical_key` between mid-run discovery and Phase B (this stays in `run_inner()`)

### Phase 2: expansion.rs (~130 lines moved)

**Tasks:**

1. Create `modules/rootsignal-scout/src/expansion.rs`
2. Move `jaccard_similarity()`, `DEDUP_JACCARD_THRESHOLD`, `MAX_EXPANSION_QUERIES_PER_RUN`
3. Define `Expansion<'a>` struct with `new()` constructor
4. Move expansion block from `run_inner()` (~lines 838-966): deferred query collection, dedup logic, WebQuery source creation
5. Update `scout.rs`: remove moved code, call `Expansion::run()`
6. Add `pub mod expansion;` to `lib.rs`
7. Verify: `cargo build` passes, `cargo test` passes (jaccard tests move with the function)

### Phase 3: metrics.rs (~110 lines moved)

**Tasks:**

1. Create `modules/rootsignal-scout/src/metrics.rs`
2. Define `Metrics<'a>` struct with `new()` constructor
3. Move the metrics/weight update block from `run_inner()` (~lines 556-668): `record_source_scrape()` calls, weight computation, cadence calculation, dead source/query deactivation
4. `Metrics::update()` takes `&RunContext` (immutable read of signal counts and query errors) and `&[SourceNode]` (the `all_sources` snapshot — NOT `fresh_sources`, preserving existing behavior)
5. Update `scout.rs`: remove moved code, call `Metrics::update()`
6. Add `pub mod metrics;` to `lib.rs`
7. Verify: `cargo build` passes, `cargo test` passes

### Phase 4: Clean up scout.rs

After all extractions, `scout.rs` should be ~200-300 lines.

**Tasks:**

1. Review remaining code in `scout.rs` — should only be: Scout struct, constructors (`new`, `with_deps`), `run()`, `run_inner()` orchestration, `ScoutStats`, cancellation check
2. Remove any dead imports
3. Verify the orchestration flow reads cleanly top-to-bottom
4. `cargo build && cargo test`

## Acceptance Criteria

- [x] `scout.rs` reduced from ~2586 lines to ~632 lines (synthesis block kept inline per plan)
- [x] Three new modules: `scrape_phase.rs`, `expansion.rs`, `metrics.rs`
- [x] `RunContext` struct holds all cross-phase mutable state
- [x] `ScrapePhase` follows the finder pattern (lifetime-parameterized, borrowed deps, `run()` entry point)
- [x] All existing `cargo test` tests pass without modification
- [x] `cargo build` succeeds with no warnings
- [x] No behavior changes — this is a pure structural refactoring
- [x] `run_inner()` reads as a clear sequence of named stages

## Quality Gates

- [x] `cargo build` — no compilation errors
- [x] `cargo test` — all existing tests pass (jaccard tests move to expansion.rs)
- [x] `cargo clippy` — no new warnings
- [x] Manual review: `run_inner()` orchestration is readable and sequential
- [x] Spot check: `known_city_urls` rebuilt before each social scrape (not cached)
- [x] Spot check: `query_api_errors` accumulates across phases (not overwritten)
- [x] Spot check: metrics uses `all_sources` (not `fresh_sources`)

## Risk Analysis

| Risk | Mitigation |
|------|------------|
| Subtle behavior change from reordering | Pure move refactoring — no logic changes. Diff should show only moves + signature changes. |
| Borrow checker issues with `ScrapePhase<'a>` + `&mut RunContext` | Follows proven finder pattern. RunContext passed per-call, not held in struct. |
| Missing `pub(crate)` visibility | Compiler will catch immediately. Fix as encountered. |
| `sanitize_url()` import cycle | Made `pub(crate)` in `scrape_phase.rs`, imported by `scout.rs`. One-directional. |

## References

- Brainstorm: `docs/brainstorms/2026-02-20-scout-pipeline-stages-brainstorm.md`
- Restate future: `docs/brainstorms/2026-02-19-restate-durable-execution-brainstorm.md`
- Pipeline architecture: `docs/architecture/scout-pipeline.md`
- Testing playbook: `docs/tests/scout-testing.md`
- Finder pattern precedent: `modules/rootsignal-scout/src/response_finder.rs:242` (struct shape)
- Test fixtures: `modules/rootsignal-scout/src/fixtures.rs` (mock implementations)
