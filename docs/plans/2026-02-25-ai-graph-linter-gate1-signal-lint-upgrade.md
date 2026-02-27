# AI Graph Linter — Gate 1: Signal Lint Upgrade

## Context

The current signal lint pipeline uses string-based review statuses (`"staged"`, `"live"`, `"rejected"`) with a binary pass/reject model. Per the brainstorm at `docs/brainstorms/2026-02-25-ai-graph-linter-brainstorm.md`, we're upgrading to a unified `ReviewStatus` enum (`Draft`, `Published`, `Quarantined`, `Rejected`) shared by signals and situations. Gate 1 focuses on signals — adding the quarantine state, upgrading to a stronger model for the gate, and wiring the admin UI for quarantine resolution.

This is the first phase of a larger AI Graph Linter system. Gate 2 (situations) and admin investigation mode come later.

## Findings from Codebase Verification

Two issues discovered that affect the plan:

1. **`promote_ready_stories()` is dead code.** Stories were archived to `LegacyStory` in a migration. The trait declares it, signal_lint.rs calls it, but the proxy wrapper in traits.rs is infinite recursion (`self.promote_ready_stories()` calling itself). The mock returns `Ok(0)`. This needs cleanup.

2. **Migration system is idempotent, not numbered.** All migrations live in a single `migrate()` function in migrate.rs using `IF NOT EXISTS` / conditional Cypher. No migration files. Our status migration needs to follow this pattern.

Also confirmed:
- 5 signal creation functions all hardcode `review_status: 'staged'`
- `set_review_status()` takes `(Uuid, &str, Option<&str>)` — tries all 5 labels
- reader.rs: 4 locations filter `= 'live'` (lines ~60, ~174, ~491, ~554) + 1 parse fallback defaulting to `"live"`
- beacon.rs: 1 location filters `= 'live'` (line ~81)
- writer.rs `search_map_signals`: 1 location uses `IN ['staged', 'live']` (line ~4939)
- `NodeMeta` already has `rejection_reason: Option<String>` — no need to add it
- `SituationNode` has NO `review_status` field (deferred to Gate 2)

## Steps

### 0. Clean up dead `promote_ready_stories()`

**Files:**
- `modules/rootsignal-scout/src/pipeline/traits.rs` — remove `promote_ready_stories()` from `SignalStore` trait
- `modules/rootsignal-scout/src/pipeline/signal_lint.rs` (line ~115) — remove the call `self.store.promote_ready_stories()`
- `modules/rootsignal-scout/src/testing.rs` — remove mock impl

This is dead code from the Story→LegacyStory archive migration. Clean it up before building on top.

### 1. Define `ReviewStatus` enum in rootsignal-common

**File:** `modules/rootsignal-common/src/types.rs`

- Add `ReviewStatus` enum: `Draft`, `Published`, `Quarantined`, `Rejected`
- `impl ReviewStatus` with `as_str()` → `"draft"`, `"published"`, `"quarantined"`, `"rejected"`
- `FromStr` / `TryFrom<&str>` for parsing from graph strings (accept both old and new values during transition: `"staged"` → `Draft`, `"live"` → `Published`)
- `Display`, `Serialize`, `Deserialize` derives
- Keep `review_status: String` on `NodeMeta` for now — the field type change is a larger refactor we don't need yet
- Add `quarantine_reason: Option<String>` to `NodeMeta` (`rejection_reason` already exists)

### 2a. Graph migration + signal creation (writer.rs, migrate.rs)

**File:** `modules/rootsignal-graph/src/migrate.rs`

Append to the `migrate()` function (idempotent pattern matching existing style):

```cypher
-- Rename review_status values: staged → draft, live → published
MATCH (n) WHERE n.review_status = 'staged' SET n.review_status = 'draft'
MATCH (n) WHERE n.review_status = 'live' SET n.review_status = 'published'
```

Safe to re-run: if values are already migrated, `WHERE` matches nothing.

**File:** `modules/rootsignal-graph/src/writer.rs` (creation + staging queries)

- 5 signal creation functions: `'staged'` → `'draft'`
- `staged_signals_in_region()`: `WHERE s.review_status = 'draft'`
- `set_review_status()`: accept `ReviewStatus` enum instead of `&str`, call `.as_str()` in Cypher params
- `search_map_signals` (line ~4939): `IN ['staged', 'live']` → `IN ['draft', 'published']`

**Verify:** `cargo check -p rootsignal-graph` passes.

### 2b. Public query filters (reader.rs, beacon.rs)

**File:** `modules/rootsignal-graph/src/reader.rs`

- 4 Cypher locations: `review_status = 'live'` → `'published'` (lines ~60, ~174, ~491, ~554)
- 1 parse fallback (line ~1499): `unwrap_or_else(|_| "live".to_string())` → `"published".to_string()`

**File:** `modules/rootsignal-graph/src/beacon.rs`

- Line ~81: `review_status = 'live'` → `'published'`

**Verify:** public queries return same results with new string values.

### 2c. Promote logic + quarantine methods (writer.rs)

**File:** `modules/rootsignal-graph/src/writer.rs`

- `promote_ready_situations()`: `WHERE s.review_status = 'staged'` → `'draft'`, `SET s.review_status = 'live'` → `'published'`, `WHERE n.review_status <> 'live'` → `<> 'published'`
- Add `set_quarantine(id, reason)` — sets `review_status = 'quarantined'`, `quarantine_reason = reason` (same multi-label pattern as `set_review_status`)
- Add `resolve_quarantine(id, new_status: ReviewStatus, reason)` — admin action: quarantined → published or rejected

