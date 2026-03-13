---
title: "feat: Graph Explorer admin page"
type: feat
date: 2026-02-25
---

# Graph Explorer — Admin App

## Overview

A split-pane graph exploration tool for debugging and diagnosing the rootsignal knowledge graph. Map on the left (Mapbox, entry point), graph canvas on the right (React Flow), filter pane in a right sidebar, and a collapsible bottom inspector pane (Chrome DevTools style) with Properties/Relationships/Logs/Tree tabs.

The map viewport drives graph scope — pan the map, graph updates. All controls (time range, node types, max nodes) live in the filter sidebar. Designed to handle thousands of records with a node cap that keeps rendering performant at any zoom level.

## Problem Statement

We have thousands of signals, actors, stories, sources, and edges in Neo4j with no way to visualize relationships or debug data issues. The existing admin pages are entity-list views — useful for browsing but blind to graph structure. When something goes wrong (bad dedup, missing edges, orphaned nodes), there's no tool to see what happened.

## Proposed Solution

Three aggressive phases, each independently useful:

1. **Phase 1: Graph canvas + filter pane + inspector** — pick a node via search, see its neighborhood, drill into properties/logs
2. **Phase 2: Map split-pane** — geo-scoped graph loading, bidirectional map↔graph highlighting
3. **Phase 3: Cross-run log accumulation** — `adminNodeEvents` query, Logs tab traces scout events by signal ID

---

## Technical Approach

### Architecture

**Route:** `/graph` in admin app, added to AdminLayout sidebar nav.

**Data flow:**
1. User sets filters (bounds from map viewport, time range, node types, max nodes)
2. Frontend calls `graphNeighborhood` GraphQL query
3. Backend reads from CachedReader (in-memory snapshot) + extracts edges between returned nodes
4. Frontend renders nodes in React Flow, dots on Mapbox map
5. Click node → inspector populates via existing detail queries + new `adminNodeEvents`

**Key constraint:** CachedReader already materializes all relationships in memory (`actors_by_signal`, `story_by_signal`, `citation_by_signal`, `tension_responses`, `tags_by_story`, etc.). The `graphNeighborhood` resolver reads from this cache — no new Neo4j queries needed for Phase 1.

### Implementation Phases

#### Phase 1: Graph Canvas + Filter Pane + Inspector

**Goal:** Search for any node, see its 1-2 hop neighborhood as a graph, inspect properties and relationships.

**New dependencies:**
- `@xyflow/react` (React Flow v12) — graph canvas
- `react-resizable-panels` — split layouts

**Frontend files:**

`modules/admin-app/src/pages/GraphExplorerPage.tsx` — main page component:
- Filter sidebar (right): node type checkboxes with counts, max nodes slider (25–500), dual-knob time range slider with activity histogram, search input
- Graph canvas (center): React Flow with custom node components per type (colored badges matching existing signal type colors)
- Inspector pane (bottom, collapsible): tabs for Properties, Relationships, Logs, Tree
- Sensible defaults: Signals + Stories + Actors on, Citations off, last 30 days, max 100 nodes

`modules/admin-app/src/components/graph/` — graph-specific components:
- `GraphNode.tsx` — custom React Flow node component (renders type badge, title, confidence)
- `GraphEdge.tsx` — custom edge with label (edge type name)
- `InspectorPane.tsx` — bottom pane with tab switching
- `FilterSidebar.tsx` — all filter controls
- `TimeRangeSlider.tsx` — dual-knob slider with histogram background

**GraphQL query:**

```graphql
query GraphNeighborhood(
  $minLat: Float, $maxLat: Float, $minLng: Float, $maxLng: Float,
  $from: DateTime!, $to: DateTime!,
  $nodeTypes: [String!]!,
  $limit: Int!
) {
  graphNeighborhood(
    minLat: $minLat, maxLat: $maxLat, minLng: $minLng, maxLng: $maxLng,
    from: $from, to: $to, nodeTypes: $nodeTypes, limit: $limit
  ) {
    nodes {
      id
      nodeType
      label
      lat
      lng
      metadata
    }
    edges {
      sourceId
      targetId
      edgeType
    }
    totalCount
  }
}
```

