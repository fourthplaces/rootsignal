# rootsignal-api

GraphQL API for Root Signal. Serves signals, stories, actors, and admin tools over a single `/graphql` endpoint.

## Quick Start

```sh
export NEO4J_URI=bolt://localhost:7687
export NEO4J_USER=neo4j
export NEO4J_PASSWORD=secret
export ADMIN_PASSWORD=changeme

cargo run -p rootsignal-api
```

The server starts on `http://localhost:3000`. GraphiQL is available at `/graphql` in debug builds.

## Endpoints

| Route | Method | Description |
|---|---|---|
| `/graphql` | POST | GraphQL API |
| `/graphql` | GET | GraphiQL IDE (debug only) |
| `/api/link-preview?url=` | GET | OG tag extraction for URL previews |
| `/` | GET | Health check (`"ok"`) |

## Environment Variables

### Required

| Variable | Description |
|---|---|
| `NEO4J_URI` | Bolt URI for Neo4j |
| `NEO4J_USER` | Neo4j username |
| `NEO4J_PASSWORD` | Neo4j password |
| `ADMIN_PASSWORD` | Admin password (also used as JWT secret if `SESSION_SECRET` is empty) |

### Optional

| Variable | Default | Description |
|---|---|---|
| `API_HOST` | `0.0.0.0` | Bind address |
| `API_PORT` / `PORT` | `3000` | Bind port |
| `SESSION_SECRET` | `ADMIN_PASSWORD` | JWT signing secret |
| `CORS_ORIGINS` | `https://rootsignal.app` | Comma-separated allowed origins |
| `REGION` | `twincities` | Default region slug |
| `ADMIN_NUMBERS` | | Comma-separated E.164 phone numbers allowed to authenticate |

### Scout (enables `runScout` / `runNewsScan` mutations)

| Variable | Description |
|---|---|
| `ANTHROPIC_API_KEY` | Claude API key |
| `VOYAGE_API_KEY` | Voyage AI key (embeddings + semantic search) |
| `SERPER_API_KEY` | Serper web search key |
| `APIFY_API_KEY` | Apify key (social scraping, optional) |
| `DATABASE_URL` | Postgres connection string (web archive) |
| `BROWSERLESS_URL` | Browserless endpoint (page rendering, optional) |
| `BROWSERLESS_TOKEN` | Browserless auth token (optional) |
| `SCOUT_INTERVAL_HOURS` | Run scout on a timer (0 = disabled) |
| `DAILY_BUDGET_CENTS` | Daily API spend cap (0 = unlimited) |

### Twilio (enables OTP authentication)

| Variable | Description |
|---|---|
| `TWILIO_ACCOUNT_SID` | Twilio account SID |
| `TWILIO_AUTH_TOKEN` | Twilio auth token |
| `TWILIO_SERVICE_ID` | Twilio Verify service ID |

## Authentication

Admin access uses phone-based OTP via Twilio. The flow:

1. `sendOtp(phone)` — sends a code via SMS
2. `verifyOtp(phone, code)` — returns a JWT in a `Set-Cookie` header
3. Subsequent requests include the cookie automatically
4. `logout()` — clears the cookie

Only phone numbers listed in `ADMIN_NUMBERS` can authenticate. JWTs expire after 24 hours.

In debug builds, `+1234567890` is accepted as a test phone number with any 6-digit code.

## GraphQL Schema

### Public Queries

#### Signals

```graphql
# Find signals near a point
signalsNear(lat: Float!, lng: Float!, radiusKm: Float!, types: [SignalType!]): [Signal!]!

# Same, as GeoJSON
signalsNearGeoJson(lat: Float!, lng: Float!, radiusKm: Float!, types: [SignalType!]): String!

# Signals in a bounding box (viewport-driven)
signalsInBounds(minLat: Float!, maxLat: Float!, minLng: Float!, maxLng: Float!, limit: Int): [Signal!]!

# Recent signals ordered by triangulation quality
signalsRecent(limit: Int, types: [SignalType!]): [Signal!]!

# Single signal by ID
signal(id: UUID!): Signal
```

