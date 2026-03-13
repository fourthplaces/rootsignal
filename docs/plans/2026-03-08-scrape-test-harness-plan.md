# ScoutRunTest Harness — Boundary Test Refactor

## Problem

Boundary tests in `scrape/boundary_tests.rs` and `scrape/chain_tests.rs` bypass production architecture:

1. Call `scrape_web_sources()` directly (bypasses `prepare_sources` and the scrape handler)
2. Decompose `ScrapeOutput` by hand (`take_events`, `std::mem::take`)
3. Apply state manually (`ctx.apply_scrape_output`, `ctx.stats = state.stats.clone()`)
4. Create throwaway engines with no prior state
5. Start at `SourcesPrepared` instead of `ScoutRunRequested`

This tests a flow that doesn't exist in production. Source selection, state setup via reducers, and event routing are all bypassed.

Additionally, three concrete types on `ScoutEngineDeps` block full testability — handlers silently skip work when these deps are None, making it impossible to test significant parts of the pipeline.

## Production Chain

```
ScoutRunRequested { run_id, scope }
  → lifecycle:prepare_sources  → SourcesPrepared (reads store, builds plan)
  → scrape:start_web_scrape    → WebScrapeCompleted (fetches, extracts)
  → signals:dedup              → WorldEvents / NoNewSignals
  → enrichment gate            → EnrichmentReady
  → enrichment:run_enrichment  → ExpansionReady
  → expansion:expand_signals   → ExpansionCompleted
  → synthesis / curiosity      → ...
```

Tests should enter at `ScoutRunRequested` — the same event production emits.

## Phase 0: Extract Traits for Concrete Dependencies (prerequisite)

Three concrete types on `ScoutEngineDeps` can't be mocked, blocking testability:

### Boundary Audit

| Dep | Type | Mockable? | Consumers | Status |
|-----|------|-----------|-----------|--------|
| `store` | `Arc<dyn SignalReader>` | Yes — `MockSignalReader` | dedup, lifecycle, everywhere | OK |
| `embedder` | `Arc<dyn TextEmbedder>` | Yes — `FixedEmbedder` | dedup similarity | OK |
| `fetcher` | `Option<Arc<dyn ContentFetcher>>` | Yes — `MockFetcher` | scrape | OK |
| `ai` | `Option<Arc<dyn Agent>>` | Yes — `MockAgent` | signal review, curiosity, enrichment | OK |
| `extractor` | `Option<Arc<dyn SignalExtractor>>` | Yes — `MockExtractor` | scrape | OK |
| `budget` | `Option<Arc<BudgetTracker>>` | Yes — `BudgetTracker::new(0)` | curiosity, expansion, discovery | OK |
| `batcher` | `Batcher` | Yes — `Batcher::new()` | signal review | OK |
| **`graph`** | **`Option<GraphReader>`** | **No — concrete** | lifecycle (region plans), curiosity, expansion, enrichment, supervisor | **GAP** |
| **`archive`** | **`Option<Arc<Archive>>`** | **No — concrete** | curiosity (investigation, concern linking, response/gathering finding) | **GAP** |
| **`pg_pool`** | **`Option<PgPool>`** | **No — concrete** | projections (scout_runs), supervisor, embedding store | **GAP** |

### 0a. Extract `GraphReader` → `dyn GraphStore` trait

**Priority: High** — blocks region-run tests AND curiosity/expansion/enrichment tests.

`GraphReader` is a concrete struct wrapping a Neo4j `GraphClient`. Handlers call methods like:
- `build_source_plan_from_region(graph, region)` — loads sources for scheduling
- `graph.get_tension_landscape(...)` — curiosity concern linking
- `graph.get_evidence_summary(...)` — curiosity investigation
- `graph.find_similar_by_embedding(...)` — expansion
- `graph.get_signal_confidence(...)` — confidence scoring

Extract a trait covering the methods handlers actually call. The concrete `GraphReader` implements it. A `MockGraphStore` returns test data.

