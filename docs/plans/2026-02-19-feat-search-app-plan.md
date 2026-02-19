---
title: "feat: Public Search App with Viewport-Driven Semantic Search"
type: feat
date: 2026-02-19
---

# Public Search App with Viewport-Driven Semantic Search

## Overview

A separate, public-facing SPA at `modules/search-app/` with a split-pane layout: searchable list (left) + Mapbox map (right). The map viewport and search query are always coupled — what you see on the map is what you see in the list. Default view surfaces signals and stories sorted by heat. Two tabs (Signals, Stories) let users switch content types. Semantic search via existing Voyage AI embeddings.

## Problem Statement

Root Signal has rich community intelligence data (signals, stories, actors) but no public-facing way for community members, journalists, or organizers to discover and explore it. The admin dashboard is internal-only. A search app lets anyone find signals by topic and geography.

## Key Design Decisions

- **D1: Viewport as spatial filter.** Map bounds = query bounds. No separate location picker. Every pan/zoom re-queries.
- **D2: Semantic search, not keyword.** Leverage existing Voyage AI embeddings + Neo4j vector indexes. "Immigration enforcement" finds "ICE" content.
- **D3: Story search via signal aggregation.** Stories have no embeddings (no `story_embedding` vector index). Search signals semantically, then aggregate to parent stories. This ships faster than adding a story embedding pipeline.
- **D4: Mapbox GL JS (direct, no wrapper).** Public-facing polish, clustering, smooth animations. Use `mapbox-gl` npm package directly in React, not `react-map-gl`.
- **D5: Search on Enter, not on keystroke.** Each embedding call costs a Voyage API round-trip. Fire on Enter key only, with debounce on viewport changes (300ms).
- **D6: Desktop-first, mobile later.** Split-pane is inherently desktop. Mobile (bottom-sheet pattern) is a follow-up.
- **D7: URL deep linking from the start.** `?lat=X&lng=Y&z=Z&q=QUERY&tab=signals&id=UUID` — shareability is core for a civic tool.
- **D8: Blended ranking when searching.** `0.6 * similarity + 0.4 * normalized_heat` when a query is active. Heat-only when no query.

## Technical Approach

### Architecture

```
┌─────────────────────────────────────────────────────┐
│                    search-app (React 19 + Vite)     │
│  ┌──────────────┐  ┌────────────────────────────┐   │
│  │  Left Pane   │  │       Right Pane            │   │
│  │  ┌────────┐  │  │                             │   │
│  │  │ Search │  │  │    Mapbox GL JS Map         │   │
│  │  │ Input  │  │  │    - GeoJSON source          │   │
│  │  ├────────┤  │  │    - Cluster layer           │   │
│  │  │Tabs:   │  │  │    - Signal point layer      │   │
│  │  │Signals │  │  │    - Popup on click           │   │
│  │  │Stories │  │  │                             │   │
│  │  ├────────┤  │  │    moveend → onBoundsChange │   │
│  │  │ Result │  │  │                             │   │
│  │  │ List   │  │  └────────────────────────────┘   │
│  │  │        │  │                                   │
│  │  ├────────┤  │  Apollo Client (no auth)          │
│  │  │ Detail │  │         │                         │
│  │  │ Panel  │  │         ▼                         │
│  │  └────────┘  │  GraphQL API (/graphql)           │
│  └──────────────┘         │                         │
└───────────────────────────┼─────────────────────────┘
                            ▼
              ┌──────────────────────────┐
              │   rootsignal-api (Axum)  │
              │   New public queries:    │
              │   - searchSignalsInBounds│
              │   - searchStoriesInBounds│
              │   - signalsInBounds      │
              │   - storiesInBounds      │
              └──────────┬───────────────┘
                         │
              ┌──────────▼───────────────┐
              │   rootsignal-graph       │
              │   New reader methods:    │
              │   - semantic_search_in_  │
              │     bounds()             │
              │   - signals_in_bounds()  │
              │   - stories_in_bounds()  │
              └──────────┬───────────────┘
                         │
              ┌──────────▼───────────────┐
              │   Neo4j + Voyage AI      │
              │   Vector KNN + bbox      │
              │   post-filter            │
              └──────────────────────────┘
```

### Implementation Phases

#### Phase 1: Backend — Bounding Box & Semantic Search Queries

New reader methods in `modules/rootsignal-graph/src/reader.rs`:

**1a. `signals_in_bounds(bbox, limit)` — no search, heat-sorted**

For the default view (no query active). Uses existing `lat/lng` property indexes.

