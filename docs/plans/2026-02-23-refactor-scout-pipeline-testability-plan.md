---
title: "Refactor Scout Pipeline for Testability"
type: refactor
date: 2026-02-23
---

# Refactor Scout Pipeline for Testability

## Overview

Make every organ of Scout testable with the pattern: **MOCK -> FUNCTION -> OUTPUT**. Extract trait abstractions for the two concrete dependencies (`Archive`, `GraphWriter`), extract pure decision functions from `store_signals`, and build hand-written mocks that enable deterministic end-to-end chain tests with zero infrastructure.

Brainstorm: `docs/brainstorms/2026-02-23-scout-pipeline-testing-brainstorm.md`
Chain test examples: `docs/brainstorms/2026-02-23-chain-test-examples.rs`

## Testing Vision

Six zoom levels — from individual pure functions to the full pipeline. Zoom in to test an organ's brain. Zoom out to test how organs compose. Every level follows MOCK → FUNCTION → OUTPUT.

```
Zoom 1:  Pure functions (canonical_value, dedup_verdict, extract_links)     ← Phase 1, 3
Zoom 2:  Single organ (store_signals, promote_links)                        ← Phase 4
Zoom 3:  Pipeline method (run_web, run_social)                              ← Phase 6
Zoom 4:  Multi-phase (tension → promote → response)                        ← Phase 6, test 6
Zoom 5:  Full pipeline (run_all)                                            ← FUTURE
Zoom 6:  SimWeb scenarios (LLM-backed worlds)                               ← Phase 7
```

This plan delivers Zoom 1–4 and 6. Zoom 5 (`run_all`) is a follow-up that requires abstracting `PgPool`, `BudgetTracker`, and `SourceFinder` — but the trait work in Phase 2 is designed to not block that path. `ScrapePipeline` will already hold `Arc<dyn SignalStore>` and `Arc<dyn ContentFetcher>` after Phase 2, so the remaining work is incremental.

## Problem Statement

Scout's core logic (`ScrapePhase::run_web`, `run_social`, `store_signals`) takes concrete `GraphWriter` and `Arc<Archive>`. Testing any of it requires Neo4j + Chrome + API keys. The testing constraint from CLAUDE.md is clear: if code can't be tested as MOCK -> FUNCTION -> OUTPUT, refactor it. Today, `ScrapePhase` can't be.

## Technical Approach

### Architecture

Two traits replace two concrete types. Everything else follows.

```
Before:  ScrapePhase(GraphWriter, Arc<Archive>, Arc<dyn SignalExtractor>, Arc<dyn TextEmbedder>)
After:   ScrapePhase(Arc<dyn SignalStore>, Arc<dyn ContentFetcher>, Arc<dyn SignalExtractor>, Arc<dyn TextEmbedder>)
                     ^^^^^^^^^^^^^^^^^^^^  ^^^^^^^^^^^^^^^^^^^^^^
                     NEW                   NEW
```

`SignalExtractor` and `TextEmbedder` are already traits. This work completes the picture.

### Design Decisions (from SpecFlow analysis)

**Where do traits live?**
Both traits are defined in `rootsignal-scout::pipeline::traits` (new module). `ArchivedPage`, `ArchivedFeed`, `ArchivedSearchResults`, `Post`, `SourceNode`, `ActorNode`, `Node`, `NodeType` are all already in `rootsignal-common`, so the traits reference common types only. Impl for `Archive` lives in `rootsignal-scout::pipeline::traits` too (thin adapter wrapping `Archive` methods). Impl for `GraphWriter` same. No circular dependencies — scout depends on archive and graph, not the other way around.

**MockSignalStore is a smart mock.**
It maintains internal state: `create_node` adds to internal maps, `find_by_titles_and_types` queries those maps, `corroborate` increments counts. This is required for multi-source corroboration chain tests where Source B's dedup query must return Source A's previously created signal. The mock gets its own unit tests.

**FixedEmbedder returns a deterministic hash-based vector for unmatched texts.**
When `embed_batch` receives `"{title} {content_snippet}"` and no exact match is registered, it hashes the input to produce a stable vector. For controlled dedup tests, register the exact composite string. For most tests, the default behavior (each unique text gets a unique vector, low cross-similarity) is sufficient.

