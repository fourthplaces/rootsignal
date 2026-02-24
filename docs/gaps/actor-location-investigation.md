---
date: 2026-02-23
topic: actor-location-investigation
status: identified
related: docs/brainstorms/2026-02-23-actor-source-flywheel-brainstorm.md
---

# Gap: Actor Location Investigation & Query-Time Derivation

## The Problem

Some actors have no location evidence in their bio or signals. Their location may be 2-3 hops away (bio link → website → /contact page → address). Without location, the actor stays invisible to `find_actors_in_region` and can't fully participate in the flywheel.

## Current Mitigation (MVP)

- Most social accounts have some location signal (bio or post content)
- Pins keep locationless actors' sources scraped
- `SOURCED_FROM` edges mean sources stay reachable through other signals in the region
- Actor location triangulation (bio > mode of recent signals) handles the common case

## The Investigation Step (Stage 2)

Trigger: actor has N signals but no location after M runs.

The existing investigation loop (from self-evolving system vision) can be pointed at actors:
- Follow actor bio link → crawl website → find /contact or /about → extract address
- Follow linked accounts → check bio on other platforms
- Web search "Org Name + City" → extract address from results

Same mechanism as source investigation ("is this real?"), different question ("where is this?").

## The Solution: Query-Time `from_location`

**Don't store `from_location` on signals. Derive it from the actor at query time.**

Currently `from_location` is snapshotted onto `NodeMeta` at write time via `score_and_filter()`. But if actor location is a living, triangulated value that reconverges each run, storing it on signals creates stale snapshots that fight the model.

Instead, `from_location` becomes a graph traversal:
```
Signal → ACTED_IN ← Actor → actor.location
```

Benefits:
- Actor location updates are instantly reflected on all their signals
- No backfill step when actor location changes
- No stale data
- Map queries become: signals where `about_location` in bbox OR author's current location in bbox
- Makes space for "where is this actor?" queries naturally — actor.location is always the living, triangulated value

Trade-off:
- Query is a join instead of a field read (slightly more expensive)
- Requires actor → signal edges to be reliably present (MVP already ensures this for `author_actor`)

## Implementation Notes

- Remove `from_location` from `NodeMeta` write path in `score_and_filter()`
- Add graph query pattern: `MATCH (a:Actor)-[:ACTED_IN]->(s:Signal) WHERE a.location_lat IS NOT NULL` for region-based signal discovery via actor location
- Actor location field on `ActorNode` remains the living, triangulated value (bio > mode of recent signals, recalculated end of run)
