# Scout Architecture

Scout is the automated signal collection engine for Root Signal. It discovers, extracts, deduplicates, and graphs **signals** — actionable information about community resources, events, needs, and tensions.

A single scout run executes a 10-stage pipeline, from source scheduling through agentic discovery.

## Pipeline Overview

```
 1. Reap expired signals
 2. Load sources + schedule
 3. Phase A — scrape tension sources (web + social)
 4. Mid-run source discovery
 5. Phase B — scrape response sources + fresh discovery sources
 6. Source metrics + weight updates
 7. Parallel synthesis (response mapping, tension linker, finders, investigation)
 8. Story weaving (Leiden clustering + LLM narratives)
 9. Signal expansion (implied queries → new sources)
10. End-of-run source discovery
```

## Data Model

Scout operates on six signal types, all sharing a common `NodeMeta`:

| Type | Description | Expiry |
|------|-------------|--------|
| **Gathering** | Time-bound events — protests, cleanups, workshops, meetings | 30 days past `ends_at` |
| **Aid** | Available resources — food shelves, free clinics, tool libraries | 60 days without re-confirmation |
| **Need** | Community requests — volunteer calls, donation drives | 60 days |
| **Notice** | Official advisories — policy changes, shelter openings | 90 days |
| **Tension** | Systemic conflicts — housing crises, environmental harm | Never (persistent) |
| **Evidence** | Source citations — URLs, content hashes, retrieval timestamps | Tied to parent signal |

Every signal carries:

- `confidence` (0.0–1.0) — quality score based on completeness + geo accuracy
- `source_diversity` — count of distinct entity sources confirming the signal
- `cause_heat` (0.0–1.0) — cross-story attention spillover
- `location` — optional lat/lng with precision level (exact / neighborhood / city)
- `implied_queries` — follow-up search terms for expansion discovery
- `mentioned_actors` — organizations and people referenced

## Stage Details

### 1. Expiry Reap

Removes signals past their type-specific TTL. Any signal older than 365 days is dropped regardless of type.

### 2. Source Scheduling

`SourceScheduler` decides which sources to scrape this run.

- **Cadence-based**: Each source has a weight-derived cadence (hours between scrapes). Higher-signal sources get scraped more frequently.
- **Exploration**: 10% of slots are reserved for random sampling of low-weight stale sources, ensuring cold corners of the graph get periodic attention.
- **Phase partitioning**: Sources are split into **Tension** (discovery of problems) and **Response** (discovery of solutions) phases based on their `SourceRole`.
- **Web query tiering**: Search queries are separately scheduled with their own tier logic.

### 3. Phase A — Find Problems

Scrapes tension-role and mixed-role sources to discover community problems, needs, and tensions.

**Web scraping pipeline:**
1. Deduplicate URLs across sources
2. Scrape via headless Chrome (Browserless or local) with Readability extraction — 10 concurrent, 30s timeout
3. Content hash check (FNV-1a) — skip unchanged pages
4. LLM extraction (Claude Haiku) — structured JSON with signals, actors, queries, resources, tags
5. Quality scoring: `confidence = completeness × 0.5 + geo_accuracy × 0.5`
6. Geo filtering — strip fake city-center coordinates (within ±0.01° of center), validate `location_name` against city geo terms
7. 3-layer deduplication (see below)
8. Embed (Voyage AI, 1024-dim) + store in graph with Evidence audit trail

**Social scraping** runs in parallel via Apify (Instagram, Facebook, Reddit).

### 4. Mid-Run Source Discovery

`SourceFinder` analyzes the current state of the graph — unmet tensions, signal imbalances, actor gaps — and uses Claude to propose new sources (URLs, queries, hashtags) before Phase B begins.

### 5. Phase B — Find Responses

Scrapes response-role sources plus any fresh sources created by mid-run discovery. Same web + social pipeline as Phase A, but focused on aid, gatherings, and initiatives addressing known tensions.

### 6. Metrics Update

`Metrics` records per-source scrape statistics and recomputes:

- **Weight**: `base_weight = (signals × corroboration) / scrape_count`
- **Cadence**: Higher weight → more frequent scraping
- **Deactivation**: Sources with 10+ consecutive empty runs are deactivated. Queries with 5+ empty runs, 3+ total scrapes, and 0 signals are deactivated.

### 7. Parallel Synthesis

Five independent processes run concurrently via `tokio::join!`:

| Process | Purpose |
|---------|---------|
| **Response Mapping** | LLM determines which Aid/Gathering signals address Need/Tension signals → `RESPONDS_TO` edges |
| **Tension Linker** | Agentic search linking orphaned signals to existing tensions |
| **Response Finder** | Agentic investigation discovering ecosystem responding to top tensions (legal aid, mutual aid, fundraising) |
| **Gathering Finder** | Agentic investigation discovering where people physically gather around tensions (vigils, town halls, solidarity meals) |
| **Investigation** | Web search corroboration for low-confidence signals → additional Evidence nodes |

