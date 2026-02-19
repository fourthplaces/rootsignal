---
title: "feat: Split Rust API server + React admin SPA"
type: feat
date: 2026-02-19
---

# Split Rust API Server + React Admin SPA

## Overview

Remove Dioxus SSR and the entire REST API from `rootsignal-api`, leaving it as a pure GraphQL API server. Build a new React/TypeScript SPA at `modules/admin-app/` using Vite + React Router + Tailwind + shadcn/ui that talks to the Rust server exclusively via GraphQL.

## Problem Statement / Motivation

Dioxus SSR provides no interactivity — it's purely a templating engine rendering HTML strings. The admin UI already depends on client-side JS (Leaflet, Chart.js). This creates:
- Slow iteration on UI changes (full Rust recompile on any template tweak)
- No client-side reactivity (forms are traditional POST, no optimistic updates)
- Dioxus SSR adds compile time and dependency weight with zero benefit over a proper SPA

## Proposed Solution

Three-phase approach: API-first (auth + GraphQL schema), then React SPA, then Dioxus removal.

## Key Architectural Decisions

### D1: Single GraphQL endpoint with per-field auth guards
Use one schema at `/graphql`. Admin queries/mutations use `async-graphql` `#[guard]` annotations. No separate `/admin/graphql` endpoint.

### D2: member_id derived from phone number
No persistent Member/User table. Generate a deterministic UUID from the phone number hash on login. The JWT `sub` claim is this UUID. `is_admin` is determined by the `ADMIN_NUMBERS` allowlist at login time.

### D3: JWT in HTTP-only cookie (not localStorage)
Matches mntogether pattern. `auth_token` cookie, 24h expiry, `HttpOnly`, `Secure` (prod), `SameSite=Lax`. Browser sends it automatically — no `Authorization` header needed.

### D4: Rust serves SPA static files in production
The Rust server serves the built SPA from `modules/admin-app/dist/`. A catch-all `/admin/*` route returns `index.html` for client-side routing. In development, Vite dev server proxies `/graphql` and `/api/*` to the Rust server.

### D5: GraphQL-only — delete the entire REST API
All REST endpoints (`/api/*`) are removed. The existing public GraphQL queries (`signalsNear`, `stories`, `actors`, `editions`) replace them. The map uses a `signalsNearGeoJson` query that returns a GeoJSON string directly. Auth (send OTP, verify OTP, logout) are GraphQL mutations — no REST auth routes.

### D6: Scout status via polling
5-second polling on city detail page when a scout is running. No WebSocket/SSE complexity for an admin tool.

### D7: Dashboard gets a city selector
Default to first active city. Some stats are global (total signals/stories/actors), others city-scoped (discovery performance, extraction yield, gap stats).

### D8: Cursor-based pagination for GraphQL
Add `after` cursor + `first` limit params to list queries. SPA uses infinite scroll or load-more buttons.

---

## Technical Approach

### Phase 1: API Foundation (Auth + GraphQL Schema)

Build the API layer that the React SPA will consume. No UI changes yet — existing Dioxus admin continues working.

#### 1.1 JWT Auth Module

Port JWT implementation from mntogether (`packages/server/src/domains/auth/jwt.rs`).

**New/modified files:**
- `modules/rootsignal-api/Cargo.toml` — add `jsonwebtoken = "9"`
- `modules/rootsignal-api/src/jwt.rs` — new file

```
jwt.rs:
  - JwtService { secret, issuer }
  - Claims { sub, phone_number, is_admin, exp, iat, iss, jti }
  - create_token(phone: String, is_admin: bool) -> Result<String>
  - verify_token(token: &str) -> Result<Claims>
  - member_id: deterministic UUID from SHA256(phone_number)
  - Expiry: 24 hours
  - Algorithm: HS256 using SESSION_SECRET env var
```

