---
date: 2026-02-20
topic: demand-driven-scout-swarm
---

# Demand-Driven Scout Swarm: Killing Regions

## The Idea

Eliminate predefined regions entirely. Scout coverage becomes emergent,
driven by two inputs: (A) user search queries and (B) global tension
detection. The concept of "region" dissolves — scout takes arbitrary
geographic scopes from a task queue, swarming wherever there's demand
or heat.

## Why Regions Are Wrong

Regions exist today solely to scope scout to a specific area. But
predefined regions require someone to manually decide "add Detroit."
That doesn't scale and it doesn't reflect actual demand.

The reading side (search-app) already doesn't need regions —
`signals_in_bounds` queries by lat/lng bounding box at any zoom level.
If signals exist at those coordinates, they show up. The map already
works at world scale; it's just empty where scout hasn't run.

## Two Scout Drivers

### A) User Query Demand

User searches become scout fuel. When someone searches "housing crisis
in Detroit," that query gets geocoded and ranked. Popular search areas
get scout attention. The "region" is emergent from user behavior.

- Rank user queries by frequency and recency
- Geocode them into center + radius
- Enqueue as scout tasks
- More searches in an area → more scout resources allocated there

### B) Global Tension Scanning (The News)

Scout does broad web queries with no geographic scope — scanning for
what's hot globally. RSS feeds, AP wire, Reddit front page. A wildfire
in California, a factory closure in Ohio, a policy change affecting a
state — scout discovers these organically, geocodes them, and swarms
on them.

- Lightweight pre-phase: broad tension queries (no geo-filter)
- Detect geographic signal from results (LLM extraction already geocodes)
- Enqueue as scout tasks with inferred center + radius
- This is scout discovering regions on its own

**The news eliminates the cold start problem.** Day one, zero users:
scout scans global news → finds tensions with geographic signal →
indexes them → map has content → users arrive → Driver A kicks in.
There is no cold start. The world is always generating signal.

## What Changes

**Scout itself barely changes.** It still takes "a geographic area +
context" as input and runs the same pipeline (bootstrap → scrape →
dedupe → enrich → discover). It just stops caring where the task
came from.

**RegionNode becomes ephemeral.** Instead of a permanent entity in the
graph, it becomes a scout task with a center point, radius, and
context. Tasks come and go based on demand.

**Task queue replaces region registry.** A ranked queue of geographic
scopes to investigate, fed by user queries and global scanning. Scout
workers pull from the queue.

## Why This Works With Current Architecture

- `signals_in_bounds` and `stories_in_bounds` already query by lat/lng
  — no region concept needed on the read path
- Search-app viewport queries already work at any zoom level
- Tensions and heat are already computed globally
- Scout already takes a center + radius + geo_terms as input

## Stress Test

### Popularity bias?

User-driven coverage (Driver A) alone would create a rich-get-richer
dynamic — affluent, connected communities generate more searches and
get more coverage. But Driver B (global scanning) finds signals
regardless of user demand. A local news article about lead pipes in a
small town contains the geography. Scout doesn't need someone to
*search for Flint* to find Flint — it just needs "water contamination"
to surface as a tension globally.

Signal is signal. Small signals aren't lost — they're in the graph with
a lat/lng and a tension and a heat score. The question is whether users
can *find* them. That's a display problem, not a discovery problem.

### The silent crisis?

Driver B handles this. Broad tension scanning without geographic scope
surfaces local stories that contain geographic signal. The LLM
extraction already geocodes results. Scout finds Flint not because
someone searched for Flint, but because "water contamination" is a
tension and a local article about lead pipes shows up in the scrape.

### Gaming / manipulation?

Finite scout budget means flooding queries has diminishing returns.
Driver B operates independently of user input entirely, providing a
manipulation-resistant baseline of coverage.

### Signal staleness?

Operational detail, not architectural flaw. Solvable with decay scores
or TTLs on signals. When scout re-visits an area (triggered by either
driver), stale signals get refreshed or aged out.

### Budget concentration?

Scheduling problem. When Driver B surfaces 500 hotspots simultaneously,
the task queue prioritizes by a blend of tension heat, coverage gaps,
and user demand. Solvable, not structural.

## Signals as Beacons: The Feedback Loop

Signals are both scout's output and its input. The cycle:

1. **Driver B** does a broad scan (e.g., "water contamination")
2. Signals land in the graph with lat/lng coordinates
3. Signals cluster geographically — a handful near Flint, MI
4. That cluster is heat — a beacon telling scout where to go deeper
5. Scout enqueues a tighter task for that area (smaller radius, more sources)
6. More signals come back → more heat → more scouting
7. Until the tension is well-covered and marginal return drops

Signals aren't just data points on a map — they're scout's own
breadcrumbs telling it where to look next.

### Geographic Cadence

Today, cadence is per-source (`cadence_hours` on `SourceNode`) — how
often a single source gets re-scraped. That's the wrong level.

With demand-driven swarming, cadence moves to the *geographic* level —
how often does scout revisit an area? This is derived from signal heat
and recency:

- **High density + recent signals** → area is hot → revisit frequently
- **High density + stale signals** → area was hot → revisit to check
  if still active
- **Low density + no new activity** → area is quiet → deprioritize

The task queue scores areas by signal heat × recency and keeps cycling.
This also solves the staleness problem — areas don't go stale because
the feedback loop keeps scout coming back as long as there's heat.

## Three User Levers on Signal

The display layer gives users three orthogonal dimensions to explore:

### 1. Where — Map Viewport
Zoom to see the whole world, then drill into a country, state, city.
The map works at every scale because signals are just lat/lng points
queried by bounding box.

### 2. What — Cause Tags
Users pick the causes they care about: housing, water quality,
immigration, policing. Tags filter signal by topic regardless of
geography or volume. Tag popularity also feeds back into Driver A —
if many users tag "water quality," that's signal about what tensions
to prioritize scouting for.

### 3. How Loud — Heat Scrubber
A slider from "trending" to "whispers."

- **Slide toward loud:** high-volume, well-documented tensions. What's
  in the news.
- **Slide toward quiet:** low-signal, under-covered tensions. Few
  sources, few mentions, but real heat. A tension with 2 signals in a
  town with no other coverage is *more interesting* on the quiet end
  than a tension with 200 signals in NYC.

This is a ranking problem, not a discovery problem. The data already
exists: signal count per tension, source diversity, geographic coverage
density.

## The Implication

Root Signal becomes self-organizing. Coverage follows attention — both
user attention (Driver A) and global attention (Driver B). No one
manually adds cities. Detroit gets covered when people care about
Detroit or when something happens in Detroit.

Tension is tension. Signal is signal. Some is loud, some is small.
The system captures all of it and gives users the levers to find
what matters to them.