```
// Cypher pattern (per signal type):
MATCH (n:Event)
WHERE n.lat >= $min_lat AND n.lat <= $max_lat
  AND n.lng >= $min_lng AND n.lng <= $max_lng
RETURN n
ORDER BY n.cause_heat DESC, n.confidence DESC
LIMIT $limit
```

**1b. `stories_in_bounds(bbox, limit)` — no search, energy-sorted**

Uses `centroid_lat/centroid_lng` on Story nodes.

```
MATCH (s:Story)
WHERE s.centroid_lat >= $min_lat AND s.centroid_lat <= $max_lat
  AND s.centroid_lng >= $min_lng AND s.centroid_lng <= $max_lng
RETURN s
ORDER BY s.energy DESC
LIMIT $limit
```

**1c. `semantic_search_signals_in_bounds(query_embedding, bbox, limit)` — KNN + bbox**

Over-fetch from vector index, post-filter by bounding box, blend scores.

```
// Run per signal type (Event, Give, Ask, Notice, Tension):
CALL db.index.vector.queryNodes($index_name, $k, $embedding)
YIELD node, score
WHERE score >= 0.3
  AND node.lat >= $min_lat AND node.lat <= $max_lat
  AND node.lng >= $min_lng AND node.lng <= $max_lng
RETURN node, score
ORDER BY (score * 0.6 + node.cause_heat * 0.4) DESC
LIMIT $limit
```

K=100 per type (500 total across 5 types), post-filter to bbox, return top 50.

**1d. `semantic_search_stories_in_bounds(query_embedding, bbox, limit)` — via signal aggregation**

Since stories lack embeddings, search signals first, then aggregate to parent stories:

```
// For each signal type:
CALL db.index.vector.queryNodes($index_name, $k, $embedding)
YIELD node, score
WHERE score >= 0.3
  AND node.lat >= $min_lat AND node.lat <= $max_lat
  AND node.lng >= $min_lng AND node.lng <= $max_lng
WITH node, score
MATCH (s:Story)-[:CONTAINS]->(node)
RETURN s, MAX(score) AS best_signal_score
ORDER BY (best_signal_score * 0.6 + s.energy * 0.4) DESC
LIMIT $limit
```

**1e. Embed search query via Voyage AI**

Add a method to `ai-client` or a new utility in `rootsignal-graph` that takes a plain text query string and returns a 1024-dim embedding via the existing Voyage AI client. The existing `embed_text()` in `modules/rootsignal-scout/src/embed.rs` can be extracted or called.

New GraphQL queries in `modules/rootsignal-api/src/graphql/schema.rs`:

```graphql
# No auth guard (public queries)

# Default browsing (no search query)
signalsInBounds(minLat: Float!, maxLat: Float!, minLng: Float!, maxLng: Float!, limit: Int, types: [SignalType]): [GqlSignal!]!
storiesInBounds(minLat: Float!, maxLat: Float!, minLng: Float!, maxLng: Float!, limit: Int): [GqlStory!]!

# With semantic search
searchSignalsInBounds(query: String!, minLat: Float!, maxLat: Float!, minLng: Float!, maxLng: Float!, limit: Int): [SearchResult!]!
searchStoriesInBounds(query: String!, minLat: Float!, maxLat: Float!, minLng: Float!, maxLng: Float!, limit: Int): [StorySearchResult!]!
```

New GraphQL types:

```graphql
type SearchResult {
  signal: GqlSignal!
  score: Float!          # blended similarity + heat
}

type StorySearchResult {
  story: GqlStory!
  score: Float!          # best signal match + energy blend
  topMatchingSignalTitle: String  # show why this story matched
}
```

**Files to modify:**
- `modules/rootsignal-graph/src/reader.rs` — add 4 new methods
- `modules/rootsignal-api/src/graphql/schema.rs` — add 4 new queries + 2 new types
- `modules/rootsignal-api/src/graphql/types.rs` — add `SearchResult`, `StorySearchResult` GQL types
- Extract or expose embedding function from scout for API use

**Files to create:**
- None — extend existing files

#### Phase 2: Frontend — Search App Scaffold

New app at `modules/search-app/` following admin-app patterns.

**Folder structure:**

