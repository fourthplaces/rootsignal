---
title: "feat: Add GraphQL API with async-graphql"
type: feat
date: 2026-02-14
---

# feat: Add GraphQL API with async-graphql

## Overview

Add a GraphQL endpoint (`/graphql` + `/graphiql`) to the existing Axum server using `async-graphql`. This replaces the REST `/api/*` routes as the primary API surface for web, mobile, and third-party consumers. The schema exposes the full Taproot domain model as read-only, with cursor-based pagination, DataLoaders for N+1 prevention, and locale-aware translation fallback.

## Problem Statement / Motivation

The current REST API (`/api/stats`, `/api/listings`, `/api/listings/:id/cluster`, `/api/heatmap`) is limited:
- Clients cannot compose queries — fetching a listing with its entity, tags, schedule, and location requires multiple round-trips
- No introspection — third-party consumers must rely on out-of-band documentation
- Fixed response shapes — geo queries return a different type (`ListingWithDistance`) than non-geo queries (`ListingDetail`), making client code brittle
- No pagination metadata — no cursors, no `hasNextPage`, no total counts

GraphQL solves all of these: clients ask for exactly the data they need in a single request, the schema is self-documenting via introspection, and the Relay Connection spec provides robust pagination.

## Proposed Solution

### Architecture

- **Library:** `async-graphql` v7 + `async-graphql-axum` v7 (code-first, derive macros)
- **Mount point:** `/graphql` (POST for queries, GET for GraphiQL playground) on the existing Axum server (`port + 1`)
- **Schema context:** `PgPool` at schema level, `Locale` newtype at request level (extracted from `Accept-Language` header or `locale` argument, following existing REST precedence rules)
- **Restate server:** Unchanged — continues handling workflow traffic on its own port

```
┌──────────────────────────────────────────────┐
│ Axum Server (port + 1)                       │
│                                              │
│  GET  /           → assessment HTML page     │
│  GET  /graphql    → GraphiQL playground      │
│  POST /graphql    → GraphQL query execution  │
│  GET  /health     → health check             │
│  GET  /api/*      → REST (deprecated)        │
│                                              │
└──────────────────────────────────────────────┘
```

### Module Structure

```
crates/taproot-server/src/
  main.rs              # Add schema construction, pass to router
  routes.rs            # Add /graphql routes alongside existing routes
  graphql/
    mod.rs             # Schema builder, MergedObject root types
    context.rs         # Locale newtype, locale extraction from headers
    error.rs           # AppError → async_graphql::Error conversion
    loaders.rs         # All DataLoader implementations
    listings/
      mod.rs           # ListingQuery root
      types.rs         # GqlListing, GqlListingEdge, ListingConnection
    entities/
      mod.rs           # EntityQuery root
      types.rs         # GqlEntity, GqlService
    tags/
      mod.rs           # TagQuery root
      types.rs         # GqlTag, GqlTagKind
    locations/
      types.rs         # GqlLocation, GqlZipCode
    schedules/
      types.rs         # GqlSchedule
    contacts/
      types.rs         # GqlContact
    sources/
      types.rs         # GqlSource (with config field excluded)
    observations/
      mod.rs           # ObservationQuery root
      types.rs         # GqlObservation, GqlInvestigation
    heat_map/
      mod.rs           # HeatMapQuery root
      types.rs         # GqlHeatMapPoint
    hotspots/
      types.rs         # GqlHotspot
    stats/
      mod.rs           # StatsQuery root
      types.rs         # GqlListingStats, GqlTagCount
    clusters/
      types.rs         # GqlCluster (lightweight, for sibling traversal)
```

### Design Decisions

**1. GraphQL types wrap raw DB models, not `ListingDetail`.**
The REST API pre-joins entity name, schedule description, etc. into `ListingDetail`. GraphQL decomposes this: the `Listing` type maps to the raw `Listing` DB struct, and relationships (`entity`, `schedules`, `tags`, `contacts`, `locations`) are resolved via DataLoaders. This gives clients control over what they fetch.

**2. Distance lives on the connection Edge.**
Geo-context data (`distanceMiles`, `zipCode`, `locationCity`) belongs on a custom `ListingEdge` type, not on the `Listing` node. It only appears when geo arguments are present.

**3. Cursor encoding: `(sort_key, id)` composite.**
All cursors use a composite of the sort field + UUID tiebreaker, base64-encoded. For chronological: `(created_at, id)`. For geo: `(distance_miles, id)`. This guarantees stable pagination.