**Verify:** promote logic works end-to-end. Situation promote updated here to avoid half-migrated state.

### 3. Update SignalStore trait

**File:** `modules/rootsignal-scout/src/pipeline/traits.rs`

- `set_review_status(id, &str, Option<&str>)` → `set_review_status(id, ReviewStatus, Option<&str>)`
- Add `quarantine_signal(id: Uuid, reason: &str) -> Result<()>` to trait
- Update the proxy wrapper impl to call through to `GraphWriter`

**File:** `modules/rootsignal-scout/src/testing.rs` (MockSignalStore)

- Update mock `set_review_status` to accept `ReviewStatus`
- Add mock `quarantine_signal` (record call, return `Ok(())`)

### 4. Upgrade SignalLinter and lint tools

**File:** `modules/rootsignal-scout/src/pipeline/signal_lint.rs`

- Add `Quarantine { reason: String }` variant to `LintVerdict`
- Add `quarantine_signal` tool definition:
  ```json
  {
    "name": "quarantine_signal",
    "description": "Flag a signal for human review when you cannot confidently verify or reject it. Use when source content is ambiguous, the signal seems plausible but unverifiable, or you're uncertain about correctness.",
    "parameters": {
      "type": "object",
      "properties": {
        "node_id": { "type": "string" },
        "reason": { "type": "string", "description": "Why this signal needs human review" }
      },
      "required": ["node_id", "reason"]
    }
  }
  ```
- Update verdict processing match:
  - `Pass` → `set_review_status(id, ReviewStatus::Published, None)`
  - `Correct` → corrections + `set_review_status(id, ReviewStatus::Published, None)`
  - `Reject` → `set_review_status(id, ReviewStatus::Rejected, Some(reason))`
  - `Quarantine` → `quarantine_signal(id, reason)` (new)
  - `None` (no verdict) → `set_review_status(id, ReviewStatus::Rejected, Some("NO_VERDICT: ..."))` (unchanged behavior)
- Update system prompt to explain quarantine option
- Make model configurable via env var or config (default stays `claude-sonnet-4-5-20250514` — upgrade later)

### 5. Verify Restate workflow

**File:** `modules/rootsignal-scout/src/workflows/lint.rs`

- No structural changes — the workflow wrapper calls `SignalLinter::lint()` which handles verdicts internally
- Verify status transitions still work: `"running_lint"` → `"lint_complete"`

### 6. Add admin GraphQL mutations for quarantine resolution

**File:** `modules/rootsignal-api/src/graphql/mutations.rs`

- Add `resolveQuarantinedSignal(id: ID!, action: ReviewAction!, reason: String!): Signal!` mutation
- `ReviewAction` enum: `Publish`, `Reject`
- Calls `resolve_quarantine()` on `GraphWriter`

**File:** `modules/rootsignal-api/src/graphql/schema.rs`

- Add `ReviewStatus` enum to GraphQL schema (maps to Rust enum)
- Add `ReviewAction` enum (`Publish`, `Reject`)
- Update `adminSignals` query to accept `ReviewStatus` filter (currently takes optional string `status`)
- Add `quarantinedSignals(regionId: ID, limit: Int): [Signal!]!` convenience query

### 7. Update admin frontend

**File:** `modules/admin-app/src/pages/SignalsPage.tsx`

- Add filter tabs for review status: All / Draft / Published / Quarantined / Rejected
- Quarantined signals highlighted with amber/warning color
- Publish/reject action buttons on quarantined signals

**File:** `modules/admin-app/src/lib/event-colors.tsx` (or new component file)

- `ReviewStatusBadge` component:
  - Draft → gray
  - Published → green
  - Quarantined → amber
  - Rejected → red

**File:** `modules/admin-app/src/graphql/mutations.ts`

- Add `RESOLVE_QUARANTINED_SIGNAL` mutation

**File:** `modules/admin-app/src/graphql/queries.ts`

- Update signal queries to use new status enum values

## Migration Strategy

The graph migration (step 2a) appends to the idempotent `migrate()` function:
- `WHERE n.review_status = 'staged' SET n.review_status = 'draft'`
- `WHERE n.review_status = 'live' SET n.review_status = 'published'`

Safe to re-run: WHERE clauses match nothing if already migrated.

**In-flight safety:** Old code writes `'staged'`, new code writes `'draft'`. The `FromStr` impl on `ReviewStatus` accepts both during transition. Migration is idempotent so can be re-run after deploy.

**Smoke test:**
```cypher
MATCH (n) WHERE n.review_status IN ['staged', 'live'] RETURN labels(n)[0], n.review_status, count(*) LIMIT 10
```
Should return 0 rows after migration.

## Verification

- [ ] `cargo check` passes for all workspace members
- [ ] Existing signal lint tests pass with updated status values
- [ ] New test: quarantine verdict creates quarantined signal with reason
- [ ] New test: resolve_quarantine transitions quarantined → published or rejected
- [ ] Graph migration converts existing status strings correctly
- [ ] Post-migration smoke test: no nodes with old status values
- [ ] Admin UI shows quarantined signals with action buttons
- [ ] Public queries only return `published` signals (same behavior, new string)
- [ ] `promote_ready_situations()` works with new status values
- [ ] `promote_ready_stories()` dead code removed cleanly
