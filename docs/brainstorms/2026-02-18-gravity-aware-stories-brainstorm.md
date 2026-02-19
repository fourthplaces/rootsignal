---
date: 2026-02-18
topic: gravity-aware-stories
---

# Gravity-Aware Stories + Places

## What We're Building

Two things that belong together:

1. **`DRAWN_TO` edge** — gatherings are not responses. A vigil doesn't "respond to" ICE enforcement. People are drawn to each other *because of* the tension. The graph should express this honestly. `RESPONDS_TO` is for instrumental responses (legal clinics, food shelves). `DRAWN_TO` is for community formation (vigils, singing, solidarity meals).

2. **Place node** — the gravity scout already discovers venues ("Lake Street Church", "Powderhorn Park"). These are currently optional strings on signal nodes. But places that attract gatherings across multiple tensions are *fourth places* — the project's core concept. A church that hosts immigration vigils, tenant meetups, AND food justice dinners is a gravitational center worth knowing about. That's a first-class node, not a string.

## Why Structural, Not Narrative

Root Signal is a platform. Weaving gatherings into LLM-synthesized prose would bake editorial decisions into text that no downstream consumer can disaggregate. A map app wants gathering pins styled differently from resource pins. A newsletter wants to lead with the human dimension. A dashboard wants to count gatherings vs responses. A "fourth places" feature wants to show which venues are gravitational centers. All of these need the distinction exposed as data, not prose.

## Graph Schema

### New edge: DRAWN_TO

```
Signal ──DRAWN_TO──▶ Tension
  properties:
    match_strength: f64
    explanation: String
    gathering_type: String  (freeform: "vigil", "singing", "solidarity meal", etc.)
```

Replaces the current `RESPONDS_TO` + `gathering_type IS NOT NULL` pattern. The edge type IS the discriminator — no property check needed.

### New node: Place

```
Place
  id: UUID
  name: String                    ("Lake Street Church")
  slug: String                    ("lake-street-church")
  place_type: String              ("church", "park", "community center", "school", etc.)
  location: GeoPoint              (actual coordinates, not city-center)
  city: String                    ("minneapolis")
  created_at: DateTime
  gathering_count: u32            (how many GATHERS_AT edges point here)
  tension_count: u32              (unique tensions from signals that gather here)
```

### New edge: GATHERS_AT

```
Signal ──GATHERS_AT──▶ Place
```

A gathering signal (Event, Give, Ask) that happens at a specific place.

### Updated story weaver queries

Story materialization and growth now union over both edge types:

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

**Implementation note:** Use `type(r)` in the collect payload to tag the edge type explicitly. Extracting `r.gathering_type` from a `RESPONDS_TO` edge returns null — the `CASE` guard makes this explicit rather than relying on silent nulls. The Rust deserializer uses `edge_type` to know whether `gathering_type` is meaningful.

### Story node additions

- `gathering_count: u32` — count of signals connected via `DRAWN_TO` (not `RESPONDS_TO`)

### Consumer query patterns

| Consumer | Query | What it gets |
|----------|-------|-------------|
| Story list / filter | `story.gathering_count > 0` | Stories where people are gathering |
| Story detail | `MATCH (sig)-[r:DRAWN_TO]->(t) RETURN sig, r.gathering_type` | Gathering signals with type |
| Story detail | `MATCH (sig)-[r:RESPONDS_TO]->(t) RETURN sig` | Instrumental responses separately |
| Fourth places | `MATCH (p:Place)<-[:GATHERS_AT]-(sig) RETURN p, count(sig)` | Places ranked by gathering activity |
| Cross-tension places | `MATCH (p:Place)<-[:GATHERS_AT]-(sig)-[:DRAWN_TO]->(t:Tension) RETURN p, collect(DISTINCT t)` | Places that attract gatherings across tensions |
| Map | Place nodes with real coordinates (not city-center) | Pin placement |

## Place Node Design

### Why first-class?

The gravity scout already discovers venues as strings. Promoting them to nodes enables:

- **Cross-tension analysis**: Lake Street Church hosts immigration vigils AND tenant meetups AND food justice dinners → it's a gravitational center for the whole community, not just one issue
- **Map features**: Places have real coordinates, not city-center approximations
- **"Fourth places" feature**: The project's namesake concept — places where community forms around shared tension
- **Temporal patterns**: Which places are gaining/losing gravity over time?

