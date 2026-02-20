---
date: 2026-02-20
topic: finder-scoping-and-general-supervisor
---

# Finder Bounding Box Scoping + General-Purpose Supervisor

## What We're Building

Two related fixes discovered during a Chicago scout run:

1. **Scope all finder target queries to city bounding box** — `response_finder`, `gathering_finder`, and `investigator` pull targets globally from the graph, causing cross-city contamination. The Chicago scout investigated a Minneapolis tension ("Youth Violence Spike in North Minneapolis"), created 7 emergent tensions referencing Minneapolis neighborhoods, and stamped them with Chicago's center coordinates.

2. **Make the supervisor more general** — instead of a checklist of specific failure modes, the supervisor should consume a batch of signals and use an LLM to identify what looks off. This catches novel issues that no one anticipated, like cross-city references in emergent tension text, speculative content, hallucinated organizations, etc.

## The Bug We Found

Running `cargo run --bin scout -- chicago` produced 11 signals:
- 7 emergent tensions with `source_url: <UNKNOWN>`, referencing "NAZ", "Penn Ave", "North Minneapolis" — all Minneapolis content geolocated to Chicago center (41.8781, -87.6298)
- All 11 signals at exact city center coordinates (no real geocoding)
- 0 stories created (not enough connected signals)

**Root cause:** `find_response_finder_targets` in `writer.rs:3386` has no bounding box filter:

```sql
MATCH (t:Tension) WHERE t.confidence >= 0.5 AND ...
```

So it picked up Minneapolis tensions. `tension_linker` already takes `min_lat/max_lat/min_lng/max_lng` params — the other finders don't.

**Current state of finder scoping:**

| Finder | Bounding box? | File |
|--------|--------------|------|
| `find_tension_linker_targets` | Yes | `writer.rs:2644` |
| `find_response_finder_targets` | **No** | `writer.rs:3382` |
| `find_gathering_finder_targets` | **No** | `writer.rs:3486` |
| `find_investigation_targets` | **No** | `writer.rs:2500` |

## Part 1: Finder Bounding Box Scoping

This is a straightforward bug fix. Apply the same pattern `tension_linker` uses:

1. Add `min_lat/max_lat/min_lng/max_lng` params to `find_response_finder_targets`, `find_gathering_finder_targets`, `find_investigation_targets`
2. Add `WHERE t.lat >= $min_lat AND t.lat <= $max_lat AND t.lng >= $min_lng AND t.lng <= $max_lng` to each query
3. Update the call sites in `scout.rs` to pass the bounding box (already calculated for `tension_linker`)
4. Update the corresponding `ResponseFinder::new()`, `GatheringFinder::new()`, `Investigator::new()` to accept and pass through the bounds

**No design decisions needed** — this follows an existing pattern.

## Part 2: General-Purpose Supervisor

### Why Change the Supervisor?

The current supervisor (6 phases) checks for specific, anticipated failure modes:
- Phase 1: Auto-fix (orphaned nodes, empty titles, fake city-center coords)
- Phase 2: Heuristic triage (5 specific suspect patterns)
- Phase 3: LLM validation (confirm/deny suspects from Phase 2)
- Phase 4: Notifications
- Phase 5: Source quality penalties
- Phase 6: Echo detection

**Problem:** It can only catch what it was programmed to catch. The Chicago bug — Minneapolis tensions with Chicago coordinates, `<UNKNOWN>` source URLs, speculative policy analysis instead of observed signals — wouldn't be caught by any existing check.

A human looking at these 11 signals would immediately say "these 7 tensions are about Minneapolis, not Chicago" and "these read like a brainstorm, not real signals." The supervisor should work the same way.

### What to Keep vs. Replace

**Keep:**
- **Phase 1 (auto-fix)** — deterministic graph hygiene is cheap and correct. No reason to LLM this.
- **Phase 5 (source penalties)** — structural feedback loop, not a detection problem
- **Phase 6 (echo detection)** — graph-structural check with a clear formula

**Replace:**
- **Phases 2-3 (triage + LLM validation)** — replace with a general LLM review pass

### Approach A: Batch Signal Review (Recommended)

After each scout run, collect all signals created/modified since the last watermark. Bundle them with full context and ask a single LLM call: "Here are the signals from this scout run in Chicago. What looks off?"

**Input to the LLM:**
- City name, center coordinates, bounding box
- Each signal: type, title, summary, source_url, lat/lng, confidence, sensitivity
- For tensions: what_would_help, severity, category
- Story membership (if any)
- Source that produced it

**Structured output:**
```
[
  {
    "signal_id": "...",
    "issue_type": "cross_city_contamination" | "speculative_content" | "hallucinated_source" | "misclassification" | "near_duplicate" | "low_quality" | "other",
    "severity": "error" | "warning" | "info",
    "description": "This tension references North Minneapolis neighborhoods but is geolocated to Chicago",
    "suggested_action": "delete" | "flag_for_review" | "reduce_confidence" | "reclassify"
  }
]
```

**Pros:**
- Catches novel issues nobody anticipated
- One LLM call per batch (cost-efficient)
- Human-like reasoning about data quality
- Self-improving — as the LLM gets smarter, so does the supervisor

**Cons:**
- Non-deterministic — may flag different things on different runs
- Requires good prompt engineering to avoid false positives
- LLM cost per run (though batching helps)

**Best when:** You want a safety net that catches the unexpected.

### Approach B: Per-Signal Review

Review each signal individually with full context about the city and source. More expensive but more thorough.

**Pros:**
- Deeper analysis per signal
- Can include the source content for grounding checks

**Cons:**
- Much more expensive (N calls instead of 1)
- Slower
- Overkill for obviously-fine signals

**Best when:** You have a small number of signals per run and want maximum thoroughness.

### Approach C: Hybrid — Cheap Triage + General Review

Keep the existing heuristic triage (Phase 2) as a pre-filter, but replace the specific LLM validation (Phase 3) with a general review. Only send signals that pass cheap sanity checks to the general reviewer.

**Pros:**
- Cheapest option — LLM only sees suspects
- Deterministic checks still catch known patterns fast

**Cons:**
- The pre-filter might miss novel issues (the whole point of going general)
- More code to maintain

**Best when:** Cost is the primary concern.

## Key Decisions

- **Approach A (batch signal review)** is the right call. The whole point is catching things we didn't anticipate. A pre-filter defeats that purpose.
- **Keep auto-fix phase** — deterministic hygiene shouldn't need LLM reasoning
- **Keep source penalties and echo detection** — structural, not detection problems
- **Batch size cap** — if a run produces 200+ signals, chunk into batches of ~50 to stay within context window
- **Action on findings** — supervisor should auto-apply "delete" and "reduce_confidence" actions, flag "reclassify" for human review

## Open Questions

- Should the general review also look at stories (not just signals)?
- Should it review the emergent tensions and response edges specifically, since those are LLM-generated and most prone to hallucination?
- What's the right model? Haiku for cost, Sonnet for quality? Could start with Sonnet and downgrade if costs are fine.
- Should the prompt include examples of past issues to guide the LLM? (few-shot vs zero-shot)

## Next Steps

1. Fix finder bounding box scoping (straightforward bug fix)
2. Plan the supervisor generalization (design the prompt, output schema, action handlers)
