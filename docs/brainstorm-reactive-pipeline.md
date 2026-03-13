# Reactive Pipeline: Let Events Drive

## The Problem

The current pipeline imposes sequential phases on an event-driven engine:

```
Tension Scrape → Source Expansion → Response Scrape → Enrichment → Signal Expansion → Synthesis
```

Each phase gates on the previous via superset completion checks, completion flags, and cascading lifecycle events. This creates:

- Complex phase-tracking state (15+ boolean flags, 2 role sets, completing_role guards)
- Artificial serialization — investigation waits for enrichment which waits for all scraping
- Orchestration bugs (superset gates firing N times, batch-reduce timing issues)
- The pipeline fights seesaw instead of using it

## The Insight

Most of the "phases" aren't real dependencies. Investigation doesn't need actor stats. Response mapping doesn't need diversity scores. These are independent operations that happen to be serialized because the pipeline was designed as stages.

The real dependencies:

| Operation | Actually needs | Doesn't need |
|-----------|---------------|-------------|
| Investigate a signal | The signal itself | Graph stats, other signals |
| Find responses to a concern | The concern | All sources scraped |
| Discover response sources | A concern with context | All tension scrapes done |
| Link concerns | Two concerns | Enrichment |
| Compute similarity | Two signals | Enrichment |
| Actor extraction | Signals without actors | All scraping done |
| Diversity scoring | Citation edges | All scraping done |
| Source weights | Scrape completion for that source | All scraping done |
| Query expansion | Implied queries from extraction | All scraping done |
| Promote links | Scraped page content | Anything else |

## The Reactive Model

### Zero phases. Just events and reactions.

Every operation fires when its input event arrives. No gates, no superset checks, no completion tracking.

```
Signal created (WorldEvent::ConcernRaised, etc.)
  ├→ investigate (if interesting)
  ├→ find responses
  ├→ link to existing concerns
  ├→ compute similarity
  └→ expand: discover response sources (concerns only)

Scrape completed (per source)
  ├→ promote links → new sources
  ├→ expand implied queries → new sources
  └→ update source weight + cadence (just this source)

Citation published (WorldEvent::CitationPublished)
  └→ recompute diversity for that signal

Actor identified (SystemEvent::ActorIdentified)
  └→ increment actor signal count

New signal without actor edges (reactive query)
  └→ extract actors from that signal's text
```

No "all sources done" gate. No batch measurement phase. Everything incremental.

## Incremental Measurement

Each measurement currently batched at end-of-run can go incremental:

### Source weights — incremental per source

**Currently**: After all scraping, iterate all sources, read graph stats, recompute weights.

**Incrementally**: When a scrape completes for source X, recompute X's weight immediately.

The inputs are all available at scrape completion:
- `signals_produced` — known from extraction
- `signals_corroborated` — already on the source node
- `scrape_count` — increment by 1
- `tension_count` — graph read for this source only
- `last_produced_signal` — now, if signals > 0
- `quality_penalty` — already on source node

Emit `SourceScraped` + `SourceChanged` for that one source. Cadence updates too.
Dead source detection (10+ empty runs) can also fire per-source after each scrape.

**Trigger**: `ScrapeEvent::WebScrapeCompleted` / `SocialScrapeCompleted` — handler knows which source was scraped and how many signals it produced.

### Diversity scoring — incremental per citation

**Currently**: After all scraping, read all evidence edges per signal label, compute diversity.

**Incrementally**: When `CitationPublished` fires, recompute diversity for the cited signal.

The citation event carries `signal_id`. Read that signal's evidence edges, recompute its three diversity scores. One graph read, one signal updated.

**Trigger**: `WorldEvent::CitationPublished`

### Actor extraction — incremental per signal

**Currently**: After all scraping, find signals without ACTED_IN edges in bounding box, batch-LLM them.

**Incrementally**: When a signal is created, extract actors from its text as part of the creation flow. The dedup handler already has the signal's title and summary. Actor extraction could happen right there — or as a reactive handler on signal creation.

The current approach queries `NOT ()-[:ACTED_IN]->(n)` which is just "signals we haven't processed yet." That's naturally incremental — new signals are the unprocessed ones.

**Trigger**: `WorldEvent::ConcernRaised` / `ResourceOffered` / etc. (any signal creation)

