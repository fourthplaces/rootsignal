---
title: "feat: Gravity-Aware Stories + Place Nodes"
type: feat
date: 2026-02-18
---

# Gravity-Aware Stories + Place Nodes

## Overview

Replace `RESPONDS_TO` + `gathering_type` property hack with a dedicated `DRAWN_TO` edge type. Promote venue strings to first-class `Place` graph nodes with `GATHERS_AT` edges. Update story weaver and reader to treat gatherings as structurally distinct from instrumental responses.

**Brainstorm:** [docs/brainstorms/2026-02-18-gravity-aware-stories-brainstorm.md](../brainstorms/2026-02-18-gravity-aware-stories-brainstorm.md)

## Problem Statement

The gravity scout discovers gatherings (vigils, solidarity meals, singing) and wires them as `RESPONDS_TO` edges with a `gathering_type` property. This is semantically wrong — people gathering at Lake Street Church aren't "responding to" ICE enforcement. They're *drawn to each other because of* the tension. The edge type should express what the relationship actually is.

Meanwhile, venue strings on signal nodes ("Lake Street Church") are invisible to graph queries. A church hosting immigration vigils, tenant meetups, AND food justice dinners is a gravitational center — the project's namesake "fourth place" concept — but you can't discover that from strings scattered across signal nodes.

## Proposed Solution

Three structural additions:

1. **`DRAWN_TO` edge** — `Signal ──DRAWN_TO──▶ Tension` with `gathering_type` property. Replaces `RESPONDS_TO` + `gathering_type IS NOT NULL`.
2. **`Place` node** — first-class venue with slug-based dedup, city-scoped.
3. **`GATHERS_AT` edge** — `Signal ──GATHERS_AT──▶ Place` linking gathering signals to their venue.

Story weaver and all readers union over `RESPONDS_TO|DRAWN_TO`. Consumers use `edge_type` to distinguish gatherings from instrumental responses.

## Technical Approach

### Schema Changes

```
Signal ──DRAWN_TO──▶ Tension
  properties:
    match_strength: f64
    explanation: String
    gathering_type: String  ("vigil", "singing", "solidarity meal", etc.)

Place
  id: UUID
  name: String
  slug: String           (normalized: lowercase, strip punctuation, collapse whitespace)
  city: String           (city slug — scoped dedup)
  lat: f64               (city-center initially, geocoded: false)
  lng: f64
  geocoded: bool         (false until real geocoding)
  created_at: DateTime

Signal ──GATHERS_AT──▶ Place
```

### Key Decisions (from brainstorm + pressure test)

- **Place dedup: slug + city only.** No embeddings. "First Baptist Church" and "First United Methodist Church" have high cosine similarity but are different buildings. `MERGE ON (slug, city)`.
- **Geocoding deferred.** Places start with city-center + `geocoded: false`. Real coordinates in a future phase.
- **cause_heat unchanged.** It's embedding-based all-pairs cosine similarity — no edge walking. `DRAWN_TO` signals radiate heat automatically via embedding proximity.
- **No venue backfill.** Existing venue strings were never persisted to signal node properties — only used for future query seeding. Places are created going forward only.
- **Slug function in `rootsignal-common`.** First algorithmic slug generation in the codebase — must be reusable and testable, not buried in `writer.rs`.

### Critical Design Notes (from spec-flow + pressure test)

1. **Place slug dedup is city-scoped.** `MERGE (p:Place {slug: $slug, city: $city})` — "Lake Street Church" in Minneapolis ≠ "Lake Street Church" in St. Paul.
2. **A signal can have BOTH `RESPONDS_TO` AND `DRAWN_TO` to the same tension.** A legal clinic that also hosts a vigil. `DRAWN_TO` takes precedence for display. `RESPONDS_TO` is not duplicated.
3. **Curiosity uninvestigated check must include `DRAWN_TO`.** `find_curiosity_targets` at `writer.rs:2323` filters `NOT (n)-[:RESPONDS_TO]->(:Tension)` — must become `NOT (n)-[:RESPONDS_TO|DRAWN_TO]->(:Tension)`.
4. **Tension merge must re-point `DRAWN_TO` edges.** `merge_duplicate_tensions` at `writer.rs:2625` only re-points `RESPONDS_TO`. Without this fix, `DETACH DELETE` on the absorbed tension silently destroys all `DRAWN_TO` edges. **Must ship in Phase 1.**
5. **`PlaceNode` is a standalone struct** (like `StoryNode`, `CityNode`), NOT in the `NodeType` signal enum. Places aren't signals.
6. **Label guards on `create_drawn_to_edge`.** Must follow `create_response_edge` pattern: `WHERE (s:Give OR s:Event OR s:Ask)` — no label-less full scans.
7. **Defensive MERGE semantics.** `create_drawn_to_edge` must use `ON CREATE SET` / `ON MATCH SET` (matching existing `create_gravity_edge`), not bare `SET`.
8. **`wire_also_addresses` also calls `create_gravity_edge`.** Both callsites in gravity_scout.rs must switch to `create_drawn_to_edge`.
9. **Null deserialization.** `gathering_type` on `TensionRespondent` must use `.ok()` to produce `Option<String>`, not `.unwrap_or_default()` which would convert null to empty string.

