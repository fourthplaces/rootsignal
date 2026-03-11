---
date: 2026-03-10
topic: situation-investigator-groups
---

# Situation Investigator & Signal Groups

## Context

Situation weaving has been decoupled from the scout pipeline into its own workflow (`GenerateSituationsRequested`). The current weaving approach processes signals in batches of 5, matching them against existing situations via embedding similarity. This is assignment, not discovery — it can't find patterns across signals or create situations from scratch with any intelligence.

We need a fundamentally different approach: active investigation, not passive assignment.

## Core Insight: The LLM Is a Journalist, Not a Classifier

The current model gives the LLM 5 signals and asks "where do these go?" That's a classifier. A journalist works differently:

1. Notice something concerning (a seed signal)
2. Pull threads — "what else is happening around this?"
3. Ask investigative questions — "what are the responses? who needs help?"
4. Cluster naturally — signals group by the threads that found them
5. Name the pattern — only after enough evidence accumulates

The LLM should drive the investigation, not evaluate pre-computed candidates.

## Three-Layer Architecture

### Layer 1: Signals (exists today)
Raw evidence. Concerns, Conditions, Resources, HelpRequests, Gatherings, Announcements. Each has text, location, tags, embeddings.

### Layer 2: Groups (new)
Clustered signals defined by their search queries. Agnostic to meaning — just "these signals are related." A Group is:

- A set of member signals
- A set of search queries (its "gravitational fingerprint")
- Optional parent group (for nesting)
- Event-sourced and projected into the graph

Groups are mechanical. They don't have headlines, temperature, or dispatches. They're clusters.

### Layer 3: Situations (exists today, refactored)
Narrative layer pointing to groups. Headline, temperature, dispatches, structured state. Situations give meaning to the clustering. A Situation points to a Group — it doesn't own signals directly.

**Clean separation**: you can cluster without committing to narrative. A group of 3 signals might not be a Situation yet. Let it grow. Situations can be created, merged, split without touching the underlying clustering.

## The Situation Investigator

A workflow tool that gives the LLM a seed and search tools. The LLM generates its own queries, pulls threads, and clusters emerge from the investigation.

### Tooling

The investigator has search tools with no prescribed dimensions:
- `search_signals(query, location?, radius_km?)` — text/tag search, optionally scoped to a geography
- `find_similar(signal_id)` — embedding-based similarity from a specific signal

All parameters except `query` are optional. The LLM decides whether to constrain by location and at what radius, based on what the group's signals tell it. A topical group like "ICE enforcement nationwide" uses no location constraint. A hyper-local group like "Lake Street food access" uses tight geography. The LLM composes these however the investigation demands.

The key insight: **search queries are extracted from the group itself.** As a group grows, the LLM reads the group's collective language and generates investigative queries:
- "What are the responses to this?"
- "Who needs help?"
- "What's causing this?"
- "Are there similar patterns nearby?"

These are the threads the journalist pulls.

### Workflow (Fixed Depth, Flywheel)

Each run is bounded — fixed number of rounds (e.g., 3). Groups grow incrementally across runs.

```
Round 1: Seed signal → LLM generates queries from seed text
         → search → initial results

Round 2: LLM clusters results into proto-groups
         → extracts NEW queries from each group's collective language
         → search → add results to groups

Round 3: Same — extract, search, add. Diminishing returns.

Done. Next run picks up where left off.
```

Termination: diminishing returns within a run (last round found < K new signals), plus fixed depth cap so workflows don't run away. The flywheel model means each run expands groups incrementally — no single run needs to find everything.

### Groups as Gravitational Wells

A group's search queries define its gravity. New signals matching those queries get pulled in. Groups naturally attract related signals over time.

- **Multi-membership is natural.** A signal about "community organizing against workplace raids" lives in both an enforcement group and a response group. No conflict — it's evidence in both gravitational wells.
- **Groups within groups.** Inner groups form when a sub-theme develops enough gravity to become its own attractor. The outer group still contains everything; the inner group is a tighter well within it.
- **The "global" group is implicit.** Unassigned signals are part of an invisible global group — everything that hasn't been pulled into a specific gravitational well yet.

### Clustering Dimensions Are Emergent