Batching still makes sense here for LLM efficiency (8 signals per call). Could batch per-source rather than per-run: after a scrape produces N signals, extract actors from that batch.

### Actor stats — incremental per link

**Currently**: After all scraping, count ACTED_IN edges per actor.

**Incrementally**: When `ActorLinkedToSignal` fires, increment that actor's count. Simple counter, no graph query needed.

**Trigger**: `SystemEvent::ActorLinkedToSignal`

### Actor location — incremental per signal

**Currently**: After all scraping, triangulate all actors.

**Incrementally**: When a signal with a location is linked to an actor, re-triangulate that actor. Only actors with new evidence need updating.

**Trigger**: `SystemEvent::ActorLinkedToSignal` (if the signal has location data)

### Cause heat — incremental per concern

**Currently**: Computed by supervisor after synthesis.

**Incrementally**: When a new concern is created or a new EVIDENCE_OF edge lands, recompute heat for that concern's cluster. The clustering is local — it doesn't need the full graph.

**Trigger**: `WorldEvent::ConcernRaised` or `WorldEvent::CitationPublished` (for concerns)

### Severity inference — incremental per signal

**Currently**: After synthesis, re-evaluate announcements based on EVIDENCE_OF edges.

**Incrementally**: When a new evidence edge lands on an announcement, re-evaluate that announcement's severity.

**Trigger**: Evidence edge projection events

## What This Means for Multiple Scouts

With incremental measurement, parallel scouts just work:

- Scout A scrapes Instagram, creates signals, weights update per-source
- Scout B scrapes web, creates signals, diversity updates per-citation
- No coordination needed — each event triggers its own incremental update
- Graph converges naturally as events flow in from any scout

No "measurement phase" means no question of "whose measurement phase runs when." Each fact triggers its own downstream computation, regardless of which scout produced it.

## Tension/Response: Lens, Not Phase

The tension/response split stays as a way to guide the LLM:

- **Tension lens**: "Look for problems, concerns, needs, friction"
- **Response lens**: "Look for resources, services, aid, solutions"

Same source can be scraped with both lenses. An Instagram account might surface concerns AND resources. The lens determines what the LLM pays attention to — it's a prompt parameter, not a pipeline stage.

## Source Expansion as Reactive

**Currently**: All tension sources scraped → batch source expansion → scrape all response sources.

**Reactively**: When a concern is raised, immediately look for who's responding. No need to wait for all tension scrapes to finish.

```
ConcernRaised (from any source, any scout)
  └→ expand: discover response sources for this concern
       └→ new sources created → scraped with response lens
```

Each concern carries enough context (category, location, description) to drive source expansion independently. The current batch approach collects all concerns and expands them together, but the expansion logic doesn't actually need cross-concern context — it queries per concern.

**Benefits:**
- Response sources start scraping earlier — don't wait for slowest tension source
- Natural parallelism: concern A's response sources scrape while tension source B is still being scraped
- Multiple scouts: Scout A finds a concern, response source discovery starts immediately while Scout B continues tension scraping
- Simpler: no `response_scrape_done()` gate, no `TensionScrapeCompleted` → `SourceExpansionCompleted` → response scrape cascade