**Auth as GraphQL mutations** (no REST auth routes):
- `sendOtp(phone: String!)` → `SendOtpResult { success: Boolean! }` — sends OTP via Twilio. No auth required.
- `verifyOtp(phone: String!, code: String!)` → `VerifyOtpResult { success: Boolean! }` — validates OTP + allowlist. On success, sets `Set-Cookie: auth_token=<jwt>; HttpOnly; SameSite=Lax; Path=/; Max-Age=86400`. No auth required.
- `logout` → `LogoutResult { success: Boolean! }` — clears `auth_token` cookie. No auth required.

These mutations are **not** gated by `AdminGuard` — they are the entry point for authentication. The JWT cookie is set via the HTTP response headers from the GraphQL handler (async-graphql supports setting response headers via `Context`).

**GraphQL auth context:**
- `modules/rootsignal-api/src/graphql/context.rs` — new file
- Extract `auth_token` cookie from request headers in the GraphQL handler
- Call `jwt.verify_token()`, attach `Option<Claims>` to async-graphql `Context`
- Define `AdminGuard` implementing `async-graphql::Guard` — checks `claims.is_admin == true`

**Tests:**
- JWT roundtrip (create → verify → claims match)
- Expired token rejection
- Tampered token rejection
- OTP send/verify flow (mock Twilio)
- Allowlist enforcement

#### 1.2 Admin GraphQL Queries

Extend `QueryRoot` with admin-only queries gated by `#[guard(AdminGuard)]`.

**New/modified files:**
- `modules/rootsignal-api/src/graphql/schema.rs` — add admin queries
- `modules/rootsignal-api/src/graphql/types.rs` — add admin GQL types
- `modules/rootsignal-api/src/graphql/loaders.rs` — add admin dataloaders if needed

**Admin queries to add:**

| Query | Returns | Source (current handler) |
|---|---|---|
| `adminDashboard(city: String)` | `DashboardData` | `pages/mod.rs:741-910` (15 parallel calls) |
| `adminCities` | `[AdminCity]` | `pages/mod.rs` cities handler |
| `adminCity(slug: String)` | `AdminCityDetail` | `pages/mod.rs` city_detail handler |
| `adminCitySources(slug: String)` | `[AdminSource]` with schedule preview | `pages/mod.rs` city sources tab |
| `adminSignals(city: String, first: Int, after: String)` | `SignalConnection` | `pages/mod.rs` signals list |
| `adminSignalDetail(id: UUID)` | `AdminSignalDetail` with evidence + responses | `pages/mod.rs` signal detail |
| `adminStories(city: String, first: Int, after: String)` | `StoryConnection` | existing + evidence counts |
| `adminActors(city: String, first: Int, after: String)` | `ActorConnection` | existing + story count |
| `adminEditions(city: String)` | `[AdminEdition]` | existing |
| `adminScoutStatus(city: String)` | `ScoutStatus` (running, last_scouted, sources_due) | `writer.is_scout_running()` etc. |

**New GQL types:**

```
DashboardData {
  totalSignals, totalStories, totalActors, totalSources, totalTensions: Int
  scoutStatuses: [CityScoutStatus]
  signalVolumeByDay: [DayCount]
  countByType: [TypeCount]
  storyCountByArc: [ArcCount]
  storyCountByCategory: [CategoryCount]
  freshnessDistribution: [BucketCount]
  confidenceDistribution: [BucketCount]
  sourceWeightBuckets: [BucketCount]
  unmetTensions: [TensionRow]
  topSources: [SourcePerformanceRow]
  bottomSources: [SourcePerformanceRow]
  extractionYield: [YieldRow]
  gapStats: [GapStatRow]
}

AdminCity {
  slug, name, lat, lng, radius_km, status
  signalCount, sourceCount, lastScouted
  scoutRunning, sourcesDue: Int
}

AdminSource {
  id, url, sourceType, weight, qualityPenalty, effectiveWeight
  discoveryMethod, lastScouted, cadenceHours
  scheduleReason: String  // "cadence", "new", "exploration"
}

SignalConnection { edges: [SignalEdge], pageInfo: PageInfo }
SignalEdge { node: GqlSignal, cursor: String }
PageInfo { hasNextPage: Boolean, endCursor: String }
```

#### 1.3 GraphQL Mutations