```
modules/search-app/
├── index.html
├── package.json
├── tsconfig.json
├── vite.config.ts
├── src/
│   ├── main.tsx                    # Entry: ApolloProvider + BrowserRouter
│   ├── App.tsx                     # Routes
│   ├── index.css                   # Tailwind v4 theme
│   ├── vite-env.d.ts
│   ├── lib/
│   │   ├── graphql-client.ts       # Apollo Client (no auth, no credentials)
│   │   ├── utils.ts                # cn() utility
│   │   └── url-state.ts            # URL <-> state sync (lat/lng/z/q/tab/id)
│   ├── graphql/
│   │   └── queries.ts              # GQL queries for search
│   ├── hooks/
│   │   ├── useDebouncedBounds.ts   # 300ms debounce on viewport changes
│   │   ├── useUrlState.ts          # Sync URL params with React state
│   │   └── useSearch.ts            # Orchestrates query + bounds → Apollo
│   ├── pages/
│   │   └── SearchPage.tsx          # Main (and only) page
│   └── components/
│       ├── MapView.tsx             # Mapbox GL JS wrapper
│       ├── SearchInput.tsx         # Text input, fires on Enter
│       ├── TabBar.tsx              # Signals / Stories tabs
│       ├── SignalList.tsx          # Signal results list
│       ├── StoryList.tsx           # Story results list
│       ├── SignalCard.tsx          # Single signal in list
│       ├── StoryCard.tsx           # Single story in list
│       ├── SignalDetail.tsx        # Detail panel for signal
│       ├── StoryDetail.tsx         # Detail panel for story
│       └── EmptyState.tsx          # "No results" / "Zoom in" messages
```

**Dependencies (package.json):**
- `react` 19, `react-dom` 19, `react-router` 7
- `@apollo/client`, `graphql`
- `mapbox-gl` (v3.x)
- `tailwindcss` v4, `@tailwindcss/vite`
- `clsx`, `tailwind-merge`

**No `react-map-gl`** — use mapbox-gl directly for full control.

#### Phase 3: Frontend — Core Interactions

**3a. SearchPage layout (split pane)**

```
┌──────────────────────────────────────────────┐
│ ┌──────────────┬───────────────────────────┐ │
│ │   400px      │          flex-1            │ │
│ │              │                            │ │
│ │  SearchInput │     MapView               │ │
│ │  TabBar      │     (Mapbox GL JS)        │ │
│ │  ResultList  │                            │ │
│ │  or          │                            │ │
│ │  DetailPanel │                            │ │
│ │              │                            │ │
│ └──────────────┴───────────────────────────┘ │
└──────────────────────────────────────────────┘
```

**3b. State flow**

```
URL params ←→ React state ←→ Map + List

State shape:
{
  bounds: LngLatBounds | null   // from map moveend (debounced 300ms)
  query: string                  // from search input (fires on Enter)
  tab: 'signals' | 'stories'    // from tab bar
  selectedId: string | null      // from list click
  flyToTarget: {lng, lat} | null // triggers map.flyTo()
}
```

- URL syncs bidirectionally: changing state updates URL, loading URL sets state
- `bounds` change → if `query` is empty, fire `signalsInBounds` or `storiesInBounds`
- `bounds` change + `query` present → fire `searchSignalsInBounds` or `searchStoriesInBounds`
- Click signal → set `selectedId` + `flyToTarget` → detail panel + map animation
- Clear search → revert to heat-sorted viewport query

**3c. MapView component**

- Initialize Mapbox GL JS in `useEffect` with `useRef` for map instance
- GeoJSON source with `cluster: true`, `clusterMaxZoom: 14`, `clusterRadius: 50`
- Three layers: cluster circles, cluster counts, individual signal points
- Signal points colored by type (Event=blue, Give=green, Ask=orange, Notice=gray, Tension=red)
- `moveend` event → `onBoundsChange(map.getBounds())`
- Click unclustered point → callback to parent with signal id + coords
- Click cluster → zoom in to expansion zoom
- Cursor: pointer on hover over interactive features
- `flyTo` effect reacts to `flyToTarget` prop changes

**3d. URL deep linking**

Pattern: `/?lat=44.97&lng=-93.27&z=12&q=ICE&tab=signals&id=<uuid>`

- On load: parse URL → set initial map center/zoom + query + tab + selection
- On state change: update URL via `replaceState` (no history entries for viewport changes)
- `query` and `tab` changes push to history (back button navigable)

#### Phase 4: Docker & CORS

**4a. Docker compose service**

```yaml
search-app:
  image: node:22
  working_dir: /app
  volumes:
    - ./modules/search-app:/app
  ports:
    - "5174:5174"  # different port from admin-app (5173)
  command: sh -c "npm install && npm run dev -- --host --port 5174"
  environment:
    - API_URL=http://api:3000
```

