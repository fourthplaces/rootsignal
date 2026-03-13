---
date: 2026-03-03
topic: in-memory-event-cache
---

# In-Memory Event Cache

## What We're Building

A bounded in-process cache of the most recent events (up to 500K), fed from the same dispatch loop that feeds the graph projection. This gives the admin panel fast text search and instant tree traversal without hitting Postgres.

## Why This Approach

The admin panel has two slow paths today:

1. **Text search** across JSON event payloads — inherently expensive in SQL, no index fixes it well
2. **Event tree loading** — chasing `parent_seq`/`caused_by_seq` chains requires recursive CTEs or multiple round-trips

An in-memory projection solves both: text search becomes a linear scan over structs (low milliseconds for 500K items), and tree building becomes `HashMap` lookups.

We already hold an in-memory graph projection, so this is the same pattern — just another subscriber on the dispatch loop.

### Approaches Considered

- **In-process ring buffer** (chosen) — simplest, fastest, no new infra, same pattern as graph projection
- **Redis sidecar** — survives restarts but adds infra complexity we don't need for a single-server admin tool
- **SQLite in-memory with FTS5** — gives free full-text search but adds a dependency and query layer for marginal benefit over a linear scan at this scale

## Key Decisions

- **Cap at 500K events**: ~250MB at 500 bytes/event avg. Can adjust later.
- **LRU-style eviction**: bounded `VecDeque`, oldest events evicted as new ones push in
- **Side indexes**: `HashMap` indexes on `seq`, `aggregate_id`, `event_type`, `caused_by_seq` for O(1) tree lookups
- **Fed from dispatch loop**: same subscriber pattern as the graph projection — no polling, no separate sync
- **Text search = linear scan**: at 500K items, brute-force string matching is fast enough — no need for a search index

## Hydration Strategy

- **On startup**: query Postgres for the most recent 500K events, populate the cache
- **Ongoing**: subscribe via PG `LISTEN`/`NOTIFY` — cache updates the moment events are committed, no polling, no dispatch loop coupling
- Cache everything — no event type filtering

## Open Questions

- None — ready for planning

## Next Steps

-> `/workflows:plan` for implementation details