**Backend — GraphQL resolver** (`schema.rs`):
```rust
#[guard(AdminGuard)]
async fn graph_neighborhood(
    &self, ctx: &Context<'_>,
    min_lat: Option<f64>, max_lat: Option<f64>,
    min_lng: Option<f64>, max_lng: Option<f64>,
    from: DateTime<Utc>, to: DateTime<Utc>,
    node_types: Vec<String>,
    limit: u32,
) -> Result<GqlGraphNeighborhood>
```

**Backend — Reader function** (`cached_reader.rs`):

New function `graph_neighborhood()` that:
1. Loads cache snapshot via `self.cache.load_full()`
2. Collects matching nodes (filtered by bounds if provided, time range, node types, limit)
3. Builds a `HashSet<Uuid>` of returned node IDs
4. Walks relationship maps (`actors_by_signal`, `story_by_signal`, `citation_by_signal`, `tension_responses`, `tags_by_story`) to extract edges where **both endpoints are in the returned set**
5. Returns `(nodes, edges, total_count)`

This is pure in-memory work on the existing cache — no new Neo4j queries.

**Backend — New GQL types** (`types.rs`):
```rust
#[derive(SimpleObject)]
pub struct GqlGraphNeighborhood {
    pub nodes: Vec<GqlGraphNode>,
    pub edges: Vec<GqlGraphEdge>,
    pub total_count: u32,
}

#[derive(SimpleObject)]
pub struct GqlGraphNode {
    pub id: String,        // UUID as string
    pub node_type: String, // "Gathering", "Actor", "Story", etc.
    pub label: String,     // title, name, or headline
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub metadata: String,  // JSON blob with type-specific fields
}

#[derive(SimpleObject)]
pub struct GqlGraphEdge {
    pub source_id: String,
    pub target_id: String,
    pub edge_type: String, // "Contains", "ActedIn", "SourcedFrom", etc.
}
```

**Routing** (`App.tsx`):
- Add `<Route path="graph" element={<GraphExplorerPage />} />`

**Nav** (`AdminLayout.tsx`):
- Add "Graph" to sidebar navigation

**Acceptance criteria:**
- [x] `/graph` route renders with filter sidebar + graph canvas + inspector
- [x] Search for a signal by title → graph shows signal + connected actors, story, citations
- [x] Node type checkboxes filter which types appear
- [x] Max nodes slider caps rendered nodes
- [x] Time range slider filters by extractedAt/contentDate
- [x] Click node in graph → inspector shows properties tab with all fields
- [x] Relationships tab shows connected nodes with edge types
- [x] Ghost edge counts show hidden neighbor counts when node types are filtered out
- [x] URL search params encode selected node, time range, and filters (deep linking)
- [x] Node opacity reflects confidence; edge line style varies by edge type
- [x] `npx tsc --noEmit` passes

---

#### Phase 2: Map Split-Pane

**Goal:** Map on left, graph on right, draggable divider. Map viewport drives graph scope. Bidirectional highlighting.

**Frontend changes:**

Update `GraphExplorerPage.tsx`:
- Wrap map + graph in `react-resizable-panels` `PanelGroup` with horizontal direction
- Left panel: Mapbox GL map (reuse pattern from existing `MapPage.tsx` — clustering, signal type colors, GeoJSON features)
- Right panel: React Flow canvas (from Phase 1)
- Divider: draggable resize handle

**Map → Graph data flow:**
- On map `moveend` event, read viewport bounds via `map.getBounds()`
- Debounce (300ms) then re-fetch `graphNeighborhood` with new bounds
- Graph canvas updates with filtered nodes

**Bidirectional highlighting:**
- Click node in graph → `flyTo` that location on map, pulse the marker
- Click marker on map → select that node in graph, scroll to it, open inspector
- Hover sync: highlight corresponding element in both views

**Map markers:**
- One marker per node that has lat/lng
- Colored by node type (existing color scheme from MapPage)
- Clustered at wide zoom (Mapbox GL `cluster` source option)

**Acceptance criteria:**
- [x] Split pane with draggable divider renders map + graph side by side
- [x] Pan/zoom map → graph updates with nodes in viewport
- [x] Click graph node → map flies to location
- [x] Click map marker → graph selects node, inspector opens
- [x] Divider draggable to resize panels (can go nearly full-map or full-graph)
- [x] Node cap ("Showing 100 of 847") displayed in filter sidebar
- [x] Activity histogram behind time slider shows signal volume per day

---

#### Phase 3: Cross-Run Log Accumulation

