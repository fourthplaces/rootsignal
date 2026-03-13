---
date: 2026-03-03
topic: events-browser
---

# Event Store Browser (`/events`)

## What We're Building

A dedicated admin page at `/events` that provides a paginated, filterable timeline of all event-sourcing events, with a side panel that renders the full causal tree for any selected event. This gives operators first-class visibility into the event store — what happened, why, and what it caused.

## Why This Approach

The existing `LogsTab` in the graph inspector is scoped to a single graph node and only shows events whose JSONB payload references that node's ID. It can't answer "what was the full chain that led to this `ConfidenceScored`?" because the chain spans multiple nodes and event types. A standalone page with a recursive causal tree query solves this.

We chose a side-panel tree (not inline expand or drill-down) because:
- The timeline stays visible for context while exploring the tree
- You can click between tree nodes and timeline events without losing your place
- It mirrors the existing inspector pattern (bottom panel) but oriented for a different use case

## Key Decisions

- **Two new GraphQL queries**: `adminEvents` (paginated list) and `adminEventCausalTree` (recursive CTE). Keeps concerns separated — the list query is simple/fast, the tree query is heavier but only fires on selection.
- **Recursive CTE on `parent_seq`**: Walk up to root, then down to all descendants. The `idx_events_parent` index already exists. No new indexes needed.
- **Three-layer badges**: Color-code events by layer (World = blue, System = amber, Pipeline = gray) using the event type prefix to classify.
- **Filter dimensions**: Layer toggle, event_type dropdown, correlation_id search, run_id search, time range. All have existing indexes.
- **Pagination**: Cursor-based on `seq DESC` (most recent first). Simple and efficient with the primary key.

## Data Flow

```
[Filter Bar]
    │
    ▼
adminEvents(limit, cursor, filters)
    │
    ▼
[Timeline List]  ──click──▶  adminEventCausalTree(seq)
                                      │
                                      ▼
                              [Causal Tree Panel]
```

### `adminEvents` query

```graphql
query AdminEvents(
  $limit: Int!
  $cursor: Int          # seq to paginate from
  $layers: [String!]    # "world", "system", "pipeline"
  $eventType: String    # exact event_type filter
  $correlationId: String
  $runId: String
  $from: DateTime
  $to: DateTime
) {
  adminEvents(
    limit: $limit
    cursor: $cursor
    layers: $layers
    eventType: $eventType
    correlationId: $correlationId
    runId: $runId
    from: $from
    to: $to
  ) {
    events {
      seq
      ts
      eventType
      layer          # derived: "world" | "system" | "pipeline"
      parentSeq
      correlationId
      runId
      payload        # raw JSONB as string for the detail panel
      summary        # server-side extracted 1-line summary
    }
    nextCursor
    totalEstimate    # pg_class reltuples or COUNT, for "~N events"
  }
}
```

### `adminEventCausalTree` query

```graphql
query AdminEventCausalTree($seq: Int!) {
  adminEventCausalTree(seq: $seq) {
    events {
      seq
      ts
      eventType
      layer
      parentSeq
      summary
      payload
    }
    rootSeq
  }
}
```

Backend implementation: two CTEs.
1. Walk up from `seq` via `parent_seq` to find `rootSeq`.
2. Walk down from `rootSeq` via `parent_seq` to collect all descendants.
Return the union.

### SQL sketch for causal tree

```sql
WITH RECURSIVE
  -- Walk up to root
  ancestors AS (
    SELECT * FROM events WHERE seq = $1
    UNION ALL
    SELECT e.* FROM events e
    JOIN ancestors a ON e.seq = a.parent_seq
  ),
  root AS (
    SELECT seq FROM ancestors WHERE parent_seq IS NULL
  ),
  -- Walk down from root
  descendants AS (
    SELECT * FROM events WHERE seq = (SELECT seq FROM root)
    UNION ALL
    SELECT e.* FROM events e
    JOIN descendants d ON e.parent_seq = d.seq
  )
SELECT * FROM descendants ORDER BY seq;
```

## UI Layout

```
┌─────────────────────────────────────────────────────────────────┐
│ /events                                                         │
├──────────────────────────────────────────┬──────────────────────┤
│ [World] [System] [Pipeline]  Type ▾      │ Causal Tree          │
│ Correlation: ______  Run: ______         │                      │
│ From: ______ To: ______                  │  ● GatheringAnnounced│
│──────────────────────────────────────────│    ├─ CitationRecorded│
│ #1042  Mar 3 14:23  World                │    ├─ ConfidenceScored│
│        GatheringAnnounced                │    │   └─ ► NodeMerged│
│        "Community Garden Meetup..."      │    └─ CategoryClassif.│
│──────────────────────────────────────────│                      │
│ #1041  Mar 3 14:23  System               │  ► = selected event  │
│        ConfidenceScored                  │                      │
│        node_id: abc123, score: 0.85      │  Click any tree node │
│──────────────────────────────────────────│  to re-select it in  │
│ #1040  Mar 3 14:22  System               │  the timeline.       │
│        CitationRecorded                  │                      │
│        ...                               │                      │
├──────────────────────────────────────────┴──────────────────────┤
│ Showing 50 of ~12,340 events                    [Load more]     │
└─────────────────────────────────────────────────────────────────┘
```

## Layer Classification

Derive the layer from event_type prefix:

| Prefix pattern | Layer |
|---|---|
| `gathering_announced`, `resource_offered`, `help_requested`, `announcement_published`, `concern_raised` | World |
| `confidence_scored`, `category_classified`, `actor_identified`, `situation_*`, `task_*`, etc. | System |
| `url_scraped`, `budget_checkpoint`, `extraction_completed`, `scrape_*` | Pipeline |

This mapping lives server-side and is returned as a `layer` field.

## Open Questions

- **Payload rendering**: Should the tree panel show a raw JSON viewer for the full payload, or extract key fields per event type? Start with key fields + expandable raw JSON.
- **Event volume**: With high event volumes, should we add a "jump to seq" input for direct navigation? Probably yes, easy to add.
- **Cross-linking**: Should clicking an event that references a `node_id` in its payload link to `/graph?node=<id>`? Nice to have, not blocking.

## Next Steps

→ `/workflows:plan` for implementation details (backend queries, React components, routing)
