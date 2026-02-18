# Scout System Architecture

Scout is the automated civic intelligence collection engine for Root Signal. It discovers, extracts, deduplicates, embeds, and graphs **civic signals** — actionable information about community resources, events, needs, and tensions.

## System Diagram

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           SCOUT PIPELINE                                │
│                         (rootsignal-scout)                              │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  ┌──────────────────── PHASE 1: SOURCE COLLECTION ──────────────────┐  │
│  │                                                                   │  │
│  │  ┌─────────────┐  ┌──────────────┐  ┌────────────────────────┐  │  │
│  │  │  Curated     │  │  Tavily Web  │  │  Social Media (Apify)  │  │  │
│  │  │  Sources     │  │  Search      │  │                        │  │  │
│  │  │  (~35 URLs)  │  │  (~35 queries│  │  Instagram  Facebook   │  │  │
│  │  │  per city    │  │   per city)  │  │  Reddit    Hashtags    │  │  │
│  │  └──────┬───────┘  └──────┬───────┘  └───────────┬────────────┘  │  │
│  │         │                 │                       │               │  │
│  │         ▼                 ▼                       │               │  │
│  │  ┌──────────────────────────────┐                │               │  │
│  │  │  URL Dedup + Sort            │                │               │  │
│  │  └──────────────┬───────────────┘                │               │  │
│  └─────────────────┼────────────────────────────────┼───────────────┘  │
│                    │                                │                   │
│  ┌─────────────────▼──── PHASE 2: SCRAPING ─────────▼───────────────┐  │
│  │                                                                   │  │
│  │  ┌─────────────────────────┐    ┌──────────────────────────────┐ │  │
│  │  │  ChromeScraper           │    │  Apify Social Scraper       │ │  │
│  │  │  (headless Chromium      │    │  (10 posts/account)         │ │  │
│  │  │   + Readability)         │    │                              │ │  │
│  │  │  buffer_unordered(10)    │    │  buffer_unordered(10)       │ │  │
│  │  │  30s timeout per URL     │    │                              │ │  │
│  │  └──────────┬──────────────┘    └────────────┬─────────────────┘ │  │
│  │             │                                │                    │  │
│  │             ▼                                │                    │  │
│  │  ┌─────────────────────────┐                │                    │  │
│  │  │  Content Hash Check     │                │                    │  │
│  │  │  (FNV-1a)               │                │                    │  │
│  │  │  Unchanged? → Skip      │                │                    │  │
│  │  └──────────┬──────────────┘                │                    │  │
│  └─────────────┼────────────────────────────────┼────────────────────┘  │
│                │                                │                       │
│  ┌─────────────▼──── PHASE 3: LLM EXTRACTION ──▼───────────────────┐   │
│  │                                                                   │  │
│  │  ┌───────────────────────────────────────────────────────────┐   │  │
│  │  │  Claude Haiku (claude-haiku-4-5-20251001)                 │   │  │
│  │  │                                                           │   │  │
│  │  │  Input: page content (≤30K chars) + source URL            │   │  │
│  │  │  Output: JSON → Vec<ExtractedSignal>                      │   │  │
│  │  │                                                           │   │  │
│  │  │  Signal Types:                                            │   │  │
│  │  │    Event   - Time-bound gathering/activity                │   │  │
│  │  │    Give    - Available resource/service                   │   │  │
│  │  │    Ask     - Community need requesting help               │   │  │
│  │  │    Notice  - Official advisory/policy change              │   │  │
│  │  │    Tension - Systemic conflict/misalignment               │   │  │
│  │  └───────────────────────────┬───────────────────────────────┘   │  │
│  └──────────────────────────────┼────────────────────────────────────┘  │
│                                 │                                       │
│  ┌──────────────────────────────▼─── PHASE 4: QUALITY + GEO ────────┐  │
│  │                                                                   │  │
│  │  Quality Score:  confidence = completeness×0.4 + geo×0.3         │  │
│  │                              + freshness×0.3                      │  │
│  │                                                                   │  │
│  │  Geo Filter:  Strip fake city-center coords (±0.01°)             │  │
│  │               Check location_name against CityProfile.geo_terms  │  │
│  │               Off-geography → drop                                │  │
│  └──────────────────────────────┬────────────────────────────────────┘  │
│                                 │                                       │
│  ┌──────────────────────────────▼─── PHASE 5-8: 3-LAYER DEDUP ──────┐  │
│  │                                                                   │  │
│  │  Layer 1: Within-batch exact title + type (HashSet)              │  │
│  │                          │                                        │  │
│  │                          ▼                                        │  │
│  │  Layer 2: URL-scoped title match (graph query)                   │  │
│  │           + Global exact title+type match                         │  │
│  │           Match? → corroborate (↑ source_diversity)              │  │
│  │                          │                                        │  │
│  │                          ▼                                        │  │
│  │  Layer 3: Vector similarity (Voyage AI, 1024-dim)                │  │
│  │           ┌─────────────────────────────────────┐                │  │
│  │           │  In-memory EmbeddingCache            │                │  │
│  │           │  threshold: 0.85 (same src)          │                │  │
│  │           │            0.92 (cross-src)          │                │  │
│  │           └──────────────┬──────────────────────┘                │  │
│  │           ┌──────────────▼──────────────────────┐                │  │
│  │           │  Memgraph Vector Index                │                │  │
│  │           │  threshold: 0.85 / 0.92             │                │  │
│  │           └──────────────┬──────────────────────┘                │  │
│  │                          │                                        │  │
│  │           Duplicate? → corroborate + Evidence node                │  │
│  │           New?       → create_node + embed + Evidence             │  │
│  └──────────────────────────┬────────────────────────────────────────┘  │
│                             │                                           │
│  ┌──────────────────────────▼─── PHASE 9: ACTOR LINKING ─────────────┐ │
│  │  mentioned_actors → find_or_create Actor → link to signal         │ │
│  └──────────────────────────┬────────────────────────────────────────┘ │
│                             │                                           │
│  ┌──────────────────────────▼─── PHASE 10: POST-PROCESSING ─────────┐  │
│  │                                                                   │  │
│  │  ┌──────────────┐  ┌────────────────┐  ┌──────────────────────┐  │  │
│  │  │  Clustering   │  │  Response       │  │  Investigation      │  │  │
│  │  │  (Leiden      │  │  Mapping        │  │                     │  │  │
│  │  │   algorithm)  │  │                 │  │  Top 5 signals      │  │  │
│  │  │              │  │  Give/Event     │  │  ≤3 Tavily queries  │  │  │
│  │  │  Groups      │  │  addresses      │  │  per signal         │  │  │
│  │  │  signals     │  │  Ask/Tension    │  │  ≤10 queries total  │  │  │
│  │  │  into        │  │                 │  │                     │  │  │
│  │  │  Stories     │  │  (Sonnet LLM)   │  │  Claude evaluates   │  │  │
│  │  └──────┬───────┘  └───────┬─────────┘  │  new Evidence       │  │  │
│  │         │                  │             └──────────┬───────────┘  │  │
│  │         ▼                  ▼                        ▼              │  │
│  │  ┌─────────────────────────────────────────────────────────────┐  │  │
│  │  │  Cause Heat Computation                                     │  │  │
│  │  │  For each signal: Σ(similarity × neighbor.source_diversity) │  │  │
│  │  │  where neighbor similarity > 0.7                            │  │  │
│  │  │  Normalized 0.0–1.0                                         │  │  │
│  │  └─────────────────────────────────────────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘

                                 │
                                 ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                           NEO4J / MEMGRAPH                              │
