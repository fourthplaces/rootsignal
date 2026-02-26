---
title: "refactor: Redesign Event Data Model with Typed Variants"
type: refactor
date: 2026-02-25
---

# Redesign Event Data Model with Typed Variants

## Overview

Redesign the `Event` enum in `rootsignal-common/src/events.rs` based on the decisions in `docs/brainstorms/2026-02-25-event-data-model-design-brainstorm.md`. Replace the mega-struct `SignalDiscovered` with 5 typed discovery variants, replace untyped JSON blobs with nested typed enums, and replace the catch-all `RelationshipEstablished` with typed relationship events.

This is a greenfield redesign — no production event data exists in the store. The EventStore itself (`rootsignal-events`) is domain-agnostic and requires zero changes.

## Problem Statement

The current `Event` enum was reverse-engineered from `writer.rs` Cypher queries. It works but carries structural debt:

1. **SignalDiscovered mega-struct** (lines 98-145) — 13 shared fields + 13 type-specific `Option` fields. The `node_type` discriminant is checked at runtime in both producers and the reducer. A `GatheringDiscovered` event can have `severity` set (a Tension-only field) and the type system won't complain.

2. **Untyped JSON blobs** — `SourceUpdated.changes` and `SituationEvolved.changes` are `serde_json::Value`. The reducer iterates over arbitrary JSON keys and dynamically constructs Cypher SET clauses using `sanitize_field_name()`. Type errors are impossible to catch at compile time.

3. **Catch-all relationship** — `RelationshipEstablished` uses `relationship_type: String` and `properties: Option<serde_json::Value>`. It's the loosest event in the system (and currently only used in test fixtures).

4. **Untyped field corrections** — `FieldCorrection` uses `old_value: serde_json::Value, new_value: serde_json::Value`. No compile-time guarantee about what can be corrected.

## Proposed Solution

Apply the brainstorm decisions:

1. Split `SignalDiscovered` into `GatheringDiscovered`, `AidDiscovered`, `NeedDiscovered`, `NoticeDiscovered`, `TensionDiscovered` — each owning only its relevant fields
2. Replace `SourceUpdated`/`SituationEvolved` with `SourceChanged`/`SituationChanged` carrying nested typed enums
3. Remove `RelationshipEstablished` — existing typed events already cover all relationship types
4. Replace `SignalFieldsCorrected` with typed per-entity correction events
5. Keep `corroboration_count` as a producer-computed absolute value (not derived — the reducer cannot read the graph)
6. **Remove all "Signal" naming** — events describe observations, not domain concepts. Renames:
   - `SignalCorroborated` → `ObservationCorroborated`
   - `SignalConfidenceScored` → `ConfidenceScored`
   - `SignalRejected` → `ObservationRejected`
   - `SignalExpired` → `EntityExpired`
   - `SignalPurged` → `EntityPurged`
   - `SignalDeduplicated` → `DuplicateDetected`
   - `SignalRefreshed` → `FreshnessConfirmed`
   - `SignalDroppedNoDate` → `ExtractionDroppedNoDate`

## Technical Approach

### Serde Strategy for Nested Enums