**Goal:** Inspector Logs tab traces all scout run events that touched a specific node across all runs.

**New GraphQL query:**
```graphql
query AdminNodeEvents($nodeId: String!, $limit: Int) {
  adminNodeEvents(nodeId: $nodeId, limit: $limit) {
    id parentId seq ts type sourceUrl query url provider platform
    signalType title resultCount confidence success action
    matchedId existingId spentCents reason field oldValue newValue summary
  }
}
```

**Backend — new resolver** (`schema.rs`):
```rust
#[guard(AdminGuard)]
async fn admin_node_events(
    &self, ctx: &Context<'_>,
    node_id: String,
    limit: Option<u32>,
) -> Result<Vec<ScoutRunEvent>>
```

**Backend — new DB function** (`scout_run.rs`):
```rust
pub async fn list_events_by_node_id(
    pool: &PgPool,
    node_id: &str,
    limit: u32,
) -> Result<Vec<EventRow>>
```

SQL:
```sql
SELECT * FROM scout_run_events
WHERE node_id = $1 OR matched_id = $1 OR existing_id = $1
ORDER BY ts DESC
LIMIT $2
```

**Note:** `node_id`, `matched_id`, and `existing_id` columns may need indexes:
```sql
CREATE INDEX IF NOT EXISTS idx_scout_run_events_node_id ON scout_run_events(node_id);
CREATE INDEX IF NOT EXISTS idx_scout_run_events_matched_id ON scout_run_events(matched_id);
CREATE INDEX IF NOT EXISTS idx_scout_run_events_existing_id ON scout_run_events(existing_id);
```

**Frontend — Inspector Logs tab:**
- Lazy-loaded: only fetches when Logs tab is active and a node is selected
- Shows timeline of events that touched this node
- Each event: timestamp, event type (colored badge), summary, link to full scout run
- Parent-child nesting via `parentId` (collapsible tree)

**Frontend — Inspector Tree tab:**
- For sources: reuse `discoveryTree` from SourceDetail query (already exists)
- For scout events: render parent→child chain from `parentId` relationships

**Acceptance criteria:**
- [x] Select a signal node → Logs tab shows all scout events that created/deduped/linted it
- [x] Events grouped by run, ordered by timestamp DESC
- [x] Parent-child nesting via parentId renders as collapsible tree
- [x] DB migration adds indexes on node_id, matched_id, existing_id columns
- [x] Tree tab shows source discovery tree for source nodes

---

## Files Modified

### Phase 1
| File | Action | Description |
|------|--------|-------------|
| `modules/admin-app/package.json` | Edit | Add `@xyflow/react`, `react-resizable-panels` |
| `modules/admin-app/src/App.tsx` | Edit | Add `/graph` route |
| `modules/admin-app/src/layouts/AdminLayout.tsx` | Edit | Add "Graph" nav item |
| `modules/admin-app/src/graphql/queries.ts` | Edit | Add `GRAPH_NEIGHBORHOOD` query |
| `modules/admin-app/src/pages/GraphExplorerPage.tsx` | Create | Main page |
| `modules/admin-app/src/components/graph/GraphNode.tsx` | Create | Custom React Flow node |
| `modules/admin-app/src/components/graph/GraphEdge.tsx` | Create | Custom React Flow edge |
| `modules/admin-app/src/components/graph/InspectorPane.tsx` | Create | Bottom inspector |
| `modules/admin-app/src/components/graph/FilterSidebar.tsx` | Create | Right sidebar controls |
| `modules/admin-app/src/components/graph/TimeRangeSlider.tsx` | Create | Dual-knob slider |
| `modules/rootsignal-api/src/graphql/schema.rs` | Edit | Add `graphNeighborhood` resolver |
| `modules/rootsignal-api/src/graphql/types.rs` | Edit | Add `GqlGraphNeighborhood`, `GqlGraphNode`, `GqlGraphEdge` |
| `modules/rootsignal-graph/src/cached_reader.rs` | Edit | Add `graph_neighborhood()` function |

### Phase 2
| File | Action | Description |
|------|--------|-------------|
| `modules/admin-app/src/pages/GraphExplorerPage.tsx` | Edit | Add map split-pane |
| `modules/admin-app/src/components/graph/GraphMap.tsx` | Create | Mapbox map component |

