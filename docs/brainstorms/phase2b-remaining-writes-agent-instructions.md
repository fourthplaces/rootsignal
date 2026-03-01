# Agent Instructions: Convert Remaining Activity Graph Writes to Events

## Context

We are completing Phase 2 of type-level graph write prevention. We split `GraphStore` into `GraphReader` (read-only) + `GraphStore` (read+write via `Deref`). All handler/activity code now receives `&GraphReader`, but 8 write-method calls remain that won't compile. Each must be converted to event emission — the GraphProjector in `reducer.rs` will handle the actual writes.

The code does NOT compile right now. Your job is to make it compile by converting these 8 remaining direct graph writes into events.

## Architecture Rules

- Activities NEVER write to the graph directly. They emit events via `seesaw_core::Events`.
- The `GraphProjector` in `modules/rootsignal-graph/src/reducer.rs` is the ONLY place that writes to Neo4j (besides `cause_heat.rs` and `beacon.rs` infra modules).
- Events are defined in `modules/rootsignal-common/src/system_events.rs` as `SystemEvent` variants.
- Each new `SystemEvent` variant needs: (1) the variant definition, (2) an `event_type()` arm in the same file, (3) a projector case in `reducer.rs`.
- Read methods live on `GraphReader` in `modules/rootsignal-graph/src/writer.rs`. Write methods live on `GraphStore`. After this work, the write methods being replaced should be DELETED from `GraphStore` (they're dead code once activities emit events instead).

## Files You'll Modify

| File | Purpose |
|------|---------|
| `modules/rootsignal-common/src/system_events.rs` | Add new SystemEvent variants |
| `modules/rootsignal-graph/src/reducer.rs` | Add projector cases for new events |
| `modules/rootsignal-graph/src/writer.rs` | Split mixed read+write methods; move read portions to GraphReader; delete replaced write methods |
| `modules/rootsignal-graph/src/lib.rs` | Export any new types |
| `modules/rootsignal-scout/src/domains/expansion/activities/expansion.rs` | Fix: change `GraphStore` → `GraphReader`, emit event for clearing implied_queries |
| `modules/rootsignal-scout/src/domains/synthesis/activities/investigator.rs` | Emit event instead of `mark_investigated` |
| `modules/rootsignal-scout/src/domains/synthesis/activities/tension_linker.rs` | Emit events instead of `find_tension_linker_targets` pre-pass and `mark_tension_linker_investigated` |
| `modules/rootsignal-scout/src/domains/synthesis/activities/gathering_finder.rs` | Emit events instead of `mark_gathering_found`, `find_or_create_place`, `create_gathers_at_edge` |
| `modules/rootsignal-scout/src/domains/supervisor/activities.rs` | Emit event instead of `merge_duplicate_tensions` |

---

## Conversion 1: `get_recently_linked_signals_with_queries` (expansion.rs)

**Current location:** `writer.rs:3058` on `GraphStore`
**Called from:** `expansion/activities/expansion.rs:81`

This method does TWO things:
1. **READ**: Finds Aid/Gathering signals with `implied_queries` linked to heated tensions, returns the queries
2. **WRITE**: Clears `implied_queries = null` on those signals to prevent replay

**Fix:**
1. Split into a pure read method on `GraphReader` called `get_recently_linked_signals_with_queries` that returns `Result<Vec<(String, Vec<String>)>, neo4rs::Error>` — returning `(signal_id, queries)` pairs so the caller knows which signals to clear
2. Add a new `SystemEvent::ImpliedQueriesConsumed { signal_ids: Vec<String> }` variant
3. Add projector case: for each signal_id, `MATCH (s {id: $id}) WHERE s:Aid OR s:Gathering SET s.implied_queries = null`
4. In `expansion.rs:81`: call the read method, collect queries, emit `ImpliedQueriesConsumed` with the signal IDs
5. Change `expansion.rs` import from `GraphStore` to `GraphReader`
6. Delete the old mixed read+write method from `GraphStore`

---

## Conversion 2: `mark_investigated` (investigator.rs)

**Current location:** `writer.rs:3235` on `GraphStore`
**Called from:** `investigator.rs:206`

Sets `n.investigated_at = datetime($now)` on a signal node by label.

**Fix:**
1. Add `SystemEvent::SignalInvestigated { signal_id: Uuid, node_type: NodeType, investigated_at: DateTime<Utc> }`
2. Add projector case: dynamic label dispatch like the write method does, `MATCH (n:{Label} {id: $id}) SET n.investigated_at = datetime($ts)`. For `Citation` node_type, return `NoOp`.
3. In `investigator.rs:206`: replace `self.graph.mark_investigated(...)` with `events.push(SystemEvent::SignalInvestigated { ... })`. The `events: &mut seesaw_core::Events` is already available — it's passed into the `run` method. You need to thread it through to the loop body that calls `mark_investigated`.
4. Delete `mark_investigated` from `GraphStore`

---

## Conversion 3: `find_tension_linker_targets` pre-pass (tension_linker.rs)

**Current location:** `writer.rs:3269` on `GraphStore`
**Called from:** `tension_linker.rs:235`

This method does TWO things:
1. **WRITE** (pre-pass): Promotes signals with `curiosity_investigated = 'failed'` and `retry_count >= 3` to `'abandoned'`
2. **READ**: Finds unlinked signals for investigation

**Fix:**
1. Split: move the read portion to `GraphReader` as `find_tension_linker_targets` (same name, but WITHOUT the pre-pass write)
2. Add `SystemEvent::ExhaustedRetriesPromoted { promoted_at: DateTime<Utc> }` — a batch event, the projector runs the same promote query
3. Add projector case: `MATCH (n) WHERE (n:Aid OR n:Gathering OR n:Need OR n:Notice) AND n.curiosity_investigated = 'failed' AND n.curiosity_retry_count >= 3 SET n.curiosity_investigated = 'abandoned'`
4. In `tension_linker.rs`: emit `ExhaustedRetriesPromoted` before calling `find_tension_linker_targets`. The `events: &mut seesaw_core::Events` is passed into `run()`.
5. Delete the old mixed method from `GraphStore`; the new read-only version goes on `GraphReader`

---

## Conversion 4: `mark_tension_linker_investigated` (tension_linker.rs)

**Current location:** `writer.rs:3335` on `GraphStore`
**Called from:** `tension_linker.rs:356`

Sets `curiosity_investigated = $outcome` (and increments `curiosity_retry_count` if outcome is `Failed`) on a signal node.

**Fix:**
1. Add `SystemEvent::TensionLinkerOutcomeRecorded { signal_id: Uuid, label: String, outcome: String, increment_retry: bool }`
2. Add projector case: dynamic label, two branches (with/without retry increment), matching the existing write method logic
3. In `tension_linker.rs:354`: replace `self.graph.mark_tension_linker_investigated(...)` with `events.push(SystemEvent::TensionLinkerOutcomeRecorded { signal_id: target.signal_id, label: target.label.clone(), outcome: outcome.as_str().to_string(), increment_retry: outcome == TensionLinkerOutcome::Failed })`
4. Delete `mark_tension_linker_investigated` from `GraphStore`

---

## Conversion 5: `mark_gathering_found` (gathering_finder.rs)

**Current location:** `writer.rs:3553` on `GraphStore`
**Called from:** `gathering_finder.rs:386`

Sets `gravity_scouted_at` and `gravity_scout_miss_count` on a Tension node.

**Fix:**
1. Add `SystemEvent::GatheringScouted { tension_id: Uuid, found_gatherings: bool, scouted_at: DateTime<Utc> }`
2. Add projector case: `MATCH (t:Tension {id: $id}) SET t.gravity_scouted_at = datetime($ts), t.gravity_scout_miss_count = CASE WHEN $found THEN 0 ELSE coalesce(t.gravity_scout_miss_count, 0) + 1 END`
3. In `gathering_finder.rs:384`: replace `deps.graph.mark_gathering_found(...)` with `events.push(SystemEvent::GatheringScouted { ... })`. The `events` param is available in `investigate_tension` which is called from `run_gathering_finder` — thread it through if needed.
4. Delete `mark_gathering_found` from `GraphStore`

**IMPORTANT**: Check how `events` flows in gathering_finder.rs. The `run_gathering_finder` function has `events: &mut seesaw_core::Events` available. The `mark_gathering_found` call is in `run_gathering_finder` itself (line 384), so `events` should be directly in scope.

---

## Conversion 6: `find_or_create_place` (gathering_finder.rs)

**Current location:** `writer.rs:3609` on `GraphStore`
**Called from:** `gathering_finder.rs:602`

Creates a Place node via MERGE (dedup on slug). Returns the Place UUID.

**Fix:**
1. Add `SystemEvent::PlaceDiscovered { place_id: Uuid, name: String, slug: String, lat: f64, lng: f64, discovered_at: DateTime<Utc> }`
2. Add projector case: `MERGE (p:Place {slug: $slug}) ON CREATE SET p.id = $id, p.name = $name, p.lat = $lat, p.lng = $lng, p.geocoded = false, p.created_at = datetime($ts)`
3. In `gathering_finder.rs:600-617`: generate a `Uuid::new_v4()` for `place_id`, emit `PlaceDiscovered`, then emit `GathersAtPlaceLinked` (see below) using that `place_id`. Remove both `find_or_create_place` and `create_gathers_at_edge` calls.
4. Delete `find_or_create_place` from `GraphStore`

**NOTE**: The old method used MERGE + RETURN to get either an existing or new ID. With events, we can't read-then-write atomically. Instead, always generate a new UUID. The projector's `MERGE` on slug ensures idempotency — if the Place already exists, the `ON CREATE` block doesn't fire, and the place_id won't matter because the edge (below) matches on slug too.

---

## Conversion 7: `create_gathers_at_edge` (gathering_finder.rs)

**Current location:** `writer.rs:3649` on `GraphStore`
**Called from:** `gathering_finder.rs:608`

Creates a GATHERS_AT edge from a signal to a Place.

**Fix:**
1. Add `SystemEvent::GathersAtPlaceLinked { signal_id: Uuid, place_slug: String }`
2. Add projector case: `MATCH (s) WHERE s.id = $sid AND (s:Aid OR s:Gathering OR s:Need) MATCH (p:Place {slug: $slug}) MERGE (s)-[:GATHERS_AT]->(p)`
3. In `gathering_finder.rs`: emit `GathersAtPlaceLinked` after `PlaceDiscovered`. Use the slug (computed from venue name via `rootsignal_common::slugify`) instead of place_id to match reliably.
4. Delete `create_gathers_at_edge` from `GraphStore`

---

## Conversion 8: `merge_duplicate_tensions` (supervisor/activities.rs)

**Current location:** `writer.rs:3375` on `GraphStore`
**Called from:** `supervisor/activities.rs:43`

This is the most complex one. It:
1. Reads all tensions with embeddings in a bbox
2. Finds cosine-similar pairs above threshold
3. For each pair: re-points RESPONDS_TO, DRAWN_TO, PART_OF edges from duplicate to survivor, bumps corroboration_count, then DETACH DELETEs the duplicate

**Fix:**
This method has significant read+compute logic that must stay in activity code, and a write portion that should become events.

1. Move the READ + COMPUTE portion to `GraphReader` as a new method `find_duplicate_tension_pairs(threshold, min_lat, max_lat, min_lng, max_lng) -> Result<Vec<(String, String)>>` that returns `(survivor_id, duplicate_id)` pairs
2. Add `SystemEvent::DuplicateTensionMerged { survivor_id: Uuid, duplicate_id: Uuid }` — one event per merge pair
3. Add projector case that does all 5 steps: re-point RESPONDS_TO, re-point DRAWN_TO, re-point PART_OF, bump corroboration_count, DETACH DELETE duplicate
4. In `supervisor/activities.rs`: call `find_duplicate_tension_pairs`, emit one `DuplicateTensionMerged` per pair, return events. Change the function to return `seesaw_core::Events`.
5. Delete `merge_duplicate_tensions` from `GraphStore`

**IMPORTANT for the supervisor:** The `supervise` function currently doesn't return events. You'll need to change it to return `seesaw_core::Events` and have the caller collect them. Check `supervisor/mod.rs` to see how the handler calls `supervise()` and ensure events flow back.

---

## Event Naming Convention

Follow the existing pattern in `system_events.rs`:
- Past tense verb phrases: `ResponseScouted`, `QueryEmbeddingStored`, `CuriosityTriggered`
- `event_type()` returns a snake_case string: `"response_scouted"`, `"query_embedding_stored"`, etc.

## New Events Summary

| Event | Fields |
|-------|--------|
| `ImpliedQueriesConsumed` | `signal_ids: Vec<String>` |
| `SignalInvestigated` | `signal_id: Uuid, node_type: NodeType, investigated_at: DateTime<Utc>` |
| `ExhaustedRetriesPromoted` | `promoted_at: DateTime<Utc>` |
| `TensionLinkerOutcomeRecorded` | `signal_id: Uuid, label: String, outcome: String, increment_retry: bool` |
| `GatheringScouted` | `tension_id: Uuid, found_gatherings: bool, scouted_at: DateTime<Utc>` |
| `PlaceDiscovered` | `place_id: Uuid, name: String, slug: String, lat: f64, lng: f64, discovered_at: DateTime<Utc>` |
| `GathersAtPlaceLinked` | `signal_id: Uuid, place_slug: String` |
| `DuplicateTensionMerged` | `survivor_id: Uuid, duplicate_id: Uuid` |

## Verification

After all changes:
1. `cargo check -p rootsignal-common` — new events compile
2. `cargo check -p rootsignal-graph` — projector cases compile, split methods work
3. `cargo check -p rootsignal-scout` — all activity code compiles with `&GraphReader`
4. Grep for any remaining `GraphStore` imports in `modules/rootsignal-scout/src/domains/` — there should be NONE (only `GraphReader`)
5. Grep for any remaining `.run(` calls in activity files — there should be NONE (only `.execute(` for reads)

## Critical Notes

- Do NOT revert any activity file back to `GraphStore`. The whole point is that activities only get `&GraphReader`.
- Thread `events: &mut seesaw_core::Events` through function signatures where it doesn't already exist.
- When a function currently returns just stats, add events to the return type (e.g., return a tuple or a struct with an `events` field).
- Use `Utc::now()` in activity code for timestamps — the projector receives the timestamp from the event payload, it never generates its own.
- The `rootsignal_common::slugify` function is already available for Place slug computation.
- For `NodeType` in `SignalInvestigated`: it's already imported/available in the files that need it.
- `TensionLinkerOutcome` is defined in `rootsignal-graph` — its `as_str()` method gives the string value.
