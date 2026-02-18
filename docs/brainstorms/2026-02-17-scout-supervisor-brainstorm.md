---
date: 2026-02-17
topic: scout-supervisor
---

# Scout Supervisor

## What We're Building

A standalone Rust crate (`rootsignal-scout-supervisor`) that periodically inspects the graph for problems introduced by the scout's ingestion pipeline. It auto-fixes deterministic issues (orphaned nodes, expired events, actor dedup) and flags ambiguous ones (misclassification, incoherent stories, bad edges) via pluggable notification backends like Slack, with routing so different issue categories can go to different channels.

Critically, this is not just a janitor — it's a feedback loop that makes the scout anti-fragile. Every problem it finds should make the scout less likely to repeat that mistake.

## Why This Approach

The scout writes aggressively — it has no minimum confidence threshold, and the LLM extraction can misclassify signals, hallucinate details, or produce near-duplicates that slip through the 0.85–0.92 similarity gap. Today there's no feedback loop to catch these problems. Rather than slowing down the scout's write path with inline validation, a decoupled periodic checker keeps ingestion fast while surfacing real bugs in the extraction logic over time.

## Core Loop

1. Read last-run timestamp (from a `ValidatorState` node or local state)
2. Query signals where `extracted_at > last_run`, stories where `last_updated > last_run`
3. Run deterministic checks → auto-fix
4. Run cheap heuristic triage → flag suspects for LLM review
5. LLM-powered checks on suspects only (cost-bounded)
6. Send notifications via pluggable backend
7. Apply feedback loops (source penalties, extraction rules, threshold adjustments)
8. Update last-run timestamp

## Auto-Fix Checks (deterministic, safe)

- Expired events (`ends_at` in the past, not yet reaped)
- Orphaned Evidence nodes (no `SOURCED_FROM` edge)
- Orphaned `ACTED_IN` edges (Actor or Signal deleted, edge remains)
- Soft duplicate Actors (normalized name match)
- Signals with near-city-center coordinates that slipped through geo-filter
- Signals with empty/null titles or summaries

## Flag Checks (ambiguous, needs judgment)

- **Misclassification** — LLM re-reads Evidence snippets, disagrees with signal type
- **Incoherent stories** — LLM reviews a story's signals together, finds no coherent narrative
- **Bad RESPONDS_TO edges** — give/event doesn't actually address the linked tension
- **Near-duplicate signals** — pairs in the 0.85–0.92 similarity range that should have been corroborated
- **Low-confidence signals in high-visibility positions** — in a confirmed story or featured edition
- **Contradictory actor roles** — same Actor marked as organizer and opponent in related signals

## Feedback Loops (anti-fragility)

The validator is not just a janitor — it's a teacher. Every problem it finds should make the scout less likely to repeat that mistake.

### Source weight penalties

Sources that consistently produce flagged signals (misclassified, hallucinated, incoherent) get their `weight` reduced by the validator. The scout already uses weight for scheduling priority, so bad sources naturally get scraped less. This closes a gap: the scout only deactivates sources that produce *nothing*, not sources that produce *junk*.

### Extraction rules

Repeated failure patterns become codified rules stored as `ExtractionRule` nodes in the graph. Examples:

- "Domain X always produces hallucinated dates → strip dates from X"
- "Reddit posts are misclassified as Notices 60% of the time → bias toward Ask/Tension for Reddit"

The scout reads these at startup and applies them during extraction.

### Confidence floor calibration

The validator tracks the correlation between confidence scores and flag rates. If signals below a threshold are flagged at a high rate, it writes an updated `min_confidence` to a config node that the scout respects. The floor tightens automatically as evidence accumulates.

### Dedup threshold recommendations

Track the rate of near-duplicate pairs found in the 0.85–0.92 similarity gap. If the rate is high, recommend (or directly adjust) the cross-source corroboration threshold downward.

### Story → signal tracing

When a story is flagged as incoherent, don't just flag the story. Trace back to the constituent signals and identify which ones are the root cause (bad summary, wrong type, hallucinated details). Flag those signals specifically so the pattern can feed into extraction rules.

## Notification Design

Pluggable backend trait:

```rust
trait NotifyBackend {
    async fn send(&self, issue: &ValidationIssue) -> Result<()>;
}
```

Implementations: Slack (webhook), GitHub Issues, generic webhook. Routing config maps issue category/severity to a destination channel. This lets us steer e.g. "misclassification" flags to one Slack channel and "orphaned nodes" auto-fix summaries to another.

## Key Decisions

- **Decoupled from scout**: separate crate, own schedule, reads and writes to graph
- **Incremental via `extracted_at`**: no new timestamp needed, watermark-based
- **Cost-bounded LLM checks**: cheap graph queries triage first, LLM only on suspects
- **Auto-fix vs flag split**: deterministic problems fixed silently, ambiguous ones notify humans
- **Pluggable notifications**: trait-based backends with per-category routing
- **Anti-fragile feedback**: validator findings feed back into scout behavior (weights, rules, thresholds)

## Open Questions

- How aggressive should source weight penalties be? Gradual decay vs hard penalty after N flags?
- Should extraction rules be auto-applied or require human approval before the scout uses them?
- What's the minimum sample size before the validator adjusts confidence floors or dedup thresholds?
- Should the validator run on its own schedule or trigger after each scout run completes?
- How do we prevent feedback loops from over-correcting? (e.g., validator lowers threshold → scout corroborates too aggressively → validator sees fewer dupes → raises threshold). Probably small increments with a cooldown period.
- Should auto-fixes be logged/summarized in a periodic digest, or only flagged issues?
- Should the validator write `ValidationIssue` nodes into the graph (for API access) in addition to sending notifications?
- How do we handle "resolved" flags — manual dismissal, auto-expire, or re-validate?

## Next Steps

→ `/workflows:plan` for implementation details
