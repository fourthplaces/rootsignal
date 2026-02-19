---
title: "feat: Gravity-Aware Stories + Place Nodes"
type: feat
date: 2026-02-18
---

# Gravity-Aware Stories + Place Nodes

## Overview

Replace `RESPONDS_TO` + `gathering_type` property hack with a dedicated `DRAWN_TO` edge type. Promote venue strings to first-class `Place` graph nodes with `GATHERS_AT` edges. Update story weaver, reader, API, and migration to treat gatherings as structurally distinct from instrumental responses.

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
  place_type: String     ("church", "park", "community center", etc.)
  city: String           (city slug — scoped dedup)
  lat: f64               (city-center initially, geocoded: false)
  lng: f64
  geocoded: bool         (false until real geocoding)
  created_at: DateTime

Signal ──GATHERS_AT──▶ Place
```

### Key Decisions (from brainstorm)

- **Place dedup: slug + city only.** No embeddings. "First Baptist Church" and "First United Methodist Church" have high cosine similarity but are different buildings. `MERGE ON (slug, city)`.
- **`gathering_count: u32` on Story nodes.** Convenience counter, recomputed in Phase B from `DRAWN_TO` edge count.
- **Geocoding deferred.** Places start with city-center + `geocoded: false`. Real coordinates in a future phase.
- **`SourceRole::Gravity` variant.** The ontology split carries through the pipeline.
- **cause_heat unchanged.** It's embedding-based all-pairs cosine similarity — no edge walking. `DRAWN_TO` signals radiate heat automatically via embedding proximity.

### Critical Design Notes (from spec-flow analysis)

1. **Place slug dedup is city-scoped.** `MERGE (p:Place {slug: $slug, city: $city})` — "Lake Street Church" in Minneapolis ≠ "Lake Street Church" in St. Paul.
2. **A signal can have BOTH `RESPONDS_TO` AND `DRAWN_TO` to the same tension.** A legal clinic that also hosts a vigil. `DRAWN_TO` takes precedence for display — the signal is tagged as a gathering with instrumental properties. `RESPONDS_TO` is not duplicated.
3. **Curiosity uninvestigated check must include `DRAWN_TO`.** `find_curiosity_targets` at `writer.rs:2323` filters `NOT (n)-[:RESPONDS_TO]->(:Tension)` — must become `NOT (n)-[:RESPONDS_TO|DRAWN_TO]->(:Tension)`.
4. **Tension merge must re-point `DRAWN_TO` edges.** When dedup merges tensions, both `RESPONDS_TO` and `DRAWN_TO` edges from the absorbed tension must be re-pointed.
5. **`PlaceNode` is a standalone struct** (like `StoryNode`, `CityNode`), NOT in the `NodeType` signal enum. Places aren't signals.
6. **Reader methods tag edge type with `type(r)`.** Union queries use `CASE WHEN type(r) = 'DRAWN_TO' THEN r.gathering_type ELSE null END` to make the distinction explicit.

## Implementation Phases

### Phase 1: Types + Migration

Add types and schema. No behavior changes yet.

**`modules/rootsignal-common/src/types.rs`:**

- [ ] Add `EdgeType::DrawnTo` variant (line 700)
- [ ] Add `EdgeType::GathersAt` variant (line 700)
- [ ] Add `SourceRole::Gravity` variant (line 544) + Display + `from_str_loose`
- [ ] Add `PlaceNode` struct (after `CityNode` at line 406):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceNode {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub place_type: String,
    pub city: String,
    pub lat: f64,
    pub lng: f64,
    pub geocoded: bool,
    pub created_at: DateTime<Utc>,
}
```

- [ ] Add `gathering_count: u32` to `StoryNode` (line 354) with `#[serde(default)]`
- [ ] Add `TensionRespondent.edge_type: String` field and `gathering_type: Option<String>` field

**`modules/rootsignal-graph/src/migrate.rs`:**

- [ ] Add Place uniqueness constraint: `CREATE CONSTRAINT place_id_unique IF NOT EXISTS FOR (n:Place) REQUIRE n.id IS UNIQUE`
- [ ] Add Place composite index: `CREATE INDEX place_slug_city IF NOT EXISTS FOR (n:Place) ON (n.slug, n.city)`
- [ ] Add Place city index: `CREATE INDEX place_city IF NOT EXISTS FOR (n:Place) ON (n.city)`

### Phase 2: Graph Writer — Place + DRAWN_TO

Add write methods. Gravity scout still calls old method until Phase 3.

**`modules/rootsignal-graph/src/writer.rs`:**

- [ ] New `find_or_create_place(&self, name: &str, place_type: &str, city: &str, lat: f64, lng: f64) -> Result<Uuid>`:
  - Slugify name: lowercase, strip non-alphanumeric (keep spaces), collapse whitespace, replace spaces with hyphens
  - `MERGE (p:Place {slug: $slug, city: $city}) ON CREATE SET p.id = $id, p.name = $name, p.place_type = $type, p.lat = $lat, p.lng = $lng, p.geocoded = false, p.created_at = datetime() RETURN p.id`

