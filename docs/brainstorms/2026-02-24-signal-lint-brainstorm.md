---
date: 2026-02-24
topic: signal-lint
---

# Signal Lint

## What We're Building

A post-scrape audit step that verifies every signal against its source content before publishing. An LLM with tool access checks extraction fidelity, completeness, and structural integrity — auto-correcting what it can and quarantining what it can't. Replaces the supervisor batch review.

## Why This Approach

The existing supervisor does a blind LLM review without re-reading source content. Signal lint goes deeper: it re-reads the source, compares every field, and catches hallucinations, missed signals, bad dates, wrong locations, and broken URLs. It runs at the end of each scout task, not as a separate workflow.

### Approaches Considered

1. **Restate journal as staging store** — stage signals in memory, journal via `ctx.run()`, only write to graph after lint. Rejected: major refactor to the write path, dedup breaks (signals not in graph for vector similarity), can't query/inspect staged signals, Restate journal is replay machinery not a data store.

2. **Graph with status field (chosen)** — write signals as `staged`, lint flips to `published` or `quarantined`. Zero changes to existing scrape pipeline. Dedup, evidence, actors, schedules all work as-is. Staged signals are queryable and visible in admin.

## Pipeline

1. **Scrape** — existing pipeline, writes signals to graph with `status = 'staged'`
2. **Lint** — audit LLM with tools verifies every field against source content
3. **Publish** — passing signals flipped to `published`, failures to `quarantined`

## Signal Lint: What Gets Checked

Full-spectrum audit of every signal field against source content:

- **Extraction fidelity** — title, summary, type match what the source actually says (no hallucination/embellishment)
- **Completeness** — no signals missed that were clearly present in the source
- **Structural integrity** — dates parse and make sense, URLs resolve, coordinates are real and match location name, schedule is correct, type matches content, all required fields populated

## Audit LLM Tools

- **Fetch source content** — retrieve archived page/posts/feed by URL
- **Read signal** — get full signal node with all fields
- **Correct signal** — update specific fields with a change reason
- **Quarantine signal** — mark as quarantined with a reason
- **Validate URL** — check if a URL resolves
- **Validate location** — confirm coordinates match location name

## Outcomes Per Signal

- **Pass** — signal is correct, flip to `published`
- **Correct** — fixable issues, auto-correct with change note logged, flip to `published`
- **Quarantine** — unfixable issues, mark as `quarantined` with reason, flagged for review

## Audit Trail

Full run log of every tool call, LLM response, correction, and quarantine decision. Visible in admin trace tab.

## Key Decisions

- **Graph status field over Restate staging**: simpler, preserves all existing behavior, staged data is queryable. One where-clause change to read paths.
- **Replaces supervisor batch review**: signal lint is more pointed — it reads the actual source content instead of doing blind narrative review.
- **Batch by source URL**: fetch source once, verify all signals from it. Efficient.
- **Quarantine, not delete**: bad signals are recoverable if the audit was wrong.
- **Auto-correct with paper trail**: every correction gets a reason logged to the run log.

## Open Questions

- Quarantine cleanup: how long do quarantined signals stay in graph before pruning?
- Admin UI: surface lint results in trace tab, or a dedicated lint tab?
- Batch size: how many signals per LLM call before splitting?
- Should lint check for missed signals (completeness) by re-reading source and comparing, or just verify what was extracted?

## Next Steps

`/workflows:plan` for implementation details.
