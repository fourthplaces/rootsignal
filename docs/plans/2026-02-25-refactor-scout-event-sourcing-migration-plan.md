---
title: "refactor: Migrate scout from direct GraphStore writes to event-sourcing"
type: refactor
date: 2026-02-25
updated: 2026-02-26
depends_on: docs/plans/2026-02-25-refactor-enrichment-pipeline-plan.md
---

# Migrate Scout from GraphStore to Event-Sourcing

## Overview

Migrate all scout write operations from direct `GraphStore` Cypher to the event-sourcing path: append events to Postgres via `EventStore`, then project each event to Neo4j via `GraphProjector`. The graph is a materialized view of the event log. Enrichment (embeddings, diversity, cause_heat) runs as post-projection passes at end of each scout run.

**Architecture (implemented):**
```
Scout Pipeline
     │
     ▼
EventSourcedStore
     │
     ├─ 1. Build Event from method args
     ├─ 2. Append to EventStore (Postgres) → StoredEvent
     └─ 3. GraphProjector.project(&stored_event) → Neo4j MERGE
                                                        │
                                                  (end of run)
                                                        │
                                                  Embedding Enrichment
                                                  Metric Enrichment (diversity, cause_heat, actor_stats)
```

**Key design choice:** Inline append+project (not batch). Each write appends an event then immediately projects it, so graph reads always see current state. No read-after-write staleness.

## What's Done (Phase 1)

Phase 1 is complete. The signal lifecycle goes through events:

- [x] `EventSourcedStore` wraps `EventStore` + `GraphProjector` as the `SignalStore` implementation
- [x] `create_node` → `{Type}Discovered` event → project
- [x] `create_evidence` → `CitationRecorded` event → project
- [x] `corroborate` → `ObservationCorroborated` event → project (idempotent count)
- [x] `refresh_signal` → `FreshnessConfirmed` event → project
- [x] `refresh_url_signals` → batch `FreshnessConfirmed` events → project
- [x] `link_signal_to_source` → `SignalLinkedToSource` event → project
- [x] `ScrapePipeline::new()` builds `GraphProjector` and wires into `EventSourcedStore`
- [x] Embedding enrichment runs post-projection in `run_all()`
- [x] Metric enrichment (diversity, actor_stats, cause_heat) runs post-projection in `run_all()`
- [x] `append_and_read()` on EventStore returns full `StoredEvent` from INSERT RETURNING
- [x] All 301 scout tests pass, 82 graph tests pass, 30 event tests pass

**Key files:**
- `modules/rootsignal-scout/src/pipeline/event_sourced_store.rs` — append+project implementation
- `modules/rootsignal-scout/src/pipeline/scrape_pipeline.rs` — enrichment wiring
- `modules/rootsignal-graph/src/reducer.rs` — all projector handlers
- `modules/rootsignal-graph/src/embedding_enrichment.rs` — post-projection embedding pass
- `modules/rootsignal-events/src/store.rs` — `append_and_read()`

## Event Variant Coverage

**71 total variants.** All have either a projector handler or are explicitly no-op:
- **52 variants** with graph mutation handlers (signal CRUD, actors, sources, situations, tags, pins, resources, etc.)
- **19 variants** intentionally no-op (observability: `UrlScraped`, `LlmExtractionCompleted`, etc.; informational: `DuplicateDetected`, `ObservationRejected`, etc.)

No coverage gaps in the reducer. Every Event variant is accounted for.

## Remaining Work

### What still bypasses events

**Discovery workflows create signals via `GraphStore` directly:**
- `gathering_finder.rs` — calls `writer.create_node()` for discovered gatherings
- `response_finder.rs` — calls `writer.create_node()` for discovered aid signals and tensions

These are the only remaining signal creation paths not going through events. They're called from `ScrapePhase` discovery methods, which run during scrape phases.

**Actor lifecycle via `GraphStore` (pass-through in EventSourcedStore):**
- `upsert_actor()` → `ActorIdentified` event variant exists, reducer handler exists, but EventSourcedStore passes through to writer
- `link_actor_to_signal()` → `ActorLinkedToEntity` event exists + handler, but passes through
- `link_actor_to_source()` → `ActorLinkedToSource` event exists + handler, but passes through
- `update_actor_location()` → `ActorLocationIdentified` event exists + handler, but passes through
- `actor_extractor.rs` calls `writer.upsert_actor()` and `writer.find_actor_by_name()` directly

**Source lifecycle via `GraphStore`:**
- `upsert_source()` → `SourceRegistered` event exists + handler, but passes through
- `record_source_scrape()` → `SourceScrapeRecorded` event exists + handler, called from `metrics.rs` directly
- Source weight/cadence updates via `update_source_weight()` → `SourceChanged` event exists