**`dedup_verdict` is pure; caller handles cache mutation.**
The function returns `DedupVerdict` only. The loop in `store_signals` handles `embed_cache.add()` after a Create verdict. Tests of `dedup_verdict` don't worry about cache side effects.

**Chain tests 1–5 operate at `ScrapePhase` level.**
They call `phase.run_web()` and `phase.run_social()` directly — this is where the trust-critical logic lives. **Chain test 6 operates at the multi-phase level** — it manually orchestrates tension → promote → response to test the discovery loop across phases. Full `ScrapePipeline::run_all()` testing is Zoom 5 (future work, see below).

**`ContentFetcher::page()` returns full `ArchivedPage` including `raw_html`.**
The `HtmlListing` code path in `run_web` reads `page.raw_html`. Mocks populate it as empty string by default.

**`discover_from_topics` gets refactored to use `ContentFetcher` methods.**
The three `self.archive.source()` calls are replaced with direct `ContentFetcher::search_topics()` and `ContentFetcher::site_search()` calls. The `SourceHandle` indirection is eliminated.

---

## Implementation Phases

### Phase 1: Shared Utility Tests

**Zero dependencies. Immediate value. Catches real bugs.**

Pure function test batteries. No traits, no mocks, no async. `cargo test` in seconds.

#### 1a. `canonical_value()` tests

**File:** `modules/rootsignal-common/src/types.rs` (inline `#[cfg(test)]` module)

```rust
// Social platform normalization
canonical_value("https://www.instagram.com/mplsmutualaid/")  == "instagram.com/mplsmutualaid"
canonical_value("https://instagram.com/mplsmutualaid")       == "instagram.com/mplsmutualaid"
canonical_value("https://twitter.com/handle")                == "x.com/handle"
canonical_value("https://x.com/handle")                      == "x.com/handle"
canonical_value("https://www.tiktok.com/@handle/")           == "tiktok.com/handle"
canonical_value("https://www.reddit.com/r/Minneapolis/")     == "reddit.com/r/Minneapolis"

// Web URL edge cases (document actual behavior, likely expose gaps)
canonical_value("https://www.example.com/page") vs canonical_value("https://example.com/page")
canonical_value("https://example.com/page#section") vs canonical_value("https://example.com/page")
canonical_value("https://example.com/page/") vs canonical_value("https://example.com/page")
canonical_value("https://Example.COM/Page") vs canonical_value("https://example.com/page")

// Web queries pass through unchanged
canonical_value("site:linktr.ee mutual aid Minneapolis") == "site:linktr.ee mutual aid Minneapolis"
```

#### 1b. `extract_all_links()` tests

**File:** `modules/rootsignal-archive/src/links.rs` (inline `#[cfg(test)]` module)

```rust
// Href extraction
extract_all_links("<a href='https://instagram.com/org'>IG</a>", "https://example.com")
  → ["https://instagram.com/org"]

// Relative URL resolution
extract_all_links("<a href='/about'>About</a>", "https://example.com")
  → ["https://example.com/about"]

// Empty / malformed
extract_all_links("", "https://example.com") → []
extract_all_links("<a href=''>empty</a>", "https://example.com") → []
```

#### 1c. `normalize_title()` tests

**File:** `modules/rootsignal-scout/src/pipeline/scrape_phase.rs`

Currently private. Make `pub(crate)` and add inline tests:
```rust
normalize_title("  Free Legal Clinic  ") == "free legal clinic"
normalize_title("FREE LEGAL CLINIC") == "free legal clinic"
normalize_title("") == ""
```

#### 1d. Document URL normalization inconsistencies

Add a test that calls all three systems on the same URL and documents where they differ:
```rust
// canonical_value vs sanitize_url vs strip_tracking_params
// Same input, different outputs → document for future unification
```

