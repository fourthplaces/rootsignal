---
title: "feat: Scout Supervisor"
type: feat
date: 2026-02-17
---

# Scout Supervisor

## Overview

A new Rust crate (`rootsignal-scout-supervisor`) that periodically validates the graph, auto-fixes deterministic issues, flags ambiguous ones via pluggable notifications, and feeds back into scout behavior for anti-fragility. Runs as a standalone binary on a per-city basis, scheduled externally (same pattern as the scout).

## Problem Statement

The scout writes aggressively — no minimum confidence threshold, LLM extraction can misclassify signals, hallucinate details, or produce near-duplicates that slip through the 0.85–0.92 similarity gap. Today there is no feedback loop. The same mistakes repeat every run. Sources that produce junk are never penalized (only sources that produce *nothing* get deactivated).

## Proposed Solution

A decoupled supervisor that:
1. Runs deterministic graph-hygiene checks and auto-fixes them
2. Uses cheap heuristics to triage suspects, then LLM-reviews only those
3. Sends notifications via pluggable backends (Slack first) with per-category routing
4. Feeds findings back into the scout's behavior (source penalties, extraction rules, confidence floors)

## Technical Approach

### Architecture

```
rootsignal-scout-supervisor/
  src/
    main.rs           # Binary entrypoint: load config, connect, run, exit
    lib.rs            # pub mod declarations
    supervisor.rs     # Core Supervisor struct + run() method
    checks/
      mod.rs
      auto_fix.rs     # Deterministic fixes
      triage.rs       # Cheap heuristic suspect identification
      llm.rs          # LLM-powered flag checks
    notify/
      mod.rs
      backend.rs      # NotifyBackend trait
      slack.rs        # SlackWebhook impl
      webhook.rs      # GenericWebhook impl
      noop.rs         # NoopBackend for tests
    feedback/
      mod.rs
      source_penalty.rs
      extraction_rules.rs
      confidence.rs
    state.rs          # SupervisorState node read/write (watermark)
    types.rs          # ValidationIssue, CheckResult, SupervisorStats
```

**Dependencies:** `rootsignal-graph`, `rootsignal-common`, `ai-client`. Does NOT depend on `rootsignal-scout` (avoids circular dependency).

### New Graph Nodes

#### SupervisorState

Singleton per city. Stores watermark and calibrated thresholds.

```cypher
CREATE CONSTRAINT ON (s:SupervisorState) ASSERT s.id IS UNIQUE;

-- Properties:
-- id: UUID
-- city: String
-- last_run: datetime
-- min_confidence: f64 (calibrated floor, starts at 0.0 = no filter)
-- dedup_threshold_recommendation: f64 (starts at 0.92)
-- version: i64 (schema version for future migration)
```

#### ExtractionRule

Codified failure patterns the scout reads at startup.

```cypher
CREATE CONSTRAINT ON (r:ExtractionRule) ASSERT r.id IS UNIQUE;

-- Properties:
-- id: UUID
-- city: String
-- rule_type: String (enum: "strip_dates", "bias_type", "skip_domain", etc.)
-- source_pattern: String (domain glob or source_type match)
-- instruction: String (natural language for LLM prompt injection)
-- approved: bool (default true for auto-apply; false = pending human review)
-- applied_count: i64 (how many times scout used this rule)
-- created_at: datetime
-- expires_at: Option<datetime> (auto-expire after pattern stops recurring)
```

**Decision: auto-apply by default.** Rules are created with `approved: true`. If a rule causes problems, operators can set `approved: false` via a CLI command or direct Cypher. This ships without requiring an admin UI. A future `--require-approval` flag can flip the default.

#### ValidationIssue

Written to graph for deduplication and API visibility.

```cypher
CREATE CONSTRAINT ON (v:ValidationIssue) ASSERT v.id IS UNIQUE;
CREATE INDEX ON :ValidationIssue(status);
CREATE INDEX ON :ValidationIssue(city);

-- Properties:
-- id: UUID
-- city: String
-- issue_type: String (enum: "misclassification", "incoherent_story", "bad_responds_to", "near_duplicate", "low_confidence_high_visibility")
-- severity: String (enum: "info", "warning", "error")
-- target_id: UUID (signal or story id)
-- target_label: String (e.g., "Event", "Story")
-- description: String (human-readable explanation)
-- suggested_action: String (what should be done)
-- status: String (enum: "open", "resolved", "dismissed")
-- created_at: datetime
-- resolved_at: Option<datetime>
-- resolution: Option<String> (how it was resolved)
```