**4. Forward-only pagination initially.**
Only `first`/`after` supported. `last`/`before` deferred — most consumers paginate forward.

**5. `totalCount` is lazy.**
Computed only when the field is requested (via `ctx.look_ahead()`). Avoids expensive COUNT queries when clients don't need them.

**6. Polymorphic associations stay as relationship fields, not union types.**
`listing.tags`, `listing.locations`, etc. resolve through DataLoaders keyed by `(type_discriminator, id)`. Union types for observation/investigation subjects are deferred — the `subjectType`/`subjectId` fields are exposed as strings for now.

**7. Sensitive fields excluded from schema.**
- `Source.config` — may contain API keys
- `MemberIdentifier.identifier_hash` — correlation risk
- `Embedding` / `SimilarRecord` — internal vector data
- Join table models (`Taggable`, `Locationable`, etc.) — hidden behind relationship fields

**8. Locale precedence (same as REST).**
Explicit `locale` argument > `Accept-Language` header > `"en"` default. Unsupported locales silently fall back to `"en"`.

**9. Complexity budget: connection-aware.**
Connection fields cost `1 + first * child_complexity`. Leaf fields cost 0. Relationship fields cost 1. Budget set to 1000 (raised from 500 to accommodate typical listing-with-relationships queries). Depth limit: 10.

**10. CORS.**
Add `tower-http` CORS layer to the Axum router using `config.allowed_origins` (already in `AppConfig`).

## Implementation Phases

### Phase 1: Foundation (scaffold + one query end-to-end)

**Goal:** `/graphql` serves a working `listings` connection with cursor pagination and locale support. GraphiQL playground functional.

**Tasks:**

- [x] Add `async-graphql` and `async-graphql-axum` to workspace `Cargo.toml`
  - `async-graphql = { version = "7", features = ["dataloader", "chrono", "uuid"] }`
  - `async-graphql-axum = "7"`
  - Add to `taproot-server/Cargo.toml` as `.workspace = true`

- [x] Create `crates/taproot-server/src/graphql/mod.rs`
  - Define `QueryRoot` as `#[derive(MergedObject, Default)]` composing domain query structs
  - Define `AppSchema` type alias
  - Implement `build_schema(pool: PgPool) -> AppSchema` with `.data(pool)`, `.limit_depth(10)`, `.limit_complexity(1000)`

- [x] Create `crates/taproot-server/src/graphql/context.rs`
  - `pub struct Locale(pub String)` newtype
  - `pub fn extract_locale(headers: &HeaderMap, explicit: Option<&str>) -> Locale` — reuse logic from `parse_accept_language`

- [x] Create `crates/taproot-server/src/graphql/error.rs`
  - `AppError` enum: `NotFound`, `Db(sqlx::Error)`, `InvalidInput`
  - `impl From<AppError> for async_graphql::Error` with error extensions (`code`, `status`)

- [x] Create `crates/taproot-server/src/graphql/listings/types.rs`
  - `GqlListing` wrapping `Listing` fields (exclude `relevance_breakdown`)
  - Derive `SimpleObject` with `#[graphql(complex)]`
  - Custom `ListingEdge` with optional `distance_miles`, `zip_code`, `location_city`

- [x] Create `crates/taproot-server/src/graphql/listings/mod.rs`
  - `ListingQuery` with:
    - `listing(id: Uuid) -> Result<GqlListing>` — single lookup
    - `listings(first, after, locale, zipCode, radiusMiles, ...filters) -> Result<ListingConnection>` — cursor-paginated
  - Implement cursor encoding/decoding: base64 of `"created_at|uuid"` (or `"distance|uuid"` for geo)
  - Use `connection::query()` helper

- [x] Wire into `routes.rs`
  - `build_router(pool: PgPool) -> Router` now calls `graphql::build_schema(pool.clone())`
  - Add route: `.route("/graphql", get(graphiql_handler).post(graphql_handler))`
  - `graphql_handler`: extract `Accept-Language`, inject `Locale` into request data
  - `graphiql_handler`: serve `GraphiQLSource::build().endpoint("/graphql").finish()`

- [x] Add CORS middleware
  - Use `tower_http::cors::CorsLayer` with `config.allowed_origins`

- [x] Verify: `cargo build`, run server, open GraphiQL, execute `{ listings(first: 10) { edges { cursor node { id title } } pageInfo { hasNextPage } } }`

### Phase 2: DataLoaders + Relationships

**Goal:** Listing relationship fields resolve efficiently via batched DataLoaders.

**Tasks:**