**4b. CORS**

Add `http://localhost:5174` to allowed origins in `modules/rootsignal-api/src/main.rs` (debug mode). Production: add search app domain to `CORS_ORIGINS` env var.

**4c. Vite proxy**

```ts
// vite.config.ts
server: {
  port: 5174,
  proxy: {
    '/graphql': {
      target: process.env.API_URL || 'http://localhost:3001',
      changeOrigin: true,
    },
  },
}
```

**Files to modify:**
- `docker-compose.yml` — add search-app service
- `modules/rootsignal-api/src/main.rs` — add CORS origin

**Files to create:**
- `modules/search-app/` — full new app directory (see Phase 2 structure)

## Acceptance Criteria

### Functional Requirements

- [ ] App loads at `localhost:5174`, shows map centered on US with signals sorted by heat
- [ ] Panning/zooming the map updates the list to show signals/stories within the visible area
- [ ] Typing a query and pressing Enter filters results semantically within the current viewport
- [ ] Panning with an active search query updates results (query + bounds coupled)
- [ ] Clearing the search reverts to heat-sorted viewport results
- [ ] Clicking a signal in the list flies the map to it and shows a detail panel
- [ ] Signals tab shows individual signals; Stories tab shows aggregated stories
- [ ] Clicking a story shows its detail with constituent signals
- [ ] URL reflects current state (lat/lng/zoom/query/tab/id) and is shareable
- [ ] Map clusters signals at lower zoom levels, expanding on click
- [ ] Signal points are colored by type

### Non-Functional Requirements

- [ ] No authentication required
- [ ] Viewport change → list update within 500ms (300ms debounce + 200ms query)
- [ ] Semantic search round-trip (embed + KNN + filter) under 1 second
- [ ] Works in Chrome, Firefox, Safari (WebGL 2 required for Mapbox)

### Quality Gates

- [ ] No auth tokens or credentials in client code
- [ ] Coordinate fuzzing preserved (sensitive signals show approximate locations)
- [ ] Empty states for: no results, no data in viewport, search with no matches
- [ ] Loading states during query execution

## Dependencies & Prerequisites

- Mapbox account + access token (store in `VITE_MAPBOX_TOKEN` env var)
- Voyage AI API available for query embedding (existing infrastructure)
- Neo4j vector indexes populated (already exist for all signal types)
- Embedding function accessible from API layer (currently in scout module)

## Open Questions (Deferred to Implementation)

- Exact cluster styling (colors, sizes, breakpoints)
- Detail panel content depth (which fields to show, how much evidence/actor info)
- Story-to-signals drill-down navigation pattern
- Rate limiting strategy for public API endpoints
- Mobile layout (bottom-sheet pattern — future phase)

## Future Enhancements

- **Typesense/Meilisearch** for typo-tolerant instant search
- **Story embeddings** via story_weaver pipeline for direct story semantic search
- **Mobile layout** with bottom-sheet pattern
- **Signal type filters** in the UI
- **Geolocation** to auto-center map on user's location
- **Saved searches / bookmarks** (requires auth)

## References

### Internal References

- Brainstorm: `docs/brainstorms/2026-02-19-search-app-brainstorm.md`
- Admin app patterns: `modules/admin-app/src/`
- GraphQL schema: `modules/rootsignal-api/src/graphql/schema.rs`
- Graph reader (spatial queries): `modules/rootsignal-graph/src/reader.rs`
- Vector similarity: `modules/rootsignal-graph/src/similarity.rs`
- Existing KNN usage: `modules/rootsignal-graph/src/writer.rs:481`, `writer.rs:1749`, `response.rs:107`
- Embedding function: `modules/rootsignal-scout/src/embed.rs`
- CORS config: `modules/rootsignal-api/src/main.rs:196-213`
- Docker compose: `docker-compose.yml`
- Data quality learnings: `docs/solutions/2026-02-17-unwrap-or-masks-data-quality.md`
- Scaling bottlenecks: `docs/analysis/scaling-bottlenecks.md`

### External References

- [Mapbox GL JS React tutorial](https://docs.mapbox.com/help/tutorials/use-mapbox-gl-js-with-react/)
- [Mapbox clustering example](https://docs.mapbox.com/mapbox-gl-js/example/cluster/)
- [Neo4j vector index KNN queries](https://neo4j.com/docs/cypher-manual/current/indexes/semantic-indexes/vector-indexes/)
- [Neo4j vector similarity functions](https://neo4j.com/docs/cypher-manual/current/functions/vector/)