All synthesis stages are budget-gated — they check remaining daily budget before running.

### 8. Story Weaving

Runs **after** synthesis (reads the similarity edges and response mappings created above).

- Builds `SIMILAR_TO` edges between signals using vector similarity
- Clusters signals into **Stories** using Leiden community detection
- LLM generates story titles and summaries
- Computes story metrics: energy, velocity, arc (emerging / growing / stable / fading)

### 9. Signal Expansion

Collects `implied_queries` from Tension/Need signals (immediate) and from Aid/Gathering signals linked to high-heat tensions (deferred). Deduplicates via Jaccard similarity + embedding cosine distance, then creates up to 10 new `DiscoveryMethod::SignalExpansion` sources for the next run.

### 10. End-of-Run Discovery

A second pass of `SourceFinder` with updated graph state, creating sources for the next scout run.

## 3-Layer Deduplication

Deduplication is the most critical quality gate. It prevents signal flooding while ensuring corroboration is tracked.

```
Signal extracted from page
    │
    ├─ Layer 1: Within-batch exact
    │  Title + type HashSet — catches duplicates from the same scrape batch
    │
    ├─ Layer 2: Graph exact match
    │  Query graph for title+type match (URL-scoped first, then global)
    │  Match → corroborate (increment source_diversity, update freshness)
    │
    └─ Layer 3: Vector similarity
       Embed signal text (Voyage AI, 1024-dim)
       ├─ In-memory EmbeddingCache: threshold 0.85 (same source) / 0.92 (cross-source)
       └─ Graph vector index: same thresholds
       Match → corroborate + create Evidence node
       New   → create node + embed + Evidence
```

## Agentic Investigation

The gathering finder, response finder, and tension linker are **agentic** — they use Claude with tool-use (`web_search` + `read_page`) to autonomously investigate tensions. Each agent:

1. Receives a tension or signal as context
2. Formulates search queries
3. Reads and evaluates web pages
4. Returns structured discoveries with `match_strength` (0.0–1.0) and explanation
5. New signals are stored through the standard dedup pipeline

## Budget System

`BudgetTracker` enforces a configurable daily spend limit (`DAILY_BUDGET_CENTS`). Each operation type has an estimated cost:

- LLM calls (extraction, synthesis, investigation, story weaving)
- Web search queries (Serper)

Synthesis stages check `has_budget()` before running and gracefully skip if exhausted.

## Concurrency Model

| Operation | Concurrency | Notes |
|-----------|-------------|-------|
| Web scraping | `buffer_unordered(10)` | 30s timeout per URL |
| Social scraping | `buffer_unordered(10)` | Via Apify |
| Web search | `buffer_unordered(5)` | Via Serper |
| Synthesis | 5 concurrent tasks | `tokio::join!` |
| Signal storage | Sequential | Embedding cache + dedup ordering |
| Scout lock | Exclusive per city | `ScoutLock` node in graph |

## Extension Points

All core operations use async trait objects for dependency injection and testability:

```rust
trait SignalExtractor   // LLM signal extraction → ExtractionResult
trait TextEmbedder      // Vector embeddings (Voyage AI)
trait PageScraper       // Web page fetching (Chrome / Browserless)
trait WebSearcher       // Web search API (Serper)
trait SocialScraper     // Social media scraping (Apify)
```

Tests use the `simweb` crate (simulated web) and mock trait implementations for deterministic, offline testing.

## Graph Schema

```
Nodes                          Relationships
─────                          ─────────────
Gathering, Aid, Need,          Signal ──SOURCED_FROM──▶ Evidence
Notice, Tension, Evidence,     Actor  ──ACTED_IN──▶ Signal
Story, Actor, Source,          Signal ──RESPONDS_TO──▶ Tension
City, Resource, Tag,           Story  ──CONTAINS──▶ Signal
Edition, Lock                  Story  ──EVOLVED_FROM──▶ Story
                               Signal ──SIMILAR_TO──▶ Signal

Indices
───────
- Vector index (1024-dim per signal)
- Content hash + URL (dedup)
- Title + type (global dedup)
- Source canonical key
```

## Safety and Quality

| Layer | Mechanism |
|-------|-----------|
| PII detection | Regex patterns for SSN, phone, email, credit card — can drop signals |
| Sensitivity levels | General / Elevated / Sensitive — filtered from public API |
| Geo filtering | Strip fake city-center coords, validate against geo terms |
| Quality scoring | Completeness + geo accuracy = confidence (0.0–1.0) |
| Source diversity | Corroboration from distinct entities increases trust |
| Evidence trail | Every signal linked to source(s) with retrieval timestamp + content hash |
| Budget limits | Per-operation cost estimation, configurable daily cap |
| Review gate | Supervisor batch-reviews new signals; only `review_status = 'live'` shown to users |
