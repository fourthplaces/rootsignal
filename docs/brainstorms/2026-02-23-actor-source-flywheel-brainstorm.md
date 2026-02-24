---
date: 2026-02-23
topic: actor-source-flywheel
---

# Actor → Source → Signal Flywheel

## What We're Building

A clean flywheel where actors, sources, and signals reinforce each other through eventual consistency across scout runs. Every scout run is an **evidence-gathering pass** — nothing is final after one pass. The core loop: scouts find signals → signals reveal actors (via authorship) → actors link to sources → sources get scraped in future runs → more signals → richer location data → better discovery.

## Core Principle: Every Run Is Evidence Gathering

Location and identity are not facts stamped once — they are **confidence that converges with evidence**. An actor's location, an actor's identity across platforms, which sources matter for a region — all of these become clearer over successive scout runs. The system embraces eventual consistency rather than demanding certainty upfront.

This maps to the same epistemological stance as tension gravity: heat=0 means "not yet understood," and a locationless actor means "not yet placed." Neither is wrong — understanding just hasn't arrived yet.

## Alignment with Vision

### Emergent Over Engineered

The flywheel is deeply emergent. Actor location converges from evidence, not assignment. Identity across platforms emerges from shared links, not name matching. Source relevance to a region is derived from actors and signals that have locations, not stored. Nobody tells the system which actors matter — the graph reveals it.

### Self-Evolving System

The flywheel IS the self-evolving feedback loop applied to actors. The vision says: *"The system discovers its own sources by analyzing what it already knows."* That's exactly what `author_actor → HAS_SOURCE` does — signals reveal the source's owner, and the graph grows.

### Antifragile

Source-scoped actors resist name-based gaming. You can't create a fake "Friends of the Falls" that auto-merges with the real one. Triangulated location means one fake signal can't relocate an actor. Evidence-based merging (stage 2) means you can't fake institutional depth across platforms. Each run makes the system more accurate, not just bigger.

### Trust Propagates Through the Graph

Trust doesn't need a propagation algorithm — it needs **good seeds**. If the bootstrap seeds are trusted anchors (real orgs with verified presence), the flywheel naturally expands through their social graph:

```
Trusted Actor A (seeded) → HAS_SOURCE → Source A
  → Signal: "Partnering with Friends of the Falls on cleanup"
    → Friends of the Falls discovered as author on THEIR source
      → one hop from a trusted anchor
        → THEIR signals mention other orgs...
          → trust propagates through the social graph
```

Actors closer to trusted anchors (fewer hops) are implicitly more trustworthy because real community orgs reference real community orgs. Astroturfed actors aren't referenced by anyone in the trust graph. The graph topology IS the trust model.

Actor trust follows the same principle as source trust: **trust is evidence**. An actor discovered from a bootstrap web query starts with low trust, same as a newly discovered source. The investigation loop should eventually examine it — is this a real 501(c)(3)? Does it have institutional depth? Evidence accumulates, trust rises, the actor's sources get scraped more confidently.

## Key Design Decisions

### Entities with location: Actors, Signals, Pins (not Sources)

Sources don't have locations — they're just "a place to fetch data from" (a URL, a query, a feed). A single source (e.g., a statewide news RSS feed) could be relevant to multiple regions.

Region membership is derived, not stored. A scout finds its work by:
- `find actors in bbox` → follow `HAS_SOURCE` → those sources
- `find signals in bbox` → follow `SOURCED_FROM` → those sources
- `find pins in bbox` → their sources

### Actors are source-scoped (MVP)

An actor is derived from **one source's profile**. If "Friends of the Falls" posts on Facebook and Instagram, those are two separate actor nodes until proven otherwise.

```
Source A (facebook) → Actor: "Friends of the Falls" (entity_id: facebook.com/friendsofthefalls)
Source B (instagram) → Actor: "Friends of the Falls" (entity_id: instagram.com/friendsfalls)
```

Entity ID = source `canonical_value` (the normalized URL). E.g., `instagram.com/friendsfalls`, `facebook.com/friendsofthefalls`, `friendsofthefalls.org`. The archive already normalizes these via `canonical_value()`. No prefix vocabulary needed — the URL IS the identity.

Each actor has exactly one `HAS_SOURCE` edge. No auto-merging on name alone. This is honest — we **know** that profile authored those signals.

### No mentioned actors (MVP)