**Deduplication:** before creating a new issue, check for an existing `open` issue with the same `target_id` and `issue_type`. If found, skip notification (suppresses re-alerting on every run).

**Resolved flow:** issues auto-expire after 30 days if still `open`. Operators can set `status: "dismissed"` to permanently suppress. Re-validation on next run creates a new issue if the problem persists after expiry.

### Source Quality Penalty (avoiding weight conflicts)

The scout writes `Source.weight` based on productivity metrics. To avoid overwriting, the supervisor writes to a **separate property**: `Source.quality_penalty` (f64, 0.0–1.0, default 1.0).

The scout's `compute_weight()` is updated to:
```rust
let effective_weight = computed_weight * source.quality_penalty;
```

**Penalty formula:** each flagged signal from a source reduces `quality_penalty` by 0.05, clamped to `[0.1, 1.0]`. Recovery: if a source produces 10+ unflagged signals in a row, `quality_penalty` increases by 0.02 per clean signal, capped at 1.0.

### Locking Strategy

1. **SupervisorLock** — same pattern as `ScoutLock`. Prevents two supervisor instances from running concurrently. 30-minute stale TTL.

2. **Scout awareness** — the supervisor checks for `ScoutLock` before its feedback-write phase (step 7). If held, it defers feedback writes and retries after a short sleep. Read-only check phases (steps 3–6) are safe to run concurrently with the scout.

3. **Weight coordination** — because the supervisor writes `quality_penalty` (not `weight`), there is no conflict with the scout's `weight` updates. They compose multiplicatively.

### City Scoping

One supervisor instance per city (same model as scout). `SupervisorState`, `ExtractionRule`, and `ValidationIssue` nodes are all scoped by `city` property.

### Watermark and Backlog

- Watermark stored in `SupervisorState.last_run`
- First boot: seed watermark to `now - 24h`
- **24h cap per run:** if `now - last_run > 24h`, process `last_run` to `last_run + 24h` only. Catches up over subsequent runs. Prevents cost blowout after outages.

### LLM Budget