- [x] Create `crates/taproot-server/src/graphql/loaders.rs` with DataLoader implementations:
  - `EntityByIdLoader` — batch `SELECT * FROM entities WHERE id = ANY($1)`
  - `ServiceByIdLoader` — batch `SELECT * FROM services WHERE id = ANY($1)`
  - `TagsForLoader` — batch `SELECT * FROM taggables t JOIN tags ON ... WHERE (t.taggable_type, t.taggable_id) IN ...`
    - Key type: `(String, Uuid)` — composite polymorphic key
  - `LocationsForLoader` — batch `SELECT * FROM locationables l JOIN locations ON ... WHERE (l.locatable_type, l.locatable_id) IN ...`
  - `SchedulesForLoader` — batch by `(scheduleable_type, scheduleable_id)`
  - `ContactsForLoader` — batch by `(contactable_type, contactable_id)`
  - `NotesForLoader` — batch by `(noteable_type, noteable_id)`

- [x] Register all DataLoaders in `build_schema()` via `.data(DataLoader::new(loader, tokio::spawn))`

- [x] Add `#[ComplexObject]` impl for `GqlListing`:
  - `async fn entity(&self, ctx) -> Result<Option<GqlEntity>>` via `EntityByIdLoader`
  - `async fn service(&self, ctx) -> Result<Option<GqlService>>` via `ServiceByIdLoader`
  - `async fn tags(&self, ctx) -> Result<Vec<GqlTag>>` via `TagsForLoader`
  - `async fn locations(&self, ctx) -> Result<Vec<GqlLocation>>` via `LocationsForLoader`
  - `async fn schedules(&self, ctx) -> Result<Vec<GqlSchedule>>` via `SchedulesForLoader`
  - `async fn contacts(&self, ctx) -> Result<Vec<GqlContact>>` via `ContactsForLoader`
  - `async fn notes(&self, ctx) -> Result<Vec<GqlNote>>` via `NotesForLoader`
  - `async fn cluster_siblings(&self, ctx) -> Result<Vec<GqlListing>>` via direct query (low cardinality, no loader needed)

- [x] Create GraphQL types for each related model:
  - `graphql/entities/types.rs` — `GqlEntity` (`SimpleObject`), `GqlService` (`SimpleObject`)
  - `graphql/tags/types.rs` — `GqlTag` (`SimpleObject`), `GqlTagKind` (`SimpleObject`)
  - `graphql/locations/types.rs` — `GqlLocation` (`SimpleObject`)
  - `graphql/schedules/types.rs` — `GqlSchedule` (`SimpleObject`)
  - `graphql/contacts/types.rs` — `GqlContact` (`SimpleObject`)
  - `graphql/sources/types.rs` — `GqlSource` (`SimpleObject`, `#[graphql(skip)]` on `config`)

- [x] Verify: query `{ listings(first: 5) { edges { node { title entity { name } tags { displayName } schedules { description } } } } }` returns correct nested data with batched SQL (check logs for query count)

### Phase 3: Remaining Domain Types

**Goal:** All domain types are queryable, either as root queries or through relationships.

**Tasks:**

- [x] `graphql/entities/mod.rs` — `EntityQuery`:
  - `entity(id: Uuid) -> Result<GqlEntity>`
  - `entities(first, after) -> Result<EntityConnection>` — cursor-paginated
  - Add `#[ComplexObject]` on `GqlEntity` for: `listings`, `services`, `locations`, `tags`, `contacts`, `notes`, `observations`, `investigations`

- [x] `graphql/tags/mod.rs` — `TagQuery`:
  - `tags(kind: Option<String>) -> Result<Vec<GqlTag>>` — filter by tag kind for dropdown population
  - `tagKinds -> Result<Vec<GqlTagKind>>`

- [x] `graphql/observations/mod.rs` — `ObservationQuery`:
  - `observation(id: Uuid) -> Result<GqlObservation>`
  - Add `GqlObservation` with `subjectType`, `subjectId`, `value` as `JSON` scalar
  - `GqlInvestigation` with relationship to observations
  - `investigation(id: Uuid) -> Result<GqlInvestigation>`

- [x] `graphql/heat_map/mod.rs` — `HeatMapQuery`:
  - `heatMapPoints(zipCode: Option<String>, radiusMiles: Option<f64>, entityType: Option<String>) -> Result<Vec<GqlHeatMapPoint>>`

- [x] `graphql/hotspots/types.rs` — `GqlHotspot` with relationship to nearby listings