### Phase 3
| File | Action | Description |
|------|--------|-------------|
| `modules/admin-app/src/graphql/queries.ts` | Edit | Add `ADMIN_NODE_EVENTS` query |
| `modules/admin-app/src/components/graph/InspectorPane.tsx` | Edit | Add Logs + Tree tabs |
| `modules/rootsignal-api/src/graphql/schema.rs` | Edit | Add `adminNodeEvents` resolver |
| `modules/rootsignal-api/src/db/models/scout_run.rs` | Edit | Add `list_events_by_node_id` |
| `modules/rootsignal-api/migrations/XXX_node_event_indexes.sql` | Create | Indexes on node_id columns |

---

## Acceptance Criteria

### Functional Requirements
- [x] `/graph` route accessible from admin sidebar
- [x] Graph renders nodes as colored, typed badges (matching existing signal type color scheme)
- [x] Edges render between connected nodes with type labels
- [x] Filter sidebar controls: node types (with counts), max nodes slider, time range dual-knob slider, search
- [x] Inspector pane: Properties, Relationships, Logs, Tree tabs
- [x] Map + graph split-pane with draggable divider
- [x] Map viewport drives graph scope (debounced)
- [x] Bidirectional map↔graph highlighting
- [x] Node cap keeps rendering performant ("Showing X of Y nodes")
- [x] Logs tab traces scout events by signal/source node ID across all runs

### Non-Functional Requirements
- [x] Graph renders 100+ nodes without jank (React Flow handles this natively)
- [x] Map pan → graph update debounced to ≤1 request per 300ms
- [x] Inspector tabs lazy-load data (don't fetch Logs until tab is active)
- [x] `npx tsc --noEmit` passes after each phase

---

## Design Decisions

### Dagre (hierarchical) layout as default
Force-directed layouts jitter when nodes are added/removed (e.g. on map pan). For a debugging tool, stability matters more than organic aesthetics. Default to dagre hierarchical layout; offer force-directed as a toggle for exploration.

### Deep linking via URL search params
Encode `nodeId`, `lat`, `lng`, `zoom`, `timeFrom`, `timeTo` in URL search params. This lets developers share a specific graph view via Slack when debugging data issues together.

### Ghost edges for filtered-out neighbors
When node type checkboxes hide a type, show a count badge on remaining nodes indicating hidden neighbors (e.g. "3 hidden citations"). Prevents confusion when debugging — an "orphaned" node might just have its connections filtered out.

### Visual encoding
| Feature | Encoding |
|---------|----------|
| Node type | Border color + icon (existing signal type color scheme) |
| Confidence | Node opacity (low confidence = faded) |
| Edge type | Line style: solid for Contains, dashed for SimilarTo/RespondsTo |
| Time recency | Border thickness (recent = thicker) |

### Z-index management
Use `react-resizable-panels` to strictly contain Mapbox and React Flow in their panels. Both libraries use heavy absolute positioning — containment prevents markers bleeding under the inspector. Inspector pane gets explicit `z-50`.

---

## Risk Analysis & Mitigation

| Risk | Impact | Mitigation |
|------|--------|------------|
| Force-directed layout unstable with 100+ nodes | Graph jumps around on expand | Dagre hierarchical layout as default, force-directed as toggle |
| Map pan triggers too many refetches | Performance/API spam | Debounce 300ms, abort in-flight requests on new pan |
| CachedReader edge extraction slow at limit=500 | HashSet lookups spike | Relationship maps are HashMap→Vec — both-endpoints-in-set check is O(edges), cap keeps it bounded |
| node_id columns not indexed | Slow Logs tab queries | Phase 3 migration adds btree indexes before query ships |
| React Flow + Mapbox bundle size | Larger admin app | Both tree-shakeable; acceptable for admin tool |
| Z-index collisions between map/graph/inspector | UI glitches | Strict panel containment via react-resizable-panels |

## References

### Internal
- Brainstorm: `docs/brainstorms/2026-02-25-graph-explorer-brainstorm.md`
- Existing MapPage: `modules/admin-app/src/pages/MapPage.tsx`
- CachedReader cache structure: `modules/rootsignal-graph/src/cache.rs` (SignalCache struct)
- Signal type colors: `modules/admin-app/src/lib/event-colors.tsx`
- Scout run events model: `modules/rootsignal-api/src/db/models/scout_run.rs`
- Edge types: `modules/rootsignal-common/src/types.rs:1579-1610`
