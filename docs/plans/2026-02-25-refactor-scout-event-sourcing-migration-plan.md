---
title: "refactor: Migrate scout from direct GraphWriter writes to event-sourcing"
type: refactor
date: 2026-02-25
depends_on: docs/plans/2026-02-25-refactor-enrichment-pipeline-plan.md
---

# Migrate Scout from GraphWriter to Event-Sourcing

## Overview

The scout pipeline currently writes directly to Neo4j via `GraphWriter` — 155 Cypher call sites, ~100 public methods. The event-sourcing infrastructure (EventStore, GraphReducer, Pipeline, Enrichment) is built and tested but nothing calls it. This plan migrates the scout to emit events instead of direct writes, with reduce→enrich running as a separate step after each scout run.

**Current architecture:**
```
Scout → GraphWriter.create_node() → Neo4j (immediate Cypher)
Scout → GraphWriter.corroborate() → Neo4j (immediate Cypher + inline diversity computation)
```

**Target architecture:**
```
Scout → EventStore.append() → Postgres (facts only)
                                    ↓
                              Pipeline.process() → Reducer → Neo4j (factual)
                                                 → Enrich → Neo4j (derived)
```

## Problem Statement

1. **No audit trail**: When the scout writes directly, we have no record of what happened or why. Run logs capture some metadata but not the actual data mutations.
2. **Duplicate enrichment logic**: `GraphWriter.corroborate()` computes diversity inline. `enrich.rs` does the same. Two places, two chances to diverge.
3. **Non-idempotent writes**: `GraphWriter.upsert_actor()` increments `signal_count` on MATCH — replaying the same scrape produces different counts.
4. **No replay guarantee**: If the graph corrupts or we change the schema, there's no way to rebuild from source data.

## Scope

### In scope
- Scout emits events to EventStore instead of calling GraphWriter for **signal lifecycle** operations (create, corroborate, evidence, expire)
- Pipeline.process() runs after each scout phase or at end of run
- Remove enrichment logic from GraphWriter (diversity, cause_heat)

### Out of scope (future work)
- Source management events (SourceRegistered, SourceScrapeRecorded) — these are infrastructure, not signal facts
- ScoutTask lifecycle — operational coordination, not domain events
- Story/ClusterSnapshot — deprecated layer, being replaced by SituationWeaver
- Real-time NOTIFY subscription — future optimization
- Multi-server consensus — deferred until we actually need horizontal scaling

## Surface Area Analysis

### GraphWriter methods by category

**Already have Event variants (~30 methods):**
- Signal creation: `create_node` → `{Type}Discovered`
- Evidence: `create_evidence` → `CitationRecorded`
- Corroboration: `corroborate` → `ObservationCorroborated`
- Freshness: `refresh_signal` → `FreshnessConfirmed`
- Confidence: `update_signal_confidence` → `ConfidenceScored`
- Expiry: `reap_expired` → `EntityExpired` + `EntityPurged`
- Sources: `upsert_source` → `SourceRegistered`, `record_source_scrape` → `SourceScrapeRecorded`
- Actors: `upsert_actor` → `ActorIdentified`, `link_actor_to_signal` → `ActorLinkedToEntity`
- Situations: `create_situation` → `SituationIdentified`, `create_dispatch` → `DispatchCreated`
- Tags: `batch_tag_signals` → `TagsAggregated`

**Need new Event variants (~20 methods):**
- `create_response_edge` → needs `ResponseLinked`
- `create_drawn_to_edge` → needs `GravityLinked`
- `find_or_create_place` → needs `PlaceDiscovered`
- `find_or_create_resource` → needs `ResourceDiscovered`
- `create_requires/prefers/offers_edge` → needs `ResourceEdgeCreated`
- `link_signal_to_source` → needs `SignalLinkedToSource`
- `merge_duplicate_tensions` → needs `DuplicateTensionsMerged`
- `consolidate_resources` → needs `ResourcesConsolidated`
- `mark_response_found/mark_gathering_found/mark_investigated` → needs scout lifecycle events

**Pure reads (~25 methods) — no migration needed:**
- `find_duplicate`, `find_by_titles_and_types`, `existing_titles_for_url`, `content_already_processed`
- `get_active_tensions`, `get_tension_landscape`, `find_tension_hubs`
- All `find_*_targets` methods
- All `get_*` accessor methods

**Enrichment logic to remove from GraphWriter (~8 methods):**
- `compute_source_diversity` — move to `enrich.rs` (already done)
- `compute_channel_diversity` — move to `enrich.rs` (already done)
- Inline diversity in `corroborate` — remove, let enrichment handle it
- `merge_duplicate_tensions` cosine similarity — enrichment pass
- `consolidate_resources` cosine clustering — enrichment pass
- `boost_sources_for_situation_headline` — enrichment pass
- Story metrics (cause_heat, gap_score, velocity, energy) — enrichment pass

## Implementation Phases