**Files touched:**
- `modules/rootsignal-common/src/types.rs` — add `#[cfg(test)]` module (~30 tests)
- `modules/rootsignal-archive/src/links.rs` — add `#[cfg(test)]` module (~15 tests)
- `modules/rootsignal-scout/src/pipeline/scrape_phase.rs` — make `normalize_title` `pub(crate)`, add tests (~5 tests)

---

### Phase 2: Trait Abstractions

**Unlocks all organ-level, boundary, and chain testing.**

#### 2a. `ContentFetcher` trait

**New file:** `modules/rootsignal-scout/src/pipeline/traits.rs`

```rust
#[async_trait]
pub trait ContentFetcher: Send + Sync {
    async fn page(&self, url: &str) -> Result<ArchivedPage>;
    async fn feed(&self, url: &str) -> Result<ArchivedFeed>;
    async fn posts(&self, identifier: &str, limit: u32) -> Result<Vec<Post>>;
    async fn search(&self, query: &str) -> Result<ArchivedSearchResults>;
    async fn search_topics(&self, platform_url: &str, topics: &[&str], limit: u32) -> Result<Vec<Post>>;
    async fn site_search(&self, query: &str, max_results: usize) -> Result<ArchivedSearchResults>;
}
```

6 methods. Impl for `Archive`:

```rust
impl ContentFetcher for Archive {
    async fn page(&self, url: &str) -> Result<ArchivedPage> {
        self.page(url).await  // Archive already has this method
    }
    async fn search(&self, query: &str) -> Result<ArchivedSearchResults> {
        self.search(query).await
    }
    // ... thin delegation for each method
}
```

**Refactor `discover_from_topics`:** Replace `self.archive.source(platform_url).await?.search_topics(...)` with `self.fetcher.search_topics(platform_url, topics, limit)`. Replace `search_handle.search(&query).max_results(n).await` with `self.fetcher.site_search(&query, n)`. This eliminates all `SourceHandle` usage in `ScrapePhase`.

#### 2b. `SignalStore` trait

**Same file:** `modules/rootsignal-scout/src/pipeline/traits.rs`

20 methods (listed in brainstorm). Impl for `GraphWriter`:

```rust
impl SignalStore for GraphWriter {
    async fn create_node(&self, node: &Node, embedding: &[f32], created_by: &str, run_id: &str) -> Result<Uuid> {
        self.create_node(node, embedding, created_by, run_id).await
    }
    // ... thin delegation for each method
}
```

**Also update `promote_links`** in `link_promoter.rs` to take `&dyn SignalStore` instead of `&GraphWriter`. It only calls `upsert_source`, which is on the trait.

#### 2c. Refactor `ScrapePhase`

```rust
// Before
pub(crate) struct ScrapePhase {
    writer: GraphWriter,
    archive: Arc<Archive>,
    // ...
}

// After
pub(crate) struct ScrapePhase {
    store: Arc<dyn SignalStore>,
    fetcher: Arc<dyn ContentFetcher>,
    // ...
}
```

Update `new()`, every `self.writer.` call becomes `self.store.`, every `self.archive.` call becomes `self.fetcher.`.

Update `ScrapePipeline` to construct `ScrapePhase` with `Arc::new(writer) as Arc<dyn SignalStore>` and `archive.clone() as Arc<dyn ContentFetcher>`.

#### 2d. Mocks

**New file:** `modules/rootsignal-scout/src/testing.rs` (gated behind `#[cfg(test)]`)

Re-exported from `lib.rs` as `#[cfg(test)] pub mod testing;`

**`MockFetcher`** — HashMap-based. Builder pattern: `.on_page()`, `.on_search()`, `.on_posts()`, `.on_feed()`. Returns `Err` for unknown URLs (tests must register every URL the organ will hit).

**`MockSignalStore`** — Stateful in-memory graph. Internal `HashMap<String, StoredSignal>` keyed by normalized title. `create_node` inserts, `find_by_titles_and_types` queries, `corroborate` increments counts. Has assertion helpers: `signals_created()`, `has_signal_titled()`, `has_actor()`, `actor_linked_to_signal()`, `corroborations_for()`, `evidence_count_for()`, `sources_promoted()`, `has_source_url()`.

