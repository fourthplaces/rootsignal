---
title: "feat: Interactive Heat Map Page"
type: feat
date: 2026-02-14
---

# Interactive Heat Map Page

## Overview

Add a dedicated `/map` page to the admin app that lets admins type natural language queries like "where is help needed right now" and see results geographically as a heat map. Supports multiple toggleable layers (signal density, coverage gaps, entity distribution), geographic zoom with clustering, and a detail sidebar for drilling into individual listings/entities.

## Problem Statement / Motivation

The GraphQL API already exposes rich geospatial data (`heatMapPoints`, `signalDensity`, `signalGaps`, `signalTrends`) and NLP search (`parseQuery`), but none of it is visualized in the admin UI. Admins currently have no way to understand geographic coverage, identify gaps, or see where help is needed at a glance. This page turns the admin app into a situational awareness dashboard.

## Proposed Solution

A full-page interactive map at `/map` using Mapbox GL JS directly (no `react-map-gl` wrapper — heatmap/cluster config is the same either way, and the wrapper adds little value here). The page is a client component loaded via `next/dynamic` with `ssr: false` to avoid Mapbox's `window` dependency during SSR.

### Architecture Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Map library | `mapbox-gl` v3 directly | Best native heatmap/clustering support; `react-map-gl` wrapper adds abstraction without simplifying heatmap config |
| SSR handling | `next/dynamic` + `ssr: false` wrapping a `"use client"` component | Belt-and-suspenders: prevents server-side import of `mapbox-gl` |
| Data fetching | Client-side `fetch("/api/graphql")` | Map requires DOM/interactivity; follows existing Observations page pattern |
| Layers | Mutually exclusive (radio toggle) | Simpler UX; density and gaps use fundamentally different visual treatments |
| Search results on map | Plot as temporary pins using `locations[].latitude/longitude` | Search results are a distinct overlay on top of the active layer |
| Layout | Standard sidebar layout, but remove `p-6` padding for map page | Keep navigation accessible; maximize map viewport |
| Mapbox style | `mapbox://styles/mapbox/light-v11` | Clean, matches admin app's white aesthetic |
| Detail links | Open in new tab (`target="_blank"`) | Preserves map state |

### Interaction Model

```
[Search Bar (NLP)] [Domain ▼] [Category ▼] [Entity Type ▼] [Zip] [Radius]
[● Density  ○ Gaps  ○ Entities]
┌─────────────────────────────────────────────────────────┬──────────────┐
│                                                         │              │
│                      MAP                                │   SIDEBAR    │
│                                                         │  (on click)  │
│    ◉ cluster (42)                                       │              │
│         ◉ cluster (12)      • pin                       │  Listing:    │
│              • pin  • pin                               │  "Food Bank  │
│                                                         │   Volunteers │
│                                                         │   Needed"    │
│                           ◉ cluster (8)                 │              │
│                                                         │  [View →]    │
│                                                         │              │
└─────────────────────────────────────────────────────────┴──────────────┘
```

## Technical Approach

### Phase 1: Foundation — Map Page with Density Layer

**New dependency:**
```
pnpm add mapbox-gl
```

**New files:**

- `app/(app)/map/page.tsx` — Server component shell that uses `next/dynamic` to load the map client component with `ssr: false`. Renders a loading skeleton while the map loads.

- `app/(app)/map/map-view.tsx` — `"use client"` component. Core map logic:
  - Initializes Mapbox GL JS in `useEffect` with `useRef` for the container div
  - Imports `mapbox-gl/dist/mapbox-gl.css`
  - Loads `heatMapPoints` (all) on mount as the default density layer
  - Configures heatmap layer with weight-based color ramp (transparent → blue → green → yellow → red)
  - Heatmap fades out at zoom ~9, circle points fade in at zoom ~7 (smooth transition)
  - Clustering on the GeoJSON source (`cluster: true`, `clusterMaxZoom: 14`, `clusterRadius: 50`)
  - Click cluster → `getClusterExpansionZoom` → `easeTo` animation
  - Click individual pin → open sidebar
  - Auto-centers map on data bounds using `map.fitBounds()` on initial load
  - Access token from `process.env.NEXT_PUBLIC_MAPBOX_TOKEN`

- `app/(app)/map/sidebar.tsx` — `"use client"` component. Detail panel:
  - Receives `entityType` ("listing" | "entity") and `entityId` (UUID)
  - Fetches full record via `/api/graphql` using `listing(id)` or `entity(id)` query
  - Shows key fields: title, description, status, location, tags
  - "View" link opens `/listings/[id]` or `/entities/[id]` in new tab
  - Close button (X) and Escape key to dismiss

**Modified files:**

- `app/(app)/layout.tsx` — Add `{ href: "/map", label: "Map" }` to `NAV_ITEMS` array. Conditionally remove `p-6` padding from `<main>` when on the `/map` route (use `usePathname` or pass as prop).

**Environment:**

- `.env.local` — Add `NEXT_PUBLIC_MAPBOX_TOKEN=pk.eyJ1Ijo...`
- Mapbox dashboard — Create token with URL restrictions for deployment domain

### Phase 2: Search and Filters

**Modified files:**