### Deferred (YAGNI)

These are intentionally cut from this iteration:

- **`SourceRole::Gravity`** — nothing dispatches on it. Gravity scout runs independently, not via source-role scheduling. Add when gravity sources need distinct routing.
- **`gathering_count` on Story** — denormalized cache maintained in 2 places with no consumer. Compute from edges on demand when a consumer needs it.
- **`place_type` on PlaceNode** — no consumer filters or displays it. Add when type-based queries are needed.
- **Place reader methods** (`get_places_by_city`, `get_place_with_signals`, `get_cross_tension_places`) — no UI or consumer exists. Build when a frontend needs them.
- **`GqlPlace` + Place GraphQL queries** — no frontend work planned. Add with the Place UI.
- **Venue backfill migration** — existing venue data was never persisted to node properties. Places are created going forward only.

## Implementation Phases

### Phase 1: Types + Writer + Data Migration

Types, write methods, edge conversion, and tension merge fix. One atomic PR — migration runs before any behavioral change.

**`modules/rootsignal-common/src/types.rs`:**

- [x] Add `EdgeType::DrawnTo` variant (line 700)
- [x] Add `EdgeType::GathersAt` variant (line 700)
- [x] Add `PlaceNode` struct (after `CityNode` at line 406):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceNode {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub city: String,
    pub lat: f64,
    pub lng: f64,
    pub geocoded: bool,
    pub created_at: DateTime<Utc>,
}
```

- [x] Add `TensionRespondent.edge_type: String` and `gathering_type: Option<String>` fields (struct is in `writer.rs:3337`, not `types.rs`)

**`modules/rootsignal-common/src/lib.rs` (or new `slug.rs`):**

- [x] Add `pub fn slugify(name: &str) -> String` — lowercase, strip non-alphanumeric (keep spaces), collapse whitespace, replace spaces with hyphens. Reusable utility alongside `extract_domain`, `haversine_km`.

**`modules/rootsignal-graph/src/migrate.rs`:**

- [x] Add Place uniqueness constraint: `CREATE CONSTRAINT place_id_unique IF NOT EXISTS FOR (p:Place) REQUIRE p.id IS UNIQUE`
- [x] Add Place slug+city index — used two single-property indexes (slug, city) for Memgraph compatibility
- [x] Add Place city index: `CREATE INDEX place_city IF NOT EXISTS FOR (p:Place) ON (p.city)`
- [x] Data migration — convert existing gathering edges (idempotent, single Cypher statement):

```cypher
MATCH (sig)-[old:RESPONDS_TO]->(t:Tension)
WHERE old.gathering_type IS NOT NULL
MERGE (sig)-[new:DRAWN_TO]->(t)
SET new.match_strength = old.match_strength,
    new.explanation = old.explanation,
    new.gathering_type = old.gathering_type
