---
date: 2026-02-20
topic: region-based-scout
---

# Region-Based Scout: From City Slugs to Pin Drops

## The Idea

Replace the city-first scout model with a coordinate-first model. Instead of "scout Minneapolis," the primitive becomes "scout this point on the map." The system discovers what's there and adapts its behavior accordingly.

**Current flow:** City slug → hardcoded templates → sources → signals

**Proposed flow:** Coordinates → characterize region → contextual discovery → sources → signals

## Why

The current scout is tightly coupled to the concept of a city:

- **Canonical keys** are prefixed with city slug (`minneapolis:reddit.com/r/housing`)
- **Bootstrap templates** are hardcoded for urban contexts (`site:gofundme.com/f/ {city_name}`)
- **Source finder fallbacks** embed the slug directly (`{help_text} resources services {city_slug}`)
- **Discovery prompts** assume a city name exists
- **Lock keys** and **metric filters** are slug-scoped

This means scout literally cannot operate on a non-city location. But there's no fundamental reason for this — the gathering finder already queries by lat/lng bounds derived from city center + radius. That pattern generalizes.

## The Model

### Two distinct concepts: regions and data

**Regions are sandboxes for scout's attention.** A region defines where scout looks — center coordinates, radius, characterization, discovery strategies. It drives scheduling, locks, metrics, and bootstrap. Regions live in the database as the unit of work.

**Sources and signals are coordinate-anchored, region-independent.** A source's identity is its URL or content hash — not which region found it. A signal's location is its coordinates — not which region's scout extracted it. Data exists in the graph on its own terms.

**Regions drive discovery. Coordinates drive data.**

This means: a source discovered by the Minneapolis region isn't "owned by" Minneapolis. It exists in the graph at its coordinates. If an Uptown region's bounding box overlaps, it sees the same source and signals without duplication. Scout scrapes the source once. Multiple regions can benefit from it.

### Region as the primitive

A **Region** is:
- Center coordinates (lat, lng)
- Radius in kilometers
- A generated slug (for scheduling, locks, metrics — operational scoping, not data ownership)
- A characterization (discovered, not configured)

### The flow

1. **Pin drop** — user or system provides coordinates + radius
2. **Characterize** — reverse geocode + LLM determines what this region is: a city, a rural area, a body of water, a desert, a national park, etc.
3. **Contextual discovery** — instead of hardcoded urban templates, the LLM generates search strategies appropriate to the region type. For Minneapolis: community orgs, local news, subreddits. For the Great Pacific Garbage Patch: environmental research, shipping data, cleanup organizations, scientific monitoring.
4. **Source creation** — sources stored with their own identity (URL/content hash), not tagged with region slug. Region records which sources it discovered (for tracking coverage), but the source node itself is region-independent.
5. **Signal extraction** — unchanged, already coordinate-aware. Signals land in the graph at their coordinates.

### What stays the same

- Lat/lng bounding box queries (gathering finder already does this)
- Lock and metric scoping (region slug is still the operational partition key)
- The scrape → extract → store pipeline
- Embedding dedup (now global rather than region-scoped — same URL/content is the same source regardless of who found it)

### What changes

- **Canonical keys** — drop the region prefix. Source identity is URL/content hash, not `region:url`. This is the core data model change.
- **Bootstrap** — replace hardcoded search templates with LLM-generated discovery strategies based on region characterization
- **Source finder** — remove mechanical fallback templates that assume urban context; let the LLM generate contextually appropriate queries
- **CityNode → RegionNode** — same fields (center_lat, center_lng, radius_km, slug, name) but name and slug are derived from characterization rather than preconfigured
- **Discovery prompts** — parameterized by region type, not just city name
- **Config** — accept coordinates + radius instead of requiring a city slug lookup
- **Querying** — "show me signals in this area" becomes a spatial query on coordinates, not a string filter on a slug prefix

## What This Unlocks