```rust
// On ScoutEngineDeps:
// Before:
pub graph: Option<GraphReader>,
// After:
pub graph: Option<Arc<dyn GraphStore>>,
```

Without this, `prepare_sources` for region runs falls through to "No region or graph available, skipping source plan" and emits nothing. Tests can only use `RunScope::Sources` to bypass graph reads.

### 0b. Extract `Archive` → `dyn ArchiveReader` trait

**Priority: High** — blocks curiosity investigation tests.

`Archive` is a concrete struct wrapping PgPool + Serper API + Browserless. Curiosity handlers call:
- `archive.source(query).search(query)` — web search for investigation
- Page reading for concern linking / response finding

Extract a trait for the search/read interface. `MockArchive` returns canned search results.

```rust
// Before:
pub archive: Option<Arc<Archive>>,
// After:
pub archive: Option<Arc<dyn ArchiveReader>>,
```

Without this, curiosity handlers silently skip: `(region, graph, budget, archive) = match (...) { ... _ => { emit SignalInvestigated; return } }`. Investigation never runs, confidence stays at 0.5 in tests.

### 0c. `PgPool` — defer

**Priority: Low.** Only used by projections (`scout_runs_projection`) and supervisor. Projections silently skip when `pg_pool` is None. Handler behavior is unaffected. Can test projections separately with integration tests.

## Phase 1: ScoutRunTest Harness

### API

```rust
let harness = ScoutRunTest::new()
    .region(mpls_region())
    .source("https://localorg.org/events", archived_page(url, "Community dinner"))
    .extraction("https://localorg.org/events", ExtractionResult {
        nodes: vec![tension_at("Community Dinner", 44.95, -93.27)],
        ..Default::default()
    })
    .build();

harness.run().await;

assert_eq!(harness.stats().signals_stored, 1);
assert_eq!(harness.store().node_count(), 1);
```

### Surface

| Method | Purpose |
|--------|---------|
| `.region(scope)` | Set the scout run's region |
| `.source(url, page)` | Register source in MockSignalReader AND page in MockFetcher |
| `.social_source(url, posts)` | Register social source + mock social fetcher response |
| `.extraction(url, result)` | Register mock extractor response for a URL |
| `.embedder(impl)` | Override default FixedEmbedder (for dedup similarity tests) |
| `.graph(impl)` | Inject `MockGraphStore` (after Phase 0a) |
| `.archive(impl)` | Inject `MockArchive` (after Phase 0b) |
| `.ai(impl)` | Inject `MockAgent` for LLM-dependent handlers |
| `.build()` | Construct engine with all mocks wired, return harness |
| `.run()` | Emit `ScoutRunRequested { run_id, scope }`, settle |
| `.stats()` | `engine.singleton::<PipelineState>().stats` |
| `.store()` | Access MockSignalReader for node/actor assertions |
| `.captured()` | Access captured events for event-level assertions |

### What `.source()` does internally

One call sets up both sides of the boundary:
1. Creates a `SourceNode` with the URL and registers it in `MockSignalReader` (so `prepare_sources` finds it during source selection)
2. Registers the page content in `MockFetcher` (so `start_web_scrape` fetches it)

### What `.run()` does internally

```rust
pub async fn run(&self) {
    self.engine.emit(LifecycleEvent::ScoutRunRequested {
        run_id: self.run_id,
        scope: self.scope.clone(),
    }).settled().await.unwrap();
}
```

That's it. The engine's real handlers do everything else.

### What `.build()` does internally

1. Creates `MockFetcher` from registered pages
2. Creates `MockExtractor` from registered extractions
3. Populates `MockSignalReader` with registered sources
4. Wires ALL deps into `ScoutEngineDeps` (fetcher, extractor, store, embedder, graph, archive, ai, captured_events)
5. Calls `build_engine(deps, None)` — real engine, real handlers, no seesaw store

### RunScope selection

