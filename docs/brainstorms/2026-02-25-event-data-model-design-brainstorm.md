---
date: 2026-02-25
topic: event-data-model-design
---

# Event Data Model Design

## What We're Building

A deliberately designed event data model for the RootSignal event-sourcing system. The current events.rs was reverse-engineered from existing writer.rs Cypher queries — it works, but carries structural debt: a mega-struct (SignalDiscovered with ~30 fields), untyped JSON blobs (SourceUpdated.changes, SituationEvolved.changes), a catch-all relationship event, and derived values baked into facts.

This brainstorm defines the principles and structure for events as **self-contained observations** — decoupled from graph schema, typed end-to-end, and auditable on their own.

## Why This Approach

We evaluated three tensions:

1. **Mega-struct vs typed variants** — A single SignalDiscovered with all-optional type-specific fields makes it impossible to know what a valid gathering vs. aid looks like. Typed variants (GatheringDiscovered, AidDiscovered, etc.) give compile-time guarantees.

2. **Shared base struct vs islands** — A shared `SignalBase` creates coupling — fields that happen to share a name today (e.g. `name`) aren't semantically the same across types. Each event owning its fields allows independent evolution.

3. **Graph-language in events vs observation-language** — Events like `ActorLinkedToSignal` leak the graph model into the fact stream. Events should describe what was *observed*, and the reducer maps observations to graph structure.

## Key Decisions

### 1. Split SignalDiscovered into typed variants

Replace the single `SignalDiscovered` mega-struct with:
- `GatheringDiscovered` — events, meetups, cleanups
- `AidDiscovered` — resources, services, programs
- `NeedDiscovered` — requests for help, unmet needs
- `NoticeDiscovered` — announcements, policy changes
- `TensionDiscovered` — conflicts, concerns, systemic issues

Each carries only the fields relevant to that type. No `Option` fields that only apply to other types.

### 2. Each event owns all its fields (islands, not inheritance)

No shared base struct. Fields like `title`, `summary`, `lat`, `lng` appear on multiple event types but they're not *the same field* — they're the name of a gathering vs. the name of an aid resource. Repetition is honest. Independent evolution is easy.

### 3. Events describe observations, not graph properties

An event says "we observed a gathering called X at location Y." The reducer decides that means `MERGE (n:Gathering {id: $id}) SET n.name = $title`. The mapping from observation fields to graph properties is the reducer's job. Event field names should make sense to a human reading the audit log, not mirror Neo4j property names.

### 4. Typed change structs replace JSON blobs (nested enum pattern)

`SourceUpdated { changes: serde_json::Value }` and `SituationEvolved { changes: serde_json::Value }` become one top-level event variant per entity type, with a nested typed enum for the field change:

```rust
// One variant in Event enum, not one per field
SourceChanged { source_id: Uuid, change: SourceChange }

enum SourceChange {
    Weight { old: f64, new: f64 },
    Url { old: String, new: String },
    Role { old: SourceRole, new: SourceRole },
    Active { old: bool, new: bool },
}

SituationChanged { situation_id: Uuid, change: SituationChange }

enum SituationChange {
    Arc { old: SituationArc, new: SituationArc },
    Temperature { old: f64, new: f64 },
    Headline { old: String, new: String },
}
```

**Why nested enums instead of individual top-level variants:** Discovery events have fundamentally different shapes (GatheringDiscovered vs TensionDiscovered carry different fields) — individual variants make sense. But field changes are structurally uniform (old → new) — the variation is *which field* and *what type*. A nested enum keeps the main Event enum manageable while preserving full type safety.

### 5. Typed relationship events with universal entity IDs

Replace the catch-all `RelationshipEstablished { from_id, to_id, relationship_type: String, properties: serde_json::Value }` with typed events:
- `EvidenceLinked { subject: Uuid, evidence: Uuid }`
- `MentionObserved { entity: Uuid, actor: Uuid, role: String }`
- `ParentSourceIdentified { child: Uuid, parent: Uuid }`
- `SignalGroupedIntoSituation { signal: Uuid, situation: Uuid }`
- etc.

Each entity in the system gets a UUID when first observed. Relationship events reference entities by ID — the type is established at creation time, not repeated.

### 6. Derived values stay out of events

`SignalCorroborated.new_corroboration_count` is a running total — derivable by counting corroboration events. Events record facts only: "entity X was corroborated by source Y with similarity Z." The count is a projection concern. No risk of stale aggregates.

### 7. Typed corrections follow the same nested enum pattern

Replace `SignalFieldsCorrected { corrections: Vec<FieldCorrection> }` (where FieldCorrection uses `serde_json::Value`) with one event per entity type + nested correction enum:

```rust
GatheringCorrected { entity_id: Uuid, correction: GatheringCorrection, reason: String }

enum GatheringCorrection {
    Title { old: String, new: String },
    Summary { old: String, new: String },
    Location { old: GeoPoint, new: GeoPoint },
}
```

Same principle as changes: one top-level variant per entity type, typed inner enum per field. Each correction is its own auditable fact.

## Open Questions

- **Naming the entity ID field**: Should it be `id`, `entity_id`, or something else? The universal ID concept means every event that creates an entity should use a consistent field name.
- **Event versioning**: When an event's shape changes (e.g. adding a field to GatheringDiscovered), how do we handle old events in the store? Serde defaults + `skip_serializing_if` may suffice, or we may need explicit versioning.
- **Admin/manual edits**: The current model doesn't have events for admin overrides (e.g. an admin manually editing a field in the graph). Should AdminFieldEdited be a first-class event?

## Next Steps

-> `/workflows:plan` to design the concrete struct changes, migration path from current events.rs, and updated reducer.
