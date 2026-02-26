---
title: "feat: Signal Lint — Post-Pipeline Audit Before Publishing"
type: feat
date: 2026-02-24
---

# Signal Lint

## Overview

A post-pipeline audit step that verifies every extracted signal against its source content before publishing. An LLM with tool access checks extraction fidelity, completeness, and structural integrity — auto-correcting fixable issues and quarantining the rest. Replaces the supervisor's batch review as the promotion gate from `staged` → `live`.

## Problem Statement

Signals currently go through extraction, dedup, and quality scoring — but none of these verify that the extracted data actually matches the source content. The supervisor's batch review does a blind LLM check without re-reading sources. This means hallucinated titles, wrong dates, fabricated locations, and broken action URLs can make it into the live graph.

## Proposed Solution

Insert a **signal lint** step at the end of the full run pipeline, after all signal creation (scrape + synthesis) is complete but before signals are promoted to `live`. The lint step:

1. Queries all `staged` signals created during this run
2. Groups them by source URL
3. For each group, fetches the archived source content and sends it + signals to an audit LLM with tools
4. LLM verifies every field, auto-corrects what it can, quarantines what it can't
5. Passing signals are promoted to `live`

### Pipeline Placement

```
Bootstrap → Scrape → Synthesis → SituationWeaver → **Signal Lint** → Supervisor (reduced)
```

Lint runs where the supervisor's batch review currently sits. The existing supervisor continues to run afterward for auto-fixes, echo detection, source penalties, and other housekeeping — just without the batch review step.