### Phase 1: Scout emits events for signal lifecycle

The smallest valuable increment. Scout appends events to EventStore for the core signal lifecycle, then Pipeline.process() runs those events through reduce→enrich.

**Key change**: `ScrapePhase` calls `EventStore.append()` instead of `GraphWriter.create_node()`.

**Files:**
- `modules/rootsignal-scout/src/pipeline/scrape_phase.rs` — MODIFY: emit events instead of direct writes
- `modules/rootsignal-scout/src/pipeline/scrape_pipeline.rs` — MODIFY: call Pipeline.process() at end of phase
- `modules/rootsignal-scout/src/pipeline/traits.rs` — MODIFY: SignalStore trait to emit events
- `modules/rootsignal-scout/Cargo.toml` — ADD: rootsignal-events dependency

**Events covered:**
- `{Type}Discovered` (5 variants)
- `CitationRecorded`
- `ObservationCorroborated`
- `FreshnessConfirmed`

**Tasks:**
- [ ] Add rootsignal-events dependency to scout
- [ ] Modify SignalStore trait: `create_node` → `append(TypeDiscovered)` + return ID
- [ ] Modify SignalStore trait: `create_evidence` → `append(CitationRecorded)`
- [ ] Modify SignalStore trait: `corroborate` → `append(ObservationCorroborated)` (remove inline diversity)
- [ ] Modify SignalStore trait: `refresh_signal` → `append(FreshnessConfirmed)`
- [ ] Add Pipeline.process() call at end of each scrape phase
- [ ] Remove `compute_source_diversity` and `compute_channel_diversity` from GraphWriter
- [ ] Verify: same signals created, same diversity values, same corroboration counts
- [ ] Write integration test: scrape phase emits events → pipeline produces correct graph

### Phase 2: Actor and source events

Extend event coverage to actor management and source lifecycle.

**Events covered:**
- `ActorIdentified`, `ActorLinkedToEntity`, `ActorLinkedToSource`, `ActorLocationIdentified`
- `SourceRegistered`, `SourceScrapeRecorded`, `SourceDeactivated`
- `ConfidenceScored`, `EntityExpired`, `EntityPurged`

**Tasks:**
- [ ] Modify actor_extractor.rs: emit actor events instead of GraphWriter calls
- [ ] Modify source management: emit source events
- [ ] Modify reap_expired: emit expiry events
- [ ] Add reducer handlers for any missing event types
- [ ] Integration test: actors created correctly via events

### Phase 3: Edge and relationship events

Add events for the graph edges that currently have no Event variant.

**New Event variants needed:**
- [ ] `ResponseLinked { signal_id, tension_id, strength, explanation }`
- [ ] `GravityLinked { signal_id, tension_id, gathering_type }`
- [ ] `PlaceDiscovered { place_id, name, slug }`
- [ ] `ResourceDiscovered { resource_id, name, slug, description }`
- [ ] `ResourceEdgeCreated { signal_id, resource_id, edge_type }` (requires/prefers/offers)
- [ ] `SignalLinkedToSource { signal_id, source_id }`

**Tasks:**
- [ ] Add Event variants to events.rs
- [ ] Add reducer handlers for each new variant
- [ ] Modify scout callers to emit events instead of direct GraphWriter calls
- [ ] Contract test: verify new variants are classified correctly

### Phase 4: Remove GraphWriter signal write methods

Once all signal lifecycle operations go through events, remove the direct-write methods from GraphWriter. Keep read methods (they become part of a `GraphReader` interface).

**Tasks:**
- [ ] Remove `create_node`, `corroborate`, `create_evidence`, `refresh_signal` from GraphWriter
- [ ] Remove enrichment methods from GraphWriter
- [ ] Rename remaining GraphWriter to something clearer (it's now mostly reads)
- [ ] Verify no direct writes bypass the event path

## Risks

- **Performance**: EventStore.append() + Pipeline.process() is two DB round-trips instead of one direct Cypher. Mitigate: batch events per scrape phase, process once at end.
- **Read-after-write**: Scout reads graph state mid-run (e.g., `find_duplicate` during scraping). If events aren't reduced yet, reads see stale data. Mitigate: Phase 1 runs Pipeline.process() after each scrape phase, not just at end of run.
- **Migration complexity**: 155 Cypher call sites. Mitigate: phase the migration — start with signal lifecycle (Phase 1), which covers the most important audit trail.

## References

- Enrichment pipeline plan: `docs/plans/2026-02-25-refactor-enrichment-pipeline-plan.md`
- Event data model: `docs/plans/2026-02-25-refactor-event-data-model-plan.md`
- Current writer: `modules/rootsignal-graph/src/writer.rs` (5,917 lines)
- Current scrape phase: `modules/rootsignal-scout/src/pipeline/scrape_phase.rs`
- SignalStore trait: `modules/rootsignal-scout/src/pipeline/traits.rs`