**`FixedEmbedder`** — HashMap of text→vector. Unmatched texts get a deterministic hash-based vector (each unique text gets a unique, low-similarity vector). Has `.on_text()` for controlled similarity tests.

**`MockExtractor`** — HashMap of URL→`ExtractionResult`. Already compatible with existing `SignalExtractor` trait.

**Test helpers:** `mpls_region() -> ScoutScope`, `web_query_source(query) -> SourceNode`, `page_source(url) -> SourceNode`, `social_source(url) -> SourceNode`, `test_meta_defaults(url) -> NodeMeta`.

**Add `Default` impls** to `ArchivedPage`, `NodeMeta` (with `id: Uuid::new_v4()`, `extracted_at: Utc::now()`) to enable `..Default::default()` in test setup.

**MockSignalStore gets its own tests:**
```rust
#[test]
fn create_then_find_returns_created_signal() { ... }

#[test]
fn corroborate_increments_count() { ... }

#[test]
fn find_by_titles_returns_empty_for_unknown() { ... }
```

**Files touched:**
- `modules/rootsignal-scout/src/pipeline/traits.rs` — NEW (~200 lines: 2 traits + 2 impls)
- `modules/rootsignal-scout/src/pipeline/mod.rs` — add `pub mod traits;`
- `modules/rootsignal-scout/src/pipeline/scrape_phase.rs` — change `writer`→`store`, `archive`→`fetcher` (~50 find-and-replace edits)
- `modules/rootsignal-scout/src/pipeline/scrape_pipeline.rs` — update `ScrapePhase::new()` call, update `promote_collected_links`
- `modules/rootsignal-scout/src/enrichment/link_promoter.rs` — `promote_links` takes `&dyn SignalStore`
- `modules/rootsignal-scout/src/testing.rs` — NEW (~400 lines: 4 mocks + helpers + mock tests)
- `modules/rootsignal-scout/src/lib.rs` — add `#[cfg(test)] pub mod testing;`
- `modules/rootsignal-common/src/types.rs` — add `Default` for `ArchivedPage`, `NodeMeta`

**Verification:** After this phase, `cargo test` passes — all existing tests still work (concrete types still used in production paths via trait impls). New tests can construct `ScrapePhase` with mocks.

---

### Phase 3: Internal Extractions

**Makes the Signal Processor's brain testable without its hands.**

#### 3a. `DedupVerdict` enum + `dedup_verdict()` function

**File:** `modules/rootsignal-scout/src/pipeline/scrape_phase.rs`

Extract from `store_signals` (currently ~lines 1380-1703):

```rust
pub(crate) enum DedupVerdict {
    Create,
    Corroborate { existing_id: Uuid, existing_url: String, similarity: f64 },
    Refresh { existing_id: Uuid, similarity: f64 },
}

pub(crate) fn dedup_verdict(
    node: &Node,
    source_url: &str,
    embedding: &[f32],
    global_matches: &HashMap<(String, NodeType), (Uuid, String)>,
    embed_cache: &EmbeddingCache,
    graph_duplicate: Option<DuplicateMatch>,
) -> DedupVerdict
```

Pure function. Caller (`store_signals`) does the async `find_duplicate` query, passes result in, then executes the verdict. Caller also handles `embed_cache.add()` after a Create.

**Tests (~20):**
```rust
// Cross-source title match → Corroborate
// Same-source title match → Refresh
// No title match + embedding match cross-source → Corroborate
// No title match + embedding match same-source → Refresh
// No match anywhere → Create
// Cache hit → Corroborate/Refresh (depending on source)
// Corroboration decay: same title different dates → documents gap
```

#### 3b. `score_and_filter()` function

Already nearly pure in `store_signals`. Extract:

```rust
pub(crate) fn score_and_filter(
    nodes: Vec<Node>,
    url: &str,
    geo_config: &GeoFilterConfig,
    actor_ctx: Option<&ActorContext>,
) -> (Vec<Node>, GeoFilterStats)
```