- [x] `graphql/stats/mod.rs` — `StatsQuery`:
  - `listingStats -> Result<GqlListingStats>` — mirrors `ListingStats::compute()`

- [x] `graphql/clusters/types.rs` — `GqlCluster` (lightweight, primarily accessed via `listing.clusterSiblings`)

- [x] Register all new query structs in `QueryRoot` `MergedObject`

- [x] Verify: full schema introspection in GraphiQL shows all types, all relationships navigable

### Phase 4: Hardening

**Goal:** Production-ready with security, performance, and observability.

**Tasks:**

- [ ] Tune complexity budget — test realistic queries against the 1000-point limit, adjust per-field costs
- [ ] Add `#[graphql(complexity = "first.unwrap_or(20) * child_complexity + 1")]` on all connection fields
- [ ] Add tracing spans to GraphQL handler for observability (`tracing::instrument`)
- [ ] Gate GraphiQL behind `config` flag (optional — schema is read-only and public)
- [ ] Verify CORS works from browser origins
- [ ] Deprecation: add `#[deprecated]` note to REST route comments, but do not remove them yet
- [ ] Smoke test all query paths: listings (with/without geo, with/without filters, with/without locale), single listing with full relationship tree, entities, tags, stats, heat map, observations

## Acceptance Criteria

### Functional Requirements

- [ ] `POST /graphql` accepts and executes GraphQL queries
- [ ] `GET /graphql` serves the GraphiQL interactive playground
- [ ] `listings` connection supports cursor-based forward pagination (`first`/`after`)
- [ ] `listings` connection supports `locale`, `zipCode`, `radiusMiles`, and all 9 tag filter arguments
- [ ] Geo queries populate `distanceMiles` on connection edges
- [ ] All relationship fields (`entity`, `tags`, `locations`, `schedules`, `contacts`, `notes`, `service`) resolve via DataLoaders
- [ ] Translation fallback chain works: requested locale -> English -> source text
- [ ] Single-item lookups: `listing(id)`, `entity(id)`, `observation(id)`, `investigation(id)`
- [ ] Aggregate stats available via `listingStats` query
- [ ] Heat map points queryable with optional geo/type filters
- [ ] `Source.config` field is NOT exposed in the schema
- [ ] Existing REST `/api/*` routes and assessment page continue to work unchanged
- [ ] Health check at `/health` continues to work

### Non-Functional Requirements

- [ ] Query depth limit of 10 enforced
- [ ] Query complexity limit of 1000 enforced
- [ ] CORS configured using `config.allowed_origins`
- [ ] No N+1 queries — all list relationship fields use DataLoaders
- [ ] `totalCount` computed lazily (only when field is selected)

## Dependencies & Risks

**Dependencies:**
- `async-graphql` v7 + `async-graphql-axum` v7 (new crate dependencies)
- Existing `tower-http` already in workspace (for CORS)

**Risks:**
- **Compile time increase** — async-graphql's proc macros add to build times. Mitigated by keeping GraphQL types in the server crate only (not in taproot-domains).
- **Polymorphic DataLoader complexity** — Composite `(String, Uuid)` keys for polymorphic associations require careful SQL (`WHERE (type, id) IN ...`). May need array unnesting for efficient batch queries on Postgres.
- **Cursor pagination refactor** — Existing queries use `LIMIT/OFFSET`. Rewriting to keyset pagination requires modifying WHERE clauses and sort orders. The `ListingDetail` CTE query is particularly complex.

## References & Research

### Internal References
- Brainstorm: `docs/brainstorms/2026-02-14-graphql-api-brainstorm.md`
- Current REST routes: `crates/taproot-server/src/routes.rs`
- Listing models: `crates/taproot-domains/src/listings/models/listing.rs`
- Entity models: `crates/taproot-domains/src/entities/models/entity.rs`
- Domain module index: `crates/taproot-domains/src/lib.rs`
- ServerDeps: `crates/taproot-core/src/deps.rs`
- AppConfig: `crates/taproot-core/src/config.rs`
- Translation model: `crates/taproot-domains/src/entities/models/translation.rs`
- Heat map: `crates/taproot-domains/src/heat_map.rs`

### External References
- [async-graphql Book](https://async-graphql.github.io/async-graphql/en/)
- [async-graphql-axum crate](https://docs.rs/async-graphql-axum/latest/async_graphql_axum/)
- [Relay Connection Specification](https://relay.dev/graphql/connections.htm)
- [async-graphql DataLoader docs](https://async-graphql.github.io/async-graphql/en/dataloader.html)