Groups don't prescribe what dimension they cluster on. The LLM reads the group's signals, notices what they share, and generates queries along whatever axis is relevant:

- **Topic**: "ICE enforcement" — signals across 8 states, no geographic constraint
- **Location**: "Lake Street corridor" — signals sharing a neighborhood
- **Actor**: "Hennepin County Sheriff" — signals about the same institutional actor
- **Cause**: "housing displacement from gentrification" — signals sharing a root cause
- **Response pattern**: "know your rights workshops" — signals sharing a response type

A single group can cluster on multiple dimensions simultaneously. "ICE enforcement Twin Cities" is both topical and geographic. The investigator discovers which dimensions matter by reading the group — it's not told.

**Location emerges from the signals, not the other way around.** If 10 of 12 signals in a group are within 5km of each other, the LLM notices and generates geographically-scoped queries. If they're scattered nationwide, it doesn't. Radius is inferred from the data:

- Signals within a few blocks → neighborhood search (~1km)
- Spread across a metro → metro-scale search (~30km)
- Spread across a state → state-scale search (~300km)
- Nationwide → no geographic constraint

Geographic sub-clusters emerge naturally within topical groups. A nationwide "ICE enforcement" group develops sub-groups when the LLM notices "these 8 signals are all in the Twin Cities, those 5 are in Chicago" and generates location-scoped queries for each cluster.

### Example: Local Seed, Emergent Geography

```
Seed: "ICE raids reported near Lake Street"

LLM generates queries (no geographic constraint — lets results reveal scope):
  → "ICE enforcement raids"
  → "immigration raids Lake Street"
  → "workplace immigration enforcement"

Results: 18 signals — 10 in Minneapolis, 3 in Chicago, 2 in Houston, 3 nationwide

LLM clusters into proto-groups:
  A: "ICE enforcement actions" (12 signals, multi-city)
  B: "Community rapid response — Twin Cities" (6 signals, geographically tight)
  C: "Legal challenges to ICE" (4 signals, nationwide)

For each group, extract NEW queries — dimensions emerge from the group:
  A: "workplace raid", "detention bus", "ICE facility" (topical — no location)
  B: "sanctuary network Minneapolis", "know your rights Twin Cities" (topic + location)
  C: "federal lawsuit immigration", "ACLU ICE challenge" (topical — no location)

Round 2 results: Group A grows to 20 signals across 6 cities.
  LLM notices geographic sub-clusters → generates sub-group queries:
  → "ICE enforcement Twin Cities" (8 signals within ~30km)
  → "ICE enforcement Chicago area" (5 signals within ~40km)
  Geographic sub-groups emerge WITHIN the topical group.
```

### Example: Ecological, No Location Seed

```
Seed: "PFAS contamination detected in drinking water"

LLM generates queries:
  → "PFAS forever chemicals water"
  → "drinking water contamination"
  → "PFAS health effects community"

Results: 14 signals — scattered across Minnesota, Wisconsin, Michigan

LLM clusters:
  A: "PFAS contamination in water systems" (9 signals)
  B: "Community response to PFAS" (5 signals — cleanups, advocacy, testing)

Group A has signals near Minnehaha Creek (3), near Camp Ripley (2), in Michigan (4).
LLM notices geographic patterns, generates sub-group queries:
  → "PFAS Minnehaha Creek" → sub-group forms
  → "PFAS Camp Ripley military" → sub-group forms (different cause — military base)
  → Location emerged from signals. Cause emerged too (military vs. industrial).
```

## Event Model

Groups are event-sourced, following the existing pattern:

```
GroupCreated { group_id, seed_signal_id?, queries: Vec<String> }
SignalAddedToGroup { signal_id, group_id }
GroupQueriesUpdated { group_id, queries: Vec<String> }
GroupNested { child_group_id, parent_group_id }
```

## Graph Schema

```
(Signal)-[:MEMBER_OF]->(Group)
(Group)-[:SUBGROUP_OF]->(Group)      // hierarchy
(Situation)-[:ABOUT]->(Group)         // situation points to group
```

Signals can be in multiple groups (multi-membership). Groups can nest. Situations reference groups, not signals directly.

## Refinements (from review)

### Weighted Query Schemas

Group queries shouldn't be flat strings — they should carry weight. A group's gravity isn't just "ICE raids," it's:

