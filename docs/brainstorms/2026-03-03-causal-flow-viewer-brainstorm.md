---
date: 2026-03-03
topic: causal-flow-viewer
---

# Causal Flow Viewer

## What We're Building

A real-time flowchart pane in the Events tab that visualizes the causal decision tree of a scout run. Each run produces a DAG of events and handlers: events trigger handlers, handlers decide what to emit. The flow viewer projects this causal chain into a React Flow graph where event-type nodes show volume (x count) and handler nodes show the decision points.

Users can follow a run's lifecycle in real time as events stream in, and click event-type nodes to drill into individual events.

## Why This Approach

The causal chain data already exists — `correlation_id` links trees, `parent_id` tracks parent→child, `run_id` scopes to a run. The missing piece is `handler_id`: knowing which handler produced which events. With that metadata stamped during seesaw's dispatch, we can project the full `event → handler → events` graph without any static schema or predefined phase definitions.

This is better than a static flowchart because the pipeline's branching is dynamic — handlers decide at runtime what to emit based on conditions. The graph reflects what actually happened (or is happening), not what we predicted would happen.

## Key Decisions

- **Handlers as nodes**: Include handlers in the graph as decision-point nodes between event groups. This requires stamping `handler_id` on events in seesaw's dispatch loop.
- **Event-type grouping**: Nodes represent event types with counts (e.g., `[ScrapeCompleted x 12]`), not individual events. Clicking a node expands to show individual events.
- **React Flow**: Use existing `@xyflow/react` dependency for rendering.
- **Real-time updates**: Filter the existing `EVENTS_SUBSCRIPTION` by `run_id` and append nodes/edges as events arrive.
- **Run-scoped**: The graph is scoped to a single `run_id`. Could later support merging/overlaying multiple runs.

## Design

### Graph Structure

```
[EngineStarted]
       |
  (lifecycle:reap)
       |
[PhaseCompleted(ReapExpired)]
       |
  (scrape:fetch)
      / \
[ScrapeCompleted x 12]  [ScrapeFailed x 2]
      |
  (signals:extract)
      |
[NewSignalAccepted x 8]  [SignalDeduplicated x 3]
      |
  (lifecycle:finalize)
      |
[RunCompleted]
```

- **Rectangle nodes**: Event types (grouped, with count badge)
- **Rounded/pill nodes**: Handlers (the decision points)
- **Edges**: Causal links (parent_id → child groupings)

### Node Interaction

- Click event-type node → expand to see individual events (could open in timeline filtered, or inline list)
- Hover → show timing info (first/last event ts, duration)
- Color-coded by layer (world/system/telemetry)

### Changes Required

#### 1. Seesaw: Stamp handler_id on emitted events

During dispatch, when a handler emits events, stamp the handler's `id` (from `#[handle(id = "...")]`) onto each emitted event's metadata. This flows through to `AppendEvent` and into the DB.

#### 2. DB: Add handler_id column

```sql
ALTER TABLE events ADD COLUMN handler_id TEXT;
```

#### 3. GraphQL: New query for flow graph

```graphql
query AdminCausalFlow($runId: String!) {
  adminCausalFlow(runId: $runId) {
    nodes {
      id          # unique node id (event_type or handler_id)
      kind        # "event" | "handler"
      label       # display name
      eventType   # for event nodes
      handlerId   # for handler nodes
      count       # number of events (for event nodes)
      layer       # world/system/telemetry
    }
    edges {
      source
      target
    }
  }
}
```

The API builds the graph server-side: group events by `(handler_id, event_type)`, create nodes + edges from parent_id relationships.

#### 4. Frontend: Flow pane

New pane type `"causal-flow"` in the Events tab pane registry. Uses `@xyflow/react` with dagre or elkjs for auto-layout.

- Subscribe to `EVENTS_SUBSCRIPTION` filtered by active `run_id`
- On new events: re-query or incrementally update the graph
- Integrate with existing `selectSeq` to cross-link with timeline/causal tree panes

## Open Questions

- Should the flow graph be built server-side (single GraphQL query returns nodes+edges) or client-side (client fetches raw events and builds the graph)? Server-side is cleaner but less flexible for real-time incremental updates.
- Auto-layout algorithm: dagre (simpler, good for DAGs) vs elkjs (more options, heavier)?
- How to handle very large runs (hundreds of events)? Collapse by default, expand on click?

## Next Steps

→ `/workflows:plan` for implementation details across seesaw, API, and frontend
