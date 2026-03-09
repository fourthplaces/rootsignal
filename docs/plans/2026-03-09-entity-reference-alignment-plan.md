# Entity, Schedule & Reference Alignment Plan

**Date:** 2026-03-09
**Status:** Pressure-tested, ready for implementation
**Safety net:** Neo4j is a derived projection — replay events to rebuild graph at any point.

---

## Design Decisions

These were settled during pressure testing against the vision docs and adversarial threat model:

1. **No Person entities.** `EntityType::Person` is removed. Extracting person names as graph nodes accumulates into de facto organizer profiles — exactly what the adversarial threat model prohibits. Public figures (elected officials) can be revisited later with explicit classification, but for this work: total omission.

2. **Place entities merge into Location.** No separate `Entity(Place)` nodes. When the LLM identifies a place ("Powderhorn Park"), it enriches the existing `Location` infrastructure — name, coordinates, role. One representation, not two.

3. **No string normalization for entity MERGE.** Lowercase/trim heuristics are fragile and give false confidence. If the LLM says two strings are the same entity, they get the same name. Otherwise, duplicates exist until a dedicated entity resolution pass (future `EntityMerged` system event). The MERGE key is `{name, entity_type}` with no cleaning.

4. **Real entity types, not defaults.** Per-type field synthesis (`organizer`, `source_authority`, `observed_by`) does NOT hardcode `Organization`. The extraction prompt provides typed entities; synthesis from per-type fields is a fallback that preserves whatever type information exists. If the type is genuinely unknown, it's `Organization` as a last resort — but the architecture flows real types through.

5. **Schedule creation moves into the projector.** Currently `ScheduleNode` bypasses the event store and writes directly to Neo4j via the writer. This violates "Neo4j is a derived projection." The world `Schedule` struct gets enriched to carry full recurrence data, and the projector creates `:Schedule` nodes from events. Occurrences are computed at query time in Rust (no node explosion).

---

## Phase 1: World Crate Schema Updates

### 1a. Add `GovernmentBody` to `EntityType`

**File:** `modules/rootsignal-world/src/types.rs`

Add `GovernmentBody` variant to `EntityType` enum. Serde rename: `government_body`.

### 1b. Remove `Person` and `Place` from `EntityType`

**File:** `modules/rootsignal-world/src/types.rs`

Remove `Person` variant entirely. Remove `Place` variant — place information flows through `Location`, not `Entity`.

Remaining variants: `Organization`, `Group`, `GovernmentBody`, `Thing`.

### 1c. Enrich `Schedule` struct

**File:** `modules/rootsignal-world/src/values.rs`

Current `Schedule` has: `starts_at`, `ends_at`, `all_day`, `rrule`, `timezone`.

Add fields to match what `ScheduleNode` carries:
```rust
/// Human-readable schedule as stated in the source.
/// Always captured when the source mentions a schedule, even if rrule is also provided.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub schedule_text: Option<String>,

/// Additional occurrence dates for irregular schedules (RFC 5545 RDATE).
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub rdates: Vec<DateTime<Utc>>,

/// Dates excluded from the recurrence pattern (RFC 5545 EXDATE).
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub exdates: Vec<DateTime<Utc>>,
```

All new fields use `serde(default)`, so existing serialized events deserialize without breakage.

---

## Phase 2: NodeMeta Entity Migration

### 2a. Replace `mentioned_actors` with `mentioned_entities`

**File:** `modules/rootsignal-common/src/types.rs`

```
- pub mentioned_actors: Vec<String>,
+ pub mentioned_entities: Vec<Entity>,
```

Import `Entity` from `rootsignal-world`. Add `serde(default)` so existing data deserializes to empty vec.

Add helper for callers that still need flat names:
```rust
impl NodeMeta {
    pub fn mentioned_actor_names(&self) -> Vec<&str> {
        self.mentioned_entities.iter().map(|e| e.name.as_str()).collect()
    }
}
```

### 2b. Fix all compilation errors from rename

Grep for `mentioned_actors` across codebase and update each struct literal site. This is a mechanical pass.

### 2c. Delete dead `author_actor_types` code

**File:** `modules/rootsignal-scout/src/core/aggregate.rs`

Remove `author_actor_types: HashMap<Uuid, ActorType>` field and any code that references it. Never populated, never read.

---

## Phase 3: Extraction Pipeline

### 3a. Update `ExtractedSignal`

**File:** `modules/rootsignal-scout/src/core/extractor.rs`

```
- pub mentioned_actors: Option<Vec<String>>,
+ pub mentioned_entities: Option<Vec<Entity>>,
```

Update the LLM prompt to ask for typed entities with the reduced type set:
```json
"mentioned_entities": [
  {"name": "Minneapolis City Council", "entity_type": "government_body", "role": "decision_maker"},
  {"name": "Loaves & Fishes", "entity_type": "organization", "role": "organizer"}
]
```