This ordering works because:
- Synthesis already reads `staged` signals (it doesn't filter on `live`)
- Lint catches issues from ALL signal creators (scraper, tension_linker, response_finder, investigator)
- The supervisor's remaining phases operate on `live` signals after lint promotes them

### Status Model

Keep existing naming to avoid churn across 15+ reader queries:
- `staged` — freshly extracted, awaiting lint (already exists)
- `live` — passed lint, visible to public queries (already exists)
- `quarantined` — failed lint, hidden from public, flagged for review (new)

No migration needed for `staged`/`live`. Add `quarantined` as a new valid status.

## Technical Approach

### Architecture

```
SignalLintWorkflow (Restate workflow)
  │
  ├── Query staged signals for this run (by run_id + region)
  ├── Group by source_url
  │
  └── For each source group (ctx.run per batch):
      ├── Fetch archived source content
      ├── Send signals + content to audit LLM with tools
      ├── LLM calls tools to verify fields
      ├── Apply corrections (with reasons logged)
      ├── Promote passing signals to 'live'
      ├── Quarantine failing signals
      └── Log all events to run log
```

### LLM Tool Schema

The audit LLM gets these tools:

**`read_source`** — Fetch archived content for a source URL. Returns markdown/text content as stored in the archive. For social posts, returns the post content from the archive tables. Does NOT re-fetch from the web.

**`correct_signal`** — Update specific fields on a signal with a reason. Allowlisted fields only:
- `title`, `summary` — text corrections
- `starts_at`, `ends_at`, `content_date` — date corrections
- `location_name`, `about_location` (lat/lng) — location corrections
- `action_url` — URL correction
- `organizer`, `what_needed`, `goal` — type-specific field corrections
- `sensitivity`, `severity`, `category` — classification corrections

Immutable fields (cannot be corrected, must quarantine instead): `signal_type`, `source_url`, `id`

**`quarantine_signal`** — Mark a signal as quarantined with a reason string. Used when the signal is fundamentally wrong (wrong type, hallucinated content, no basis in source).

**`pass_signal`** — Mark a signal as verified. Promotes to `live`.

When the `read_source` tool fails (archive miss, truncated content), the LLM should quarantine all signals from that source with reason `SOURCE_UNREADABLE` rather than guessing.

No URL validation or geocoding tools in v1 — keep it focused on source content verification. These can be added later.

### Batching Strategy

- One LLM call per source URL (fetch source once, verify all signals from it)
- If a source URL has >15 signals, split into sub-batches
- Each batch is one `ctx.run()` in Restate (atomic journaling)
- Target: ~50k tokens max per LLM call (source content + signal data + tool schema)
- Model: `claude-sonnet-4-5-20250514` — deliberately different model class than the extractor (Haiku) to avoid shared inductive biases. Using the same model family for both extraction and linting risks "consistent hallucinations" where the linter agrees with the extractor's mistakes. Sonnet is more expensive per call but lint is batched by source URL (far fewer calls than extraction), so the cost premium is manageable.

### Run ID Scoping

Query signals by `run_id` matching the current full run's scrape phase run ID. This isolates lint to signals from this run only, preventing interference with concurrent runs.

For signals created by synthesis modules (which generate their own `run_id`), query additionally by region + `extracted_at` within the run's time window. Alternatively, thread the full run's `run_id` through synthesis modules.

### Audit Trail

New `EventKind` variants for the run log:

```rust
EventKind::LintBatch {
    source_url: String,
    signal_count: usize,
    passed: usize,
    corrected: usize,
    quarantined: usize,
}

EventKind::LintCorrection {
    node_id: Uuid,
    signal_type: String,
    title: String,
    field: String,
    old_value: String,
    new_value: String,
    reason: String,
}

EventKind::LintQuarantine {
    node_id: Uuid,
    signal_type: String,
    title: String,
    reason: String,
}
```

These nest under the lint phase's parent event in the trace tree.

### Supervisor Changes

Remove `batch_review` from the supervisor's phase list. Keep:
- Phase 1: auto_fix (deterministic cleanup)
- Phase 2: triage_suspects (heuristic queries — repurpose for quarantine review stats)
- Phase 4: source_penalty (feedback loop on bad sources)
- Echo detection, duplicate merging, cause heat, beacon detection

The supervisor no longer promotes signals — lint does that.

## Implementation Phases

### Phase 1: Lint Module + Traits

Create the signal lint module with trait-based dependencies for testability.

**Files:**
- `modules/rootsignal-scout/src/pipeline/signal_lint.rs` — core lint logic
- `modules/rootsignal-scout/src/pipeline/lint_tools.rs` — tool definitions and execution

**Key types:**
```rust
pub struct SignalLinter {
    store: Arc<dyn SignalStore>,
    fetcher: Arc<dyn ContentFetcher>,
    anthropic_api_key: String,
    region: ScoutScope,
    run_id: String,
}

pub enum LintVerdict {
    Pass,
    Correct { corrections: Vec<FieldCorrection> },
    Quarantine { reason: String },
}

pub struct FieldCorrection {
    field: String,
    old_value: String,
    new_value: String,
    reason: String,
}
```

**SignalStore additions:**
```rust
// New methods on the SignalStore trait
async fn staged_signals_for_run(&self, run_id: &str, region: &str) -> Result<Vec<StagedSignal>>;
async fn update_signal_fields(&self, id: Uuid, corrections: &[FieldCorrection]) -> Result<()>;
async fn set_review_status(&self, id: Uuid, status: &str) -> Result<()>;
```

### Phase 2: LLM Audit with Tool Use

Build the LLM integration using the Anthropic tool-use API (not structured output — lint needs multi-turn tool calling).

**Files:**
- `modules/ai-client/src/lib.rs` — add tool-use support if not already present

**Approach:**
- Send system prompt with lint instructions + region context
- Include all signals from one source URL as structured data
- Provide tools: `read_source`, `correct_signal`, `quarantine_signal`, `pass_signal`
- Let the LLM call tools in a loop until all signals have a verdict
- Parse tool call results, apply corrections, record verdicts

### Phase 3: Workflow Integration

Wire lint into the full run pipeline as a Restate workflow step.

**Files:**
- `modules/rootsignal-scout/src/workflows/lint.rs` — Restate workflow wrapper
- `modules/rootsignal-scout/src/workflows/full_run.rs` — add lint step after synthesis
- `modules/rootsignal-scout/src/workflows/mod.rs` — register new workflow
- `modules/rootsignal-scout/src/workflows/supervisor.rs` — remove batch_review invocation

**Restate pattern:**
```rust
// In full_run.rs, after synthesis and situation weaving:
ctx.run(|| async {
    journaled_write_task_phase_status(&ctx, &deps, &task_id, "lint").await?;
    let linter = SignalLinter::new(/* deps */);
    linter.run(&logger).await
}).await?;
```

Each source-URL batch is a separate `ctx.run()` for granular journaling.

### Phase 4: Run Log Events + Admin UI

Add lint event types and surface them in the trace tab.

**Files:**
- `modules/rootsignal-scout/src/infra/run_log.rs` — new EventKind variants
- `modules/admin-app/src/components/SourceTrace.tsx` — render lint events
- `modules/admin-app/src/graphql/queries.ts` — include lint events in trace query

### Phase 5: Testing

Follow MOCK → FUNCTION → OUTPUT pattern.

**Files:**
- `modules/rootsignal-scout/tests/signal_lint_test.rs` — integration tests

**Test cases:**
- `signal_matching_source_content_passes_lint` — correct signal gets promoted to live
- `signal_with_wrong_date_gets_corrected` — LLM fixes date, signal promoted with correction logged
- `signal_with_hallucinated_content_gets_quarantined` — no basis in source, quarantined
- `empty_run_produces_no_lint_events` — zero signals, lint returns immediately
- `source_content_unavailable_quarantines_signals` — archive miss, signals quarantined
- `corrections_are_logged_to_run_log` — verify audit trail events
- `quarantined_signals_not_visible_in_public_queries` — reader filters work

**Mock setup:**
- `MockSignalStore` with staged signals pre-loaded
- `MockContentFetcher` returning fixture HTML/markdown
- Fixed LLM responses (snapshot-based or hand-crafted)

## Acceptance Criteria

- [x] All signals created during a scout run pass through lint before becoming `live`
- [x] Lint verifies signal fields against archived source content
- [x] Fixable issues are auto-corrected with reasons logged
- [x] Unfixable signals are quarantined (hidden from public, visible in admin)
- [x] Full audit trail in run log: every correction, quarantine, and pass
- [x] Supervisor batch review removed; other supervisor phases preserved
- [x] Public queries continue to show only `live` signals (no change needed — already filtered)
- [ ] `quarantined` signals visible in admin trace tab with reasons

## Operational

- **Stale staged alert**: Monitor for signals stuck in `staged` status for >6 hours. If lint crashes or times out, signals remain staged (safe — not visible to public). Alert triggers investigation.
- **Lint is the promotion gate**: If lint doesn't run, nothing goes live. This is intentional — better to delay publication than publish unverified data.

## Open Questions (Deferred to v2)

- **Quarantine pruning** — how long do quarantined signals stay? Add to reaper workflow. Default: 30 days.
- **Budget tracking** — lint adds LLM cost. Wire into BudgetTracker. For v1, monitor manually.
- **scrape_url standalone** — standalone single-URL scrapes bypass lint. For v1, these write directly as `live` (implicit human oversight). Consider adding inline lint for v2.

## References

- **Brainstorm**: `docs/brainstorms/2026-02-24-signal-lint-brainstorm.md`
- **Existing review_status**: `modules/rootsignal-graph/src/writer.rs` (nodes created as `staged`)
- **Public query filtering**: `modules/rootsignal-graph/src/reader.rs` (filters on `review_status = 'live'`)
- **Supervisor batch review**: `modules/rootsignal-scout-supervisor/src/checks/batch_review.rs`
- **Run log events**: `modules/rootsignal-scout/src/infra/run_log.rs`
- **Full run orchestration**: `modules/rootsignal-scout/src/workflows/full_run.rs`
- **Validation issues table**: `modules/rootsignal-api/migrations/010_validation_issues.sql`
- **Data quality learning**: `docs/solutions/2026-02-17-unwrap-or-masks-data-quality.md`
- **Testing patterns**: `docs/brainstorms/2026-02-23-scout-pipeline-testing-brainstorm.md`
