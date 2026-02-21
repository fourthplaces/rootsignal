# Root Signal

```sh
./dev.sh
```

Root Signal is a civic intelligence system. It continuously discovers, investigates, and maps the full landscape of community tension and response — then exposes the forces that shape what you're allowed to see.

## What It Does

Root Signal makes the full shape of civic reality legible — including the forces that try to make it illegible.

**Discover tension.** The Scout agent crawls curated URLs, web search results, RSS feeds, and social media for a city. It extracts structured signals using LLMs, deduplicates across three layers (exact match, URL-scoped, vector similarity), clusters them into stories, and persists everything into a knowledge graph. A curiosity loop investigates *why* signals exist — tracing every community event, resource, and need back to the underlying tension that produced it.

**Map response.** For every tension, the system discovers the ecosystem of human response assembling around it — legal clinics, mutual aid networks, community gatherings, fundraisers, volunteer coordination. Tensions and responses are wired together in the graph so you can see not just what's wrong, but what's forming to address it.

**Detect distortion.** The information environment is corrupted by the incentive structures of whoever produces it. A Divergence Analyst investigates how local narratives diverge from grounded reality elsewhere — surfacing real-world evidence from other geographies that reveals what's incomplete or distorted in the local picture. Suppression detection catches information that was actively erased, buried, or pre-empted, and preserves the paper trail.

**Resist manipulation.** The system is structurally anti-fragile. Triangulation over echo — you can manufacture claims, but you can't easily manufacture a coordinated ecosystem of gatherings, aid, needs, and tensions that all cohere. Graph position over source trust. Community crisis produces the richest, most triangulated signal in the entire graph. The system gets stronger under pressure.

**Six signal types:**

| Type | What it captures |
|------|-----------------|
| **Gathering** | Time-bound events — volunteer shifts, cleanups, protests, workshops |
| **Aid** | Available resources — food shelves, tool libraries, free clinics, mutual aid |
| **Need** | Community requests — volunteer calls, donation drives, skill requests |
| **Notice** | Official advisories — policy changes, shelter openings, city announcements |
| **Tension** | Systemic conflicts — housing crises, environmental harm, civil rights issues |
| **Evidence** | Source citations — URLs, content hashes, retrieval timestamps backing each signal |

## Repository Structure

```
modules/
  rootsignal-common/           Types, quality scoring, safety (PII detection), config
  rootsignal-graph/            Neo4j/Memgraph client, dedup, clustering, story weaving
  rootsignal-scout/            Scout agent — scraping, extraction, investigation, geo-filtering
  rootsignal-scout-supervisor/ Supervisor — auto-fixes, health checks, notifications
  rootsignal-api/              GraphQL API (async-graphql + DataLoaders)
  admin-app/                   Admin dashboard (TypeScript/React)
  search-app/                  Search and discovery interface (TypeScript/React)
  ai-client/                   Provider-agnostic LLM client (Claude, OpenAI, OpenRouter)
  apify-client/                Social media scraping via Apify (Instagram, Facebook, Reddit)
  browserless-client/          Headless Chrome scraping via Browserless
  simweb/                      Simulated web for deterministic testing
  twilio-rs/                   Twilio OTP and WebRTC

docs/
  vision/                Principles, values, problem space, milestones, kill tests
  architecture/          Scout pipeline, story weaver, feedback loops, curiosity loop
  landscape/             Competitive analysis, ecosystem vision
  reference/             Signal sources, quality dimensions, use cases
  brainstorms/           Architecture and feature brainstorms
  gaps/                  Identified gaps in system capabilities
  plans/                 Implementation plans
  interviews/            User and community interviews
  analysis/              Signal and source analysis
  tests/                 Testing playbooks
  audits/                System audits
  solutions/             Documented learnings
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
| `SERPER_API_KEY` | Yes | Web search for signal discovery |
| `APIFY_API_KEY` | No | Social media scraping |
| `BROWSERLESS_URL` | No | Headless Chrome endpoint for scraping |
| `BROWSERLESS_TOKEN` | No | Auth token for Browserless |
| `CITY` | No | Target city (twincities, nyc, portland, berlin). Default: twincities |

### Running tests

```sh
cargo test
```

Integration tests use [testcontainers](https://github.com/testcontainers/testcontainers-rs) to spin up Memgraph automatically.

## Documentation

- [`docs/vision/principles-and-values.md`](docs/vision/principles-and-values.md) — Why this exists
- [`docs/vision/problem-space-positioning.md`](docs/vision/problem-space-positioning.md) — The problem we're solving
- [`docs/architecture/scout-pipeline.md`](docs/architecture/scout-pipeline.md) — Scout pipeline architecture
- [`docs/architecture/story-weaver.md`](docs/architecture/story-weaver.md) — Story weaving and clustering
- [`docs/reference/signal-sources-and-roles.md`](docs/reference/signal-sources-and-roles.md) — Signal sources and quality dimensions
- [`docs/reference/use-cases.md`](docs/reference/use-cases.md) — Concrete user stories

## License

MIT
