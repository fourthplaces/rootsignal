# Root Signal

A community signal utility. Root Signal discovers, concentrates, and serves actionable local signal — volunteer needs, fundraisers, mutual aid, events, ecological stewardship, civic action — through a GraphQL API that any application can build on.

The internet made information abundant but orientation scarce. There is no reliable place to answer: **where is life asking for help right now, and how do I show up?** Root Signal is that place.

## How It Works

```
Sources ──→ Engine (crawl, extract, geo-locate, dedup, store) ──→ GraphQL API ──→ Consumers
                        ▲
                  Configuration
            (prompts, taxonomy, sources, hotspots)
```

Root Signal crawls public sources (org websites, Eventbrite, GoFundMe, VolunteerMatch, government sites, environmental orgs, news outlets), extracts structured signal via AI, geo-localizes it, deduplicates it, and serves it through a typed API. The engine is domain-agnostic — what makes it serve community signal is configuration.

Signal is organized across four domains: **human services**, **ecological stewardship**, **civic & economic action**, and **knowledge & awareness**. Each signal is tagged with audience roles (volunteer, donor, attendee, advocate, citizen scientist, land steward, etc.) so people can filter by how they want to help.

## Architecture

Rust + TypeScript monorepo under `modules/`:

| Module | Purpose |
|--------|---------|
| `rootsignal-server` | Axum HTTP server, GraphQL API (async-graphql), auth, routes |
| `rootsignal-core` | Shared types, database queries, core logic |
| `rootsignal-domains` | Domain models and business rules |
| `ai-client` | LLM extraction client (Claude API) |
| `twilio-rs` | Twilio integration for SMS/voice intake |
| `api-client-js` | TypeScript GraphQL client with codegen |
| `admin-app` | Next.js admin interface |
| `dev/cli` | Developer CLI for local workflows |

### Key Technologies

- **Rust** — Axum web framework, async-graphql, SQLx
- **PostgreSQL 16** with pgvector for semantic search and PostGIS-style geography
- **Restate** — Durable execution for crawl/extraction workflows
- **GraphQL** — Primary API surface with codegen'd TypeScript client
- **Docker Compose** — Local dev (Postgres + Restate)

### Data Model

The schema tracks entities (organizations, people), their locations, listings (the actionable signals), sources, tags (full taxonomy), schedules, contacts, observations, hotspots, heat map points, clusters, and search infrastructure. Signal tiering enforces privacy boundaries at the database level — Tier 2 enrichment data is never served to consumers.

## Getting Started

### Prerequisites

- Rust (stable)
- Docker & Docker Compose
- pnpm (for JS codegen)

### Setup

```sh
# Start Postgres and Restate
docker compose up -d

# Run database migrations
# (migrations are in ./migrations/, applied in order)

# Start the server
cargo run --bin rootsignal-server
```

### Dev CLI

```sh
# The dev CLI auto-builds on first run
./dev.sh <command>
```

### GraphQL Schema

```sh
# Export the schema to SDL
make schema

# Generate TypeScript types from the schema
make codegen
```

The GraphQL API exposes queries for entities, listings, locations, tags, hotspots, heat maps, sources, observations, schedules, contacts, notes, stats, and search. Mutations support creating entities, listings, and observations. Auth is handled via JWT.

## Signal Tiering

| Tier | Source | Policy |
|------|--------|--------|
| **Tier 1** | Public web (org sites, APIs, open data) | Displayable with attribution |
| **Tier 2** | Semi-public (social media via scrapers) | Enrichment only, never served to consumers |
| **Tier 3** | Direct intake (SMS, email, web forms, API) | Highest quality, consensual |

Tier 2 data manifests only as computed flags (freshness score, capacity status, confidence boost) on Tier 1 records. This boundary is structural, enforced at the database and API level.

## Project Principles

- **Signal is a public good** — open source, open API, no paywalls, no data selling
- **Utility, not platform** — reliable infrastructure, not an engagement product
- **Privacy as architecture** — Tier 2 boundaries are structural, not policy
- **Attribution** — every signal links back to its original source
- **Community ownership** — designed for self-hosting, no corporate lock-in
- **Life, not just people** — ecological signal is first-class alongside human services

## Documentation

Detailed project documentation lives in `docs/`:

- [`docs/vision/`](docs/vision/) — Problem space, principles, milestones, kill tests
- [`docs/architecture/`](docs/architecture/) — Signal taxonomy, service architecture, discovery queries
- [`docs/landscape/`](docs/landscape/) — Ecosystem and adjacent systems
- [`docs/brainstorms/`](docs/brainstorms/) — Feature exploration sessions
- [`docs/plans/`](docs/plans/) — Implementation plans

## Status

Early development. Currently building the signal pipeline for the first hotspot (Twin Cities, MN). The focus is proving signal quality — volume, freshness, actionability — before expanding scope or geography.

## License

MIT