### Dedup

Place names are messy ("Lake Street Church" vs "Lake St Church" vs "Lake Street Church of Christ"). **Slug-only dedup for v1:**
- Normalize: lowercase, strip punctuation, collapse whitespace → slug
- Exact slug match (`lake-street-church`)

**Do NOT use text embeddings for Place dedup.** Embeddings capture semantic meaning, not geographic reality. "First Baptist Church" and "First United Methodist Church" have high cosine similarity (shared tokens, shared context) but are entirely different buildings. Embedding dedup would silently merge distinct venues.

**v1.1 (post-geocoding):** Composite dedup — `slug_similarity AND spatial_distance < 50m`. This catches both name variants of the same building and same-building-different-name cases.

### Coordinate enrichment

The gravity scout gets a venue name string from the LLM. Geocoding (Google/Mapbox API) converts this to real coordinates. If geocoding fails, fall back to city-center with `GeoPrecision::City`. This is a new external dependency but high-value — Place nodes with real coordinates are dramatically more useful than city-center approximations.

If geocoding is too much for v1, Places can start with city-center coordinates and a flag (`geocoded: false`) for future enrichment.

## What Changes

### Gravity scout (`gravity_scout.rs`)
- `create_gravity_edge` → `create_drawn_to_edge` (new edge type in writer)
- Create Place node from `venue` field (with dedup)
- Wire `GATHERS_AT` edge from gathering signal → Place

### Graph writer (`writer.rs`)
- New `create_drawn_to_edge` method (parallel to `create_response_edge`)
- New `create_place` / `find_or_create_place` methods
- New `create_gathers_at_edge` method
- Update `find_tension_hubs` to include `DRAWN_TO` edges
- Update `find_story_growth` to include `DRAWN_TO` edges
- New `get_existing_gravity_signals` to query `DRAWN_TO` edges (currently queries `RESPONDS_TO` with `gathering_type IS NOT NULL`)

### Common types (`rootsignal-common`)
- New `PlaceNode` type
- New `NodeType::Place` variant (or separate — Places aren't signals)

### Story weaver (`story_weaver.rs`)
- Phase A/B queries union over `RESPONDS_TO|DRAWN_TO`
- Compute `gathering_count` from `DRAWN_TO` edge count

### API (`rootsignal-api`)
- Expose `gathering_count` on Story
- Expose `gathering_type` on `DRAWN_TO` edges in story detail
- New Place queries (list, by-city, cross-tension)

### Migration
- Existing `RESPONDS_TO` edges with `gathering_type IS NOT NULL` → convert to `DRAWN_TO` edges
- Extract existing `venue` strings → create Place nodes + `GATHERS_AT` edges

## Key Decisions

- **`DRAWN_TO` not `RESPONDS_TO`**: Semantic honesty. Gatherings are community formation, not problem-solving. The graph should express what the relationship actually is.
- **Place as a node, not a string**: Fourth places are the project's core concept. A venue that attracts gatherings across tensions is structurally interesting — you can't discover that from strings.
- **`gathering_count` on Story**: Convenience counter for list views. Eventually consistent, recomputed in Phase B.
- **Geocoding deferred to v1.1**: Places start with city-center coordinates + `geocoded: false`. Real coordinates come when geocoding API is integrated.
- **Platform-first**: Consumers decide how to present the response/gathering distinction. The graph provides clean, queryable structure.

## Resolved Questions

- **cause_heat through `DRAWN_TO`?** Yes — identical to `RESPONDS_TO`. Both edge types are evidence that a tension is active. The decay difference isn't about the edge type, it's about `is_recurring` on the signal — a recurring weekly vigil sustains heat the same way a persistent legal clinic does. A one-off vigil cools the same way a one-off popup clinic does. No special decay logic for `DRAWN_TO`.
- **Place nodes in vector search?** No. Semantic search on "Lake Street Church" against "immigration anxiety" adds noise. Places are found via graph traversal (`Tension → DRAWN_TO → Signal → GATHERS_AT → Place`) or strict name/geo queries.
- **`SourceRole::Response` → `SourceRole::Gravity`?** Yes. The ontology split should carry through the entire pipeline — graph edges, source roles, and budget tracking.

## Next Steps

`/workflows:plan` for implementation details.