- [ ] New `create_drawn_to_edge(&self, signal_id: Uuid, tension_id: Uuid, match_strength: f64, explanation: &str, gathering_type: &str)`:
  - Pattern follows `create_response_edge` at line 1823
  - `MATCH (s {id: $sid}), (t:Tension {id: $tid}) MERGE (s)-[r:DRAWN_TO]->(t) SET r.match_strength = $ms, r.explanation = $exp, r.gathering_type = $gt`

- [ ] New `create_gathers_at_edge(&self, signal_id: Uuid, place_id: Uuid)`:
  - `MATCH (s {id: $sid}), (p:Place {id: $pid}) MERGE (s)-[:GATHERS_AT]->(p)`

- [ ] Update `get_existing_gravity_signals` (line 3082): query `DRAWN_TO` edges instead of `RESPONDS_TO WHERE gathering_type IS NOT NULL`

- [ ] Update `find_curiosity_targets` (line 2323): change `NOT (n)-[:RESPONDS_TO]->(:Tension)` to `NOT (n)-[:RESPONDS_TO|DRAWN_TO]->(:Tension)`

### Phase 3: Gravity Scout — Wire New Edges

Switch gravity scout from RESPONDS_TO to DRAWN_TO + Place.

**`modules/rootsignal-scout/src/gravity_scout.rs`:**

- [ ] Replace `create_gravity_edge` call with `create_drawn_to_edge`
- [ ] After creating signal, if `gathering.venue` is Some:
  1. Call `find_or_create_place(venue, place_type, city, city_lat, city_lng)` — use city-center coords, `geocoded: false`
  2. Call `create_gathers_at_edge(signal_id, place_id)`
- [ ] Extract `place_type` from LLM output. Add `place_type: Option<String>` to `GravityGathering` struct. Update prompt to ask for place type (church, park, community center, school, library, etc.). Default to `"unknown"` if not provided.

**`modules/rootsignal-graph/src/writer.rs`:**

- [ ] Deprecate/remove `create_gravity_edge` (line 3133) once all callers updated

### Phase 4: Story Weaver — Union Queries

Update story materialization and growth to see gatherings.

**`modules/rootsignal-graph/src/writer.rs`:**

- [ ] Update `find_tension_hubs` (line 2431):

```cypher
MATCH (t:Tension)<-[r:RESPONDS_TO|DRAWN_TO]-(sig)
WHERE NOT (t)<-[:CONTAINS]-(:Story)
WITH t, collect({
    sig_id: sig.id,
    source_url: sig.source_url,
    strength: r.match_strength,
    explanation: r.explanation,
    edge_type: type(r),
    gathering_type: CASE WHEN type(r) = 'DRAWN_TO' THEN r.gathering_type ELSE null END
}) AS respondents
WHERE size(respondents) >= 2
```

- [ ] Update `find_story_growth` (line 2494): same `RESPONDS_TO|DRAWN_TO` union pattern

- [ ] Update `TensionRespondent` struct to carry `edge_type` and `gathering_type`

**`modules/rootsignal-graph/src/story_weaver.rs`:**

- [ ] In `phase_materialize` (line 117): compute `gathering_count` from respondents where `edge_type == "DRAWN_TO"`, write to Story node
- [ ] In `phase_grow` (line 265): recompute `gathering_count` from current graph state

### Phase 5: Reader + API

Expose gatherings and places to consumers.

**`modules/rootsignal-graph/src/reader.rs`:**

- [ ] Update `get_story_tension_responses` (line 432): union `RESPONDS_TO|DRAWN_TO`, tag `edge_type` and `gathering_type` in response JSON
- [ ] New `get_places_by_city(city: &str) -> Vec<PlaceNode>`: `MATCH (p:Place {city: $city}) RETURN p ORDER BY p.name`
- [ ] New `get_place_with_signals(place_id: Uuid)`: Place + its gathering signals via `GATHERS_AT`
- [ ] New `get_cross_tension_places(city: &str)`: Places with gatherings across 2+ tensions

**`modules/rootsignal-api/src/graphql/types.rs`:**

- [ ] Add `gathering_count` to `GqlStory` (line 431)
- [ ] Add `edge_type` and `gathering_type` to story response items
- [ ] New `GqlPlace` type with id, name, slug, place_type, city, lat, lng
- [ ] New queries: `places(city: String)`, `place(id: ID)`, `crossTensionPlaces(city: String)`

### Phase 6: Migration — Convert Existing Data

Convert existing `RESPONDS_TO` + `gathering_type` edges to `DRAWN_TO`. Extract venue strings to Place nodes.

**`modules/rootsignal-graph/src/migrate.rs`:**

- [ ] Data migration (run once, idempotent):

```cypher
// Convert gathering edges: RESPONDS_TO with gathering_type → DRAWN_TO
MATCH (sig)-[old:RESPONDS_TO]->(t:Tension)
WHERE old.gathering_type IS NOT NULL
MERGE (sig)-[new:DRAWN_TO]->(t)
SET new.match_strength = old.match_strength,
    new.explanation = old.explanation,
    new.gathering_type = old.gathering_type
DELETE old
```

- [ ] Venue extraction (idempotent — MERGE on slug+city):

