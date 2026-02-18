# Root Signal

A civic intelligence system. Root Signal continuously discovers civic signal across the web — volunteer shifts, mutual aid requests, environmental actions, policy changes, community tensions — builds a living knowledge graph, and makes it freely navigable. A search engine for civic life.

## Status

Greenfield. Building Phase 1a — the smallest loop that proves the core concept works: scout agent discovers signal, graph stores it, web surface serves it, quality measurement keeps it honest.

## What It Does

Root Signal sits between fragmented civic sources and people who want to act. The Scout agent crawls curated URLs, web search results, and social media accounts for a city, then extracts structured signals using LLMs. Signals are deduplicated across three layers (exact match, URL-scoped, vector similarity), scored for quality, and persisted into a knowledge graph.

**Five signal types:**

| Type | What it captures |
|------|-----------------|
| **Event** | Time-bound gatherings — volunteer shifts, cleanups, protests, workshops |
| **Give** | Available resources — food shelves, tool libraries, free clinics |
| **Ask** | Community needs — volunteer calls, donation drives, skill requests |
| **Notice** | Official advisories — policy changes, shelter openings, city announcements |
| **Tension** | Systemic conflicts — housing crises, environmental harm, civil rights issues |

## Repository Structure

```
modules/
  rootsignal-common/     Types, quality scoring, safety (PII detection), config
  rootsignal-graph/      Neo4j/Memgraph client, dedup, clustering, cause heat
  rootsignal-scout/      Scout agent — scraping, extraction, geo-filtering
  rootsignal-web/        Axum web server, graph queries, templates
  rootsignal-editions/   Curated thematic signal collections
  ai-client/             Provider-agnostic LLM client (Claude, OpenAI, OpenRouter)
  apify-client/          Social media scraping via Apify (Instagram, Facebook, Reddit)
  twilio-rs/             Twilio OTP and WebRTC

docs/
  vision/                Principles, values, problem space, milestones, kill tests
  landscape/             Competitive analysis, ecosystem vision
  reference/             Signal sources, quality dimensions, use cases
  brainstorms/           Architecture brainstorms
```

## Development

### Prerequisites

- Rust (2021 edition)
- Docker and Docker Compose

### Running locally

```sh
# Start Memgraph and web server
docker compose up

# Run the scout for a city
CITY=twincities docker compose --profile scout up scout
```

### Environment variables

| Variable | Required | Purpose |
|----------|----------|---------|
| `ANTHROPIC_API_KEY` | Yes | LLM extraction and clustering (Claude) |
| `VOYAGE_API_KEY` | Yes | Vector embeddings (Voyage AI) |
| `TAVILY_API_KEY` | Yes | Web search for signal discovery |
| `FIRECRAWL_API_KEY` | No | Advanced web scraping |
| `APIFY_API_KEY` | No | Social media scraping |
| `CITY` | No | Target city (twincities, nyc, portland, berlin). Default: twincities |

### Running tests

```sh
cargo test
```

Integration tests use [testcontainers](https://github.com/testcontainers/testcontainers-rs) to spin up Memgraph automatically.

## Documentation

- [`docs/vision/principles-and-values.md`](docs/vision/principles-and-values.md) — Why this exists
- [`docs/vision/problem-space-positioning.md`](docs/vision/problem-space-positioning.md) — The problem we're solving
- [`docs/scout-architecture.md`](docs/scout-architecture.md) — Scout system architecture
- [`docs/reference/signal-sources-and-roles.md`](docs/reference/signal-sources-and-roles.md) — Signal sources and quality dimensions
- [`docs/reference/use-cases.md`](docs/reference/use-cases.md) — Concrete user stories

## License

MIT