The parent `Event` enum uses `#[serde(tag = "type", rename_all = "snake_case")]` (internally tagged). Nested enums **cannot share the `"type"` key** — serde would collide. The nested change enums will use adjacently-tagged representation with a distinct key:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "field", content = "value")]
pub enum SourceChange {
    Weight { old: f64, new: f64 },
    Url { old: String, new: String },
    Role { old: SourceRole, new: SourceRole },
    QualityPenalty { old: f64, new: f64 },
    GapContext { old: Option<String>, new: Option<String> },
}
```

Serialized form:
```json
{
  "type": "source_changed",
  "source_id": "abc-123",
  "change": { "field": "weight", "value": { "old": 0.5, "new": 0.8 } }
}
```

This avoids tag collision while keeping the JSON human-readable.

### corroboration_count Resolution

The brainstorm said "remove derived values" but the reducer **cannot read the graph** (contract test enforced). The existing plan already resolved this: `new_corroboration_count` is an **absolute value computed by the producer before append**. The boundary test (line 301) explicitly asserts: "new_corroboration_count IS a fact (absolute value computed by producer before append)."

**Decision:** Keep `new_corroboration_count` on the corroboration event. It's a producer-computed fact, not a derived value. Same for `ActorStatsUpdated.signal_count` and `SourceScrapeRecorded` counters — these are producer-computed snapshots.

### Relationship Events Scope

`RelationshipEstablished` is only used in test fixtures — no production code constructs it. However, the typed replacements are forward-looking for when `GraphWriter` operations migrate to the event store (Phase 3 of the event-sourcing plan). The initial set covers relationships that the reducer already handles or that `writer.rs` creates:

- `EvidenceLinked` — already handled by `CitationRecorded` (keep as-is, don't duplicate)
- `MentionObserved` — already handled by `ActorLinkedToSignal` (keep as-is)
- `ParentSourceIdentified` — already handled by `SourceLinkDiscovered` (keep as-is)

**Decision:** Remove `RelationshipEstablished` entirely. The existing typed events (`CitationRecorded`, `ActorLinkedToSignal`, `ActorLinkedToSource`, `SourceLinkDiscovered`, `TagsAggregated`) already cover all relationships that flow through the event store. Future relationship types get their own typed events when they're needed, not speculatively.

### Implementation Phases

#### Phase 1: Define Supporting Types

Create value types, nested change enums, and correction enums.

**File: `modules/rootsignal-common/src/events.rs`**

- [ ] Define `Schedule` value type:
  ```rust
  pub struct Schedule {
      pub starts_at: Option<DateTime<Utc>>,
      pub ends_at: Option<DateTime<Utc>>,
      pub all_day: bool,
      pub rrule: Option<String>,
      pub timezone: Option<String>,
  }
  ```
  Replaces scattered starts_at/ends_at/is_recurring. Absorbs ScheduleRecorded.

- [ ] Define `Location` value type:
  ```rust
  pub struct Location {
      pub point: Option<GeoPoint>,
      pub name: Option<String>,
      pub address: Option<String>,
  }
  ```
  Replaces scattered point/name fields. Used on all 5 discovery types.

- [ ] Define `SourceChange` enum (adjacently-tagged: `#[serde(tag = "field", content = "value")]`)
  - `Weight { old: f64, new: f64 }`
  - `Url { old: String, new: String }`
  - `Role { old: SourceRole, new: SourceRole }`
  - `QualityPenalty { old: f64, new: f64 }`
  - `GapContext { old: Option<String>, new: Option<String> }`
  - `Active { old: bool, new: bool }`

- [ ] Define `SituationChange` enum (same tagging)
  - `Headline { old: String, new: String }`
  - `Lede { old: String, new: String }`
  - `Arc { old: SituationArc, new: SituationArc }`
  - `Temperature { old: f64, new: f64 }`
  - `Location { old_lat: Option<f64>, old_lng: Option<f64>, new_lat: Option<f64>, new_lng: Option<f64>, new_name: Option<String> }`
  - `Sensitivity { old: SensitivityLevel, new: SensitivityLevel }`
  - `Category { old: Option<String>, new: Option<String> }`
  - `StructuredState { old: String, new: String }`