```cypher
// Extract venue strings from signals with DRAWN_TO edges to Place nodes
MATCH (sig)-[:DRAWN_TO]->(:Tension)
WHERE sig.venue IS NOT NULL AND sig.venue <> ''
WITH sig, sig.venue AS venue_name, sig.city AS city
// (slug computation in Rust, not Cypher — call find_or_create_place per signal)
```

**Implementation note:** The venue→Place extraction should happen in Rust (calling `find_or_create_place` per signal) because slug normalization is non-trivial in Cypher. Run as a one-time migration function, not raw Cypher.

## Acceptance Criteria

### Functional

- [ ] Gravity scout creates `DRAWN_TO` edges (not `RESPONDS_TO` with `gathering_type`)
- [ ] Gravity scout creates `Place` nodes from venue strings with `GATHERS_AT` edges
- [ ] Place dedup works: same slug + city merges, different cities stay separate
- [ ] `find_tension_hubs` returns both `RESPONDS_TO` and `DRAWN_TO` signals
- [ ] `find_story_growth` returns both `RESPONDS_TO` and `DRAWN_TO` signals
- [ ] Story nodes have `gathering_count` reflecting `DRAWN_TO` edge count
- [ ] `find_curiosity_targets` excludes signals with `DRAWN_TO` edges (not just `RESPONDS_TO`)
- [ ] Reader API exposes `edge_type` and `gathering_type` on story responses
- [ ] Place queries work: by-city, by-id, cross-tension
- [ ] Existing data migrated: old `RESPONDS_TO` with `gathering_type` → `DRAWN_TO`

### Non-Functional

- [ ] No regression in story materialization (stories still form from 2+ respondents)
- [ ] No regression in curiosity loop (signals with `DRAWN_TO` aren't re-investigated)
- [ ] cause_heat unaffected (no code changes needed — embedding-based, not edge-based)
- [ ] All existing tests pass

## Testing

### Unit Tests

- [ ] `find_or_create_place` — dedup by slug+city, idempotent
- [ ] `create_drawn_to_edge` — creates edge with properties
- [ ] Slug normalization: "Lake Street Church" → "lake-street-church", "Lake St. Church!!!" → "lake-st-church"
- [ ] `TensionRespondent` serialization with edge_type/gathering_type fields

### Integration Tests

- [ ] Full gravity scout run creates `DRAWN_TO` edge + `Place` node + `GATHERS_AT` edge
- [ ] Story weaver materializes story from 2+ signals where some are `DRAWN_TO`
- [ ] `gathering_count` computed correctly on Story
- [ ] Migration converts existing data without data loss

### Litmus Test Additions

- [ ] Add `DRAWN_TO` edge assertions to existing litmus tests
- [ ] Add Place node creation to litmus test scenarios

## Risk Analysis

| Risk | Mitigation |
|------|-----------|
| Slug collisions (different places, same slug in same city) | Accept in v1. v1.1 adds spatial distance composite dedup |
| Place proliferation from inconsistent LLM venue names | Slug normalization absorbs most variants. Review counts periodically |
| Migration breaks existing stories | Run migration after schema changes, before next scout run. Backup first |
| `type(r)` not supported in Memgraph | Verify — Memgraph supports `type()` function. Fallback: separate queries |

## Files Changed (Summary)

| File | Changes |
|------|---------|
| `modules/rootsignal-common/src/types.rs` | `PlaceNode`, `EdgeType::DrawnTo`, `EdgeType::GathersAt`, `SourceRole::Gravity`, `StoryNode.gathering_count`, `TensionRespondent` fields |
| `modules/rootsignal-graph/src/migrate.rs` | Place constraints + indexes, data migration |
| `modules/rootsignal-graph/src/writer.rs` | `find_or_create_place`, `create_drawn_to_edge`, `create_gathers_at_edge`, update `find_tension_hubs`, `find_story_growth`, `find_curiosity_targets`, `get_existing_gravity_signals` |
| `modules/rootsignal-graph/src/story_weaver.rs` | `gathering_count` computation in phase_materialize/phase_grow |
| `modules/rootsignal-graph/src/reader.rs` | Update `get_story_tension_responses`, new Place queries |
| `modules/rootsignal-scout/src/gravity_scout.rs` | Switch to `create_drawn_to_edge`, create Place + GATHERS_AT |
| `modules/rootsignal-api/src/graphql/types.rs` | `GqlPlace`, `gathering_count` on GqlStory, Place queries |

## References

- Brainstorm: [docs/brainstorms/2026-02-18-gravity-aware-stories-brainstorm.md](../brainstorms/2026-02-18-gravity-aware-stories-brainstorm.md)
- Story weaver architecture: [docs/architecture/story-weaver.md](../architecture/story-weaver.md)
- Signal → Tension → Response chain: [docs/architecture/signal-to-response-chain.md](../architecture/signal-to-response-chain.md)
- Learning: [docs/solutions/2026-02-17-unwrap-or-masks-data-quality.md](../solutions/2026-02-17-unwrap-or-masks-data-quality.md) — use `Option<T>` over defaults for new Place fields