**What this replaces:**
- `tension_done_expansion_pending` filter
- `expand_sources` handler gated on all tension scrapes completing
- The entire "source expansion phase" between tension and response scraping
- `ResponseScrapeSkipped` event (no skip needed — expansion just fires per concern or doesn't)

## What Changes

### Disappears
- `EnrichmentRoleCompleted` / `SynthesisRoleCompleted` superset gates
- `response_scrape_done()` gate
- `tension_done_expansion_pending` filter and the source expansion phase
- `ResponseScrapeSkipped` event (no skip needed — if no concerns, no expansion fires)
- `MetricsCompleted` → `ExpansionCompleted` → synthesis cascade
- The enrichment domain (absorbed into reactive handlers)
- The synthesis domain (absorbed into reactive handlers)
- The expansion domain (absorbed into reactive handlers)
- `completed_enrichment_roles`, `completed_synthesis_roles` state tracking
- `enrichment_completing_role`, `synthesis_completing_role` guards
- Most completion flags in PipelineState
- The "measurement phase" entirely

### Stays
- Tension/response as LLM extraction lens
- Event sourcing, projections, aggregate state
- Dedup pipeline (scrape → extract → dedup → project)
- Seesaw as the engine — this is what it was designed for

### Reorganizes
- Investigation: synthesis phase → reactive on signal creation
- Response finding: synthesis phase → reactive on signal creation
- Gathering finding: synthesis phase → reactive on signal creation
- Concern linking: synthesis phase → reactive on signal creation
- Similarity: synthesis phase → reactive on signal creation
- Source expansion: gated on all tension scrapes → reactive on concern creation
- Link promotion: gated on phase completion → reactive on scrape completion
- Query expansion: gated on MetricsCompleted → reactive on scrape completion
- Source weights: batch end-of-run → incremental per scrape completion
- Diversity: batch end-of-run → incremental per citation
- Actor extraction: batch end-of-run → per source or per signal
- Actor stats: batch end-of-run → incremental per actor link

## Threshold-Based Reactions: The Debounce Pattern

Some operations need critical mass before they're worth running. Story weaving, situation clustering, and similar graph-wide narrative operations produce poor results with too few signals. But they don't need a "phase" — they need a debounce.

**Pattern: "Every N signals or every T hours, whichever comes first."**

```
Signal created
  └→ threshold check: signals_since_last_weave >= N || hours_since_last_weave >= T
       ├→ yes: weave, reset counters
       └→ no: skip
```

Two counters on the aggregate, one conditional. Still reactive — the handler fires on signal creation, checks the threshold, and either acts or waits. No phase gates, no superset checks.

**Terminal event as fallback**: At end of run, always run these operations if there's unprocessed material. Belt and suspenders — the threshold catches it mid-run when there's enough new signal, the terminal event catches whatever's left.

**Works across parallel scouts**: Whoever crosses the threshold first triggers the operation. No coordination needed. The counters are on the shared aggregate — incremented by any scout's signals.

### Candidates for threshold-based reactions

| Operation | Threshold (N signals) | Timeout (T hours) | Why |
|-----------|----------------------|-------------------|-----|
| Story weaving | 10-15 new signals | 4-6 hours | Needs enough narrative threads to weave |
| Situation clustering | 5-10 new concerns | 4-6 hours | Needs cluster density |
| Cause heat recomputation | 3-5 new concerns | 2-4 hours | Heat changes meaningfully with new evidence |
| Severity inference | 5 new evidence edges | 4 hours | Re-evaluation needs meaningful new evidence |

### What this replaces

Currently these operations are gated on phase completion (all synthesis done, all enrichment done). The threshold pattern replaces phase gates with data-driven triggers — the operation runs when there's enough material, not when a stage finishes.

## Code Organization

```
domains/
  scrape/          — fetch content, extract signals, dedup, project
  discovery/       — find and register new sources (link promotion, expansion)
  curiosity/       — investigate, find responses/gatherings, link concerns, similarity
  scheduling/      — source weights, cadence, dead source cleanup
```

Four domains. No phases. Each handler declares its trigger event and fires when it arrives.

## Open Questions

1. **Actor extraction batching**: Per-signal LLM calls are expensive. Batch per-source (after scrape completes, extract actors from that source's new signals) may be the sweet spot. Still incremental, but efficient.

2. **Similarity timing**: Computing similarity requires embeddings of both signals. Incremental similarity means partial results as new signals arrive. Is that OK, or does similarity need a "settling" period?

3. **Response mapping direction**: If a concern arrives after its response is already in the graph, the concern's reactive handler should find the existing response. Bidirectional: both sides trigger the search.

4. **Budget granularity**: Per-signal investigation is more granular. Budget needs per-operation cost tracking instead of per-phase. Already partially there with OperationCost.

5. **Idempotency on refresh**: If a signal is refreshed (DedupOutcome::Refreshed), should investigation re-run? Probably not for mere freshness confirmation. Only for new/changed signals.

6. **Source weight race conditions**: If two scouts scrape the same source simultaneously, both try to update its weight. Idempotent MERGE in Neo4j handles this — last write wins, and the computation is deterministic given the same inputs.

7. **Dead source detection**: Currently batch-queries graph for sources with 10+ empty runs. Could fire per-source after each empty scrape: if consecutive_empty_runs >= 10, deactivate. Simpler, no batch needed.