- [ ] Define 5 per-entity correction enums (same tagging) — each only contains fields that exist on that entity type

  `GatheringCorrection`:
  - `Title { old: String, new: String }`
  - `Summary { old: String, new: String }`
  - `Confidence { old: f32, new: f32 }`
  - `Sensitivity { old: SensitivityLevel, new: SensitivityLevel }`
  - `Location { old_lat: Option<f64>, old_lng: Option<f64>, new_lat: Option<f64>, new_lng: Option<f64> }`
  - `StartsAt { old: Option<DateTime<Utc>>, new: Option<DateTime<Utc>> }`
  - `EndsAt { old: Option<DateTime<Utc>>, new: Option<DateTime<Utc>> }`
  - `Organizer { old: Option<String>, new: Option<String> }`
  - `IsRecurring { old: Option<bool>, new: Option<bool> }`
  - `ActionUrl { old: Option<String>, new: Option<String> }`

  `AidCorrection`:
  - `Title { old: String, new: String }`
  - `Summary { old: String, new: String }`
  - `Confidence { old: f32, new: f32 }`
  - `Sensitivity { old: SensitivityLevel, new: SensitivityLevel }`
  - `Location { old_lat: Option<f64>, old_lng: Option<f64>, new_lat: Option<f64>, new_lng: Option<f64> }`
  - `ActionUrl { old: Option<String>, new: Option<String> }`
  - `Availability { old: Option<String>, new: Option<String> }`
  - `IsOngoing { old: Option<bool>, new: Option<bool> }`

  `NeedCorrection`:
  - `Title { old: String, new: String }`
  - `Summary { old: String, new: String }`
  - `Confidence { old: f32, new: f32 }`
  - `Sensitivity { old: SensitivityLevel, new: SensitivityLevel }`
  - `Location { old_lat: Option<f64>, old_lng: Option<f64>, new_lat: Option<f64>, new_lng: Option<f64> }`
  - `Urgency { old: Option<Urgency>, new: Option<Urgency> }`
  - `WhatNeeded { old: Option<String>, new: Option<String> }`
  - `Goal { old: Option<String>, new: Option<String> }`

  `NoticeCorrection`:
  - `Title { old: String, new: String }`
  - `Summary { old: String, new: String }`
  - `Confidence { old: f32, new: f32 }`
  - `Sensitivity { old: SensitivityLevel, new: SensitivityLevel }`
  - `Location { old_lat: Option<f64>, old_lng: Option<f64>, new_lat: Option<f64>, new_lng: Option<f64> }`
  - `Severity { old: Option<Severity>, new: Option<Severity> }`
  - `Category { old: Option<String>, new: Option<String> }`
  - `EffectiveDate { old: Option<DateTime<Utc>>, new: Option<DateTime<Utc>> }`
  - `SourceAuthority { old: Option<String>, new: Option<String> }`

  `TensionCorrection`:
  - `Title { old: String, new: String }`
  - `Summary { old: String, new: String }`
  - `Confidence { old: f32, new: f32 }`
  - `Sensitivity { old: SensitivityLevel, new: SensitivityLevel }`
  - `Location { old_lat: Option<f64>, old_lng: Option<f64>, new_lat: Option<f64>, new_lng: Option<f64> }`
  - `Severity { old: Option<Severity>, new: Option<Severity> }`
  - `WhatWouldHelp { old: Option<String>, new: Option<String> }`

- [ ] Write round-trip serde tests for each nested enum to verify tag strategy works

#### Phase 2: Split SignalDiscovered into Typed Variants

Replace the mega-struct with 5 typed discovery events. Each owns all its fields — no shared base struct.

**File: `modules/rootsignal-common/src/events.rs`**

- [ ] Add `GatheringDiscovered` variant with gathering-specific fields:
  ```rust
  GatheringDiscovered {
      id: Uuid,
      title: String,
      summary: String,
      sensitivity: SensitivityLevel,
      confidence: f32,
      source_url: String,
      extracted_at: DateTime<Utc>,
      content_date: Option<DateTime<Utc>>,
      location: Option<GeoPoint>,
      location_name: Option<String>,
      from_location: Option<GeoPoint>,
      implied_queries: Vec<String>,
      mentioned_actors: Vec<String>,
      author_actor: Option<String>,
      // Gathering-specific
      starts_at: Option<DateTime<Utc>>,
      ends_at: Option<DateTime<Utc>>,
      action_url: Option<String>,
      organizer: Option<String>,
      is_recurring: Option<bool>,
  }
  ```