│                                                                         │
│  Nodes:                          Relationships:                         │
│  ┌─────────┐ ┌──────────┐     Signal ──SOURCED_FROM──▶ Evidence        │
│  │ Event   │ │ Give     │     Actor ──ACTED_IN──▶ Signal               │
│  │ Ask     │ │ Notice   │     Give/Event/Ask ──RESPONDS_TO──▶ Tension  │
│  │ Tension │ │ Evidence │     Story ──CONTAINS──▶ Signal               │
│  │ Actor   │ │ Story    │     Story ──EVOLVED_FROM──▶ Story            │
│  │ Source  │ │ Edition  │     Edition ──FEATURES──▶ Story              │
│  │ Submit. │ │ Lock     │     Signal ──SIMILAR_TO──▶ Signal            │
│  └─────────┘ └──────────┘     Submission ──SUBMITTED_FOR──▶ Source     │
│                                                                         │
│                                Indices:                                │
│                                  - Vector (1024-dim per signal)         │
│                                  - Content hash + URL (dedup)           │
│                                  - Title + type (global dedup)          │
└─────────────────────────────────────────────────────────────────────────┘
```

## External Service Dependencies

```
┌─────────────┐     ┌──────────────┐     ┌──────────────┐     ┌──────────┐
│  Anthropic   │     │  Voyage AI   │     │  Tavily      │     │  Apify   │
│  (Claude)    │     │              │     │              │     │          │
│              │     │  voyage-3-   │     │  Web Search  │     │  Social  │
│  Haiku: ext- │     │  large       │     │  API         │     │  Media   │
│  raction,    │     │  1024-dim    │     │  ~35 queries │     │  Scraper │
│  investigate │     │  embeddings  │     │  + invest.   │     │          │
│              │     │              │     │              │     │  IG/FB/  │
│  Sonnet:     │     │  1 batch     │     │  "advanced"  │     │  Reddit  │
│  clustering, │     │  call per    │     │  depth       │     │          │
│  response    │     │  run         │     │              │     │          │
│  mapping     │     │              │     │              │     │          │
└─────────────┘     └──────────────┘     └──────────────┘     └──────────┘
```

## Signal Lifecycle

```
Source (URL/Post)
  │
  ├─ Scrape ──▶ Raw HTML/Text
  │
  ├─ Extract (Haiku) ──▶ ExtractedSignal { title, summary, type, location, timing, ... }
  │
  ├─ Quality Score ──▶ confidence (0.0–1.0)
  │
  ├─ Geo Filter ──▶ drop off-geography, strip fake coords
  │
  ├─ 3-Layer Dedup ──▶ new? CREATE │ duplicate? CORROBORATE (↑ source_diversity)
  │
  ├─ Embed (Voyage) ──▶ 1024-dim vector stored on node
  │
  ├─ Evidence ──▶ audit trail linking signal ←→ source
  │
  ├─ Cluster (Leiden) ──▶ Story membership
  │
  ├─ Response Map (Sonnet) ──▶ Give/Event RESPONDS_TO Ask/Tension
  │
  ├─ Investigate (Tavily + Haiku) ──▶ additional Evidence nodes
  │
  └─ Cause Heat ──▶ cross-story attention boosting (0.0–1.0)