- **Any geography** — cities, rural areas, watersheds, coastlines, disaster zones, national parks
- **Overlapping regions** — a neighborhood-scale pin inside a city-scale pin, each with appropriate discovery strategies
- **Dynamic region creation** — drop a pin on a news event location, scout spins up automatically
- **No preconfiguration** — you don't need to define cities in the database before scouting them

## Pressure Test

### 1. Characterization Failures

**What if the LLM gets it wrong?** Drop a pin on the edge of Minneapolis — is it "Minneapolis," "Richfield," "the MSP metro," or "suburban Hennepin County"? The characterization step is non-deterministic. Two pin drops 500 meters apart could produce different characterizations.

**Does it break the system?** No — and this is key. The characterization drives discovery strategy, not data storage. If the system characterizes a pin as "Richfield" instead of "south Minneapolis," it generates slightly different search queries, but the lat/lng bounding box catches the same physical area regardless. The signals found are coordinate-anchored, not name-anchored. Bad characterization means slightly worse discovery, not wrong data.

**Anti-fragile angle:** If characterization is wrong, the system discovers fewer sources initially. But the expansion loop (existing signals → new sources) corrects over time — real activity in the bounding box gets found regardless of what the system called the area. The name is a hint for bootstrap, not a constraint on what gets captured.

### 2. The Empty Region Problem

**Drop a pin in the middle of the Sahara.** There's no reverse geocode result, no community, no sources, nothing. What happens?

The system characterizes it as "uninhabited desert," generates search strategies accordingly (geological surveys, satellite monitoring, nomadic routes, climate research), and probably finds very little. That's fine — "nothing is happening here" is a valid answer. The system shouldn't hallucinate activity where there is none.

**The real risk:** The system spends resources (API calls, LLM tokens) on regions that will never produce signal. Need a "minimum viability" check — if bootstrap produces zero sources after characterization, park the region as dormant rather than running full scout cycles on nothing.

### 3. Adversarial Region Creation

**Someone drops thousands of pins across a city to map sensitive activity at fine granularity.** Each pin creates a small-radius region. By aggregating results across overlapping micro-regions, an adversary builds a high-resolution map of who's doing what where.

**This is the same threat as the current model.** The adversarial threat model (see `docs/vision/adversarial-threat-model.md`) already addresses this — geographic fuzziness for sensitive signal, aggregate-only heat maps for sensitive domains, no actor timelines. Region-based scouting doesn't change the attack surface because the public API's mitigations apply regardless of how the data was collected. The scout's internal representation uses precise coordinates; the public surface does not.

**But:** Dynamic region creation does mean the system could be pointed at arbitrary locations more easily than the current model (which requires pre-configured cities). If region creation is user-facing, it needs the same access controls as any write operation. If it's system-driven (e.g., triggered by news events), the trigger mechanism needs to be robust against manipulation.

### 4. Overlapping Regions and Source Ownership

**Minneapolis (15km radius) and Uptown (2km radius) overlap.** A source like `reddit.com/r/Minneapolis` is relevant to both. Who owns it?

**Option A: Sources belong to one region.** Simple but wrong — the subreddit serves the whole city, not just whichever region claimed it first.

**Option B: Sources can belong to multiple regions.** The canonical key includes the region slug, so the same source gets separate entries per region (`minneapolis:reddit.com/r/Minneapolis` and `uptown:reddit.com/r/Minneapolis`). This means duplicate scraping and duplicate signals.

**Option C: Sources are region-independent, signals are coordinate-anchored.** Sources exist in the graph without region ownership. Signals extracted from sources have coordinates. Regions are just query scopes — "show me all signals within this bounding box." This is the cleanest model and aligns with how gathering finder already works.

Option C is probably right. It means the canonical key drops the region prefix entirely and becomes content-addressed (URL or content hash). Region slug becomes a query-time filter, not a storage-time partition. This is a bigger architectural change but a simpler model.

### 5. Discovery Strategy Quality for Unfamiliar Regions