- [ ] Add `AidDiscovered` variant with aid-specific fields:
  ```rust
  AidDiscovered {
      id: Uuid,
      title: String,
      summary: String,
      sensitivity: SensitivityLevel,
      confidence: f32,
      source_url: String,
      extracted_at: DateTime<Utc>,
      content_date: Option<DateTime<Utc>>,
      location: Option<GeoPoint>,
      location_name: Option<String>,
      from_location: Option<GeoPoint>,
      implied_queries: Vec<String>,
      mentioned_actors: Vec<String>,
      author_actor: Option<String>,
      // Aid-specific
      action_url: Option<String>,
      availability: Option<String>,
      is_ongoing: Option<bool>,
  }
  ```

- [ ] Add `NeedDiscovered` variant:
  ```rust
  NeedDiscovered {
      id: Uuid,
      title: String,
      summary: String,
      sensitivity: SensitivityLevel,
      confidence: f32,
      source_url: String,
      extracted_at: DateTime<Utc>,
      content_date: Option<DateTime<Utc>>,
      location: Option<GeoPoint>,
      location_name: Option<String>,
      from_location: Option<GeoPoint>,
      implied_queries: Vec<String>,
      mentioned_actors: Vec<String>,
      author_actor: Option<String>,
      // Need-specific
      urgency: Option<Urgency>,
      what_needed: Option<String>,
      goal: Option<String>,
  }
  ```

- [ ] Add `NoticeDiscovered` variant:
  ```rust
  NoticeDiscovered {
      id: Uuid,
      title: String,
      summary: String,
      sensitivity: SensitivityLevel,
      confidence: f32,
      source_url: String,
      extracted_at: DateTime<Utc>,
      content_date: Option<DateTime<Utc>>,
      location: Option<GeoPoint>,
      location_name: Option<String>,
      from_location: Option<GeoPoint>,
      implied_queries: Vec<String>,
      mentioned_actors: Vec<String>,
      author_actor: Option<String>,
      // Notice-specific
      severity: Option<Severity>,
      category: Option<String>,
      effective_date: Option<DateTime<Utc>>,
      source_authority: Option<String>,
  }
  ```

- [ ] Add `TensionDiscovered` variant:
  ```rust
  TensionDiscovered {
      id: Uuid,
      title: String,
      summary: String,
      sensitivity: SensitivityLevel,
      confidence: f32,
      source_url: String,
      extracted_at: DateTime<Utc>,
      content_date: Option<DateTime<Utc>>,
      location: Option<GeoPoint>,
      location_name: Option<String>,
      from_location: Option<GeoPoint>,
      implied_queries: Vec<String>,
      mentioned_actors: Vec<String>,
      author_actor: Option<String>,
      // Tension-specific
      severity: Option<Severity>,
      what_would_help: Option<String>,
  }
  ```

- [ ] Remove the old `SignalDiscovered` variant
- [ ] Update `event_type()` match arms for 5 new variants (returns `"gathering_discovered"`, `"aid_discovered"`, etc.)
- [ ] Update unit tests

**Note on field naming:** Fields use observation language (`id` not `signal_id`, `location` not `about_location`). The old `signal_id` → `id`, `about_location` → `location`. The reducer maps these to graph properties.

#### Phase 3: Replace Untyped Change Events

**File: `modules/rootsignal-common/src/events.rs`**

- [ ] Replace `SourceUpdated { source_id, canonical_key, changes: serde_json::Value }` with:
  ```rust
  SourceChanged {
      source_id: Uuid,
      canonical_key: String,
      change: SourceChange,
  }
  ```

- [ ] Replace `SituationEvolved { situation_id, changes: serde_json::Value }` with:
  ```rust
  SituationChanged {
      situation_id: Uuid,
      change: SituationChange,
  }
  ```

