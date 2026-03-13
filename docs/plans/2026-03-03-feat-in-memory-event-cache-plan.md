---
title: "feat: In-Memory Event Cache for Admin Panel"
type: feat
date: 2026-03-03
brainstorm: docs/brainstorms/2026-03-03-in-memory-event-cache-brainstorm.md
---

# In-Memory Event Cache for Admin Panel

## Overview

Add a bounded in-process cache of the most recent 500K events, replacing slow SQL queries in the admin panel with in-memory lookups. Text search becomes a linear scan over structs; causal tree/flow lookups become HashMap hits. The cache follows the same projection pattern as `SignalCache` — hydrate on startup, stay live via the existing `EventBroadcast` (PG LISTEN/NOTIFY).

## Problem Statement

The admin panel has two confirmed slow paths:

1. **Text search** — `ILIKE '%term%'` on `payload::text` cannot be indexed, full table scan every query
2. **Causal tree loading** — recursive lookups chasing `correlation_id` / `parent_seq` chains across SQL round-trips

Both are developer-facing DX friction today.

## Proposed Solution

### Data Structure

```
EventCache {
    events: VecDeque<Arc<StoredEvent>>,       // bounded at 500K, newest at back
    by_seq: HashMap<i64, Arc<StoredEvent>>,   // O(1) event lookup
    by_correlation: HashMap<Uuid, Vec<i64>>,  // correlation_id → [seq]
    by_run: HashMap<String, Vec<i64>>,        // run_id → [seq]
    by_handler: HashMap<String, Vec<i64>>,    // handler_id → [seq]
    capacity: usize,                          // 500_000
}
```

Bucket `Vec<i64>` values are kept sorted by seq for O(log n) removal on eviction.

### Thread Safety

`Arc<RwLock<EventCache>>` — reads are concurrent (many GraphQL resolvers), writes are serialized (one event at a time from broadcast listener). This matches the admin-only access pattern. Not using `ArcSwap` because the cache is mutated incrementally (append + evict), not swapped wholesale like `SignalCache`.

### Hydration Strategy

1. **On startup**: `SELECT {COLUMNS} FROM events ORDER BY seq DESC LIMIT 500000`, reversed into VecDeque order (oldest first, newest at back). Build all HashMaps during load.
2. **Live updates**: Subscribe to the existing `EventBroadcast::subscribe()` receiver. On each `AdminEvent`, convert to `StoredEvent` (or store `AdminEvent` directly — see open decision below), append to back of VecDeque, update all indexes, evict from front if at capacity.
3. **Startup window**: Return empty results until hydration completes. The admin panel already handles empty state gracefully. Hydration for 500K rows should take single-digit seconds.

### Cache Miss → Fall Through to Postgres

**The cache is an optimization, not a replacement.** When a `seq`, `correlation_id`, or `run_id` is not found in any HashMap, the resolver falls through to the existing SQL query. This ensures:

- Deep links to old events still work
- Causal trees spanning beyond the 500K window are still loadable
- No silent data loss from the admin perspective

The GraphQL resolvers gain a two-tier read path: cache first, Postgres second.

### EventHandle::log Fix

`EventHandle::log` (fire-and-forget) currently does not call `notify_new_event`. Fix this by changing the spawned INSERT to `INSERT ... RETURNING seq` and calling `notify_new_event(pool, seq)` afterward. Small, self-contained change that closes the known NOTIFY gap for all consumers, not just the cache.

**File:** `modules/rootsignal-events/src/store.rs` (~line 434)

### Text Search Semantics

Match the existing SQL behavior exactly:
- **Case-insensitive** (to match `ILIKE`)
- **Fields searched**: serialized `payload` JSON string, `event_type`, `run_id`, `correlation_id` as string
- Implementation: `.to_lowercase().contains(&term.to_lowercase())` across each field per event

At 500K events this is sub-10ms for a single-threaded scan.

### Eviction & Index Coherence

When `VecDeque` is at capacity and a new event arrives:

1. `pop_front()` → get the evicted event
2. Remove `evicted.seq` from `by_seq`
3. For each bucket index (`by_correlation`, `by_run`, `by_handler`):
   - Find the bucket by the evicted event's key
   - Binary search for the seq, remove it
   - If bucket is now empty, remove the HashMap key

This is O(log n) per bucket per eviction — negligible overhead.

### Cursor Pagination Over VecDeque

The VecDeque is ordered by seq (ascending, oldest at front). For cursor-based pagination (`seq < cursor, ORDER BY seq DESC, LIMIT N`):