`Signal` is a union of `GatheringSignal`, `AidSignal`, `NeedSignal`, `NoticeSignal`, and `TensionSignal`. All share common fields (`id`, `title`, `summary`, `confidence`, `location`, `sourceUrl`, `extractedAt`, `sourceDiversity`, `causeHeat`, `evidence`, `story`, `actors`). Each type adds domain-specific fields:

- **Gathering** — `startsAt`, `endsAt`, `actionUrl`, `organizer`, `isRecurring`
- **Aid** — `actionUrl`, `availability`, `isOngoing`
- **Need** — `urgency`, `whatNeeded`, `actionUrl`, `goal`
- **Notice** — `severity`, `category`, `effectiveDate`, `sourceAuthority`
- **Tension** — `severity`, `category`, `whatWouldHelp`, `responses`

#### Stories

```graphql
# Stories in a bounding box
storiesInBounds(minLat: Float!, maxLat: Float!, minLng: Float!, maxLng: Float!, tag: String, limit: Int): [Story!]!

# Top stories by energy
stories(limit: Int, status: String): [Story!]!

# Single story
story(id: UUID!): Story

# Filter by category or arc
storiesByCategory(category: String!, limit: Int): [Story!]!
storiesByArc(arc: String!, limit: Int): [Story!]!
storiesByTag(tag: String!, minLat: Float, maxLat: Float, minLng: Float, maxLng: Float, limit: Int): [Story!]!
```

A `Story` has: `id`, `headline`, `summary`, `signalCount`, `firstSeen`, `lastUpdated`, `velocity`, `energy`, `centroidLat`, `centroidLng`, `dominantType`, `status`, `arc`, `category`, `lede`, `narrative`, `causeHeat`, `gapScore`, `gapVelocity`, `signals`, `actors`, `tags`, `evidenceCount`.

#### Semantic Search

```graphql
# Search signals by natural language query within a bounding box
searchSignalsInBounds(query: String!, minLat: Float!, maxLat: Float!, minLng: Float!, maxLng: Float!, limit: Int): [SearchResult!]!

# Search stories (aggregates from signal-level matches)
searchStoriesInBounds(query: String!, minLat: Float!, maxLat: Float!, minLng: Float!, maxLng: Float!, limit: Int): [StorySearchResult!]!
```

Requires `VOYAGE_API_KEY`. Embeds the query via Voyage AI, then finds nearest signals via vector KNN.

#### Other

```graphql
# Tensions with < 2 respondents, not in any story
unrespondedTensionsInBounds(minLat: Float!, maxLat: Float!, minLng: Float!, maxLng: Float!, limit: Int): [Signal!]!

# Actors
actors(region: String!, limit: Int): [Actor!]!
actor(id: UUID!): Actor

# Tags sorted by story count
tags(limit: Int): [Tag!]!

# Auth status
me: MeResult

# Story signals as GeoJSON
storySignalsGeoJson(storyId: UUID!): String!
```

### Public Mutations

```graphql
# Submit a source URL for the scout to scrape (rate-limited, 10/hr)
submitSource(url: String!, description: String, region: String): SubmitSourceResult!

# Record a demand signal from a user search (rate-limited, 10/hr)
recordDemand(query: String!, centerLat: Float!, centerLng: Float!, radiusKm: Float!): Boolean!
```

### Admin Queries

All require a valid admin JWT cookie.

```graphql
# Full dashboard data for a region
adminDashboard(region: String!): AdminDashboardData!

# Active sources with schedule info
adminRegionSources: [AdminSource!]!

# Scout status
adminScoutStatus(regionSlug: String!): RegionScoutStatus!

# Scout run history (from JSON files on disk)
adminScoutRuns(region: String!, limit: Int): [ScoutRun!]!
adminScoutRun(runId: String!): ScoutRun

# Supervisor validation findings
supervisorFindings(region: String!, status: String, limit: Int): [SupervisorFinding!]!
supervisorSummary(region: String!): SupervisorSummary!

# Scout task queue
adminScoutTasks(status: String, limit: Int): [ScoutTask!]!
```