The harness uses `RunScope::Sources` when `.source()` is called (source-targeted run).
After Phase 0a, `.region()` without `.source()` uses `RunScope::Region` with `MockGraphStore` providing sources.

## Phase 2: Migrate Boundary Tests

~50 boundary tests follow the same pattern. Migration is mechanical:

**Before:**
```rust
let fetcher = MockFetcher::new().on_page(url, archived_page(url, content));
let extractor = MockExtractor::new().on_url(url, ExtractionResult { ... });
let store = Arc::new(MockSignalReader::new());
let deps = test_scrape_deps(store.clone(), Arc::new(extractor), Arc::new(fetcher));
let source = page_source(url);
let sources: Vec<&SourceNode> = vec![&source];
let mut ctx = PipelineState::from_sources(&[source.clone()]);
let output = scrape_web_sources(&deps, &sources, &ctx.url_to_canonical_key, &ctx.actor_contexts).await;
scrape_and_dispatch(output, &mut ctx, &store).await;
assert_eq!(ctx.stats.signals_stored, 1);
```

**After:**
```rust
let harness = ScoutRunTest::new()
    .region(mpls_region())
    .source(url, archived_page(url, content))
    .extraction(url, ExtractionResult { ... })
    .build();

harness.run().await;

assert_eq!(harness.stats().signals_stored, 1);
```

## What gets deleted

- `dispatch_events()` / `dispatch_events_with()` helpers in both test files
- `scrape_and_dispatch()` / `scrape_and_dispatch_with()` helpers
- `PipelineState::from_sources()` usage in tests (reducer handles via SourcesPrepared)
- Manual `ctx.stats = state.stats.clone()` syncing
- `test_scrape_deps()` — replaced by ScoutRunTest builder
- `sources_prepared_event()` / `sources_prepared_with_web_urls()` — tests no longer construct SourcesPrepared

## What stays

- `MockFetcher`, `MockExtractor`, `MockSignalReader` — boundary mocks, correct abstraction level
- `archived_page()`, `tension_at()`, `mpls_region()` — test data factories
- `FixedEmbedder` — controls dedup similarity behavior

## Testing Tiers

Engine handlers actively read from Neo4j during runs — ~40+ queries for cross-run historical data (dedup, actor resolution, situation weaving, expansion similarity, confidence scoring). This means in-memory mocks can't serve all tests.

### Tier 1: In-Memory Mocks (Phase 1-2)

`MockGraphStore` returns canned data. Suitable for:
- Scrape → dedup pipeline (single-run, no prior signals)
- Zero-signal / NoNewSignals paths
- Extraction edge cases (malformed HTML, missing fields, dedup collisions)
- Enrichment gate logic (counters, completion filters)
- Source selection (`RunScope::Sources` bypasses graph entirely)

This covers the ~60 boundary tests being migrated. `MockGraphStore` methods return empty vecs / None by default — handlers that need graph data gracefully degrade or skip.

### Tier 2: Testcontainer Neo4j (Phase 3)

Real Neo4j for tests that need cross-run historical context. The pattern:

```
1. Spin up shared testcontainer Neo4j (once per test run, ~10s)
2. Seed: project historical events onto Neo4j (prior scout run's signals, actors, situations)
3. Run: execute ScoutRunTest with graph pointed at testcontainer
4. Assert: check projected state in Neo4j after run completes
5. Wipe: clear Neo4j between tests (serialized via mutex)
```

#### Why serialized access

The engine reads from Neo4j *during* `.run()`. If two tests share the same Neo4j instance concurrently, test A's projected seed data leaks into test B's handler reads. Serialization via mutex guarantees isolation:

```rust
static NEO4J: Lazy<Mutex<TestNeo4j>> = Lazy::new(|| {
    Mutex::new(TestNeo4j::start().await)
});

async fn with_graph<F, Fut>(seed_events: Vec<DomainEvent>, test: F)
where F: FnOnce(Arc<GraphReader>) -> Fut, Fut: Future<Output = ()>
{
    let guard = NEO4J.lock().await;
    guard.wipe().await;
    guard.project(seed_events).await;  // historical context
    test(guard.reader()).await;
    // guard drops, mutex released
}
```