**Resource/place lifecycle via `GraphStore`:**
- `find_or_create_resource()` — no event variant (graph-only operation)
- `create_requires/prefers/offers_edge()` — no event variants
- `find_or_create_place()` — no event variant

**Other direct writer writes (not through EventSourcedStore):**
- `reap_expired()` → `EntityExpired`/`EntityPurged` events exist + handlers
- ~~`batch_tag_signals()`~~ → now event-sourced via `TagsAggregated`, removed from GraphStore
- `delete_pins()` → `PinsRemoved` event exists + handler
- `set_query_embedding()` — no event variant (infrastructure)
- ~~`touch_signal_timestamp()`~~ → replaced by `store.refresh_signal()` (`FreshnessConfirmed`), removed from GraphStore
- Task management (`set_task_phase_status`, etc.) — operational, not domain events

---

## Phase 2: Actor Events through EventSourcedStore

**Goal:** Actor lifecycle emits events instead of direct writer calls. All event variants and reducer handlers already exist.

**Scope:** Convert EventSourcedStore pass-throughs from `writer.upsert_actor()` → `append_and_project(ActorIdentified)`, etc. Then convert `actor_extractor.rs` to call through EventSourcedStore instead of writer directly.

**Events (all have reducer handlers):**
- `ActorIdentified` — MERGE Actor node (idempotent on entity_id)
- `ActorLinkedToEntity` — MERGE ACTED_IN edge with role
- `ActorLinkedToSource` — MERGE HAS_SOURCE edge
- `ActorLocationIdentified` — SET location on Actor

**Tasks:**
- [x] EventSourcedStore: `upsert_actor()` → emit `ActorIdentified` event → project
- [x] EventSourcedStore: `link_actor_to_signal()` → emit `ActorLinkedToEntity` event → project
- [x] EventSourcedStore: `link_actor_to_source()` → emit `ActorLinkedToSource` event → project
- [x] EventSourcedStore: `update_actor_location()` → emit `ActorLocationIdentified` event → project
- [x] Refactor `actor_extractor.rs` to accept `&dyn SignalStore` instead of `&GraphStore`
- [x] Verify: 301 scout tests pass, actor lifecycle test still passes

**Files:**
- `modules/rootsignal-scout/src/pipeline/event_sourced_store.rs` — convert pass-throughs
- `modules/rootsignal-scout/src/enrichment/actor_extractor.rs` — accept trait instead of concrete type
- `modules/rootsignal-scout/src/pipeline/traits.rs` — may need actor methods on SignalStore trait

**Risk:** `actor_extractor.rs` calls `writer.find_actor_by_name()` (read) and `writer.upsert_actor()` (write). The read doesn't need events, but the write does. These are already in the SignalStore trait, so the refactor is straightforward.

---

## Phase 3: Source and Reap Events through EventSourcedStore

**Goal:** Source lifecycle and signal cleanup emit events. All event variants and reducer handlers already exist.

**Scope:**
- `upsert_source()` → `SourceRegistered`
- `reap_expired()` → `EntityExpired` + `EntityPurged`
- `batch_tag_signals()` → `TagsAggregated`
- `delete_pins()` → `PinsRemoved`

**Tasks:**
- [x] EventSourcedStore: `upsert_source()` → emit `SourceRegistered` event → project
- [x] Convert `scrape_pipeline.reap_expired_signals()` to emit `EntityExpired`/`EntityPurged` events
  - Needs to query which signals are expired first, then emit events
  - Currently `writer.reap_expired()` does query+delete in one Cypher
  - Refactor: split into read (find expired IDs) + emit events (one per type)
- [x] EventSourcedStore: `batch_tag_signals()` → emit `TagsAggregated` event → project (reducer updated to match any node type, not just Story)
- [x] Convert pin lifecycle: `delete_pins()` → emit `PinsRemoved` event → project
- [x] Convert `source_finder.rs`, `bootstrap.rs`, `expansion.rs` `upsert_source()` calls to go through EventSourcedStore or emit events directly
- [x] Convert `metrics.rs` `record_source_scrape()` call → emit `SourceScrapeRecorded` event
- [x] Verify: all tests pass (301 scout, 82 graph, 65 common)

**Files:**
- `modules/rootsignal-scout/src/pipeline/event_sourced_store.rs`
- `modules/rootsignal-scout/src/pipeline/scrape_pipeline.rs` — reap refactor
- `modules/rootsignal-scout/src/discovery/source_finder.rs`
- `modules/rootsignal-scout/src/discovery/bootstrap.rs`
- `modules/rootsignal-scout/src/pipeline/expansion.rs`
- `modules/rootsignal-scout/src/scheduling/metrics.rs`

