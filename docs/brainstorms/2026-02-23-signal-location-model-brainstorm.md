---
date: 2026-02-23
topic: signal-location-model
---

# Signal Location Model: FROM vs ABOUT

## The Problem

Signals currently have ONE location concept (`location: Option<GeoPoint>`, `location_name: Option<String>` on `NodeMeta`). This conflates two fundamentally different things:

1. **FROM** — where the actor/author is based (posted FROM)
2. **ABOUT** — the geographic subject of the content (posted ABOUT)

A Minneapolis mutual aid org posting "check out our new volunteer handbook" has no ABOUT location, but is clearly FROM Minneapolis. Today, this signal gets killed by the geo-filter because it has no coords and no location name.

A national org posting "Minneapolis food shelf needs help at 123 Main St" has an ABOUT location (Minneapolis) but FROM somewhere else. Today, both concepts collapse into the same field and origin info is lost.

Additionally, `source_finder.rs` stamps scout region center coordinates on every discovered source — baking ephemeral scout data into permanent source records.

## Architectural Principles

### Scout is ephemeral, signal data is permanent

- A scout's region, center, radius are **transient search parameters** that can change
- A signal's location data (FROM and ABOUT) is **intrinsic to the signal** itself
- No "distance from scout center" stored on signals, sources, or actors
- No "in-region" flag baked into any data
- Whether a signal appears in results is a **query-time calculation**, not a scrape-time decision
- ALL data (signals, sources, actors) must be decoupled from where a scout looks

### Location must be explicit — never inferred from scout

Location comes ONLY from:
- **Content** — LLM extracts location from the text/post
- **Actor** — known actor profile has coords

NEVER from the scout's region or query. Example: Scout searches for "Germany beerfest", finds a Florida beerfest page that doesn't mention Florida. We do NOT assume it's in Germany. No content location, no actor → location unknown.

### Location is retroactive

Signals and sources don't need location at scrape time. A signal scraped today with no FROM can get a FROM location next week when we discover the actor. A source discovered today gets its location when its actor is identified. The store-everything approach enables this — nothing is rejected for missing location.

## Key Decisions

### 1. Two separate location fields on signals

- `about_location: Option<GeoPoint>` — the canonical query/display field (rename from `location`)
- `about_location_name: Option<String>` — human-readable content location (rename from `location_name`)
- `from_location: Option<GeoPoint>` — actor's coords, stored as provenance (NEW field)

Both fields stored independently. `from_location` is provenance — it tells you whether the map pin came from content or actor inference.

### 2. about_location is the single query field

`about_location` is what goes on the map. It's the only field queries use. At **write time**:

- LLM extracted a content location? → `about_location` = content coords
- No content location, but actor (from_location) is known? → SET `about_location` = `from_location`
- Neither? → `about_location` = None (signal has no map pin until location is discovered retroactively)

This means: queries only use `about_location`. No read-time fallback logic needed. Simple.

`from_location` is always the actor's coords regardless — it's provenance, not a query field.

### 3. Explicit ABOUT overrides FROM

A local actor posting about a Dallas event (explicit Dallas coordinates): `about_location` = Dallas. The signal does NOT show up in Minneapolis map results even though `from_location` = Minneapolis. Content location is authoritative.

### 4. Remove scrape-time geo-filter rejection

The current geo-filter runs after LLM extraction and REJECTS signals based on geography. This is wrong:

1. **The expensive work (LLM extraction) already happened** — rejecting after extraction doesn't save the main cost
2. **geo_terms string matching is brittle** — `["Minneapolis", "Hennepin County"]` misses "Mpls", "Uptown", "Twin Cities", neighborhood names, venue names
3. **It permanently destroys signals** — a rejected signal is gone forever
4. **It couples signal data to scout configuration** — signals should not know about scouts

**New approach:** Store all extracted signals with their intrinsic location data. Query layer handles geographic relevance at read time.

### 5. Source location is redundant — location lives on actors and signals

**Problem found:** `source_finder.rs` stamps `source.center_lat = Some(self.center_lat)` where `self.center_lat` is the scout's region center. Every discovered source gets the scout's coordinates as fake location data.

**New model:** Source location is unnecessary as a concept. Geographic relevance is expressed through:
- **Signals** — `about_location` on the map
- **Actors** — actor's own location (the FROM)

You find sources on a map by their signals or their actors, not by source coordinates.

**The flow:**
1. Source discovered (via search, promotion, etc.)
2. Source scraped → produces interesting content?
3. If yes → find/create actor for this source (next run or this run)
4. Actor attached to source → now we know the FROM location
5. Signals from this source already have `about_location` (from content) or inherit from actor

`SourceNode.center_lat/center_lng` becomes unnecessary. Sources don't need intrinsic coordinates — they're just URLs. Their geographic meaning comes from their actors and the signals they produce.

### 6. Web sources CAN have actors

A food shelf website is "owned" by the food shelf org. That IS an actor. We don't need to find the actor unless the source is producing interesting content. Actor discovery happens asynchronously — the website is scraped first, produces a signal, the actor is identified later, and the FROM location is attached retroactively.

### 7. LLM should resolve neighborhood names to approximate coords

Current extraction prompt (`extractor.rs:516-521`) tells the LLM to leave lat/lng null for non-specific locations. But the LLM already knows the city context (`city_name` is in the prompt header). Update the prompt:

- "If you can identify a neighborhood or area within {city_name}, provide approximate coordinates"
- "Uptown" in Minneapolis context → ~(44.95, -93.30) with precision "neighborhood"
- This reduces the gap where signals have a location name but no usable coords

### 8. StoryNode/SituationNode centroids use about_location

Story and Situation centroids are computed from member signals' locations. These should use `about_location` (the canonical query field). Since `about_location` already incorporates the FROM fallback at write time, centroids naturally reflect the best available location data.

### 9. Beacons (future concept)

Out-of-region signals that are "interesting" could trigger beacons — kickoff discovery in that specific area. The store-everything approach enables this naturally since we no longer destroy out-of-region signals.

### 10. Geographic neutrality in extraction

The "Paris Trip Problem": a Minneapolis actor posts "Thinking about my trip to Paris!" and the LLM fails to extract Paris → `about_location` falls back to Minneapolis → signal pinned to wrong city.

Mitigation: the extraction prompt must explicitly handle geographically neutral content. If no place is mentioned or implied, location should be null — not guessed. The write-time fallback only fires when the LLM genuinely finds no geographic signal in the content.

### 11. Neighborhood coordinate consistency (future)

LLM-generated coords for "Uptown" will vary slightly across calls. Future refinement: post-extraction lookup table (GeoJSON of city neighborhoods) assigns canonical centroids. Not a blocker — LLM approximate coords cluster within ~0.5km.

### 12. Crawl budget for out-of-region sources (future)

Without geo-filter rejection, scouts may keep scraping sources that consistently produce out-of-region content. Add `consecutive_out_of_region_runs` counter to source scheduling metrics. Deprioritize after N irrelevant runs. This is a scheduling optimization, not a data integrity issue.

## Resolved Questions

- **Migration:** Rename `location` → `about_location` in Rust only. Neo4j property names (`lat`, `lng`, `location_name`) stay unchanged. Add `from_lat`/`from_lng` as new properties.
- **`from_location` precision:** Always `Approximate` — actor profiles are inherently city/area-level.
- **Source scoping:** Replace `center_lat`/`center_lng` with `region_slug` on SourceNode. Scout loads sources by slug match, not geographic bounding box.

## Next Steps

→ Implementation plan at `.claude/plans/purrfect-squishing-wombat.md`