#### What Tier 2 tests cover

Tests that exercise handler reads against real graph state:
- **Dedup**: prior signals exist in graph → new signal matches → deduplicated
- **Actor resolution**: prior actors exist → new signal references same actor → merged
- **Situation weaving**: prior situations exist → new signal contributes evidence → woven in
- **Expansion**: prior signals with embeddings → find_similar_by_embedding returns neighbors
- **Confidence**: evidence summary read from projected graph → adjustment computed
- **Region source plans**: `build_source_plan_from_region` reads sources from graph

#### Projection as the system under test

Tier 2 tests also validate that event projection works correctly. The seed step projects events through the real `GraphProjector` — if projection is broken, the seed fails, and the test fails for the right reason. This replaces separate projection unit tests.

### Tier selection in ScoutRunTest

```rust
// Tier 1: in-memory (default)
let harness = ScoutRunTest::new()
    .source(url, page)
    .extraction(url, result)
    .build();

// Tier 2: testcontainer
let harness = ScoutRunTest::new()
    .region(mpls_region())
    .graph(neo4j_reader)          // real GraphReader from testcontainer
    .seed_events(prior_run_events) // projected before .run()
    .source(url, page)
    .extraction(url, result)
    .build();
```

The harness API is the same — `.graph()` accepts either `MockGraphStore` or real `GraphReader` (both implement `dyn GraphStore` after Phase 0a). The only difference is what backs the graph.

## Phase 3: Testcontainer Integration Tests

### Setup
- `modules/rootsignal-scout/tests/integration/` — new integration test directory
- Shared `TestNeo4j` helper: start container, expose `GraphReader`, wipe, project
- `#[ignore]` attribute or feature flag for CI (testcontainer requires Docker)

### Tests to write
- `dedup_against_prior_run_signals` — seed signals from run A, run B produces same signal → deduped
- `actor_resolution_across_runs` — seed actor from run A, run B references same entity → merged
- `situation_weaving_across_runs` — seed situation, new evidence contributes → woven
- `region_source_plan_from_graph` — seed sources in graph, `RunScope::Region` finds them
- `expansion_finds_similar_signals` — seed signals with embeddings, expansion returns neighbors
- `projection_round_trip` — emit events → project → read back → verify graph state

### Files
- `modules/rootsignal-scout/tests/integration/mod.rs`
- `modules/rootsignal-scout/tests/integration/neo4j_harness.rs`
- `modules/rootsignal-scout/tests/integration/cross_run_tests.rs`

## Files

### Phase 0
- `modules/rootsignal-graph/src/writer.rs` — extract `GraphStore` trait from `GraphReader`
- `modules/rootsignal-archive/src/archive.rs` — extract `ArchiveReader` trait from `Archive`
- `modules/rootsignal-scout/src/core/engine.rs` — change deps to `dyn GraphStore`, `dyn ArchiveReader`
- `modules/rootsignal-scout/src/testing.rs` — add `MockGraphStore`, `MockArchive`

### Phase 1
- `modules/rootsignal-scout/src/testing.rs` — add `ScoutRunTest` builder

### Phase 2
- `modules/rootsignal-scout/src/domains/scrape/boundary_tests.rs` — migrate ~50 tests
- `modules/rootsignal-scout/src/domains/scrape/chain_tests.rs` — migrate ~10 tests

### Phase 3
- `modules/rootsignal-scout/tests/integration/mod.rs` — integration test entry
- `modules/rootsignal-scout/tests/integration/neo4j_harness.rs` — TestNeo4j: container lifecycle, wipe, project, mutex
- `modules/rootsignal-scout/tests/integration/cross_run_tests.rs` — cross-run dedup, actor resolution, situation weaving, expansion, projection round-trip