**Complication:** Many of these callers hold a `&GraphStore` reference, not `&dyn SignalStore`. Some source management methods aren't on the `SignalStore` trait yet. Two options:
1. Add source methods to `SignalStore` trait (makes trait larger but keeps one interface)
2. Create a separate `SourceStore` trait (cleaner separation but more wiring)

Recommend option 1 for now — the trait already has source methods (`get_active_sources`, `upsert_source`).

---

## Phase 4: Discovery Workflows through EventSourcedStore

**Goal:** `gathering_finder.rs` and `response_finder.rs` create signals through events instead of direct `writer.create_node()`.

**Scope:** These are the last remaining signal creation paths that bypass events. They need access to `EventSourcedStore` (or at least `&dyn SignalStore`).

**Tasks:**
- [x] Refactor `GatheringFinder` to accept `&dyn SignalStore` alongside `&GraphStore`
  - `writer.create_node()` → `store.create_node()` (event-sourced)
  - `writer.upsert_source()` → `store.upsert_source()` (event-sourced)
  - `writer.find_duplicate()` → `store.find_duplicate()` (pass-through read)
  - Keep `writer` for read-only methods (`find_gathering_finder_targets()`, etc.) and non-trait writes (`create_drawn_to_edge`, `touch_signal_timestamp`, etc.)
- [x] Refactor `ResponseFinder` to accept `&dyn SignalStore` alongside `&GraphStore`
  - `writer.create_node()` → `store.create_node()` (event-sourced)
  - `writer.upsert_source()` → `store.upsert_source()` (event-sourced)
  - `writer.find_duplicate()` → `store.find_duplicate()` (pass-through read)
  - `writer.find_or_create_resource()` → `store.find_or_create_resource()` (pass-through)
  - `writer.create_requires/prefers/offers_edge()` → `store.*` (pass-through)
  - `writer.mark_response_found()` stays on writer (marking flags, not domain facts)
  - `writer.create_response_edge()` moved to store in Phase 5
- [x] Pass `&writer as &dyn SignalStore` in `synthesis.rs` to both finder constructors
- [x] Verify: `cargo build --workspace`, 301 scout tests + 82 graph tests pass

**Approach taken:** Option 1 — pass both `store` + `writer`. Store handles all `SignalStore` trait methods (event-sourced writes + pass-through reads). Writer handles non-trait reads and writes that don't have event variants yet.

**Files changed:**
- `modules/rootsignal-scout/src/discovery/gathering_finder.rs`
- `modules/rootsignal-scout/src/discovery/response_finder.rs`
- `modules/rootsignal-scout/src/workflows/synthesis.rs`

---

## Phase 5: Edge Events ✅

**Goal:** Resource edge and relationship edge operations emit events.

**Design decision:** `find_or_create_resource` and `find_or_create_place` stay as pass-throughs — they're reference data management (lookup tables), not domain events. The meaningful domain facts are the edges.

**Scope:**
- `create_requires/prefers/offers_edge()` → `ResourceEdgeCreated` event (role field distinguishes)
- `create_response_edge()` → `ResponseLinked` event
- `create_drawn_to_edge()` → `GravityLinked` event
- Route finder `writer` calls through `store` for edge creation

**Tasks:**
- [x] Add `ResourceEdgeCreated { signal_id, resource_id, role, confidence, quantity, notes, capacity }` to Event enum
- [x] Add `ResponseLinked { signal_id, tension_id, strength, explanation }` to Event enum
- [x] Add `GravityLinked { signal_id, tension_id, strength, explanation, gathering_type }` to Event enum
- [x] Add reducer handlers for each new variant
- [x] Add `create_response_edge` and `create_drawn_to_edge` to SignalStore trait + GraphStore impl
- [x] Convert EventSourcedStore resource edge methods to emit events
- [x] Implement EventSourcedStore `create_response_edge` and `create_drawn_to_edge`
- [x] Route GatheringFinder `writer.create_drawn_to_edge()` through `store`
- [x] Route ResponseFinder `writer.create_response_edge()` through `store`
- [x] Add mock implementations for new trait methods
- [x] Verify: all tests pass (301 scout, 82 graph, 65 common)

**Explicitly pass-through (not domain events):**
- `find_or_create_resource` — query+command hybrid returning UUID
- `find_or_create_place` — query+command hybrid returning UUID
- ~~`batch_tag_signals`~~ — now event-sourced via `TagsAggregated`, removed from GraphStore