Actors come from `author_actor` only. Mentioned names stay as text metadata on signals for search purposes. Rationale:
- Mentioned actors have no verified identity (just a name the LLM pulled out)
- They have no source to scrape, so they don't feed the flywheel
- They pollute the actor pool and make merging harder later
- Their location is inherited from the signal, not verified
- The only value (aggregated mention counts) can be derived from signal text without actor nodes

### Actor location is a living, triangulated value

Actor location is not stamped once — it reconverges each run as new evidence arrives. Multiple evidence types contribute with different weights, and **recency decays** — a 2022 signal saying "We're in Phillips" carries less weight than a 2026 signal saying "We moved to Powderhorn."

| Evidence | Weight | Notes |
|---|---|---|
| Actor bio states location + corroborated by signal | Highest | Self-declared AND confirmed by evidence |
| Multiple recent signals cluster in same area | High | Convergent pattern, recency-weighted |
| Actor bio states location (uncorroborated) | Low | Unverified claim — treated as 1 signal |
| Single signal's about_location | Low | Could be one-off topic |
| Older signals | Decaying | Still contribute but fade over time |
| No location anywhere | Zero | Actor stays locationless until evidence arrives |

**MVP heuristic** (simple, no weighted centroid math):
1. Compute signal mode (most frequent recent `about_location`)
2. If actor bio states a location AND at least 1 signal corroborates → bio wins (highest weight)
3. If bio uncorroborated → bio counts as 1 data point, may be overridden by signal mode
4. Recalculate once at the end of each scout run

**Why corroborated-only for bio:** A bio is a self-declared, unverified claim. Requiring at least one signal to corroborate prevents bio-only location spoofing while still giving bio the highest weight when it IS corroborated.

Example triangulation:
```
Signal A (2025): about_location = Phillips           (old, decaying)
Signal B (2026): about_location = Powderhorn         (recent, strong)
Signal C (2026): about_location = Powderhorn         (recent, reinforcing)
Actor bio: "Based in South Minneapolis"              (strongest signal)
                    ↓
        Triangulated: South Minneapolis, high confidence
```

An actor becomes "visible" to `find_actors_in_region` once confidence crosses a threshold. Until then it exists but doesn't drive discovery — which is honest.

### Source type distinction (derived from structure, not stored)

We already know ownership at scrape time — the source's structure tells us:
- `SocialMedia` / `Website` → **owned** (it's a single entity's profile or site) → actor gets `HAS_SOURCE` edge
- `RssFeed` / `SearchQuery` / `EventPlatform` → **aggregated** → no `HAS_SOURCE`, authors stay as text

No `source_type` field on SourceNode. The scraping strategy already encodes the structural distinction. No author-counting heuristics needed.

Only owned sources get `HAS_SOURCE` edges. For a news article, the `author_actor` is a journalist, not the news outlet — creating `HAS_SOURCE` from journalist → news RSS feed would be wrong. Actor-to-source linking only makes sense when the actor IS the entity behind the source.

### Discovery depth (flywheel brake)

Every actor has a `discovery_depth` tracking hops from a bootstrap seed: 0 = seed, 1 = discovered from seed's signals, etc. Actors beyond depth N cannot trigger further source discovery — their source still gets scraped (signals flow), but outbound links don't create new sources. This is the structural brake on flywheel amplification and limits the blast radius of a successful injection.

## Graph Edges

### `HAS_SOURCE`: Actor → Source

Created during signal extraction when `author_actor` is resolved on an owned source. Links the actor to the source they publish from.

### `PRODUCED_BY`: Signal → Source (new)

Every signal gets an edge back to the source it was extracted from. Enables reverse traversal: find signals in a region → follow `PRODUCED_BY` → discover which sources are producing relevant content for that area. (Named `PRODUCED_BY` instead of `SOURCED_FROM` to avoid collision with the existing `SOURCED_FROM` edge type used for Signal → Evidence provenance.)

### `ACTED_IN`: Actor → Signal

Existing edge with role (e.g., "authored"). In MVP, only created for `author_actor`, not mentioned actors.

## The Flywheel In Action

**Pass 1 (bootstrap):**
```
Seed trusted anchors (human-curated orgs with verified presence)
  → their sources scraped → signals extracted
  → author_actor identified → HAS_SOURCE edge created
  → SOURCED_FROM edge: Signals → Source
  → Actor gets initial location from signal's about_location (low confidence)
  → Signals mention other orgs → discovery queries generated
```

