---
date: 2026-02-19
topic: story-pipeline-consolidation
---

# Story Pipeline Consolidation: Remove Leiden, Single Story Path

## Problem

We're seeing **duplicate stories** on `/stories`. The root cause is two independent code paths that both create `Story` nodes:

1. **Leiden clustering** (`cluster.rs`) — creates stories from signal-signal cosine similarity (`SIMILAR_TO` edges). Runs Leiden community detection via Neo4j GDS. Requires 10+ connected signals at 0.65+ threshold.

2. **StoryWeaver** (`story_weaver.rs`) — creates stories from tension-signal response patterns (`RESPONDS_TO`/`DRAWN_TO` edges). Works with as few as 2 signals + 1 tension.

Both paths have their own containment checks (50% overlap threshold), but they don't coordinate with each other. The same underlying signals can end up in stories from both paths.

### Current Pipeline Order (scout.rs)

```
1. Clusterer::run()           ← creates Story nodes from Leiden communities
     - SimilarityBuilder::build_edges()
     - run_leiden()
     - reconcile/create stories
     - synthesize_stories()
     - compute_velocity_and_energy()

2. Parallel synthesis          ← creates RESPONDS_TO / DRAWN_TO edges
     - response mapping
     - tension linker
     - response finder
     - gathering finder
     - investigation

3. StoryWeaver::run()          ← ALSO creates Story nodes from tension hubs
     - Phase A: Materialize tension hubs as stories
     - Phase B: Grow existing stories
     - Phase C: Enrich with LLM synthesis
```

Steps 1 and 3 independently create `Story` nodes with no coordination between them.

## Proposal

**StoryWeaver becomes the sole story creation path.** Clustering infrastructure is kept for metrics only (similarity edges, energy, velocity). Leiden community detection is removed entirely.

### What to Keep from cluster.rs

- `SimilarityBuilder::build_edges()` — SIMILAR_TO edges are useful for search relevance and ranking
- `story_status()` — pure function, already `pub`, used by StoryWeaver
- `story_energy()` — pure function, already `pub`, used by energy computation
- `compute_velocity_and_energy()` — snapshot creation, velocity tracking, energy scoring
- `parse_recency()` — used by energy computation

### What to Remove from cluster.rs

- `run_leiden()` — Leiden community detection (also removes Neo4j GDS plugin dependency)
- All story creation/update logic in `Clusterer::run()`:
  - The reconciliation loop (asymmetric containment matching)
  - `label_cluster()` — LLM headline generation for Leiden communities
  - `call_haiku()` — the LLM call for labeling
  - `create_story()` / `link_signal_to_story()` calls
  - `update_story_preserving()` — partial story updates
  - `fetch_signal_metadata()` — duplicated in StoryWeaver
  - `get_story_signal_ids()` — duplicated in StoryWeaver
  - `get_story_last_updated()` — used only by energy computation
- `synthesize_stories()` — duplicates StoryWeaver Phase C
- `Community` struct — only used by Leiden
- `MIN_CONNECTED_SIGNALS` constant — gate for Leiden, not needed
- `CONTAINMENT_THRESHOLD` constant in cluster.rs — StoryWeaver has its own
- `LEIDEN_GAMMA` constant
- `MAX_COMMUNITY_SIZE` constant
- `SignalMeta` struct in cluster.rs — StoryWeaver has its own

### New Pipeline

```
1. SimilarityBuilder::build_edges()   ← keep: similarity edges for search/ranking
     (no Leiden, no story creation)

2. Parallel synthesis                  ← unchanged
     - response mapping
     - tension linker
     - response finder
     - gathering finder
     - investigation

3. StoryWeaver::run()                  ← sole story creator
     - Phase A: Materialize tension hubs as stories (with centroid)
     - Phase B: Grow existing stories (refresh centroid)
     - Phase C: Enrich with LLM synthesis
     - Phase D: Compute velocity and energy for all stories

4. (no separate velocity/energy step — absorbed into Phase D)
```

### What Clusterer Becomes

The `Clusterer` struct is removed entirely. `scout.rs` calls `SimilarityBuilder::build_edges()` directly. The pure functions (`story_status()`, `story_energy()`, `parse_recency()`) move to a `story_metrics.rs` module alongside `compute_velocity_and_energy()`.