**Files:**
- `modules/rootsignal-common/src/events.rs` — 3 new variants
- `modules/rootsignal-graph/src/reducer.rs` — 3 new handlers
- `modules/rootsignal-scout/src/pipeline/traits.rs` — 2 new trait methods + GraphStore impls
- `modules/rootsignal-scout/src/pipeline/event_sourced_store.rs` — 3 converted + 2 new methods
- `modules/rootsignal-scout/src/discovery/gathering_finder.rs` — routed 2 writer calls through store
- `modules/rootsignal-scout/src/discovery/response_finder.rs` — routed 2 writer calls through store
- `modules/rootsignal-scout/src/testing.rs` — mock impls

---

## Phase 6: Clean Up GraphStore

**Goal:** Once all writes go through events, GraphStore becomes read-only. Remove write methods, rename to clarify its role.

**Tasks:**
- [x] Remove signal write methods from GraphStore (`create_node`, `upsert_node`, `corroborate`, `create_evidence`, `refresh_signal`, `refresh_url_signals`)
- [x] Remove source write methods (`upsert_source`); `record_source_scrape` kept (still called directly from metrics)
- [x] Remove actor write methods (`upsert_actor`, `link_actor_to_signal`, `link_actor_to_source`, `link_signal_to_source`, `update_actor_location`)
- [x] Remove private helpers (`create_gathering`, `create_aid`, `create_need`, `create_notice`, `create_tension`, `add_location_params`)
- [x] Remove unused free functions (`urgency_str`, `severity_str`, `sensitivity_str`)
- [x] Migrate discovery modules: `investigator` and `tension_linker` now take `&dyn SignalStore` for writes
- [x] Remove enrichment methods that are now in `enrich.rs` (already moved; no duplicates in writer.rs)
- [x] Remove inline diversity computation from corroborate (removed with corroborate method)
- [x] Remove `impl SignalStore for GraphStore` (non-event bypass path)
- [x] Route API mutations (`add_source`, `tag_story`) through `Arc<dyn SignalStore>`
- [x] Add `build_signal_store` factory + `ScoutDeps::build_store` convenience
- [x] Audit: no direct writes bypass the event path
- [x] Add `SignalStoreFactory` — per-mutation run_ids for proper event correlation
- [x] Remove startup panic — graceful error when Postgres is unavailable
- [x] ~~Make GraphStore write methods `pub(crate)`~~ — resolved by design: GraphStore is called cross-crate from EventSourcedStore, so `pub(crate)` isn't feasible. Protection is architectural: the `SignalStore` trait is the public interface, all domain writes go through `EventSourcedStore`.
- [x] ~~Rename `GraphStore`~~ — deferred: name is slightly misleading (mostly reads + infrastructure now) but not worth the churn across the codebase.

**Not removed** (kept on GraphStore or moved to GraphAdmin):
- Task management methods (operational, not domain events)
- `set_query_embedding()` (infrastructure)
- Discovery marking methods (`mark_response_found`, etc.) — operational flags
- Story/situation management (separate domain, future event-sourcing scope)

---

## Out of Scope

- **Story/situation event-sourcing** — SituationWeaver is its own domain; events exist but the weaver writes directly. Future work.
- **Multi-server consensus** — deferred until horizontal scaling is needed.
- **Real-time NOTIFY subscription** — optimization for reactive consumers.
- **Task lifecycle events** — operational coordination, not domain facts.

## Risks

- **Performance**: append+project is two DB round-trips (Postgres + Neo4j) per write. Acceptable — scout is I/O bound on scraping, not on graph writes. Monitor if batch operations (reap, tag) become slow.
- **Discovery refactor scope**: `gathering_finder` and `response_finder` take `&GraphStore` and use both read and write methods. Refactoring to `Arc<dyn SignalStore>` + `&GraphStore` (reads) is the pragmatic path.
- **Trait growth**: `SignalStore` trait already has ~30 methods. Adding source/actor/resource methods makes it larger. Consider splitting into focused traits if it gets unwieldy.

## References

- Enrichment pipeline plan: `docs/plans/2026-02-25-refactor-enrichment-pipeline-plan.md`
- Event data model: `docs/plans/2026-02-25-refactor-event-data-model-plan.md`
- Event sourcing brainstorm: `docs/brainstorms/2026-02-25-event-sourcing-brainstorm.md`
- Enrichment brainstorm: `docs/brainstorms/2026-02-25-enrichment-pipeline-design-brainstorm.md`
- GraphStore: `modules/rootsignal-graph/src/writer.rs` (~5,126 lines after final cleanup, down from ~5,990)
- EventSourcedStore: `modules/rootsignal-scout/src/pipeline/event_sourced_store.rs`
- Reducer: `modules/rootsignal-graph/src/reducer.rs` (all 71 event handlers)