Entity types allowed: `organization`, `group`, `government_body`, `thing`.
No `person` or `place` — the prompt must instruct the LLM to skip individual people and to express places through the location fields instead.

Keep `author_actor` as-is — it flows through system events, not world events.

### 3b. Set location roles by signal type

**File:** `modules/rootsignal-scout/src/core/extractor.rs`

When building `Location` from extracted data, set role based on signal type:
- Gathering → `role: Some("venue")`
- Condition → `role: Some("affected_area")`
- Others → `role: None` (default)

Place mentions from extraction that include coordinates enrich the Location vec rather than creating Entity nodes.

### 3c. Update node builders

**Files:** `modules/rootsignal-scout/src/domains/enrichment/` (materializer, etc.)

Where `NodeMeta` is constructed from `ExtractedSignal`, map `mentioned_entities` through. Per-type fields (`organizer`, `source_authority`, `observed_by`) stay on their respective node structs unchanged.

---

## Phase 4: Event Store → World Event Conversion

### 4a. Enrich `meta_to_mentioned_entities`

**File:** `modules/rootsignal-scout/src/store/event_sourced.rs`

Replace the hardcoded `EntityType::Organization` mapping. Use the actual `Entity` objects from `NodeMeta.mentioned_entities` directly — they already carry the real type from extraction.

Additionally, synthesize entities from per-type node fields when they aren't already present in `mentioned_entities`:
- `GatheringNode.organizer` → Entity with `role: Some("organizer")`
- `AnnouncementNode.source_authority` → Entity with `role: Some("source_authority")`
- `ConditionNode.observed_by` → Entity with `role: Some("observer")`

Type for synthesized entities: use the best available information. An organizer named "Minneapolis Park Board" is likely `GovernmentBody`; "Loaves & Fishes" is likely `Organization`. When genuinely ambiguous, default to `Organization` — but prefer flowing the real type from extraction when it exists.

Deduplicate by name before emitting.

### 4b. Fix `schedule_from_gathering`

**File:** `modules/rootsignal-scout/src/store/event_sourced.rs`

Currently hardcodes `rrule: None, timezone: None`. Fix to pass through from the ScheduleNode associated with this signal. The world event `Schedule` now carries the full recurrence data (rrule, schedule_text, rdates, exdates, timezone).

### 4c. Wire schedule for all signal types

Currently only `schedule_from_gathering` exists. Resource signals with `is_ongoing` or recurrence data also need schedules on their world events. Build schedule for any signal type that has schedule data.

### 4d. Wire references (stub)

Currently `references: vec![]` everywhere. Leave as-is — the field exists on WorldEvent, ready for when we extract them.

---

## Phase 5: Neo4j Projection

### 5a. Create Entity nodes with MENTIONED_IN edges

**File:** `modules/rootsignal-graph/src/projector.rs`

For each signal event, after creating the signal node, iterate `mentioned_entities`:

```cypher
UNWIND $entities AS ent
MERGE (e:Entity {name: ent.name, entity_type: ent.entity_type})
MERGE (e)-[:MENTIONED_IN {role: ent.role}]->(s)
```

No Person or Place nodes will appear — those types are excluded at extraction.

### 5b. Create SAME_AS edges to Actors

When an Entity name matches an existing Actor's `canonical_key` or `name`:

```cypher
MATCH (a:Actor)
WHERE a.name = ent.name OR a.canonical_key = ent.name
MERGE (e)-[:SAME_AS]->(a)
```

Best-effort exact match. Full entity resolution is deferred. When it arrives, the right mechanism is an `EntityMerged` system event that fuses nodes — not upfront normalization.

### 5c. Derive role-based properties from entities

After creating MENTIONED_IN edges, set convenience properties on the signal node:

```cypher
WITH s
OPTIONAL MATCH (org:Entity)-[:MENTIONED_IN {role: 'organizer'}]->(s)
SET s.organizer = org.name
```

Same for `source_authority` and `observed_by`. Denormalized for query convenience — the graph edges are the source of truth.

### 5d. Project Schedule nodes from world events

**File:** `modules/rootsignal-graph/src/projector.rs`

When a world event carries a non-empty `schedule`, the projector creates a `:Schedule` node and `HAS_SCHEDULE` edge:

```cypher
CREATE (sched:Schedule {
  id: $schedule_id,
  rrule: $rrule,
  schedule_text: $schedule_text,
  dtstart: $starts_at,
  dtend: $ends_at,
  all_day: $all_day,
  timezone: $timezone,
  rdates: $rdates,
  exdates: $exdates
})
MERGE (s)-[:HAS_SCHEDULE]->(sched)
```

This replaces the current writer-based `create_schedule` + `link_schedule_to_signal` path. Schedule creation is now event-sourced and replayable.