- `app/(app)/map/map-view.tsx` — Add search bar and filter controls above the map:
  - Search bar: text input + submit button. On submit, calls `parseQuery(q, autoSearch: true)`
  - Intent handling:
    - `IN_SCOPE` → Plot search results as pins (using `locations[0].latitude/longitude`), auto-fit bounds, populate filter dropdowns from parsed filters
    - `OUT_OF_SCOPE` → Show inline message: "This query is outside our scope. Try searching for volunteer needs, events, or organizations."
    - `NEEDS_CLARIFICATION` → Show `parsed.reasoning` with suggestion to refine
    - `KNOWLEDGE_QUESTION` → Show `parsed.reasoning` as an answer, don't alter map
  - Filter dropdowns: domain, category, entity type (populated from `tagKinds`/`tags` queries or hardcoded from known values)
  - Zip code text input + radius number input
  - Changing any filter triggers re-query of the active layer with new filter params
  - Search fires on Enter or button click (no search-as-you-type — `parseQuery` involves LLM)
  - Debounce filter changes (300ms) to batch rapid toggling
  - AbortController to cancel in-flight requests when a new query supersedes

### Phase 3: Layer Toggles and Gaps/Entity Views

**Modified files:**

- `app/(app)/map/map-view.tsx` — Add radio toggle for layers:
  - **Density** (default): `heatMapPoints` → Mapbox `heatmap` layer type
  - **Gaps**: `signalGaps(limit: 50)` → Mapbox `circle` layer with size proportional to inverse of `listingCount` (bigger circle = bigger gap), red color gradient
  - **Entities**: `heatMapPoints(entityType: selected)` → Mapbox `circle` layer with color-coded by `entityType` (nonprofits = blue, government = green, businesses = orange, faith = purple)
  - Switching layers: remove current source/layers, add new ones, re-fit bounds
  - Active filters carry over where the API supports them (domain/category for gaps; entityType/zip/radius for density/entities)
  - Disabled/grayed-out filters that don't apply to the active layer

### Phase 4: Polish

- **URL state**: Persist `q`, `domain`, `category`, `entityType`, `zip`, `radius`, `layer` in URL search params via `useSearchParams`. On mount, restore state from URL.
- **localStorage**: Save last map viewport (center, zoom) and restore on return visits.
- **Loading states**: Skeleton for initial map load; spinner overlay on the map during data fetches.
- **Empty states**: "No results found" message when a query/filter returns zero points. "No data available" for a layer with no data.
- **Responsive**: Sidebar slides in from right on desktop, slides up as a bottom sheet on mobile.

## Acceptance Criteria

- [x] `/map` page renders a Mapbox map within the admin app sidebar layout
- [x] Density heatmap layer loads by default showing all `heatMapPoints`
- [x] Map auto-centers/zooms to fit data bounds on load
- [x] Clusters break apart on zoom; clicking a cluster zooms in
- [x] Clicking an individual pin opens sidebar with listing or entity details
- [x] Sidebar "View" link opens detail page in new tab
- [x] NLP search bar accepts queries and plots results on map
- [x] `OUT_OF_SCOPE`, `NEEDS_CLARIFICATION`, `KNOWLEDGE_QUESTION` intents show appropriate messages
- [x] Structured filter dropdowns (domain, category, entity type, zip, radius) filter map data
- [x] Layer radio toggle switches between density, gaps, and entity views
- [x] Filters that don't apply to the active layer are visually disabled
- [x] URL search params preserve query/filter/layer state for shareability
- [x] Map viewport persists in localStorage across visits

## Dependencies & Risks

**Dependencies:**
- `mapbox-gl` v3 npm package (new dependency)
- Mapbox access token (free tier: 50k map loads/month — sufficient for admin usage)
- Existing GraphQL queries: `heatMapPoints`, `signalGaps`, `signalDensity`, `parseQuery`, `search`, `listing`, `entity`

**Risks:**
- **Data volume**: `heatMapPoints` with no filters returns all points. If the dataset grows to 50k+ points, initial load could be slow. Mitigation: Mapbox handles large GeoJSON well with clustering; add server-side bounding-box filtering later if needed.
- **`heatMapPoints` filter exclusivity bug**: The resolver treats `zipCode` and `entityType` as mutually exclusive (zip takes precedence). If both filters are needed simultaneously, the Rust resolver needs a fix. Mitigation: For Phase 1-2, this is acceptable; file a follow-up issue to fix the resolver.
- **`signalGaps` empty `signalDomainCounts`**: The query hardcodes `'{}'::jsonb` for domain counts. Gap tooltips cannot show which domains are missing. Mitigation: Show only `listingCount` in gap tooltips; fix the query later if domain breakdown is needed.

## References & Research

### Internal References
- Brainstorm: `docs/brainstorms/2026-02-14-heat-map-brainstorm.md`
- GraphQL schema: `modules/api-client-js/schema.graphql` (lines 89-97: HeatMapPoint, lines 368-375: ZipDensity, lines 481-505: geo queries)
- Heat map resolver: `modules/rootsignal-server/src/graphql/heat_map/mod.rs`
- Heat map domain: `modules/rootsignal-domains/src/heat_map.rs`
- Admin app layout/nav: `modules/admin-app/app/(app)/layout.tsx` (lines 6-15: NAV_ITEMS)
- Client-side fetch pattern: `modules/admin-app/app/(app)/observations/page.tsx`
- GraphQL proxy: `modules/admin-app/app/api/graphql/route.ts`

### External References
- [Mapbox GL JS heatmap example](https://docs.mapbox.com/mapbox-gl-js/example/heatmap-layer/)
- [Mapbox GL JS clustering example](https://docs.mapbox.com/mapbox-gl-js/example/cluster/)
- [Mapbox GL JS + React tutorial](https://docs.mapbox.com/help/tutorials/use-mapbox-gl-js-with-react/)
- [Next.js dynamic imports (ssr: false)](https://nextjs.org/docs/app/building-your-application/optimizing/lazy-loading)