**The LLM has strong priors about cities but weak priors about obscure locations.** Ask it for search strategies for Minneapolis and you get subreddits, local news, community orgs. Ask it for search strategies for the Mariana Trench and you might get generic oceanography results.

**Does it matter?** Somewhat. The system is only as good as its discovery. But this is self-correcting: weak initial discovery → few sources → few signals → expansion loop searches for more → discovery improves over time. The system doesn't need perfect bootstrap. It needs a foothold.

**Mitigation:** The characterization step should produce not just "what is this place" but "what kinds of information sources exist for this type of region." Oceanographic regions have research stations, monitoring buoys, shipping data, international policy bodies. Wilderness regions have park services, conservation orgs, trail communities. The LLM knows these patterns — it just needs to be prompted for them explicitly.

### 6. Radius Manipulation

**Tiny radius (100m) to pinpoint a specific building.** Could be used to identify exactly which organization is producing signal from a specific address.

**Huge radius (5000km) to vacuum up everything.** Creates a region that covers half a continent, producing so many sources that the system can't process them meaningfully.

**Mitigations:**
- Minimum radius floor (probably 1-2km) — prevents building-level targeting
- Maximum radius ceiling (probably 50-100km) — prevents continental vacuuming
- These are the same kinds of constraints the current model has implicitly (city radius is typically 10-30km)

### 7. Canonical Key Migration

**Existing data uses city-slug-prefixed canonical keys.** If we move to Option C (region-independent sources), every existing canonical key needs migration. `minneapolis:reddit.com/r/Minneapolis` becomes `reddit.com/r/Minneapolis`.

This is a one-time data migration, not an ongoing complexity. But it touches every source node in the graph. Needs to be done carefully — the canonical key is the dedup key, so any mismatch during migration means duplicate sources.

### 8. Does This Get Stronger Under Stress?

**Yes, in the same ways the current model does.** Triangulation, entity emergence, source replacement — all coordinate-based, not city-based. The region model doesn't weaken any existing anti-fragile properties.

**New anti-fragile property:** Dynamic region creation means the system can respond to events geographically. A disaster happens → pin drop at the location → scout spins up → signal captured in real time. The current model can't do this without someone pre-configuring a city. The region model lets the system grow toward where stress is happening, which is textbook anti-fragile behavior.

**New anti-fragile property:** Overlapping regions at different scales mean the system captures both neighborhood-level and city-level patterns simultaneously. When local organizations go underground (per the anti-fragile signal brainstorm), the city-scale region still captures the macro pattern while the neighborhood-scale region captures the micro displacement. More resolution under pressure.

## Verdict

The region model is architecturally sound. It doesn't introduce new attack surfaces (same public API mitigations apply), it generalizes cleanly (coordinates are more fundamental than city names), and it adds anti-fragile properties (dynamic region creation, multi-scale observation).

The biggest design decision is source ownership (Options A/B/C above). Option C (sources are region-independent, regions are query scopes) is the cleanest but requires canonical key migration and a rethink of how partitioning works in the graph layer.

## Open Questions

- **Slug generation** — geohash vs. derived name vs. something else? Geohash is deterministic from coordinates but not human-readable. Derived name is readable but requires the characterization step first.
- **Source ownership model** — Option C (region-independent sources) is cleanest but biggest change. Worth prototyping to validate.
- **Characterization stability** — if you drop a pin on the edge of a city, does the characterization change depending on slight coordinate shifts? Might need a "confirm characterization" step. Pressure test suggests this is low-risk since characterization drives discovery hints, not data boundaries.
- **Radius selection** — who decides the radius? User? Automatic based on characterization? (A city might suggest 15km, the garbage patch might suggest 500km.)
- **Dormancy threshold** — how many bootstrap sources constitute "minimum viability" before a region is worth scouting?
- **Region creation access control** — if dynamic region creation is user-facing, what prevents abuse? Rate limiting? Approval queue?