```

## Key Scoring Dimensions

| Metric | Source | Range | Purpose |
|---|---|---|---|
| `confidence` | Quality scorer | 0.0–1.0 | completeness×0.4 + geo×0.3 + freshness×0.3 |
| `freshness_score` | Time decay | 0.0–1.0 | Recency weight |
| `source_diversity` | Corroboration | 1+ | Distinct entity sources confirming signal |
| `corroboration_count` | Dedup layer 2.5+ | 0+ | Times seen from different sources |
| `external_ratio` | Evidence analysis | 0.0–1.0 | Fraction of evidence from external sources |
| `cause_heat` | Cross-story algo | 0.0–1.0 | Multi-source attention spillover |

## Concurrency Model

- **Web scraping**: `buffer_unordered(10)` — 10 concurrent Chrome + extraction pipelines
- **Web search**: `buffer_unordered(5)` — 5 concurrent Tavily queries
- **Social scraping**: `buffer_unordered(10)` — 10 concurrent Apify calls
- **Signal storage**: Sequential (graph writes + embedding cache must be ordered)
- **Scout lock**: Mutual exclusion via `ScoutLock` node in graph (prevents concurrent runs)

## City Configuration

Each city profile provides:
- **~35 curated source URLs** (nonprofits, gov sites, community orgs)
- **~35 Tavily search queries** (volunteer, food bank, advocacy, etc.)
- **~20 Instagram accounts** + Facebook pages + Reddit subreddits
- **~5 discovery hashtags** for finding new sources automatically
- **Entity mappings** linking domains to Instagram/Facebook/Reddit for cross-platform dedup
- **Geo terms** for filtering (city names, neighborhoods, state)

Active cities: **Twin Cities**, NYC, Portland, Berlin

## Signal Expiry

| Type | Expires After |
|---|---|
| Event | 30 days past `ends_at` |
| Give | 60 days without re-confirmation |
| Ask | 60 days |
| Notice | 90 days |
| Tension | Never (persistent structural issues) |
| Any signal | Dropped at extraction if >365 days old |

## Key Traits (Extension Points)

```rust
trait SignalExtractor   // LLM signal extraction from content
trait TextEmbedder      // Vector embeddings for similarity
trait PageScraper       // Web page content fetching
trait WebSearcher       // Web search API
trait SocialScraper     // Social media post fetching
```

All traits are `async + Send + Sync`, enabling both production clients and test fixtures/mocks.

## Core Struct

```rust
pub struct Scout {
    graph_client: GraphClient,
    writer: GraphWriter,
    extractor: Box<dyn SignalExtractor>,
    embedder: Box<dyn TextEmbedder>,
    scraper: Box<dyn PageScraper>,
    searcher: Box<dyn WebSearcher>,
    social: Box<dyn SocialScraper>,
    anthropic_api_key: String,
    profile: CityProfile,
}
```

Entry point: `Scout::run()` acquires the scout lock, runs the full pipeline, and releases the lock on completion (or error).