Applies actor location fallback, quality scoring, geo filtering. Tests (~10):
```rust
// Signal in region → survives
// Signal outside region → filtered
// No location + actor fallback → gets actor coords → survives
// No location + no actor → filtered (unless geo_terms match)
// Actor in NYC + Minneapolis scout → filtered after fallback
```

#### 3c. `batch_title_dedup()` function

```rust
pub(crate) fn batch_title_dedup(nodes: Vec<Node>) -> Vec<Node>
```

Within-batch dedup by (normalized_title, node_type). Tests (~5):
```rust
// Two nodes same title+type → one survives
// Two nodes same title different type → both survive
// Case/whitespace differences → deduped
```

**Files touched:**
- `modules/rootsignal-scout/src/pipeline/scrape_phase.rs` — extract 3 functions, update `store_signals` to call them (~150 lines moved, ~20 lines new glue code)

**Verification:** `store_signals` behavior unchanged. New pure function tests pass. Existing graph_write_test.rs still passes.

---

### Phase 4: Boundary Tests

**Test one organ handoff at a time.**

**File:** `modules/rootsignal-scout/tests/boundary_tests.rs` (new integration test)

Uses mocks from Phase 2. Each test: MOCK → one organ method → assert output.

| Boundary | What's tested | Test count |
|----------|---------------|------------|
| Fetcher → Extractor | `ArchivedPage.markdown` flows correctly to extractor | 3 |
| Fetcher → Link Discoverer | Page links → `extract_links` → `promote_links` with MockSignalStore | 5 |
| Extractor → Signal Processor | Extracted nodes → `store_signals` writes correct signals | 4 |
| Extractor → Actor Resolver | `mentioned_actors` + `author_actor` → actor creates + edges | 5 |
| Embedder → Signal Processor | Vector similarity → dedup verdict (via `dedup_verdict()`) | 4 |
| Location handoff | Actor fallback ↔ signal location ↔ geo filter | 5 |
| Source location bug (TDD) | `promote_links` stamps wrong coords → failing tests first | 3 |

~29 boundary tests total.

**Files touched:**
- `modules/rootsignal-scout/tests/boundary_tests.rs` — NEW

---

### Phase 5: LLM Edge Testing

**Snapshot/replay for extraction. Record once, replay in CI.**

Extend the existing snapshot infrastructure in `tests/extraction_test.rs`:

- Add edge case fixtures (adversarial, multilingual, ambiguous geo, stale content)
- Add actor extraction snapshots (for `actor_extractor.rs` path)
- Deterministic fields get exact assertions; free text gets loose assertions

**New fixtures** in `tests/fixtures/`:
- `coalition_page.txt` — multi-actor, multi-signal
- `spanish_mutual_aid.txt` — non-English content
- `stale_event.txt` — past dates
- `clickbait_spam.txt` — adversarial content
- `multi_city.txt` — ambiguous geography

**Files touched:**
- `modules/rootsignal-scout/tests/extraction_test.rs` — add test functions
- `modules/rootsignal-scout/tests/fixtures/` — 5 new fixture files

---

### Phase 6: Chain Tests

**End-to-end with mocks. MOCK → `run_web` / `run_social` → OUTPUT.**

**File:** `modules/rootsignal-scout/tests/chain_tests.rs` (new integration test)

6 chain tests (from `docs/brainstorms/2026-02-23-chain-test-examples.rs`):

1. **Linktree discovery** — search → fetch Linktrees → collected_links has content URLs, junk filtered, deduped
2. **Page → signal → actors → evidence** — page source → run_web → signal created, actors wired, evidence linked
3. **Multi-source corroboration** — 3 pages same event → run_web → 1 signal, 2 corroborations, 3 evidence trails
4. **Social with actor context** — Instagram posts + actor_ctx → run_social → signal with fallback location, @mentions collected
5. **Content unchanged** — hash match → skip extraction → links still collected
6. **Two-phase pipeline** — Phase A discovers source → Phase B scrapes it

**Files touched:**
- `modules/rootsignal-scout/tests/chain_tests.rs` — NEW

---

### Phase 7: SimWeb Integration

**`SimulatedWeb` implements `ContentFetcher`. 8 existing scenarios plug in.**

