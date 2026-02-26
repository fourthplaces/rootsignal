---
date: 2026-02-25
topic: graph-explorer
---

# Graph Explorer — Admin App

## What We're Building

A split-pane graph exploration tool in the admin app that lets you visually browse and debug the rootsignal knowledge graph. Left pane is a Mapbox map (entry point — pan/zoom to scope), right pane is a React Flow graph canvas (nodes + edges), with a right sidebar for filters and a collapsible bottom inspector pane (like Chrome DevTools) for drilling into any selected node.

The map viewport drives the graph scope — as you pan the map, the graph updates to show nodes within the visible bounds. Everything is controllable from the filter pane: time window, node types, max nodes, and search.

## Why This Approach

We considered three approaches:

1. **Full graph dump** — render everything, let the user zoom/filter. Rejected: thousands of nodes makes force-directed layout unusable.
2. **Node-centric neighborhood explorer** — pick a starting node, expand 1-hop at a time. Good for debugging but no spatial context.
3. **Geo-scoped split-pane with filters** — map as entry point, graph shows what's in view, filter pane controls density. Chosen: combines spatial intuition with graph structure, stays performant via node cap.

## Layout

```
┌─────────────────────┰─────────────────────┬───────────────┐
│                     ┃                     │  Explorer     │
│      Map            ┃      Graph          │  ───────────  │
│   (Mapbox)          ┃    (React Flow)     │               │
│                     ┃                     │  Time window  │
│   ○ ○    ○          ┃   ○──○    ○──○      │  ●━━━━━━━●    │
│     ○  ○            ┃    \ |   / |        │  Jan 12 Feb 25│
│       ○             ┃     ○───○  ○        │  ▄▂▅▇▃▆▄▂▅▇  │
│  ○      ○           ┃                     │               │
│                     ┃                     │  Max nodes    │
│  [  ◀ divider ▶  ]                       │  [===●==] 100 │
│                     ┃                     │               │
│                     ┃                     │  Node types   │
│                     ┃                     │  ☑ Signals    │
│                     ┃                     │  ☑ Stories    │
│                     ┃                     │  ☑ Actors     │
│                     ┃                     │  ☐ Citations  │
│                     ┃                     │  ☑ Sources    │
│                     ┃                     │               │
│                     ┃                     │  Search...    │
├─────────────────────┸─────────────────────┴───────────────┤
│ ▾ Inspector  [Properties] [Relationships] [Logs] [Tree]   │
│ Signal: "Free Legal Aid Clinic" · Aid · 0.87 · staged     │
└───────────────────────────────────────────────────────────┘
```

## Key Decisions

### Split pane with draggable divider
Map on left, graph on right. Divider is draggable so you can go full-map or full-graph. Bidirectional highlighting: click a node in the graph, it pulses on the map. Click a dot on the map, it selects in the graph.

### Filter pane is the single control surface
All controls live in the right sidebar:
- **Time window**: dual-knob range slider with a mini activity histogram behind it. Drag either knob or drag the filled region to slide the whole window.
- **Max nodes**: slider (25–500). "Showing 100 of 847 nodes" when over cap. Most-connected nodes shown first.
- **Node type checkboxes**: with counts. Sensible defaults — Signals, Stories, Actors, Sources on; Citations off (too numerous).
- **Search**: find a node by name/title/URL, centers it in both map and graph.

### Sensible defaults, full user control
Auto-suggestions (e.g. fewer node types at wide zoom) but every filter is manually overridable. No magic that hides data without the user knowing.

### Map viewport = graph scope
As the map pans/zooms, the graph query re-runs with the visible bounds + time range + node types + limit. This is the core data flow.

### Progressive density by zoom level
At wide zoom with the node cap active, most-connected/recent nodes surface first. At tight zoom (neighborhood), usually under cap naturally so everything shows.

### Inspector pane (bottom, collapsible)
Tabs for the selected node:
- **Properties**: all fields for that node type
- **Relationships**: table of connected nodes with edge types
- **Logs**: scout run events that touched this node (traced via nodeId/matchedId in scout_run_events). Works for signals (created/deduped/linted) and sources (scrape events across runs).
- **Tree**: for sources, the discovery tree. For scout events, the parent→child event chain via parentId.

## Graph Nodes to Surface

### Already in admin app (detail pages exist)
- Signals (5 types: Gathering, Aid, Need, Notice, Tension)
- Stories
- Sources (SourcesPage + SourceDetailPage)
- Situations

### Partially exposed (nested only, no standalone page)
- Actors (listed on signals, bounded query, no detail page)
- Dispatches (nested under situations)
- Tags (on stories/situations)

### Not yet exposed (no GraphQL, no pages)
- PlaceNode — venues (id, name, slug, lat/lng, geocoded)
- ResourceNode — capability taxonomy (name, description, signal_count; has Requires/Offers/Prefers edges)
- PinNode — ephemeral scrape instructions
- SubmissionNode — human-submitted URLs

The graph explorer would be the natural home for all of these — each node type gets a color/shape in the graph, and clicking any node shows its properties in the inspector.

## Backend Requirements

### New: `graphNeighborhood` query
Existing queries return nodes without edges. Need a new query:
```graphql
graphNeighborhood(
  minLat: Float!, maxLat: Float!, minLng: Float!, maxLng: Float!,
  from: DateTime!, to: DateTime!,
  nodeTypes: [String!],
  limit: Int
): GraphNeighborhood

type GraphNeighborhood {
  nodes: [GraphNode!]!
  edges: [GraphEdge!]!
}
```
Returns nodes + edges together so the client can render the graph in one round-trip.

### New: event lookup by node ID
For the Logs inspector tab, need to query scout_run_events by nodeId/matchedId rather than just runId:
```graphql
adminNodeEvents(nodeId: UUID!, limit: Int): [ScoutRunEvent!]!
```

### New: GraphQL types for unexposed nodes
Places, Resources need GQL types and resolvers to appear in the graph.

## Frontend Libraries

- **React Flow** for the graph canvas — nodes are React components (reuse existing badges/pills), natural "add nodes to state" expansion model, Tailwind-friendly
- **Mapbox GL JS** (or react-map-gl) for the map pane — already a dependency pattern in the codebase
- **Split pane**: `react-resizable-panels` or similar for the draggable divider

## Open Questions

- Should the graph layout be force-directed or hierarchical/layered? Force-directed is more organic but less predictable. Could offer both as a toggle.
- How to handle nodes without location (e.g. Resources, some Actors)? Show in graph but not on map, or cluster them in a "no location" bucket?
- Edge rendering: show all edges or only edges between visible nodes? Edges to off-screen nodes could show as "exit arrows" pointing toward the hidden node.
- Performance: should graphNeighborhood be a direct DB query or read from the graph store? Graph store (neo4j/memgraph if applicable) would be more natural for edge traversal.

## Next Steps

→ `/workflows:plan` for implementation breakdown — phased approach starting with the graph canvas + filter pane, then adding the map split-pane, then the inspector.
