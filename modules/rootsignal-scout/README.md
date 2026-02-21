# rootsignal-scout

Automated community signal collection engine for Root Signal. Scout continuously discovers, extracts, deduplicates, and maps signals about community tensions and the ecosystem responding to them.

## Quick Start

```bash
# Set required environment variables (or use .env at workspace root)
export NEO4J_URI=bolt://localhost:7687
export NEO4J_USER=neo4j
export NEO4J_PASSWORD=...
export ANTHROPIC_API_KEY=...
export VOYAGE_API_KEY=...
export SERPER_API_KEY=...
export CITY=twincities

# Run the scout
cargo run --bin scout

# Or specify city as CLI argument (overrides CITY env var)
cargo run --bin scout -- minneapolis

# Dump graph data as JSON (no scraping)
cargo run --bin scout -- --dump
```

## Environment Variables

### Required

| Variable | Description |
|----------|-------------|
| `NEO4J_URI` | Neo4j/Memgraph bolt connection URI |
| `NEO4J_USER` | Database username |
| `NEO4J_PASSWORD` | Database password |
| `ANTHROPIC_API_KEY` | Claude API key (extraction, synthesis, investigation) |
| `VOYAGE_API_KEY` | Voyage AI key (1024-dim signal embeddings) |
| `SERPER_API_KEY` | Serper web search API key |
| `CITY` | Target city slug (e.g. `twincities`, `nyc`, `portland`, `berlin`) |

### Optional

| Variable | Description | Default |
|----------|-------------|---------|
| `APIFY_API_KEY` | Social media scraping (Instagram, Facebook, Reddit) | Disabled |
| `BROWSERLESS_URL` | Browserless headless Chrome service URL | Local Chrome |
| `BROWSERLESS_TOKEN` | Browserless auth token | None |
| `CITY_LAT` | City center latitude | Required for cold start only |
| `CITY_LNG` | City center longitude | Required for cold start only |
| `CITY_RADIUS_KM` | Geo bounding radius | `30.0` |
| `CITY_NAME` | Human-readable city name | Same as `CITY` slug |
| `DAILY_BUDGET_CENTS` | Daily API spend limit (0 = unlimited) | `0` |
| `RUST_LOG` | Log level filter | `rootsignal=info` |

## Pipeline

Each run executes a 10-stage pipeline:

1. **Reap** expired signals
2. **Schedule** sources by weight, cadence, and exploration policy
3. **Phase A** — scrape tension sources (web + social), extract signals via Claude, quality-score, geo-filter, 3-layer dedup, embed, store
4. **Mid-run discovery** — LLM proposes new sources from graph gaps
5. **Phase B** — scrape response sources + fresh discovery sources
6. **Metrics** — update source weights and cadences, deactivate dead sources
7. **Synthesis** — five concurrent tasks: response mapping, tension linker, response finder, gathering finder, investigation
8. **Story weaving** — cluster signals into stories via Leiden + LLM
9. **Signal expansion** — collect implied queries, create new sources
10. **End-of-run discovery** — second source discovery pass

See [docs/architecture.md](docs/architecture.md) for full details.

## Modules

| Module | Description |
|--------|-------------|
| `scout` | Main orchestrator — `Scout::new()`, `Scout::run()`, `ScoutStats` |
| `scheduler` | Source scheduling by weight, cadence, and exploration sampling |
| `scrape_phase` | Unified scrape-store-dedup pipeline (`ScrapePhase`, `RunContext`, `EmbeddingCache`) |
| `scraper` | `PageScraper`, `WebSearcher`, `SocialScraper` traits + implementations (Chrome, Browserless, Serper, Apify) |
| `extractor` | LLM signal extraction — Claude Haiku → `ExtractionResult` |
| `embedder` | `TextEmbedder` trait + Voyage AI implementation |
| `quality` | Quality scoring (completeness + geo accuracy → confidence) |
| `bootstrap` | Cold-start seed generation — creates initial sources for a new city |
| `expansion` | Signal expansion — implied queries → new discovery sources |
| `source_finder` | LLM-driven source discovery from graph gaps and imbalances |
| `gathering_finder` | Agentic investigation — discovers where people gather around tensions |
| `response_finder` | Agentic investigation — discovers ecosystem responding to tensions |
| `tension_linker` | Agentic linking of orphan signals to existing tensions |
| `investigator` | Web search corroboration for low-confidence signals |
| `actor_extractor` | Extract and link mentioned organizations/people |
| `metrics` | Source weight + cadence updates, dead source deactivation |
| `sources` | Canonical key generation for source deduplication |
| `budget` | Daily API spend tracking and gating |
| `fixtures` | Test fixtures and mock trait implementations |
| `util` | Helpers — `sanitize_url`, `content_hash` |

## Key Types

```rust
// Main entry point
pub struct Scout { /* ... */ }
impl Scout {
    pub fn new(graph_client, api_keys..., city_node, budget, cancelled) -> Result<Self>;
    pub fn with_deps(graph_client, trait_objects...) -> Self;  // for testing
    pub async fn run(&self) -> Result<ScoutStats>;
}

// Run statistics
pub struct ScoutStats {
    pub urls_scraped: u32,
    pub signals_extracted: u32,
    pub signals_stored: u32,
    pub by_type: [u32; 5],  // Gathering, Aid, Need, Notice, Tension
    // ...
}

// Extension traits (all async + Send + Sync)
trait SignalExtractor   // LLM extraction
trait TextEmbedder      // Vector embeddings
trait PageScraper       // Web page fetching
trait WebSearcher       // Web search
trait SocialScraper     // Social media scraping
```

## External Dependencies

| Service | Purpose |
|---------|---------|
| [Anthropic Claude](https://anthropic.com) | Signal extraction, synthesis, investigation, story weaving |
| [Voyage AI](https://voyageai.com) | 1024-dim vector embeddings |
| [Serper](https://serper.dev) | Web search for discovery and investigation |
| [Apify](https://apify.com) | Social media scraping (optional) |
| [Browserless](https://browserless.io) | Headless Chrome service (optional, falls back to local Chrome) |
| Neo4j / Memgraph | Graph database |

## Testing

```bash
# Run unit tests (uses simweb + mock traits, no external services needed)
cargo test -p rootsignal-scout
```

Tests use the `simweb` crate for deterministic, offline simulation of web pages and search results, plus mock implementations of all extension traits.

## Related Crates

| Crate | Role |
|-------|------|
| `rootsignal-common` | Shared types (`Node`, `NodeMeta`, `CityNode`, `SourceNode`), config, quality scoring, PII detection |
| `rootsignal-graph` | Neo4j client, graph writer/reader, story weaver, cause heat, similarity, migrations |
| `ai-client` | Provider-agnostic LLM client (Claude, OpenAI, OpenRouter) |
| `apify-client` | Apify API wrapper for social scraping |
| `browserless-client` | Browserless headless Chrome wrapper |
| `simweb` | Simulated web for deterministic testing |
