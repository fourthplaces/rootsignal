---
date: 2026-02-20
topic: scout-pipeline-stages
---

# Scout Pipeline Stage Reorganization

## What We're Building

Reorganizing `scout.rs` (103KB) from a monolithic orchestrator into stage-based modules with typed boundaries. The goals are:

1. **Maintainability** — `scout.rs` does too much: URL resolution, scraping, extraction, deduplication, metrics, expansion, and orchestration. Each concern should live in its own module.
2. **Testability** — Each stage should be testable in isolation with mocked dependencies.
3. **Restate readiness** — Stage boundaries should map cleanly to durable workflow steps for future Restate integration (see `docs/brainstorms/2026-02-19-restate-durable-execution-brainstorm.md`).

## Why This Approach

### Boundaries we explored but rejected

**Scrape vs. Store separation** (`scrape_and_extract() -> Vec<Signal>`, then `store_and_dedup()`):
Rejected because three pieces of shared mutable state cross that boundary:

- **EmbeddingCache** — catches duplicates between signals extracted in the same run that haven't been indexed in the graph yet. Must accumulate across Phase A web, Phase A social, and Phase B scrapes. Splitting scrape from store breaks cross-batch dedup.
- **`url_to_canonical_key`** — mutated during WebQuery URL resolution (scraping) and consumed during signal attribution (storage). Crosses the scrape/store boundary.
- **`source_signal_counts`** — accumulated during scraping across all phases, consumed by metrics at the end.

Additionally, mid-run discovery queries the graph for Phase A's stored tensions. Storage must complete before discovery runs — you can't defer it.

### Boundaries that are natural

The pipeline already has a two-phase detection model (Find Problems → Find Responses) with discovery in between. Phase A and Phase B run the exact same scrape→extract→store→dedup pipeline, just with different source lists. The individual synthesis finders (tension_linker, response_finder, gathering_finder, investigator) are already well-isolated modules. The scheduler is already its own module.

The natural boundary isn't scrape vs. store — it's **per-phase execution** vs. **cross-phase orchestration**.

## The Design

### RunContext — shared state for the entire run

A single struct that flows through all stages, like a Redux store. Every stage reads from and writes to it.

```rust
struct RunContext {
    embed_cache: EmbeddingCache,              // cross-batch dedup
    url_to_canonical_key: HashMap<String, String>, // source attribution
    source_signal_counts: HashMap<String, u32>,    // metrics accumulation
    expansion_queries: Vec<String>,                // discovery fuel
    stats: ScoutStats,                             // run-wide counters
}
```

Coupled by lifetime (all live for the full run), owned by the orchestrator, passed as `&mut` to each stage.

### 7 Stages

| # | Stage | Module | Input | Output / Side Effects |
|---|-------|--------|-------|----------------------|
| 1 | **Schedule** | `scheduler.rs` (exists) | Sources from graph | `SchedulePlan` — tension phase, response phase, exploration lists |
| 2 | **Phase A: Scrape-Store-Dedup** | `scrape_phase.rs` (new) | Tension sources + `&mut RunContext` | Signals stored in graph, RunContext updated (embed cache, signal counts, expansion queries, URL map) |
| 3 | **Mid-run Discovery** | `source_finder.rs` (exists) | Fresh tensions in graph | New sources + social topics stored in graph |
| 4 | **Phase B: Scrape-Store-Dedup** | Same `scrape_phase.rs` | Response sources + discovery sources + `&mut RunContext` | Same as Phase A |
| 5 | **Synthesis** | Existing finder modules | Graph state | New edges (RESPONDS_TO, CONTRIBUTES_TO, DRAWN_TO, Evidence) |
| 6 | **Expansion** | `expansion.rs` (new) | `RunContext.expansion_queries` + deferred queries from response mapping | New WebQuery sources in graph |
| 7 | **Metrics** | `metrics.rs` (new) | `RunContext.source_signal_counts` + scrape outcomes | Updated weights, cadences, deactivated dead sources |

### Orchestrator

`scout.rs` shrinks to a thin orchestrator:

```rust
// 1. Schedule
let plan = scheduler.schedule(&sources);
let mut ctx = RunContext::new(embed_cache, &sources);

// 2. Phase A: Find Problems
scrape_phase.run(&plan.tension_sources, &mut ctx).await;
scrape_phase.run_social(&plan.tension_social, &mut ctx).await;

// 3. Mid-run Discovery
let discovered = discovery.run().await;

// 4. Phase B: Find Responses (includes discovery sources)
let phase_b_sources = plan.response_sources + discovered;
scrape_phase.run(&phase_b_sources, &mut ctx).await;
scrape_phase.run_social(&plan.response_social, &mut ctx).await;

// 5. Synthesis (parallel finders + story weaving)
synthesis.run().await;

// 6. Expansion
expansion.run(&mut ctx).await;

// 7. Metrics
metrics.update(&ctx).await;
```

### What moves where

| From `scout.rs` | To |
|------------------|----|
| `scrape_phase()`, `scrape_social_media()`, `discover_from_topics()`, `store_signals()`, `refresh_url_signals()`, URL resolution, content hashing | `scrape_phase.rs` |
| Expansion query dedup (Jaccard + embedding), source creation | `expansion.rs` |
| `record_source_scrape()`, weight computation, cadence calculation, dead source deactivation | `metrics.rs` |
| Everything else (phase orchestration, RunContext wiring) | Stays in `scout.rs` |

### What doesn't change

- `scheduler.rs` — already isolated
- `source_finder.rs` — already isolated
- `tension_linker.rs`, `response_finder.rs`, `gathering_finder.rs`, `investigator.rs` — already isolated
- `extractor.rs`, `scraper.rs`, `embedder.rs`, `budget.rs`, `quality.rs` — already isolated
- Trait abstractions (`SignalExtractor`, `PageScraper`, `WebSearcher`, `TextEmbedder`, `SocialScraper`) — unchanged

## Key Decisions

- **Scrape + store stay together**: The EmbeddingCache, URL map, and signal counts couple them. Forcing separation would require redesigning dedup semantics for no practical gain.
- **RunContext as shared state**: Ergonomic over explicit parameter threading. Every stage gets `&mut RunContext`. Trade-off: stages can access state they don't need. Acceptable for a single-binary pipeline.
- **Phase A and Phase B share `ScrapePhase`**: Same logic, different source lists. No need for distinct types.
- **Consolidate expansion**: Today expansion queries are collected in 3 places (scrape_phase, social scraping, deferred after response mapping). Consolidate into one module that collects from RunContext + graph.

## Open Questions

- Should `ScrapePhase` own references to scraper/extractor/writer, or receive them per call?
- Does `RunContext` need interior mutability for any concurrent access, or is `&mut` always sufficient given the sequential phase structure?
- Should post-run steps (merge duplicates, actor extraction, cause heat) be a formal stage or remain in `main.rs`?

## Next Steps

1. `/workflows:plan` for implementation — file-by-file changes, extraction order, test strategy
2. Consider writing `RunContext` and the new `scrape_phase.rs` first as the highest-value extraction