DELETE old
```

**`modules/rootsignal-graph/src/writer.rs`:**

- [x] New `find_or_create_place(&self, name: &str, city: &str, lat: f64, lng: f64) -> Result<Uuid>`:
  - Use `rootsignal_common::slugify(name)` for slug
  - `MERGE (p:Place {slug: $slug, city: $city}) ON CREATE SET p.id = $id, p.name = $name, p.lat = $lat, p.lng = $lng, p.geocoded = false, p.created_at = datetime() RETURN p.id`

- [x] New `create_drawn_to_edge(&self, signal_id: Uuid, tension_id: Uuid, match_strength: f64, explanation: &str, gathering_type: &str)`:
  - Label guard: `MATCH (s) WHERE s.id = $sid AND (s:Give OR s:Event OR s:Ask)`
  - Defensive MERGE: `ON CREATE SET r.match_strength = $ms, r.explanation = $exp, r.gathering_type = $gt ON MATCH SET r.match_strength = $ms, r.explanation = $exp, r.gathering_type = $gt`

- [x] New `create_gathers_at_edge(&self, signal_id: Uuid, place_id: Uuid)`:
  - `MATCH (s {id: $sid}), (p:Place {id: $pid}) MERGE (s)-[:GATHERS_AT]->(p)`

- [x] Update `merge_duplicate_tensions` (~line 2625): add `DRAWN_TO` re-pointing block parallel to existing `RESPONDS_TO` re-pointing, preserving `gathering_type`:

```cypher
MATCH (sig)-[r:DRAWN_TO]->(dup:Tension {id: $dup_id})
MATCH (survivor:Tension {id: $survivor_id})
WITH sig, r, survivor, dup
WHERE NOT (sig)-[:DRAWN_TO]->(survivor)
CREATE (sig)-[:DRAWN_TO {match_strength: r.match_strength, explanation: r.explanation, gathering_type: r.gathering_type}]->(survivor)
WITH r, dup
DELETE r
```

- [x] Update `get_existing_gravity_signals` (line 3082): query `DRAWN_TO` edges instead of `RESPONDS_TO WHERE gathering_type IS NOT NULL`

- [x] Update `find_curiosity_targets` (line 2323): change `NOT (n)-[:RESPONDS_TO]->(:Tension)` to `NOT (n)-[:RESPONDS_TO|DRAWN_TO]->(:Tension)`

- [x] Deprecate `create_gravity_edge` (line 3133) — keep temporarily until Phase 2 is merged

### Phase 2: Gravity Scout — Wire New Edges

Switch gravity scout from RESPONDS_TO to DRAWN_TO + Place.

**`modules/rootsignal-scout/src/gravity_scout.rs`:**

- [x] Replace `create_gravity_edge` call with `create_drawn_to_edge` (primary callsite)
- [x] Replace `create_gravity_edge` call in `wire_also_addresses` with `create_drawn_to_edge`
- [x] After creating signal, if `gathering.venue` is Some:
  1. Call `find_or_create_place(venue, city, city_lat, city_lng)` — use city-center coords, `geocoded: false`
  2. Call `create_gathers_at_edge(signal_id, place_id)`

**`modules/rootsignal-graph/src/writer.rs`:**

- [x] Remove deprecated `create_gravity_edge` (line 3133)

### Phase 3: Story Weaver + Reader — Union Queries

Make downstream consumers see `DRAWN_TO` edges. Ship as one PR — no reason to expose gatherings in the weaver but not in the reader.

**`modules/rootsignal-graph/src/writer.rs`:**

- [x] Update `find_tension_hubs` (line 2431):

```cypher
MATCH (t:Tension)<-[r:RESPONDS_TO|DRAWN_TO]-(sig)
WHERE NOT (t)<-[:CONTAINS]-(:Story)
WITH t, collect({
    sig_id: sig.id,
    source_url: sig.source_url,
    strength: r.match_strength,
    explanation: r.explanation,
    edge_type: type(r),
    gathering_type: r.gathering_type
}) AS respondents
WHERE size(respondents) >= 2
```

- [x] Update `find_story_growth` (line 2494): same `RESPONDS_TO|DRAWN_TO` union pattern with `type(r)` and `r.gathering_type`

- [x] Update `TensionRespondent` deserialization: use `.ok()` for `gathering_type` to get `Option<String>` (not `.unwrap_or_default()`) — done in Phase 1

**`modules/rootsignal-graph/src/reader.rs`:**

- [x] Update `get_story_tension_responses` (line 432): union `RESPONDS_TO|DRAWN_TO`, add `type(rel) AS edge_type` and `rel.gathering_type AS gathering_type` to response JSON
- [x] Update tension response query at line 706: add `DRAWN_TO` to relationship type union

**`modules/rootsignal-api/src/graphql/types.rs`:**

- [x] ~~Add `edge_type` and `gathering_type` to story response items in `GqlStory`~~ — YAGNI: GraphQL `responses()` maps to `GqlSignal` (no edge metadata). REST endpoint already has the data via `get_story_tension_responses`. New GraphQL type deferred until frontend needs it.

## Acceptance Criteria

### Functional

- [ ] Gravity scout creates `DRAWN_TO` edges (not `RESPONDS_TO` with `gathering_type`)
- [ ] Gravity scout creates `Place` nodes from venue strings with `GATHERS_AT` edges
- [ ] Place dedup works: same slug + city merges, different cities stay separate
- [ ] `find_tension_hubs` returns both `RESPONDS_TO` and `DRAWN_TO` signals
- [ ] `find_story_growth` returns both `RESPONDS_TO` and `DRAWN_TO` signals
- [ ] `find_curiosity_targets` excludes signals with `DRAWN_TO` edges (not just `RESPONDS_TO`)
- [ ] Tension merge re-points `DRAWN_TO` edges (not just `RESPONDS_TO`)
- [ ] Reader API exposes `edge_type` and `gathering_type` on story responses
- [ ] Existing data migrated: old `RESPONDS_TO` with `gathering_type` → `DRAWN_TO`

### Non-Functional

- [ ] No regression in story materialization (stories still form from 2+ respondents)
- [ ] No regression in curiosity loop (signals with `DRAWN_TO` aren't re-investigated)
- [ ] cause_heat unaffected (no code changes needed — embedding-based, not edge-based)
- [ ] All existing tests pass

## Testing

### Unit Tests

- [ ] `slugify` — "Lake Street Church" → "lake-street-church", "Lake St. Church!!!" → "lake-st-church", unicode, multiple spaces
- [ ] `find_or_create_place` — dedup by slug+city, idempotent, returns existing ID on match
- [ ] `create_drawn_to_edge` — creates edge with properties, label guard rejects non-signal nodes
- [ ] `TensionRespondent` deserialization: `RESPONDS_TO` edge produces `gathering_type: None`, `DRAWN_TO` edge produces `gathering_type: Some("vigil")`
- [ ] Tension merge re-points both `RESPONDS_TO` and `DRAWN_TO` edges

### Integration Tests

- [ ] Full gravity scout run creates `DRAWN_TO` edge + `Place` node + `GATHERS_AT` edge
- [ ] Story weaver materializes story from 2+ signals where some are `DRAWN_TO`
- [ ] Migration converts existing `RESPONDS_TO` + `gathering_type` edges to `DRAWN_TO` without data loss
- [ ] `wire_also_addresses` creates `DRAWN_TO` (not `RESPONDS_TO`)

### Litmus Test Additions

- [ ] Add `DRAWN_TO` edge assertions to existing litmus tests
- [ ] Add Place node creation to litmus test scenarios

## Risk Analysis

| Risk | Mitigation |
|------|-----------|
| Slug collisions (different places, same slug in same city) | Accept in v1. v1.1 adds spatial distance composite dedup |
| Place proliferation from inconsistent LLM venue names | Slug normalization absorbs most variants. Review counts periodically |
| Composite index syntax unsupported in Memgraph | Verify before deploying. Fallback: two single-property indexes (slug, city) |
| Reaper orphans Place nodes (deletes last signal with GATHERS_AT) | Accept in v1. Add periodic orphan cleanup: `MATCH (p:Place) WHERE NOT ()-[:GATHERS_AT]->(p) DELETE p` |
| `type(r)` returns SCREAMING_CASE ("DRAWN_TO") vs serde snake_case ("drawn_to") | Use raw string comparison in Rust, not EdgeType enum deserialization |

## Files Changed (Summary)

| File | Changes |
|------|---------|
| `modules/rootsignal-common/src/types.rs` | `PlaceNode`, `EdgeType::DrawnTo`, `EdgeType::GathersAt` |
| `modules/rootsignal-common/src/lib.rs` | `slugify()` utility function |
| `modules/rootsignal-graph/src/migrate.rs` | Place constraints + indexes, edge conversion migration |
| `modules/rootsignal-graph/src/writer.rs` | `find_or_create_place`, `create_drawn_to_edge`, `create_gathers_at_edge`, update `merge_duplicate_tensions`, `find_tension_hubs`, `find_story_growth`, `find_curiosity_targets`, `get_existing_gravity_signals`, `TensionRespondent` fields |
| `modules/rootsignal-graph/src/reader.rs` | Update `get_story_tension_responses` (line 432), tension response query (line 706) |
| `modules/rootsignal-scout/src/gravity_scout.rs` | Switch to `create_drawn_to_edge` (both callsites), create Place + GATHERS_AT |
| `modules/rootsignal-api/src/graphql/types.rs` | `edge_type` and `gathering_type` on story response items |

## References

- Brainstorm: [docs/brainstorms/2026-02-18-gravity-aware-stories-brainstorm.md](../brainstorms/2026-02-18-gravity-aware-stories-brainstorm.md)
- Story weaver architecture: [docs/architecture/story-weaver.md](../architecture/story-weaver.md)
- Signal → Tension → Response chain: [docs/architecture/signal-to-response-chain.md](../architecture/signal-to-response-chain.md)
- Learning: [docs/solutions/2026-02-17-unwrap-or-masks-data-quality.md](../solutions/2026-02-17-unwrap-or-masks-data-quality.md) — use `Option<T>` over defaults for new Place fields