**Pass 2 (next scout run):**
```
find_actors_in_region(bbox)
  → finds seeded actors + newly discovered actors with location evidence
  → follows HAS_SOURCE → their sources
  → scrapes sources → new signals → actor location confidence grows
  → actor bio parsed → "Based in South Minneapolis" → high confidence
  → signals reference new orgs → their sources discovered
  → trust expanding outward from seeds through the social graph
```

**Pass 3+:**
```
Actors have strong locations → reliably appear in region queries
  → all linked sources scraped every run → flywheel spinning
  → more signals → richer map → more actors discovered
  → each new actor is N hops from a trusted seed
  → the further from seeds, the lower implicit trust
  → investigation loop verifies institutional depth as needed
```

## Pins: One-Shot Search Instructions

A pin attaches a location to a source — nothing more. It's a **one-shot instruction**: "search this source at this location next run." Once executed, the pin is dropped. If the run produces actors and signals, the flywheel takes over organically. If it produces nothing, the pin is gone anyway — no clutter.

**Data model** (minimal by design):
- `location` (lat/lng)
- `source` (reference to a SourceNode)
- `created_by` (scout run ID or "human")

No weight, no cadence, no history. Pins are consumed on use.

**Use cases:**
- Bootstrap web queries that need a region context (e.g., "Minneapolis mutual aid" → search for Minneapolis)
- Mid-run discovery: gap analysis generates a new query but hasn't scraped it yet — pin it for next run
- Aggregator sources scoped to a sub-region (e.g., Eventbrite URL → monitor for South Minneapolis)
- Human curation: "search this RSS feed for North Minneapolis"

**Lifecycle:**
1. Scout (or human) creates pin: location + source
2. Next scout run picks up pins in its bbox
3. Source is scraped in that location context
4. Pin is dropped after execution
5. If scraping produced actors/signals with locations → flywheel takes over
6. If scraping produced nothing → pin is gone, no residue

## Stage 2: Actor Merging Via Shared Links (Future)

Not needed for MVP. The idea: an actor investigation step looks at actors linked to sources and follows outbound links. If two source-scoped actors share the same authoritative link (e.g., both point to `friendsofthefalls.org`), we scrape that site, confirm the org name matches, and merge the actors into a canonical entity.

```
Actor A (facebook) ──authors──→ Signal: "Visit friendsofthefalls.org"
Actor B (instagram) ──authors──→ Signal: "New post on friendsofthefalls.org"
                                          ↓
                                  shared outbound link
                                          ↓
                              Scrape site → confirm org name
                              → merge A + B into canonical Actor
                              → canonical Actor HAS_SOURCE: [facebook, instagram, website]
```

Evidence-based merging, not name matching.

## Resolved Questions

- **`entity_id` format**: Use source `canonical_value` (the URL) directly. E.g., `instagram.com/friendsfalls`, `friendsofthefalls.org`. The archive already normalizes URLs to stable identifiers via `canonical_value()`. No prefix vocabulary needed. The URL IS the identity — globally unique, immediately verifiable.
- **Actor nodes only for owned sources**: Aggregator source authors (journalists, event organizers) are text metadata, not actor nodes. Same treatment as mentioned actors. An actor IS the entity behind a URL.
- **Mentioned actors**: Don't delete extraction code — demote it. Store mentioned names as string array on Signal node (`mentioned_actors: Vec<String>`). Keeps graph clean, keeps data searchable for future discovery.
- **Triangulation implementation**: MVP heuristic — bio > mode of recent signal locations > nothing. Recalculate end of scout run.
- **Future @mention discovery**: When we want to crawl mentioned profiles, `archive.source("instagram.com/username")` provides the same identity system. The URL-as-identity model extends naturally.

## Open Questions

- How does `find_actors_in_region` change to follow `HAS_SOURCE` instead of `HAS_ACCOUNT`?
- How to distinguish owned vs aggregator sources — explicit flag on SourceNode, or inferred from scraping strategy?
- How are bootstrap pins created? Automatically from seed queries, or manually?
- What's the confidence threshold for an actor to become "visible" to region queries?

## Next Steps

→ `/workflows:plan` for implementation of MVP flywheel (HAS_SOURCE edge, SOURCED_FROM edge, source-scoped actors, living actor location, drop mentioned actors, pins)