- [ ] Replace `SignalFieldsCorrected { signal_id, corrections: Vec<FieldCorrection> }` with 5 typed correction events:
  ```rust
  GatheringCorrected { id: Uuid, correction: GatheringCorrection, reason: String }
  AidCorrected { id: Uuid, correction: AidCorrection, reason: String }
  NeedCorrected { id: Uuid, correction: NeedCorrection, reason: String }
  NoticeCorrected { id: Uuid, correction: NoticeCorrection, reason: String }
  TensionCorrected { id: Uuid, correction: TensionCorrection, reason: String }
  ```
  The compiler prevents expressing impossible corrections (e.g. correcting `urgency` on a gathering).

- [ ] Remove `FieldCorrection` struct (replaced by per-entity typed enums)
- [ ] Update `event_type()` for new variant names
- [ ] Remove `RelationshipEstablished` variant entirely

#### Phase 4: Update the Reducer

**File: `modules/rootsignal-graph/src/reducer.rs`**

- [ ] Replace `SignalDiscovered` match arm (lines 90-219) with 5 typed match arms:
  - Each directly builds Cypher for the correct label (`Gathering`, `Aid`, etc.) without the inner `node_type` match
  - No more "unused params are harmless" — each arm only binds fields that exist on that variant
  - Each uses `MERGE (n:{Label} {id: $id})` for idempotency

- [ ] Replace `SourceUpdated` handler (lines 492-516) with `SourceChanged` handler:
  - Match on `SourceChange` variants
  - Each variant produces a specific SET clause — no more dynamic `sanitize_field_name()` iteration
  ```rust
  Event::SourceChanged { canonical_key, change, .. } => {
      let q = match change {
          SourceChange::Weight { new, .. } => {
              query("MATCH (s:Source {canonical_key: $key}) SET s.weight = $val")
                  .param("val", new)
          }
          SourceChange::Role { new, .. } => {
              query("MATCH (s:Source {canonical_key: $key}) SET s.source_role = $val")
                  .param("val", new.to_string())
          }
          // ... etc
      };
      let q = q.param("key", canonical_key.as_str());
      self.client.graph.run(q).await?;
      Ok(ApplyResult::Applied)
  }
  ```

- [ ] Replace `SituationEvolved` handler (lines 828-853) with `SituationChanged` handler (same pattern)

- [ ] Replace `SignalFieldsCorrected` handler (lines 283-327) with 5 typed correction handlers:
  - `GatheringCorrected` → `MATCH (n:Gathering {id: $id}) SET n.{field} = $val`
  - `AidCorrected` → `MATCH (n:Aid {id: $id}) SET n.{field} = $val`
  - `NeedCorrected` → `MATCH (n:Need {id: $id}) SET n.{field} = $val`
  - `NoticeCorrected` → `MATCH (n:Notice {id: $id}) SET n.{field} = $val`
  - `TensionCorrected` → `MATCH (n:Tension {id: $id}) SET n.{field} = $val`
  - Each matches on its correction enum to determine the field name and typed value
  - No `OPTIONAL MATCH` across all labels needed — the event type tells us the label

- [ ] Remove `RelationshipEstablished` handler (lines 737-776)

- [ ] Remove `sanitize_field_name()` function (no longer needed — all field names are compile-time known)

#### Phase 5: Update Tests

**File: `modules/rootsignal-graph/tests/reducer_contract_test.rs`**

- [ ] Update `NOOP_EVENT_TYPES` list — remove `signal_discovered`, add no new no-ops
- [ ] Update `APPLIED_EVENT_TYPES` list — replace `signal_discovered` with `gathering_discovered`, `aid_discovered`, `need_discovered`, `notice_discovered`, `tension_discovered`; replace `source_updated` with `source_changed`; replace `situation_evolved` with `situation_changed`; replace `signal_fields_corrected` with `gathering_corrected`, `aid_corrected`, `need_corrected`, `notice_corrected`, `tension_corrected`; remove `relationship_established`
- [ ] Rewrite `build_all_events()` — construct one instance of every new variant including nested enums
- [ ] Update source-level assertions:
  - `sanitize_field_name` assertion can be removed (function is gone)
  - Verify MERGE pattern still holds for new variant handlers