**File:** `modules/simweb/src/content_fetcher.rs` (new, thin adapter)

```rust
#[async_trait]
impl ContentFetcher for SimulatedWeb {
    async fn page(&self, url: &str) -> Result<ArchivedPage> {
        let sim_page = self.scrape(url).await?;
        Ok(ArchivedPage {
            markdown: sim_page.content,
            raw_html: sim_page.raw_html.unwrap_or_default(),
            links: extract_all_links(&sim_page.raw_html.unwrap_or_default(), url),
            ..Default::default()
        })
    }
    async fn search(&self, query: &str) -> Result<ArchivedSearchResults> {
        let results = self.search(query, 10).await?;
        Ok(ArchivedSearchResults {
            results: results.into_iter().map(|r| SearchResult { url: r.url, title: r.title, snippet: r.snippet }).collect(),
        })
    }
    // ...
}
```

Wire existing 8 scenarios through `ScrapePhase::run_web` with Judge evaluation. Snapshot/replay for CI.

**Files touched:**
- `modules/simweb/src/content_fetcher.rs` — NEW
- `modules/simweb/src/lib.rs` — add module
- `modules/simweb/Cargo.toml` — add rootsignal-common dependency (for `ArchivedPage` etc.)
- `modules/rootsignal-scout/tests/simweb_scenarios_test.rs` — NEW

---

## Acceptance Criteria

### Functional Requirements

- [x] `canonical_value()` has 30+ tests covering social platforms, web URLs, edge cases
- [x] `extract_all_links()` has 15+ tests covering hrefs, relative URLs, empty/malformed
- [x] `normalize_title()` is `pub(crate)` with tests
- [ ] `ContentFetcher` trait exists with 6 methods and `impl for Archive`
- [ ] `SignalStore` trait exists with 20 methods and `impl for GraphWriter`
- [ ] `ScrapePhase` accepts `Arc<dyn SignalStore>` and `Arc<dyn ContentFetcher>`
- [ ] `promote_links` accepts `&dyn SignalStore`
- [ ] `discover_from_topics` uses `ContentFetcher` methods (no `SourceHandle`)
- [ ] `MockFetcher`, `MockSignalStore`, `FixedEmbedder`, `MockExtractor` exist in `testing.rs`
- [ ] `MockSignalStore` has its own unit tests
- [ ] `dedup_verdict()` extracted as pure function with 20+ tests
- [ ] `score_and_filter()` extracted with 10+ tests
- [ ] `batch_title_dedup()` extracted with 5+ tests
- [ ] 29+ boundary tests in `boundary_tests.rs`
- [ ] 6 chain tests in `chain_tests.rs` — each follows MOCK → FUNCTION → OUTPUT
- [ ] All existing tests still pass (zero regressions)
- [ ] `cargo test` runs all new tests without Docker, Neo4j, or API keys

### Quality Gates

- [ ] Every new test follows MOCK → FUNCTION → OUTPUT (CLAUDE.md rule)
- [ ] No test manually calls internal functions step-by-step
- [ ] `MockSignalStore` correctly simulates dedup state machine (verified by its own tests)
- [ ] Source location bug has failing TDD tests (red phase)

---

## Dependencies & Risks

**Risk: Phase 2 trait extraction touches many call sites.**
Mitigation: Trait impls are thin delegation — behavior doesn't change. Existing tests serve as regression guards.

**Risk: Phase 3 `dedup_verdict` extraction from 700-line `store_signals`.**
Mitigation: Extract as a pure function that takes pre-computed inputs. The loop in `store_signals` does the async queries and passes results in. Existing `graph_write_test.rs` catches regressions.

**Risk: `MockSignalStore` complexity is underestimated.**
Mitigation: MockSignalStore gets its own test suite. Start with the minimum methods needed for Chain Test 1 and expand incrementally.

**Risk: `FixedEmbedder` text matching for `embed_batch`.**
Mitigation: Default to hash-based vectors for unmatched text. For controlled dedup tests, register the exact composite string (`"{title} {content_snippet}"`). Document the matching strategy in the mock's doc comment.

## What We're NOT Doing