```
("ICE" AND "Lake Street") weight: 0.9
("white van" AND "uniformed") weight: 0.4
("immigration enforcement") weight: 0.7
```

This makes the gravitational fingerprint more precise and enables better signal matching.

### Tags as Features, Queries as Hypotheses

Tags are metadata confirmed at the signal level. Group queries are the LLM's hypotheses about how those tags correlate.

Flow: Signal enters → NLP extracts Tags → Investigator uses Tags to seed Queries → Resulting Groups define context that might refine future Tags.

Tags feed into query generation. Groups don't replace tags — they build on them.

### Group Deduplication via Jaccard Similarity

Don't use LLM judgment for the first pass (too expensive). Use Jaccard similarity on signal ID sets. If Group A and Group B share >60% of their signals, trigger an LLM merge review. The LLM then decides if Group B is a sub-thread of A or if they're identical.

### Anomaly-Driven Seeding

The best seed is anomaly-driven. Monitor the unassigned signal pool. When signal density spikes along any dimension — geographic area, tag frequency, actor mentions, topic similarity — that cluster's centroid becomes the seed.

This means seeds aren't random or manually chosen — the system notices "something is accumulating" and starts investigating. The accumulation might be geographic (15 signals near Broadway and Penn), topical (sudden spike in "water quality" tags), or actor-driven (multiple signals mentioning the same organization).

### Group → Situation Promotion: Coherence Score

A Group is just a pile of evidence. A Situation requires a narrative.

The LLM periodically evaluates Groups. If a Group has high "narrative density" — who, what, where, and a conflict/concern are all present — the LLM flags it as ready for promotion.

**A Group is a Draft. A Situation is Published.**

### Query Drift Anchoring

Risk: the LLM generates queries that are too abstract in later rounds, drifting from the original seed (e.g., "ICE raids" → "Federal Budget Policy").

Fix: every query generated in Round 2+ must be anchored to the original seed's primary theme. The investigator can explore laterally (different aspects of ICE enforcement, different geographies where it's happening) but not vertically into abstraction (federal immigration policy → federal budget → national politics).

Queries should mirror the community's own language. If the source signals say "ICE raids," the queries say "ICE raids" — not the LLM's reframing. This aligns with the editorial principle: Root Signal does not editorialize.

### Early Branch Killing

If a round finds no new signals for a branch, kill it early rather than continuing to generate queries. Save tokens for productive branches.

- Round 1: High recall (broad queries)
- Round 2+: High precision (niche queries)
- If Round N finds nothing: stop that branch

## Open Questions

### Situation Refactoring
Current `(Signal)-[:PART_OF]->(Situation)` becomes `(Signal)-[:MEMBER_OF]->(Group)` + `(Situation)-[:ABOUT]->(Group)`. This is a data model change. Migration path needed.

### Group Persistence Across Runs
How do groups persist between investigator runs? The event store captures the history, but the graph projection needs to maintain group state (current queries, member signals) so the next run can pick up where the last left off.

### Intersection Handling
A signal in multiple groups creates intersections. When a Situation is created from a Group, how does it handle signals that also belong to other Groups/Situations? The Situation layer chooses which emphasis to surface in dispatches — the Group layer doesn't need to resolve this.

## Key Decisions

- **Groups are decoupled from Situations.** Clustering is agnostic to situation promotion.
- **The LLM generates search queries, not us.** Queries are the group's identity — inspectable, evolvable, reasoned about.
- **Fixed depth per run, flywheel across runs.** Bounded costs, incremental growth.
- **Multi-membership is natural.** Signals can be in multiple groups.
- **Hierarchy is emergent.** Groups within groups form when sub-themes develop enough gravity.
- **Clustering dimensions are emergent.** Topic, location, actor, cause, response pattern — the LLM discovers which dimensions matter by reading the group's signals. Location is one possible dimension, not the primary axis.
- **Event-sourced groups.** Causal chain + events to assign signals and project into graph.

## Pressure Test Against Vision Docs

Tested against all principles, anti-principles, and scenarios from `docs/vision/`.

### Strong Alignments

**Emergent Over Engineered (Principle 13)** — The investigator IS this principle. Groups emerge from investigation, hierarchy is emergent, queries evolve from the group's own language.