**Auth mutations (no guard):**

| Mutation | Args | Effect |
|---|---|---|
| `sendOtp` | `phone: String!` | Send OTP via Twilio, rate-limited (10/hr per IP) |
| `verifyOtp` | `phone: String!, code: String!` | Validate OTP + allowlist → set JWT cookie |
| `logout` | — | Clear `auth_token` cookie |

**Admin mutations (gated by `#[guard(AdminGuard)]`):**

| Mutation | Args | Effect |
|---|---|---|
| `createCity` | `location: String!` | Geocode via Nominatim → create CityNode → run bootstrapper |
| `addSource` | `citySlug: String!, url: String!, reason: String` | Validate URL → create SourceNode |
| `runScout` | `citySlug: String!` | Spawn scout task (async), return immediate ack |
| `stopScout` | `citySlug: String!` | Set cancel flag |
| `resetScoutLock` | `citySlug: String!` | Release stuck lock |
| `submitSource` | `url: String!, description: String, lat: Float, lng: Float` | Public source submission (rate-limited, replaces `POST /api/submit`) |

**Modified files:**
- `modules/rootsignal-api/src/graphql/schema.rs` — change `EmptyMutation` to `MutationRoot`
- `modules/rootsignal-api/src/graphql/mutations.rs` — new file

#### 1.4 Extend Public GraphQL Queries

The existing public queries (`signalsNear`, `stories`, `actors`, `editions`) already cover most consumer needs. Add:

| Query | Returns | Replaces |
|---|---|---|
| `signalsNearGeoJson(lat: Float!, lng: Float!, radiusKm: Float!, types: [SignalType])` | `String` (GeoJSON FeatureCollection) | `GET /api/nodes/near` |
| `storySignalsGeoJson(storyId: UUID!)` | `String` (GeoJSON FeatureCollection) | `GET /api/stories/{id}/signals` |
| `tensionResponses(tensionId: UUID!)` | `[ResponseNode]` | `GET /api/tensions/{id}/responses` |

The GeoJSON queries return pre-serialized JSON strings that the map can parse directly with `JSON.parse()`. This avoids the client needing to transform GraphQL objects into GeoJSON format.

#### 1.5 Delete REST API

Remove the entire `modules/rootsignal-api/src/rest/` directory and all `/api/*` routes from `main.rs`.

**Delete:**
- `modules/rootsignal-api/src/rest/` (entire directory — submit.rs, scout.rs, handlers)
- All `/api/*` route registrations from `main.rs`
- Scout basic auth (`check_admin_auth`) from `auth.rs`

**Migrate functionality:**
- `POST /api/submit` → `submitSource` mutation (rate-limited)
- `POST /api/scout/run` → `runScout` mutation (admin-guarded)
- `GET /api/scout/status` → `adminScoutStatus` query (admin-guarded)
- All `GET /api/*` read endpoints → already covered by existing + new GraphQL queries

#### 1.6 CORS + Cookie Configuration

**Modified file:** `modules/rootsignal-api/src/main.rs`

- Add `Access-Control-Allow-Credentials: true` to CORS layer
- In debug mode: allow `http://localhost:5173` (Vite default) instead of `Any` (incompatible with credentials)
- In release mode: read from `CORS_ORIGINS` env var (already exists)
- Update CSP header to allow Vite dev server origin in debug mode
- Add cache headers: `no-store` for API responses only, long-lived caching for `/assets/*` static files

---

### Phase 2: React Admin SPA

Build the React app consuming the GraphQL API from Phase 1.

#### 2.1 Project Scaffolding