- Not rewriting `store_signals` as a pipeline. The read-write interleaving is correct.
- Not adding test frameworks (`mockall`, `wiremock`, `insta`). Hand-written mocks.
- Not testing Restate workflows. Thin durability wrapper.
- Not testing `ScrapePipeline::run_all()` directly — that's Zoom 5 (see Future section below). Chain tests 1-5 operate at `ScrapePhase` level; Chain test 6 manually orchestrates two phases to test the tension → promote → response flow.
- Not solving actor fuzzy matching. Tests document the gap.
- Not unifying URL normalization systems. Tests document the inconsistencies.

## Future: Pipeline-Level Testing (Zoom 5)

After this plan lands, the path to `MOCK → pipeline.run_all() → OUTPUT` is incremental. Phase 2's trait work means `ScrapePipeline` will already hold `Arc<dyn SignalStore>` and `Arc<dyn ContentFetcher>`. The remaining concrete dependencies:

| Dependency | Used in | Abstraction needed |
|---|---|---|
| `PgPool` | `finalize()` — saves run log to Postgres | `Option<PgPool>` (skip in tests) or `RunLogStore` trait |
| `BudgetTracker` | `discover_mid_run_sources`, `expand_and_discover` | Trait or test-constructible struct (no infra needed for a budget counter) |
| `anthropic_api_key` | `load_and_schedule_sources` (bootstrap), `discover_mid_run_sources`, `expand_and_discover` (SourceFinder) | SourceFinder + Bootstrapper accept `Arc<dyn SignalStore>` + `Arc<dyn ContentFetcher>` instead of concrete types |

**What Zoom 5 unlocks:**

- `MOCK → pipeline.run_all() → OUTPUT` — test the full scout run as one call
- `MOCK → pipeline.scrape_tension_sources() → OUTPUT` — stop after Phase A
- `MOCK → pipeline.scrape_response_sources() → OUTPUT` — stop after Phase B
- Full orchestration coverage: scheduling logic, cancellation handling, mid-run discovery, source reloading between phases, expansion, cold-start bootstrap

**Why it's not in this plan:** SourceFinder, Bootstrapper, and Expansion are their own organs with their own dependency graphs. Abstracting them is meaningful scope. The current plan focuses on the scraping core (`ScrapePhase`) where the trust-critical logic lives. Zoom 5 follows naturally once the pattern is established.

**No doors closed:** Phase 2's trait abstractions propagate up through `ScrapePipeline` to `ScrapePhase`. The remaining work is additive — more traits, more mocks, same pattern.

---

## References

- Brainstorm: `docs/brainstorms/2026-02-23-scout-pipeline-testing-brainstorm.md`
- Chain test examples: `docs/brainstorms/2026-02-23-chain-test-examples.rs`
- Boundary test examples: `docs/brainstorms/2026-02-23-example-tests.rs`
- Existing traits: `modules/rootsignal-scout/src/pipeline/extractor.rs:146` (`SignalExtractor`), `modules/rootsignal-common/src/types.rs:1388` (`TextEmbedder`)
- Concrete types to abstract: `modules/rootsignal-graph/src/writer.rs:17` (`GraphWriter`), `modules/rootsignal-archive/src/archive.rs:37` (`Archive`)
- `ScrapePhase` constructor: `modules/rootsignal-scout/src/pipeline/scrape_phase.rs:192`
- `store_signals`: `modules/rootsignal-scout/src/pipeline/scrape_phase.rs:~1241` (700 lines)
- `promote_links`: `modules/rootsignal-scout/src/enrichment/link_promoter.rs:124`
- `canonical_value`: `modules/rootsignal-common/src/types.rs:966`
- `extract_all_links`: `modules/rootsignal-archive/src/links.rs`
- Existing test patterns: `modules/rootsignal-scout/tests/extraction_test.rs`, `tests/quality_scenarios_test.rs`, `tests/graph_write_test.rs`
- SimWeb: `modules/simweb/src/sim.rs`, `modules/rootsignal-scout/tests/scenarios/`
- Scout testing playbook: `docs/tests/scout-testing.md`