**The Alignment Machine** — Groups naturally produce alignment pictures. A group containing both Concerns about rent increases AND Resources about legal aid clinics shows misalignment and response together. Multi-membership preserves alignment from multiple angles.

**Self-Evolving System** — The investigator feeds gap analysis as a side effect. When the investigator searches "bilingual Hmong interpreters Minneapolis" and finds nothing, that empty result IS a gap signal that feeds source discovery.

**Life, Not Just People (Principle 11)** — Ecological signals cluster naturally. "PFAS contamination" produces groups spanning water quality Conditions, cleanup Gatherings, EPA Announcements, advocacy Concerns. No special-case logic needed.

**Resource Matching** — Groups enable situation-level resource analysis. Aggregate all `Requires` across signals in a group → instant resource gap table per situation.

### Tensions To Resolve

**Serve the Signal, Not the Algorithm (Principle 4)** — LLM query generation is algorithmic curation. The investigator chooses which threads to pull. Ungrouped signals must surface with equal standing via recency and cause_heat. Groups add context; they don't control access.

**Will Not Gatekeep (Anti-Principle)** — If the Situation layer only reads Groups, ungrouped signals vanish from the narrative layer. The API must surface ungrouped signals independently. Groups organize signals; they don't rank them.

**cause_heat vs. Coherence Score** — These are complementary, not overlapping. cause_heat measures understanding of individual signals (traced to causal tension). Coherence Score measures cluster-level completeness (enough narrative density for a story). High coherence + low cause_heat = "complete cluster, unknown cause." Low coherence + high cause_heat = "understood cause, thin evidence." Both are needed for Situation promotion.

**Privacy intersection risk** — Multi-membership could enable inference attacks. Signal X in both "ICE enforcement" and "community mutual protection" reveals more than either alone. Needs explicit consideration in the sensitivity model.

### Gaps To Harden

**1. "No group here" exit.** The investigator must be able to walk away from a coincidental cluster. Anomaly-driven seeding detects 15 signals near Broadway and Penn — but it's 8 Eventbrite networking events + 4 yoga classes + 3 restaurant openings. After Round 1, if the LLM determines no thematic coherence, it emits no `GroupCreated` event and moves on.

**2. Crisis pacing.** The flywheel model is too slow for disaster response. When signal velocity in a geography exceeds a threshold, the investigator should run with more depth (5 rounds), more breadth (multiple concurrent seeds), and shorter intervals between runs. Mirrors the editorial doc's crisis mode: "scraping cadence accelerates."

**3. Alignment-aware clustering.** The investigator should cluster by ISSUE, not by signal type. A Concern about evictions and a legal aid clinic responding to evictions belong in the same group. Responses and tensions together — that's the alignment machine.

**4. Investigation gaps → source discovery.** "Searched for X, found nothing" should feed the self-evolving system's gap analysis pipeline. Empty investigator results are source discovery inputs.

**5. Group growth velocity → crisis detection.** A rapidly growing group (many signals added in short time) IS the editorial doc's "tension cluster crossing a threshold." Group growth velocity is the natural crisis mode trigger.

### Scenario Stress Tests

| Scenario | Result | Notes |
|----------|--------|-------|
| ICE raids Lake Street | Works | Multi-group, multi-membership, geographic sub-clusters emerge within topical group |
| ICE enforcement nationwide | Works | Topical group with no geography. Sub-groups form per-city as LLM notices clusters |
| Turkey earthquake | Pacing gap | Hundreds of signals need crisis pacing — more depth, concurrent seeds, shorter intervals |
| PFAS in Minnehaha Creek | Works | Ecological signals cluster naturally. Geographic + causal sub-groups emerge |
| 64 Eventbrite networking events | Correctly handled | Anomaly seed → investigation → "no thematic coherence" → no group. Needs "no group here" exit |
| Maui wildfire | Works + pacing gap | Resource gap analysis across groups is powerful. Crisis pacing needed |
| "I have a car" × Groups | Enhanced | Matches organized by situation context — not just "drive for X" but "drive for X as part of Y response" |
| Housing + Power analysis | Works | Group "Housing affordability" → natural target for power crate |
| Astroturfing attack | Defended | Fake sources → low evidence depth → low-coherence groups → never promote to Situation |