- Supervisor gets its own `BudgetTracker` (vendored from scout's 120-line implementation, not imported)
- Default daily budget: configurable via `SUPERVISOR_DAILY_BUDGET_CENTS` env var
- Model selection per check type:
  - **Haiku**: misclassification (binary classification), near-duplicate confirmation
  - **Sonnet**: incoherent story review, bad RESPONDS_TO detection (narrative reasoning)
- Per-run suspect cap: max 50 LLM checks per run (configurable)

## Implementation Phases

### Phase 1: Foundation (crate scaffold + auto-fix checks)

**Goal:** Get the crate running and fixing deterministic issues.

1. Add `modules/rootsignal-scout-supervisor` to workspace `Cargo.toml`
2. Scaffold crate structure (`main.rs`, `lib.rs`, module stubs)
3. Add `Config::supervisor_from_env()` to `rootsignal-common`
   - `MEMGRAPH_URI`, `MEMGRAPH_USER`, `MEMGRAPH_PASSWORD` (required)
   - `ANTHROPIC_API_KEY` (required)
   - `CITY` (optional, default `"twincities"`)
   - `SLACK_WEBHOOK_URL` (optional)
   - `SUPERVISOR_DAILY_BUDGET_CENTS` (optional, default 100)
4. Add migrations for `SupervisorState`, `SupervisorLock` nodes
5. Implement `SupervisorState` read/write (watermark)
6. Implement `SupervisorLock` acquire/release
7. Implement auto-fix checks:
   - Orphaned Evidence nodes (no `SOURCED_FROM` edge) → `DETACH DELETE`
   - Orphaned `ACTED_IN` edges → `DELETE` edge
   - Soft duplicate Actors (case-insensitive, punctuation-stripped name match) → merge into Actor with more signals, re-point edges
   - Signals with empty/null titles → delete signal + detach evidence
   - Near-city-center coordinates that slipped through → `SET lat = null, lng = null`
8. Implement `SupervisorStats` struct with `Display` impl
9. Wire up `main.rs`: config → connect → migrate → acquire lock → run checks → release lock

**Note:** Expired event reaping is skipped — the scout's `reap_expired()` already handles this. The supervisor shouldn't duplicate that work.

**Success criteria:**
- `cargo run --bin supervisor` connects, runs auto-fixes, logs stats, exits
- Auto-fix checks are idempotent (running twice produces same result)

### Phase 2: Notification system

**Goal:** Pluggable notification with Slack as first backend.

1. Define `NotifyBackend` trait:
   ```rust
   #[async_trait]
   pub trait NotifyBackend: Send + Sync {
       async fn send(&self, issue: &ValidationIssue) -> Result<()>;
       async fn send_digest(&self, stats: &SupervisorStats) -> Result<()>;
   }
   ```
2. Implement `SlackWebhook` backend (POST to incoming webhook URL via `reqwest`)
   - Format: issue type as header, description as body, link to signal in web UI
   - Rate limit: batch issues into a single digest message if > 5 per run
3. Implement `NoopBackend` for tests
4. Implement `GenericWebhook` backend (JSON POST with `ValidationIssue` payload)
5. Routing config via env vars:
   - `NOTIFY_DEFAULT_BACKEND=slack`
   - `NOTIFY_SLACK_WEBHOOK_URL=https://hooks.slack.com/...`
   - `NOTIFY_SLACK_CHANNEL_AUTOFIX=#supervisor-autofix` (optional override)
   - `NOTIFY_SLACK_CHANNEL_FLAGS=#supervisor-flags` (optional override)
6. Add `ValidationIssue` node creation with deduplication (check for existing open issue)
7. Wire auto-fix phase to send digest summary after each run

**Success criteria:**
- Auto-fix digest appears in Slack after a run
- Duplicate notifications are suppressed for open issues

### Phase 3: Heuristic triage + LLM flag checks

**Goal:** Identify suspects cheaply, then LLM-review them.

1. Implement triage heuristics (graph queries, no LLM):
   - **Misclassification suspects:** signals with confidence < 0.5 AND single evidence source
   - **Incoherent story suspects:** stories where constituent signals have < 2 shared actors AND > 3 different signal types
   - **Bad RESPONDS_TO suspects:** edges where the Give/Event confidence < 0.4 OR the linked Tension is in a different story
   - **Near-duplicate suspects:** query `SIMILAR_TO` edges with weight in [0.85, 0.92] range on signals created since last run
   - **Low-confidence high-visibility:** signals with confidence < 0.3 in a Story with `status = "confirmed"` or featured in an Edition
2. Implement LLM checks (capped at 50 per run):
   - **Misclassification** (Haiku): pass Evidence snippets, ask for signal type classification, flag if disagreement
   - **Incoherent stories** (Sonnet): pass all signal titles+summaries in a story, ask if they form a coherent narrative, identify root-cause signals
   - **Bad RESPONDS_TO** (Sonnet): pass the Give/Event and Tension summaries, ask if the response genuinely addresses the tension
   - **Near-duplicates** (Haiku): pass both signal titles+summaries, ask if they describe the same real-world thing
3. Create `ValidationIssue` nodes for each flag
4. Send notifications via configured backend
5. Implement story → signal tracing: when a story is flagged as incoherent, also flag the specific root-cause signals identified by the LLM

**Success criteria:**
- Suspects are triaged without LLM calls
- LLM checks stay within budget cap
- Flagged issues appear in Slack with actionable descriptions

### Phase 4: Feedback loops (anti-fragility)

**Goal:** Make the scout learn from supervisor findings.

1. **Source quality penalty:**
   - Add `quality_penalty: f64` property to Source nodes (migration + default 1.0)
   - Supervisor writes penalty based on flag count per source
   - Update scout's `compute_weight()` to multiply by `quality_penalty`
   - Implement recovery (gradual increase for clean signals)

2. **Extraction rules:**
   - Add `ExtractionRule` node schema (migration)
   - Supervisor creates rules from repeated patterns:
     - 3+ misclassification flags from same source type → bias rule
     - 3+ empty/hallucinated date flags from same domain → strip-dates rule
   - Scout reads `ExtractionRule` nodes at startup, appends to extractor prompt
   - Rules auto-expire after 90 days if `applied_count` hasn't increased

3. **Confidence floor calibration:**
   - Track (confidence_score, was_flagged) pairs in SupervisorState metadata
   - After 100+ samples: compute flag rate per confidence bucket
   - If signals below threshold X are flagged > 60% of the time, set `min_confidence = X`
   - Scout reads `min_confidence` from SupervisorState and filters in `store_signals()`
   - Adjustment increment: 0.05 max per calibration. Cooldown: 7 days between adjustments.
   - Bounds: min_confidence clamped to [0.0, 0.5]

4. **Dedup threshold recommendations:**
   - Track near-duplicate flag rate per run
   - After 20+ pairs flagged: recommend threshold adjustment in notification (human decides)
   - No auto-adjustment — the risk of oscillation is too high for automated tuning

**Success criteria:**
- Source quality_penalty decreases for sources producing flagged signals
- Scout reads and applies extraction rules
- Confidence floor tightens over time based on empirical data
- Near-duplicate rates are surfaced for human decision-making

### Phase 5: Echo detection (from anti-fragile brainstorm)

**Goal:** Detect echo signatures — high volume + low diversity masquerading as corroboration.

1. Add echo detection heuristic:
   ```cypher
   MATCH (s:Story)-[:CONTAINS]->(sig)
   WITH s, count(sig) AS signal_count,
        count(DISTINCT labels(sig)[0]) AS type_count,
        count(DISTINCT [(sig)-[:ACTED_IN]-(a:Actor) | a.id]) AS entity_count
   WHERE signal_count > 10 AND type_count < 2 AND entity_count < 3
   RETURN s
   ```
2. Flag stories with echo signatures
3. Add `echo_score` property to Story nodes (0.0 = diverse corroboration, 1.0 = pure echo)
4. Surface in notifications with explanation of why it looks like echo vs real corroboration

**Success criteria:**
- Stories with high volume but low type/entity diversity are flagged
- Echo score is available for downstream ranking

## Acceptance Criteria

### Functional Requirements

- [ ] Supervisor runs as a standalone binary, connects to Memgraph, processes incremental batches
- [ ] Auto-fix checks clean up orphaned nodes, duplicate actors, bad coordinates, empty signals
- [ ] Heuristic triage identifies suspects without LLM calls
- [ ] LLM flag checks stay within configurable budget cap
- [ ] Slack notifications fire for flagged issues with actionable descriptions
- [ ] Duplicate notifications are suppressed for open issues
- [ ] Source quality_penalty is written and respected by scout's weight computation
- [ ] Extraction rules are created by supervisor and read by scout
- [ ] Confidence floor calibration tightens over time with empirical data

### Non-Functional Requirements

- [ ] Idempotent: running twice on the same window produces the same result
- [ ] Cost-bounded: LLM spend capped per run and per day
- [ ] Concurrent-safe: SupervisorLock prevents double-runs; quality_penalty avoids weight conflicts
- [ ] Observable: structured tracing with span fields, SupervisorStats logged per run

## Dependencies & Prerequisites

- Scout's `compute_weight()` must be updated to multiply by `quality_penalty` (Phase 4)
- Scout's extractor must load `ExtractionRule` nodes at startup (Phase 4)
- Scout's `store_signals()` must read `min_confidence` from SupervisorState (Phase 4)
- Migrations for new node types must run before supervisor or updated scout

## Deferred to Future Release

- **FLAG 6 (contradictory actor roles):** current schema only has `role = "mentioned"`. Requires extractor changes to populate real roles first.
- **GitHub Issues notification backend:** Slack is sufficient for now.
- **Admin UI for extraction rule approval:** CLI/Cypher is sufficient for now.
- **Multi-city single-instance:** each city gets its own supervisor instance for now.

## Risk Analysis

| Risk | Mitigation |
|------|------------|
| Feedback loops over-correct (oscillation) | Small increments, cooldowns, bounds on all calibrated values |
| LLM costs blow up on large backlog | 24h watermark cap, per-run suspect limit, daily budget |
| Scout/supervisor weight conflict | Separate `quality_penalty` property, multiplicative composition |
| Actor merge incorrectly combines different entities | Conservative: only merge on exact normalized name match, not fuzzy |
| Auto-applied extraction rules cause harm | Rules are specific (domain-scoped), expire after 90 days, can be disabled |

## References

- Brainstorm: `docs/brainstorms/2026-02-17-scout-supervisor-brainstorm.md`
- Scout architecture: `docs/scout-architecture.md`
- Anti-fragile signal design: `docs/brainstorms/2026-02-17-anti-fragile-signal-brainstorm.md`
- Data quality anti-pattern: `docs/solutions/2026-02-17-unwrap-or-masks-data-quality.md`
- Scout pipeline: `modules/rootsignal-scout/src/scout.rs`
- Graph writer: `modules/rootsignal-graph/src/writer.rs`
- Graph migrations: `modules/rootsignal-graph/src/migrate.rs`
- Quality scoring: `modules/rootsignal-scout/src/quality.rs`
- Source scheduling: `modules/rootsignal-scout/src/sources.rs`