The `occurrences(from, to)` GraphQL resolver continues to expand rrules in Rust at query time with a 1000-date hard cap. No change to query-time behavior.

### 5e. Read entities back in reader

**File:** `modules/rootsignal-graph/src/reader.rs`

When reconstructing `NodeMeta`, query MENTIONED_IN edges and populate `mentioned_entities`:

```cypher
OPTIONAL MATCH (e:Entity)-[r:MENTIONED_IN]->(s)
RETURN collect({name: e.name, entity_type: e.entity_type, role: r.role}) AS entities
```

Replace the current `mentioned_actors: Vec::new()` with the populated vec.

---

## Phase 6: References (Stub Infrastructure)

### 6a. Add REFERENCES edge type to projector

**File:** `modules/rootsignal-graph/src/projector.rs`

Infrastructure-only — `references` is always `vec![]` today. When non-empty, project as:

```cypher
UNWIND $references AS ref
MATCH (target:Signal {id: ref.target_id})
MERGE (s)-[:REFERENCES {relationship: ref.relationship}]->(target)
```

Matching by `target_id` only (not title). Title-based matching is unreliable.

### 6b. Distinguish from RESPONDS_TO

`REFERENCES` is the general inter-signal relationship edge (extracted from content). `RESPONDS_TO` remains for causal chains (system-derived). Different semantic purposes, coexist.

---

## Phase 7: Missing Correction Event

### 7a. Add `ConditionCorrection`

**File:** `modules/rootsignal-common/src/events.rs`
**File:** `modules/rootsignal-common/src/system_events.rs`

Add `ConditionCorrection` enum with variants matching `ConditionNode` fields:
- `Subject`
- `ObservedBy`
- `Measurement`
- `AffectedScope`

Wire through the correction handler and reducer, following the pattern of existing corrections (e.g., `GatheringCorrection`).

---

## Phase 8: Cleanup

### 8a. Remove writer-based schedule creation

**Files:** `modules/rootsignal-graph/src/writer.rs`

Delete `create_schedule()` and `link_schedule_to_signal()` methods. These are replaced by projector-based schedule creation (Phase 5d). Any handler code that calls these methods is updated to rely on the event → projector path.

### 8b. Deprecate `is_recurring` boolean

**File:** `modules/rootsignal-common/src/types.rs` (`GatheringNode`)

`is_recurring: bool` is redundant when a `Schedule` with an `rrule` exists. Keep the field for backward compat (serde default) but stop writing it in new extractions. The presence of `rrule` on the schedule IS the recurrence signal.

---

## Implementation Order

1. **Phase 1** (world crate) — pure additions, no downstream breakage
2. **Phase 2** (NodeMeta) — compile-fix pass across codebase
3. **Phase 3** (extraction) — LLM prompt + node building
4. **Phase 4** (event conversion) — enriched world events with real entities + schedules
5. **Phase 5** (Neo4j) — entity projection, schedule projection, reader reconstruction
6. **Phase 8** (cleanup) — remove writer schedule bypass, deprecate is_recurring
7. **Phase 7** (ConditionCorrection) — independent, can be done anytime
8. **Phase 6** (references) — stub only, low priority

Phases 1-2 are one commit. Phase 3 is one commit. Phases 4-5 are one commit (tightly coupled). Phase 8 follows immediately. Phases 6-7 are independent commits.

---

## What We Gain

- **Entity nodes in Neo4j** with typed relationships — queryable graph structure instead of flat strings
- **Role-based entity queries** — "find all signals where City Council is a decision_maker"
- **Entity ↔ Actor linking** — SAME_AS edges connect extracted mentions to curated identities
- **GovernmentBody** entity type — properly represents civic institutions
- **Privacy by design** — no Person entities, no Place entities separate from Location
- **Event-sourced schedules** — ScheduleNode creation moves into projector, fully replayable
- **Rich recurrence** — rrule, schedule_text, rdates, exdates all flow through world events
- **Location semantics** — role-tagged locations (venue, affected_area, origin)
- **ConditionCorrection** — closes the gap in the correction event model
- **Reference infrastructure** — ready to wire when extraction supports it
- **Power-scout compatibility** — Entity nodes are the anchor points for future SHAPES/FUNDED_BY/VOTED_AGAINST edges

## What We Don't Lose

- Per-type fields (`organizer`, `source_authority`, `observed_by`) stay on node structs for correction compatibility
- Existing serialized events deserialize via serde defaults on all new fields
- Helper methods on NodeMeta maintain backward compat for callers
- `occurrences(from, to)` query-time expansion unchanged — no node explosion

## What We Explicitly Defer

- **Entity resolution** beyond exact name match — future `EntityMerged` system event
- **Person entities** — revisit with public figure classification
- **Reference extraction** — field exists, extraction not wired
- **Entity aliases** — solve with entity resolution, not upfront schema