```
modules/admin-app/
├── index.html
├── package.json
├── tsconfig.json
├── vite.config.ts          # proxy /graphql to localhost:3000
├── tailwind.config.ts
├── components.json          # shadcn/ui config
├── src/
│   ├── main.tsx
│   ├── App.tsx              # React Router layout
│   ├── lib/
│   │   ├── graphql-client.ts    # Apollo Client setup
│   │   └── auth.ts              # login/logout helpers
│   ├── components/
│   │   └── ui/              # shadcn/ui components
│   ├── layouts/
│   │   └── AdminLayout.tsx  # sidebar nav, auth check wrapper
│   ├── pages/
│   │   ├── LoginPage.tsx
│   │   ├── MapPage.tsx
│   │   ├── DashboardPage.tsx
│   │   ├── SignalsPage.tsx
│   │   ├── SignalDetailPage.tsx
│   │   ├── StoriesPage.tsx
│   │   ├── StoryDetailPage.tsx
│   │   ├── CitiesPage.tsx
│   │   ├── CityDetailPage.tsx
│   │   ├── ActorsPage.tsx
│   │   ├── ActorDetailPage.tsx
│   │   ├── EditionsPage.tsx
│   │   └── EditionDetailPage.tsx
│   └── graphql/
│       ├── queries.ts       # all GQL query documents
│       └── mutations.ts     # all GQL mutation documents
```

**Dependencies:**
- `react`, `react-dom`, `react-router` (v7)
- `@apollo/client`, `graphql`
- `tailwindcss`, `@tailwindcss/vite`
- shadcn/ui components (table, card, dialog, form, button, input, tabs, badge, dropdown-menu)
- `recharts` (React-native charts, replaces Chart.js)
- `react-leaflet` + `leaflet` (React wrapper for Leaflet)

**Vite config:**
```typescript
// vite.config.ts
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 5173,
    proxy: {
      '/graphql': 'http://localhost:3000',
    },
  },
})
```

#### 2.2 Auth Flow (LoginPage)

Two-step form using GraphQL mutations:
1. Phone number input → call `sendOtp` mutation → show code input
2. Code input → call `verifyOtp` mutation → cookie set via response headers → redirect to `/admin`

`AdminLayout` wrapper checks auth state:
- On mount, make a lightweight GraphQL query (e.g., `me` query returning `{ isAdmin: Boolean }`)
- If `UNAUTHENTICATED` error → redirect to `/admin/login`
- Apollo Client `onError` link: if any response has `UNAUTHENTICATED` extension code → redirect to login

#### 2.3 Pages (Priority Order)

Build pages in this order (each depends on progressively more GraphQL queries):

1. **LoginPage** — `sendOtp` + `verifyOtp` mutations
2. **CitiesPage + CityDetailPage** — `adminCities`, `adminCity`, `adminCitySources`, mutations (create city, add source, scout controls). This is the most action-heavy page and validates the full mutation flow.
3. **DashboardPage** — `adminDashboard` query, recharts for 7 charts, shadcn tables for 4 data tables, city selector dropdown
4. **MapPage** — `react-leaflet` map, `signalsNearGeoJson` query, click-through to signal detail
5. **SignalsPage + SignalDetailPage** — `adminSignals`, `adminSignalDetail` with evidence and tension responses
6. **StoriesPage + StoryDetailPage** — `adminStories` + existing `story` query with signals
7. **ActorsPage + ActorDetailPage** — `adminActors` + existing `actor` query
8. **EditionsPage + EditionDetailPage** — `adminEditions` + existing `editions` query

#### 2.4 Docker / Dev Setup

**Modified files:**
- `docker-compose.yml` — add `admin-app` service (node:22-slim, runs `npm run dev`, port 5173)
- `Dockerfile` — add stage to build React SPA, copy `dist/` into final image

**Dev flow:**
- `docker compose up` starts: neo4j, browserless, api (port 3000), admin-app (port 5173)
- Admin SPA at `http://localhost:5173/admin`, proxies `/graphql` to `:3000`

**Production flow:**
- Dockerfile builds SPA with `npm run build`
- Copies `dist/` into the Rust binary's image
- Rust server serves static files from `/admin/assets/*`
- Catch-all `/admin/*` returns `index.html`

---

### Phase 3: Dioxus Removal

Once the React SPA is functional and replaces all admin pages.

