---
date: 2026-02-14
topic: admin-heat-map
---

# Interactive Heat Map for Admin App

## What We're Building

A dedicated `/map` page in the admin app that serves as a situational awareness dashboard. Users type natural language queries like "where is help needed right now" and see results visualized geographically as a heat map. Multiple data layers (signal density, coverage gaps, entity distribution) can be toggled. The map supports geographic zoom — clusters break apart into individual pins — and clicking a pin opens a detail sidebar showing the actual listings and entities at that location.

## Why This Approach

The GraphQL API already exposes `heatMapPoints`, `signalDensity`, `signalGaps`, `parseQuery`, and `search` — rich geospatial and NLP data that isn't visualized anywhere in the admin UI. A dedicated full-page map (rather than a dashboard widget) gives enough room for the search bar, filter controls, layer toggles, and detail sidebar this interaction model requires.

## Key Decisions

- **Dedicated `/map` page**: The interaction complexity (search + filters + toggles + zoom + detail panel) requires a full page, not a widget.
- **Mapbox GL JS**: Best native support for heat map layers and point clustering. Free tier is generous. Leaflet would require plugins for comparable heat map rendering.
- **NLP + structured filters**: Combine freeform search (via `parseQuery`) with dropdowns (domain, category, entity type, zip code, radius) for maximum flexibility.
- **Three toggleable layers**: Signal density, coverage gaps, entity distribution — each backed by existing API queries.
- **Cluster → pin → detail flow**: Geographic zoom breaks clusters into individual pins. Clicking a pin opens a sidebar with listing/entity details that links back to existing detail pages (`/listings/[id]`, `/entities/[id]`).

## Interaction Model

1. **Search bar** at top — accepts natural language queries, parsed via `parseQuery`
2. **Filter controls** — dropdowns for domain, category, entity type, zip code, radius
3. **Layer toggles** — switch between density / gaps / entity views
4. **Map** — auto-centers and zooms to result clusters
5. **Zoom interaction** — clusters break apart into individual pins at higher zoom levels
6. **Detail sidebar** — click a pin to see listings/entities at that location, with links to detail pages

## Data Sources

| Layer | API Query | Returns |
|-------|-----------|---------|
| Signal density | `heatMapPoints(zipCode?, radiusMiles?, entityType?)` | Weighted lat/lng points |
| Coverage gaps | `signalGaps(domain?, category?, limit?)` | Zip-level gap scores |
| Entity distribution | `heatMapPoints(entityType?)` | Lat/lng by entity type |
| Search | `parseQuery(q!)` + `search(q?, filters...)` | Parsed intent + results |
| Pin detail | `listing(id)` / `entity(id)` | Full record for sidebar |

## Open Questions

- Mapbox API key management — env var in Next.js config
- Should the map remember last viewport/query on return visits (localStorage)?
- Any additional layers to consider in the future (e.g., temporal — activity over time)?

## Next Steps

-> `/workflows:plan` for implementation details