1. Binary search the VecDeque for the cursor seq position
2. Slice backward N events from that position
3. Return the slice (already in correct order)

This is O(log n) for the cursor lookup + O(N) for the slice — fast.

### Time Range Filtering

Linear scan with `ts` comparison. The VecDeque is seq-ordered (roughly monotonic with ts). For admin-only usage at 500K items, O(n) is acceptable. No optimization needed now.

## Implementation Phases

### Phase 1: EventCache struct + hydration

- [x] Create `modules/rootsignal-api/src/event_cache.rs`
- [x] Define `EventCache` struct with `VecDeque`, `HashMap` indexes, capacity
- [x] Implement `EventCache::hydrate(pool: &PgPool) -> Result<Self>` — bulk SELECT, build indexes
- [x] Implement `EventCache::push(&mut self, event)` — append, evict if full, update indexes
- [x] Implement `EventCache::remove_oldest(&mut self)` — eviction with index cleanup
- [x] Wire into `main.rs` alongside `EventBroadcast::spawn` — hydrate, wrap in `Arc<RwLock<_>>`, add to schema data

**Tests:**
- `hydrated_cache_contains_loaded_events`
- `cache_evicts_oldest_when_at_capacity`
- `eviction_removes_from_all_indexes`
- `push_updates_all_indexes`

### Phase 2: Live updates via EventBroadcast

- [x] Spawn a background task that calls `event_broadcast.subscribe()` and feeds events into the cache via `push()`
- [x] Fix `EventHandle::log` to call `notify_new_event` after INSERT RETURNING seq

**Tests:**
- `live_event_appears_in_cache_after_broadcast`
- `event_handle_log_fires_notify`

### Phase 3: Query methods + GraphQL resolver integration

- [x] Implement `EventCache::search(&self, term, cursor, limit, time_range, run_id) -> Vec<Arc<StoredEvent>>`
- [x] Implement `EventCache::causal_tree(&self, seq) -> Option<Vec<Arc<StoredEvent>>>`
- [x] Implement `EventCache::causal_flow(&self, run_id) -> Vec<Arc<StoredEvent>>`
- [x] Update `adminEvents` resolver: try cache first, fall through to SQL on miss
- [x] Update `adminCausalTree` resolver: try cache first, fall through to SQL on miss
- [x] Update `adminCausalFlow` resolver: try cache first, fall through to SQL on miss

**Tests:**
- `search_matches_payload_text_case_insensitive`
- `search_matches_event_type_and_run_id`
- `causal_tree_returns_all_events_with_same_correlation_id`
- `causal_flow_returns_all_events_for_run_id`
- `cursor_pagination_returns_correct_page`
- `cache_miss_falls_through_to_postgres`

## Technical Considerations

- **Memory**: ~250MB at 500K events (500 bytes avg per event + HashMap overhead). Acceptable for a single-server admin tool.
- **Startup time**: Bulk SELECT of 500K rows — estimate 2-5 seconds. Admin panel shows empty state during this window.
- **Testability**: Hydration query and broadcast subscription should be injectable (trait or closure) per CLAUDE.md testing rules. The cache struct itself is tested as a whole organ: push events in, query out, assert results.
- **No `SignalCache` coupling**: The event cache is independent of the graph cache. They are separate projections fed by different sources (Postgres vs Neo4j).

## Success Metrics

- Admin panel text search responds in <50ms (currently seconds)
- Causal tree loading responds in <10ms (currently hundreds of ms)
- No behavior regression for events outside the 500K window (fallback to SQL)

## Dependencies & Risks

- **Risk**: `EventHandle::log` fix is a prerequisite for cache completeness. Without it, fire-and-forget events are silently missing from live cache.
- **Risk**: `RwLock` write contention during high-throughput ingestion. Mitigated by the write being a single append + index update (microseconds under lock).
- **Dependency**: Existing `EventBroadcast` infrastructure (already production, no changes needed to consume it).

## References

- Brainstorm: `docs/brainstorms/2026-03-03-in-memory-event-cache-brainstorm.md`
- Event store: `modules/rootsignal-events/src/store.rs`
- Event broadcast: `modules/rootsignal-api/src/event_broadcast.rs`
- Graph cache pattern: `modules/rootsignal-graph/src/cache.rs`
- Admin queries: `modules/rootsignal-api/src/db/models/scout_run.rs`
- GraphQL resolvers: `modules/rootsignal-api/src/graphql/schema.rs`
- Admin frontend: `modules/admin-app/src/pages/events/EventsPaneContext.tsx`