```rust
// scout.rs — before parallel synthesis
let similarity = SimilarityBuilder::new(self.graph_client.clone());
similarity.clear_edges().await?;
let edges = similarity.build_edges().await?;
info!(edges, "Similarity edges built");
```

### Velocity/Energy: Phase D of StoryWeaver

`compute_velocity_and_energy()` moves into StoryWeaver as Phase D. This keeps the full story lifecycle in one place: create → grow → enrich → score. Phase D runs on ALL stories (not just those touched in the current run) because velocity is a time-series metric that needs continuous updates.

## Pressure Test

### Scenario 1: Volunteer Looking at Twin Cities Map

**User:** "I'm looking at South Minneapolis — what's happening here?"

**Before (Leiden + StoryWeaver):** Map shows a mix of Leiden-created stories (clusters of semantically similar signals) and tension-based stories. Some duplicates. Confusing.

**After (StoryWeaver only):** Map shows tension-based stories. Each story has a clear "what's the problem" + "who's responding" structure. Clicking a story reveals: the underlying tension (ICE enforcement fear), who's asking for help (legal aid requests), who's giving help (Know Your Rights clinics), what events/gatherings exist (vigils at Lake Street Church), relevant GoFundMes.

**Verdict:** Better. Stories have narrative structure, not just "these things are similar."

### Scenario 2: New City Cold Start (Austin, TX — first scout run)

**Concern:** Without Leiden, does the first run produce any stories?

**Pipeline for Run 1:**
1. Scrape ~70 sources → extract ~200 signals (Events, Gives, Asks, Notices)
2. Curiosity loop investigates signals → creates Tensions, wires RESPONDS_TO edges
3. Response scout finds responses to new tensions → more RESPONDS_TO edges
4. StoryWeaver Phase A: Any tension with 2+ respondents → Story

**Reality:** The curiosity loop runs on every signal. A single scraped "food shelf expansion" event generates a "Northside food desert" tension. The response scout then finds 2-3 food programs responding to that tension. That's 3+ RESPONDS_TO edges → story materializes on Run 1.

**Verdict:** No cold-start problem. The curiosity loop is the story seed generator. Run 1 produces stories within a single cycle: scrape → curiosity → response scout → StoryWeaver.

### Scenario 3: Five Similar Parking Events (No Obvious Tension)

**Concern:** Leiden would cluster these into a story. Without Leiden, are they orphaned?

**Pipeline:**
1. 5 "parking meeting" events extracted
2. Curiosity loop investigates the first one: "Why is there a parking meeting?" → searches → finds "contested bike lane removal" news → creates Tension: "Bike infrastructure vs. parking conflict"
3. Curiosity investigates next 3 events → links them to same tension via RESPONDS_TO
4. StoryWeaver materializes: "Bike infrastructure vs. parking conflict" story with 4 event signals

**The 5th event** might not get investigated this run (budget limits). It sits as an orphan signal until the next run. That's fine — it appears individually in signal search results, and gets absorbed into the story on the next cycle.

**Verdict:** Curiosity loop handles this. The story that emerges is "the tension these events are responding to" — a better story than "5 similar parking events."

### Scenario 4: Journalist Searching "Immigration Enforcement"

**User flow:**
1. Types "immigration enforcement" in search bar
2. `searchSignalsInBounds()` — finds signals via Voyage AI embeddings (unaffected by this change)
3. `searchStoriesInBounds()` — finds signals, aggregates to parent stories via CONTAINS edges

**Key question:** Do the stories found have useful structure?

**Before (Leiden):** Story might be "Cluster of immigration-related signals" with a generic LLM-generated headline.

**After (StoryWeaver):** Story is "ICE enforcement fear causing community withdrawal" — the actual tension — with responding signals: legal clinics, know-your-rights workshops, vigils, mutual aid funds.