### Admin Mutations

```graphql
# Trigger a scout run (geocodes the query, runs in background)
runScout(query: String!): ScoutResult!
stopScout: ScoutResult!
resetScoutLock(query: String!): ScoutResult!

# Trigger a news scan
runNewsScan: ScoutResult!

# Source management
addSource(url: String!, reason: String): AddSourceResult!

# Story tagging
tagStory(storyId: UUID!, tagSlug: String!): Boolean!
untagStory(storyId: UUID!, tagSlug: String!): Boolean!
mergeTags(sourceSlug: String!, targetSlug: String!): Boolean!

# Supervisor
dismissFinding(id: String!): Boolean!

# Scout task queue
createScoutTask(location: String!, radiusKm: Float, priority: Float): String!
cancelScoutTask(id: String!): Boolean!
```

### Auth Mutations

```graphql
sendOtp(phone: String!): SendOtpResult!
verifyOtp(phone: String!, code: String!): VerifyOtpResult!
logout: LogoutResult!
```

## Replaying Scout Runs

Every web interaction the scout makes (page fetches, search results, social posts, RSS feeds) is recorded in Postgres via the `rootsignal-archive` crate. You can replay a previous run's data without hitting the network using `Replay`:

```rust
use rootsignal_archive::{Replay, FetchBackend, FetchBackendExt};
use sqlx::PgPool;
use uuid::Uuid;

let pool = PgPool::connect("postgres://...").await?;
let run_id: Uuid = "...".parse()?;

// Create a replay backend — same interface as Archive, but reads from Postgres
let replay = Replay::for_run(pool, run_id);

// Use it exactly like a live Archive
let content = replay.fetch("https://example.com").content().await?;
let text = replay.fetch("https://example.com").text().await?;
```

`Replay` implements `FetchBackend`, so it can be injected anywhere `Archive` is used — including `Scout::new_for_test`. This lets you re-run extraction against historical web content to iterate on prompts or test pipeline changes.

### Finding Run IDs

Scout run logs are stored as JSON files in `data/scout-runs/<region>/<run_id>.json`. Each file contains the `run_id`, timestamps, stats, and a full event trace. The `adminScoutRuns` query also exposes these via the API.

Archive interactions are stored in the `web_interactions` table in Postgres, keyed by `run_id`. You can query available runs with:

```sql
SELECT DISTINCT run_id, region_slug, MIN(fetched_at) AS started, COUNT(*) AS interactions
FROM web_interactions
GROUP BY run_id, region_slug
ORDER BY started DESC;
```

### Replay vs Latest

```rust
// Replay a specific run
let replay = Replay::for_run(pool.clone(), run_id);

// Or replay the most recent content for each URL (across all runs)
let replay = Replay::latest(pool);
```

## Architecture

```
rootsignal-api
├── main.rs              # Axum server, CORS, security headers
├── jwt.rs               # JWT creation/verification, cookie management
├── link_preview.rs      # OG tag extraction with in-memory cache
└── graphql/
    ├── schema.rs        # QueryRoot + AdminGuard queries + build_schema
    ├── mutations.rs     # MutationRoot + scout spawning + interval loop
    ├── types.rs         # GQL types (Signal union, Story, Actor, Evidence, etc.)
    ├── loaders.rs       # DataLoader batching (evidence, actors, stories, tags)
    └── context.rs       # AuthContext + AdminGuard
```

The API reads from Neo4j via an in-memory `CacheStore` that reloads periodically. Writes go through `GraphWriter`. Scout runs spawn in a dedicated thread with their own Tokio runtime to avoid blocking the API event loop.
