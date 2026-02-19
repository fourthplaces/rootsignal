---
date: 2026-02-19
topic: search-app
---

# Root Signal Search App

## What We're Building

A separate, public-facing SPA with a split-pane layout: a searchable list on the left and a Mapbox map on the right. The map viewport and search query are always coupled — what you see on the map is what you see in the list. No auth required.

The default view surfaces signals and stories with the most heat/tension. Two tabs (Signals, Stories) let users switch between content types.

## Core Interactions

- **Default view**: Map loads at a default extent, list shows signals/stories sorted by heat
- **Two tabs**: Signals tab and Stories tab in the left pane
- **Viewport-driven filtering**: Every pan/zoom updates the list to match the visible map area
- **Semantic search**: Text input at the top of the list filters within the current viewport using Voyage AI embeddings. E.g. "ICE" returns ICE-related stories and signals visible on the map
- **Coupled state**: Search query + map bounds always travel together. Searching "ICE" then panning to Chicago shows ICE results in Chicago
- **Click a result**: Map flies to the signal/story location AND a detail panel slides in

## Why This Approach

- **Mapbox over Leaflet**: Public-facing app needs visual polish, clustering, and smooth interactions. Admin app uses Leaflet which is fine internally but Mapbox is better for a product experience.
- **Semantic search over keyword**: We already have Voyage AI embeddings in the stack. Semantic search handles natural language queries and conceptual matches (e.g. "immigration enforcement" finds "ICE" content) without new infrastructure.
- **Viewport as filter**: Simpler mental model than separate location pickers or region selectors. The map IS the spatial filter.
- **Separate app**: Different audience (public) and different UX goals from the admin dashboard. Clean separation.

## Tech Stack

| Layer | Technology | Notes |
|-------|-----------|-------|
| Framework | React 19 + Vite + TailwindCSS | Same as admin app |
| Map | Mapbox GL JS | Replaces Leaflet for public app |
| Data | Apollo Client + GraphQL | Existing API layer |
| Search | Voyage AI semantic search | Existing embedding infrastructure |
| API | New GraphQL queries | `search_signals_in_bounds`, `search_stories_in_bounds` |

## New Backend Work Needed

- GraphQL query: `search_signals_in_bounds(query: String, bbox: BBox, limit: Int)` — embed the query via Voyage, find nearest signals within bounding box, sort by heat
- GraphQL query: `search_stories_in_bounds(query: String, bbox: BBox, limit: Int)` — same for stories
- Bounding-box spatial filtering in graph reader (extend existing `find_nodes_near` with bbox support)
- Optional: precompute and store embeddings for signal/story text if not already persisted

## Future Enhancements

- **Typesense / Meilisearch integration**: Add a dedicated search engine for typo-tolerant, instant (<5ms) full-text search with faceting, highlighting, and relevance ranking. Would require a sync layer from Neo4j and a new Docker service. Killer upgrade once the core experience is validated.

## Open Questions

- Default map extent: whole US, or geolocate to user's position?
- Debounce strategy for viewport changes (how many ms after pan/zoom before re-querying?)
- Should "view signals by story" be a drill-down from the Stories tab, or a grouped view in the Signals tab?
- Signal detail panel: how much detail to show vs linking out?

## Next Steps

→ `/workflows:plan` for implementation details