**Delete:**
- `modules/rootsignal-api/src/components/` (entire directory — 14 files)
- `modules/rootsignal-api/src/pages/` (entire directory)
- `modules/rootsignal-api/src/templates.rs`
- `modules/rootsignal-api/src/rest/` (entire directory — already migrated to GraphQL in Phase 1)
- All `/admin/*` HTML routes from `main.rs`
- All `/api/*` REST routes from `main.rs` (already migrated to GraphQL in Phase 1)
- `AdminSession` extractor from `auth.rs` (replaced by JWT)
- `check_admin_auth` basic auth from `auth.rs` (replaced by AdminGuard)

**Modify:**
- `modules/rootsignal-api/Cargo.toml` — remove `dioxus = { version = "0.6", features = ["ssr"] }`
- `modules/rootsignal-api/src/main.rs` — remove all non-GraphQL route groups, add static file serving + SPA catch-all. Only routes remaining: `/graphql` (POST/GET), `/admin/*` (SPA catch-all), `/` (health check)
- `modules/rootsignal-api/src/auth.rs` — remove session cookie logic + basic auth, keep JWT only

**Keep:**
- GraphQL endpoint (`/graphql`) — single API surface
- Health check (`/`)
- Static file serving + SPA catch-all (`/admin/*`)

---

## Acceptance Criteria

### Functional Requirements
- [ ] Admin can log in via phone OTP and receive a JWT
- [ ] All 10 admin pages render correctly in the React SPA
- [ ] Dashboard shows all 7 charts and 4 data tables with city selector
- [ ] Map loads signals via `signalsNearGeoJson` GraphQL query with click-through to detail
- [ ] All REST endpoints (`/api/*`) removed — GraphQL is the sole API
- [ ] Cities page supports creating cities, adding sources, running/stopping/resetting scouts
- [ ] Scout status updates via polling on city detail page
- [ ] Signals, stories, actors, editions pages support pagination
- [ ] JWT expiry redirects to login page
- [ ] Dioxus dependency fully removed from Cargo.toml
- [ ] `cargo build` succeeds without Dioxus

### Non-Functional Requirements
- [ ] Dashboard loads in <2s (15 parallel DB calls, same as current)
- [ ] SPA static assets cached with content hashing (long TTL)
- [ ] API responses have `no-store` cache headers
- [ ] CORS configured correctly for both dev (localhost:5173) and prod
- [ ] HTTP-only cookie with Secure flag in production
- [ ] No admin data accessible without valid JWT

---

## Risk Analysis & Mitigation

| Risk | Impact | Mitigation |
|---|---|---|
| GraphQL schema expansion is large (15+ new queries/mutations) | High effort | Phase 1 focuses only on API; can ship incrementally |
| Dashboard data complexity (15 parallel DB calls) | Performance risk | Use DataLoader pattern + single `adminDashboard` resolver with `tokio::join!` |
| Dioxus removal breaks something unexpected | Regression | Phase 3 is last — full SPA must be working first |
| CORS + cookie issues across origins | Auth breaks | Dev proxy eliminates CORS in dev; same-origin serving in prod |
| Chart library migration (Chart.js → Recharts) | Visual regression | Implement charts one at a time, compare against current screenshots |

## References

### Internal
- Brainstorm: `docs/brainstorms/2026-02-19-admin-app-split-brainstorm.md`
- Current GraphQL schema: `modules/rootsignal-api/src/graphql/schema.rs`
- Current admin pages: `modules/rootsignal-api/src/pages/mod.rs`
- Current auth: `modules/rootsignal-api/src/auth.rs`
- Current Dioxus components: `modules/rootsignal-api/src/components/`

### MN Together (reference implementation)
- JWT service: `~/Developer/fourthplaces/mntogether/packages/server/src/domains/auth/jwt.rs`
- Auth middleware: `~/Developer/fourthplaces/mntogether/packages/server/src/common/auth/restate_auth.rs`
- Frontend auth actions: `~/Developer/fourthplaces/mntogether/packages/web-app/lib/auth/actions.ts`
- GraphQL context: `~/Developer/fourthplaces/mntogether/packages/shared/graphql/context.ts`
- Admin middleware: `~/Developer/fourthplaces/mntogether/packages/admin-app/middleware.ts`