**Verdict:** Dramatically better for a journalist. The story has a thesis (the tension), evidence (the responses), and action (what's being done).

### Scenario 5: Organizer Looking for Energy ("Where should I show up?")

**User persona:** "I want to bring people together. Where's the energy?"

**Story energy** = velocity × 0.4 + recency × 0.2 + source_diversity × 0.15 + triangulation × 0.25

**Key:** Triangulation (type_diversity / 5) rewards stories with multiple signal types. A tension-based story with Events + Gives + Asks + Notices = triangulation 0.8. A Leiden-style "similar events cluster" = triangulation 0.2 (single type).

**After consolidation:** High-energy stories are the ones with genuine multi-type community response — not echo chambers of similar events. An organizer sees where the community is already mobilizing across multiple dimensions.

**Verdict:** Energy ranking improves. Tension-based stories are structurally more triangulated than similarity-based clusters.

### Scenario 6: Donor Looking at Underserved Area

**User:** "I have money. Where does it do the most good?"

**Relevant stories surface via:** `stories_in_bounds()` sorted by energy, then drill into signals filtered by type (Ask signals = needs, Give signals = existing resources, gap = the difference).

**After consolidation:** Every story has a tension at its center, which is the "why this matters" context. The donor sees: "Affordable housing shortage driving displacement" (tension) → 3 GoFundMes (Ask), 1 legal clinic (Give), 2 community meetings (Event). The story structure literally shows the gap.

**Verdict:** Better than Leiden clusters, which would just show "housing-related signals" without the gap analysis.

### Scenario 7: Crisis Mode — Natural Disaster

**Concern:** After a tornado, 50 signals flood in within hours. Does StoryWeaver handle the burst?

**Pipeline:**
1. Scrape → extract 50 signals (emergency shelters, volunteer calls, donation drives, damage reports)
2. Curiosity loop: "Why are all these happening?" → creates Tension: "Tornado damage in North Minneapolis"
3. Response scout: finds FEMA info, Red Cross shelters, mutual aid networks
4. StoryWeaver Phase A: Tension with 20+ respondents → Story materialized immediately
5. Phase B: Subsequent signals absorbed into the story as they arrive
6. Phase C: LLM synthesis produces narrative: "North Minneapolis tornado response: where to get help and how to help"

**Leiden comparison:** Would create a community of 50 similar signals. No tension at center. No "here's the problem, here's the response" structure.

**Verdict:** StoryWeaver handles crisis better. The tension IS the crisis. The responses ARE the action items. The story structure matches what users need in a crisis: what happened, who needs help, how to help.

### Scenario 8: Slow-Burn Tension (Gentrification Over Months)

**Concern:** Gentrification signals trickle in over weeks. No single burst.

**Timeline:**
- Week 1: "New condo development approved" (Notice) → Curiosity creates Tension: "Displacement pressure in Uptown"
- Week 3: "Rent increase notices" (Notice) → links to same tension
- Week 5: "Tenant organizing meeting" (Event) → RESPONDS_TO
- Week 7: "Legal aid for renters" (Give) → RESPONDS_TO
- Week 9: "GoFundMe for displaced family" (Ask) → RESPONDS_TO

**StoryWeaver behavior:**
- Week 1: Tension exists but only 1 respondent → no story yet
- Week 3: 2 respondents → Phase A materializes story. Headline: "Displacement pressure in Uptown"
- Week 5-9: Phase B grows story. Type diversity increases (Notice → Event → Give → Ask). Energy rises.
- Arc: Emerging → Growing (velocity positive, new entities each week)

**Verdict:** StoryWeaver's accretion model is perfect for slow-burn stories. Leiden would only detect this once enough signals accumulated with high cosine similarity — and it would miss the temporal arc entirely.

### Scenario 9: Contradictory Signals (Debate/Disagreement)

**Example:** "Bike lane removal" — some signals support it, some oppose it.

**StoryWeaver behavior:**
- Curiosity creates Tension: "Contested bike lane removal on Hennepin Ave"
- Pro-removal signals: parking meetings, business owner complaints → RESPONDS_TO
- Anti-removal signals: cycling advocacy events, safety petitions → RESPONDS_TO
- Phase C enrichment: Editorial context detects mixed signal types → LLM prompt says "Surface both perspectives rather than flattening into single narrative"
- Story lede: "The proposed bike lane removal on Hennepin Ave has split the neighborhood, with business owners citing parking loss and cycling advocates citing safety data."

**Leiden comparison:** Would cluster pro-removal and anti-removal signals separately (different language/framing → different embeddings) into TWO stories. Misses the fact that it's one debate.

**Verdict:** Tension-based stories are structurally better at contradictions. The tension IS the disagreement. Both sides respond to the same tension. One story, multiple perspectives.

### Scenario 10: Ecological Stewardship (Citizen Science + Restoration)

**Signals:** River cleanup events, water quality monitoring, native plant restoration, invasive species removal.

**Curiosity:** "Why are people doing river cleanups?" → Tension: "Mississippi River water quality degradation"

**StoryWeaver:**
- Phase A: Tension + 3 response signals → Story
- Story contains: the ecological tension, volunteer events (Event), monitoring programs (Give), calls for volunteers (Ask)
- For the "land steward" persona: "I want to care for the land and water. Where do I start?" → This story answers exactly that question.

**Verdict:** Works perfectly. The tension provides the "why" and the responses provide the "how to participate."

## Critical Fix: Centroid Computation

**Bug found during pressure test:** StoryWeaver currently creates stories with `centroid_lat: None, centroid_lng: None` (story_weaver.rs:227-228). The search app's `stories_in_bounds()` query filters `WHERE s.centroid_lat IS NOT NULL`. Without fixing this, **zero StoryWeaver stories appear in geographic queries**.

**Fix:** Compute centroid in Phase A (materialize) and refresh in Phase B (grow). Same algorithm as Leiden: average lat/lng of constituent signals that have coordinates.

```rust
// Phase A: after collecting respondent signals
let lats: Vec<f64> = respondents.iter().filter_map(|r| r.lat).collect();
let lngs: Vec<f64> = respondents.iter().filter_map(|r| r.lng).collect();
let (centroid_lat, centroid_lng) = if !lats.is_empty() {
    (Some(lats.iter().sum::<f64>() / lats.len() as f64),
     Some(lngs.iter().sum::<f64>() / lngs.len() as f64))
} else {
    (None, None)
};
```

Phase B (`refresh_story_metadata`) must also recompute centroid when new signals are added, since new respondents may have better geo data than the original set.

**This is a must-ship requirement, not a follow-up.**

## Critical Fix: Sensitivity Propagation

Leiden computes story sensitivity as the maximum of constituent signals (cluster.rs:163-172). StoryWeaver currently defaults to `"general"` (story_weaver.rs:230). Must propagate max sensitivity from respondent signals.

**This is also a must-ship requirement.** A story containing a sensitive signal (e.g., ICE enforcement) must inherit that sensitivity for coordinate fuzzing to work correctly.

## Why This Is the Right Fix

1. **Single source of truth.** Tensions are the editorial unit. A story is "what's happening around this tension." Leiden creates stories from statistical clusters of embedding similarity — a fundamentally different (and weaker) definition of "story."

2. **StoryWeaver stories are structurally richer.** They have a tension at the center, typed edges (RESPONDS_TO, DRAWN_TO), signal type diversity, and editorial context (resurgence, contradictions). Leiden stories are just "these embeddings are close together."

3. **Removes Neo4j GDS dependency.** Leiden requires the GDS plugin (`gds.graph.project`, `gds.leiden.stream`). This is a significant operational dependency for a feature we're removing.

4. **StoryWeaver already handles small cities.** It works with 2 signals + 1 tension. Leiden requires 10+ connected signals — it was always the wrong tool for early-stage cities.

5. **Stories should tell narratives.** The tension-based model naturally produces stories with a "what's the problem" + "who's responding" + "what's gathering" structure. Leiden produces "these signals are semantically similar" — that's a search result, not a story.

6. **Contradictions are first-class.** Tension-based stories naturally surface disagreement (both sides respond to the same tension). Leiden separates opposing viewpoints into different clusters because their embeddings diverge.

7. **Every persona is better served.** Volunteers see what needs doing and why. Donors see the gap between needs and resources. Organizers see where energy is converging. Journalists see the tension and the community response. All of these need the "why" (tension) at the center, not just "what's similar."

8. **Crisis mode works naturally.** The tension IS the crisis. Responses ARE the action items. No special crisis-mode story logic needed.

9. **Alignment machine preserved.** The system measures community alignment (needs exceed responses = misalignment, needs decrease = alignment restored). Tension-based stories expose this directly. Leiden clusters obscure it.

## Migration

### Existing Leiden-created stories

Detectable by: no Tension node linked via CONTAINS, or `dominant_type != "tension"`.

**Recommended approach:** Delete them. Stories are materialized views — the underlying signals and tensions persist. StoryWeaver will re-materialize stories from tensions that already have 2+ respondents on the next run. Clean slate is safer than hybrid state.

```cypher
// Find Leiden-created stories (no tension in CONTAINS)
MATCH (s:Story)
WHERE NOT (s)-[:CONTAINS]->(:Tension)
RETURN s.id, s.headline, s.signal_count

// Delete them (signals and SIMILAR_TO edges are preserved)
MATCH (s:Story)
WHERE NOT (s)-[:CONTAINS]->(:Tension)
DETACH DELETE s
```

### SIMILAR_TO edges

Preserved. Used by search (`searchSignalsInBounds` aggregates via CONTAINS, not SIMILAR_TO) and by cause_heat computation. Not affected by this change.

### ClusterSnapshot nodes

Keep existing snapshots. `compute_velocity_and_energy()` (now Phase D) creates new snapshots and reads old ones for velocity calculation. Continuity preserved.

## Story Sorting and Ranking Model

### Metrics (computed per story)

These metrics are derived from the structural relationships between a story's tension and its constituent signals. They work at two levels.

| Metric | Derivation | Stored on Story? |
|--------|-----------|-----------------|
| **Energy** | velocity × 0.4 + recency × 0.2 + source_diversity × 0.15 + triangulation × 0.25 | Yes (Phase D) |
| **Cause heat** | Propagated from central tension's `cause_heat` | Yes (Phase A/B) |
| **Gap score** | Count of Ask signals minus count of Give signals | Yes (Phase A/B) |
| **Need intensity** | Count of Ask signals | Yes (Phase A/B) |
| **Gathering momentum** | Count of DRAWN_TO signals | Yes (Phase A/B) |
| **Velocity** | Entity diversity growth over 7 days | Yes (Phase D) |
| **Recency** | Time since last_updated | Yes (implicit) |
| **Response coverage** | Count of Give signals | Yes (Phase A/B) |
| **Event density** | Count of Event signals | Yes (Phase A/B) |

### Two-Level Sorting

The same metrics work at every zoom level — the computation is the same, just scoped differently:

**Level 1: Story list** — "Show me stories in South Minneapolis sorted by gap score" → surfaces stories where needs most outweigh responses. Metrics are pre-computed aggregates stored on the Story node.

**Level 2: Inside a story** — "I'm looking at the ICE enforcement story, show me the Asks first" → sorts/filters the constituent signals by type, urgency, recency. The raw signal breakdown within a CONTAINS set.

The frontend decides the scope. The API exposes the same metrics at both levels.

### Persona Mapping

| Sort | Who uses it | Question it answers |
|------|-------------|-------------------|
| Energy | Default | "What's most alive right now?" |
| Cause heat | Journalist, researcher | "What's most understood/connected?" |
| Gap score | Donor, funder | "Where does money do the most good?" |
| Need intensity | Volunteer | "Where is help most needed?" |
| Gathering momentum | Organizer, attendee | "Where are people showing up?" |
| Velocity | Anyone | "What's accelerating?" |
| Recency | Anyone | "What's new?" |

### Why Leiden Couldn't Do This

Leiden clusters signals by embedding similarity. It doesn't know which signals are Asks vs Gives vs gatherings — just that the embeddings are close. Gap score, gathering momentum, and need intensity are impossible to compute from a Leiden community because the signal roles are invisible. Tension-based stories know the role each signal plays.

## Dynamic Story Generation (Future: Conversational AI)

Pre-materialized stories (what StoryWeaver creates) are a **cache** — the fast path for map browsing. But the source of truth is the graph pattern itself: tensions + respondent signals + their relationships.

This means an AI conversational layer can generate stories **dynamically** given a lat/lng + radius:

```cypher
// 1. Find tensions in radius
MATCH (t:Tension)
WHERE point.distance(point({latitude: t.lat, longitude: t.lng}),
      point({latitude: $lat, longitude: $lng})) <= $radius_m

// 2. Pull their respondent signals with types and relationships
MATCH (t)<-[r:RESPONDS_TO|DRAWN_TO]-(sig)
RETURN t, collect({
  signal: sig,
  edge_type: type(r),
  label: labels(sig)[0],
  gathering_type: r.gathering_type
})
```

This gives the LLM structured data: tensions in this area, the Asks (needs), Gives (resources), Events (gatherings), and their relationships. The LLM synthesizes a rundown tailored to the user's question — same data as the materialized stories, but shaped by conversation rather than a batch template.

**Example user prompt:** "Give me a rundown of what's happening in this area."

**LLM receives:** 3 active tensions with their respondent signals, sorted by the same metrics (energy, gap score, gathering momentum). The LLM can say: "The biggest unmet need in this area is affordable housing — 7 people are asking for help and only 2 resources exist. Meanwhile, there's strong community energy around immigration issues with 4 vigils this month at Lake Street Church."

**Why this works:** The tension-based graph pattern is clean, queryable, and self-describing. Leiden clusters couldn't support this because they're statistical artifacts — there's no tension to anchor the narrative, no signal roles to describe, no gap to surface.

**This is not in scope for the current consolidation work.** But the architecture enables it. The consolidation makes the graph pattern clean enough that dynamic generation becomes a straightforward query + LLM call, not a research project.

## Context: What Stories Are For

Stories provide **initial momentum into acting on signals**. When someone opens the search app and looks at an area (lat/lng), stories tell the narrative of what's going on — who's asking for help, who's giving help, what events and gatherings are happening, where the GoFundMes are, what tensions the community is navigating. Stories are the context layer that turns raw signals into actionable understanding.

The system serves people in roles — volunteer, donor, attendee, advocate, organizer, journalist — not demographics. Every role needs the same thing from a story: **what's the tension, who's responding, and how can I participate?** Leiden clusters answer a different question ("what signals are semantically similar?") that no persona actually asks.

## Files Affected

| File | Change |
|------|--------|
| `modules/rootsignal-graph/src/cluster.rs` | Remove Clusterer struct, run_leiden, reconciliation loop, label_cluster, synthesize_stories, Community struct, SignalMeta struct, all constants except story_status/story_energy/parse_recency. Consider renaming to `story_metrics.rs`. |
| `modules/rootsignal-graph/src/story_weaver.rs` | Add centroid computation (Phase A + B). Add sensitivity propagation (Phase A). Add Phase D: velocity/energy (moved from cluster.rs). Fetch lat/lng in signal metadata queries. |
| `modules/rootsignal-scout/src/scout.rs` | Replace `Clusterer::new().run()` with direct `SimilarityBuilder::build_edges()` call. Remove Clusterer import. Remove `compute_velocity_and_energy()` call (now in StoryWeaver Phase D). |
| `modules/rootsignal-graph/src/lib.rs` | Update exports: remove Clusterer, add story_metrics module (or inline into story_weaver). |
| `modules/rootsignal-graph/src/migrate.rs` | Add migration to delete Leiden-created stories (optional, can be run manually). |
| `docs/tests/clustering-testing.md` | Update: remove Leiden-specific tests (sections 2-5), update story creation tests to reference StoryWeaver, keep similarity edge tests (section 1) and velocity/energy tests (sections 10-11). |

## Pressure Test Results

Tested against 15 user journeys. 4 PASS, 4 PARTIAL, 4 FAIL (fixable), 3 UNKNOWN (out of scope).

### Critical Issues Found (Must Address)

#### 1. Story centroid fuzzing for sensitive stories

**Severity: Privacy bug.** Even after fixing sensitivity propagation, the story centroid is computed from raw (unfuzzed) signal coordinates. `GqlStory` exposes `centroid_lat`/`centroid_lng` directly with no fuzzing. A sensitive story's centroid reveals the approximate geographic center of sensitive signals.

Individual signals get coordinate fuzzing in `reader.rs:fuzz_node()`, but `row_to_story()` returns raw centroid coordinates. There is no `fuzz_story()` function.

**Fix:** When `story.sensitivity == "sensitive"`, fuzz the centroid in `row_to_story()` the same way individual signals are fuzzed. Must ship with the consolidation — this is not a follow-up.

#### 2. Story cleanup / zombie stories

Stories never expire. After all constituent signals age past `FRESHNESS_MAX_DAYS`, the story persists with a headline but zero displayable signals inside it. A user who scrolls past the high-energy stories will find zombie stories.

The energy score decays toward 0 (recency drops to 0 after 14 days, velocity drops with no new signals), so zombies sort to the bottom. But they never disappear.

**Fix options (pick during planning):**
- **Archive:** Set `arc = "Archived"` when signal_count of displayable signals drops to 0. Filter from default queries.
- **TTL:** Delete stories with `last_updated` older than N days and no recent signal activity.
- **Reap in Phase D:** During velocity/energy computation, if all CONTAINS signals have expired, delete the story.

#### 3. Self-explanatory signal gap

A large class of editorially valid signals — farmers markets, repair cafes, tool libraries, volunteer opportunities, yoga classes — correctly skip the curiosity loop (they're self-explanatory, not curious). They never get RESPONDS_TO edges. They never appear in stories. If the search app is story-primary, these signals are structurally invisible to the primary navigation model.

These are exactly the "ethical consumption" and "ecological stewardship" categories from `editorial-and-signal-inclusion-principles.md`.

**This is a design tension, not a bug.** The consolidation doesn't create this gap (Leiden also wouldn't cluster a single farmers market signal into a story). But it's important to acknowledge.

**Design decision:** The search app must treat **signal browsing as a first-class path**, not just a fallback when stories aren't found. The Signals tab and Stories tab are peers. Signals that don't belong to any story are still valuable — they just don't have narrative context. The empty state on the Stories tab when signals exist should say something like "No active stories in this area — browse individual signals for community resources."

**Future option (not in scope):** A "community resources" catch-all story type that groups self-explanatory signals by geography without requiring a tension. Deferred — adds complexity and the signal tab handles this adequately.

#### 4. Containment order-dependence

When two tension hubs overlap (e.g., "Housing affordability" and "Gentrification displacement" share 5 signals), whichever hub is processed first in Phase A wins the headline. The second hub gets absorbed, losing its distinct framing.

**Fix:** `find_tension_hubs()` must return hubs in **deterministic, meaningful order**. Sort by respondent count descending (the hub with the most respondents gets its own story). Ties broken by cause_heat descending (the better-understood tension gets priority).

This ensures the "bigger" or "better-understood" tension anchors the story, and smaller overlapping tensions are absorbed as context. Phase C enrichment should then surface the absorbed tension's framing in the narrative.

#### 5. `find_tension_hubs` limit of 10

Phase A only processes 10 hubs per run (`self.writer.find_tension_hubs(10)`). In a high-density area with 50 new tension hubs, it takes 5 runs to materialize all stories.

**Fix:** Increase limit to 50 (or remove it). Phase A is cheap — no LLM calls, just graph queries + writes. The limit was conservative for the initial implementation but is now a bottleneck for dense areas.

#### 6. Story centroid quality

The centroid is a naive average of all signal coordinates. If signals are scattered across a wide area, the centroid lands somewhere meaningless (middle of a lake, between two neighborhoods). This matters for map display.

**Accept for now.** Naive average is good enough for v1 — most stories are geographically coherent because signals respond to the same tension in the same area. Revisit with weighted centroid or convex hull if map display quality suffers in practice.

### Issues Acknowledged (Not Regressions)

#### Post-disaster latency

The brainstorm previously claimed "no special crisis-mode logic needed." This is **overconfident about latency**, though correct about the story creation mechanism. The real bottleneck is pipeline latency: the scout cycle may not have run yet 2 hours after a disaster. This is not a regression from removing Leiden (Leiden had the same latency) and is not in scope for this consolidation. But the claim has been softened.

#### Non-English search

The system is English-only in practice. All LLM prompts, tension titles, narratives, and synthesis are in English. Voyage AI embeddings have some cross-lingual capability, so "ayuda para inmigrantes" may find English-language immigration signals. But this is untested and not addressed by the consolidation. Not a regression, not in scope.

#### Rural empty states

A rural area with 5 signals and no tension with 2+ respondents produces zero stories. The user sees individual signal pins only. Leiden also couldn't help here (required 10+ connected signals). The search app must have a useful empty state for the Stories tab. This is a frontend concern for the search app plan, not the pipeline consolidation.

#### Multi-story signal membership

A signal responding to multiple tensions (via RESPONDS_TO to both) gets CONTAINS edges from both stories. But `batch_story_by_signal_ids()` (DataLoader) returns only one parent per signal. The signal's `story` resolver is lossy. This is a pre-existing data model limitation, not introduced by the consolidation.

### Full Journey Results

| # | Journey | Verdict | Notes |
|---|---------|---------|-------|
| 1 | First-time visitor | **PASS** (with centroid fix) | Centroid fix is must-ship. New stories get energy=0 initially but Phase D in same run fixes this. |
| 2 | Teacher/volunteer search | **PARTIAL** | Signal search works. Self-explanatory volunteer signals don't appear in stories — by design, not a bug. |
| 3 | Nonprofit food landscape | **PASS** | Story structure naturally shows gap between needs and resources. |
| 4 | Journalist investigation | **PASS** | Evidence chain intact. Contradictions surface in single story. |
| 5 | Post-disaster donor | **PARTIAL** | Pipeline latency is the bottleneck, not story creation. Not a regression. |
| 6 | Organizer tracking momentum | **PARTIAL** | Current state visible (velocity, arc, energy). No trajectory history API. Not in scope. |
| 7 | Farmers markets (no tension) | **PARTIAL** | Signals findable but not in stories. Signal tab is first-class. Acknowledged. |
| 8 | Rural (few signals) | **PARTIAL** | No stories below 2-respondent threshold. Not a regression from Leiden. Empty state needed. |
| 9 | Overlapping tensions | **PASS** (with ordering fix) | Deterministic hub ordering ensures better tension anchors the story. |
| 10 | Stale/resolved story | **PASS** (with cleanup) | Story cleanup mechanism needed. Phase D reap or archive. |
| 11 | Sensitive content | **PASS** (with centroid fuzzing) | Must fuzz story centroid for sensitive stories. Privacy bug otherwise. |
| 12 | Non-English search | **UNKNOWN** | Not a regression. Out of scope. |
| 13 | High-density area | **PARTIAL** | Performance OK. Increase `find_tension_hubs` limit. Frontend clustering needed. |
| 14 | Multi-tension signal | **PARTIAL** | Signal in multiple stories works. DataLoader lossy — pre-existing limitation. |
| 15 | Empty area | **UNKNOWN** | Frontend empty state design. Out of scope for pipeline work. |

## Resolved Questions

- **Where does velocity/energy live?** Phase D of StoryWeaver. Keeps the full story lifecycle in one place.
- **What about signals without tensions?** The curiosity loop investigates every signal and either links it to a tension or abandons it after 3 failures. Abandoned signals shouldn't be in stories — they're not understood yet. Self-explanatory signals (farmers markets, etc.) are visible via signal search but not in stories — the search app must treat signal browsing as first-class.
- **Cold start problem?** No. Curiosity loop + response scout create tensions and responses within a single run. Stories materialize immediately when tensions accumulate 2+ respondents.
- **What about signal dedup/redundancy?** SIMILAR_TO edges still exist for search relevance. Story-level dedup is handled by StoryWeaver's containment check. Redundant signals responding to the same tension are naturally grouped.
- **Crisis mode handling?** The tension IS the crisis. Story creation works naturally. But pipeline latency (scout cycle timing) is the real bottleneck for time-sensitive stories — that's an orthogonal concern to the consolidation.
- **Contradictions?** Both sides of a debate respond to the same tension → one story, multiple perspectives. StoryWeaver Phase C enrichment explicitly detects this and prompts the LLM to surface disagreement.
- **Zombie stories?** Stories with no active signals must be archived or reaped in Phase D. Energy decay sorts them to the bottom, but they should eventually be removed.
- **Sensitive story centroids?** Must fuzz centroid coordinates for sensitive stories, not just individual signals.
- **Overlapping tension hubs?** Deterministic ordering (respondent count DESC, cause_heat DESC) ensures the most significant tension anchors the story.
- **Hub processing limit?** Increase from 10 to 50. Phase A is cheap.

## References

- Story weaver architecture: `docs/architecture/story-weaver.md`
- Clustering testing playbook: `docs/tests/clustering-testing.md`
- Search app plan (consumer): `docs/plans/2026-02-19-feat-search-app-plan.md`
- Gravity-aware stories: `docs/plans/2026-02-18-feat-gravity-aware-stories-places-plan.md`
- Editorial principles: `docs/vision/editorial-and-signal-inclusion-principles.md`
- Alignment machine: `docs/vision/alignment-machine.md`
- Tension gravity: `docs/vision/tension-gravity.md`
- Signal-to-response chain: `docs/architecture/signal-to-response-chain.md`
- Scaling bottlenecks (Leiden is listed): `docs/analysis/scaling-bottlenecks.md`
- Key commit — similarity threshold fix: `669a6c0`