- [ ] Add test: nested enum round-trip through serde for each `SourceChange`, `SituationChange`, `SignalCorrection` variant

**File: `modules/rootsignal-common/tests/event_reducer_boundary_test.rs`**

- [ ] Update `GRAPH_MUTATING_TYPES` and `OBSERVABILITY_TYPES` lists
- [ ] Fix pre-existing classification disagreement (align with reducer's actual behavior)
- [ ] Rewrite `build_all_events()`
- [ ] Update schema evolution tests — verify old payloads with extra/missing optional fields still deserialize
- [ ] Update round-trip tests for new variants

**File: `modules/rootsignal-common/src/events.rs` (unit tests)**

- [ ] Update `signal_discovered_roundtrip` test → `gathering_discovered_roundtrip` (and add one per type)
- [ ] Update `all_event_types_are_unique` test with new variant names
- [ ] Add nested enum serde tests

## Acceptance Criteria

### Functional
- [ ] `Event` enum has 5 typed discovery variants instead of 1 mega-struct
- [ ] `SourceChanged` uses `SourceChange` nested typed enum
- [ ] `SituationChanged` uses `SituationChange` nested typed enum
- [ ] 5 typed correction events (`GatheringCorrected`, `AidCorrected`, `NeedCorrected`, `NoticeCorrected`, `TensionCorrected`) each with per-entity correction enum
- [ ] `RelationshipEstablished` is removed
- [ ] `FieldCorrection` struct is removed
- [ ] No `serde_json::Value` in any event variant payload (except observability events which are pass-through)
- [ ] `sanitize_field_name()` is removed from the reducer

### Quality Gates
- [ ] `cargo check --workspace` compiles clean
- [ ] All existing tests pass (with updated expectations)
- [ ] New round-trip serde tests for every nested enum variant
- [ ] Contract tests verify: no `Utc::now()`, no `Uuid::new_v4()`, all writes use MERGE, no embeddings/diversity/cause_heat in reducer
- [ ] Boundary test classification lists agree with reducer behavior (fix pre-existing disagreement)

### Non-Goals (Explicit)
- EventStore changes — it's domain-agnostic, stores `event_type: String` + `payload: Value`
- Scout pipeline changes — it still writes via `GraphWriter`, not events
- Enrichment pass changes — embeddings/diversity/cause_heat are separate
- Production data migration — no production events exist yet

## Dependencies & Risks

**Dependencies:**
- Phase 1 and Phase 2 of the event-sourcing foundation plan must be complete (they are — committed)

**Risks:**
- **Nested enum serde tag collision** — mitigated by using `#[serde(tag = "field", content = "value")]` on inner enums (distinct from outer `#[serde(tag = "type")]`)
- **Large PR size** — events.rs, reducer.rs, and two test files all change. Consider splitting into 2 PRs: (1) events.rs + unit tests, (2) reducer.rs + contract/boundary tests
- **Missing SourceChange/SituationChange variants** — start with the known set, add more as needed. New variants are additive and don't break existing events

## References

- Brainstorm: `docs/brainstorms/2026-02-25-event-data-model-design-brainstorm.md`
- Event-sourcing plan: `docs/plans/2026-02-25-refactor-event-sourcing-foundation-plan.md`
- Current events: `modules/rootsignal-common/src/events.rs`
- Current reducer: `modules/rootsignal-graph/src/reducer.rs`
- Reducer contract tests: `modules/rootsignal-graph/tests/reducer_contract_test.rs`
- Boundary tests: `modules/rootsignal-common/tests/event_reducer_boundary_test.rs`
- Typed node pattern (model to follow): `modules/rootsignal-common/src/types.rs` (NodeMeta, GatheringNode, etc.)
- Learning: `docs/solutions/2026-02-17-unwrap-or-masks-data-quality.md` — use `Option<T>` for LLM-extracted fields
